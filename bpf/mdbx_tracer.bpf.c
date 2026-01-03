// SPDX-License-Identifier: GPL-2.0 OR BSD-3-Clause
//
// eBPF program to trace MDBX page faults in MDBX applications
//
// This traces memory-mapped file access patterns to understand
// trie traversal I/O behavior.

#include "vmlinux.h"
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_tracing.h>
#include <bpf/bpf_core_read.h>

// Maximum file path length we track
#define MAX_PATH_LEN 256

// Event types
#define EVENT_PAGE_FAULT     1
#define EVENT_MMAP           2
#define EVENT_CURSOR_SEEK    3

// Page fault event sent to userspace
struct page_fault_event {
    __u64 timestamp_ns;      // Kernel timestamp
    __u64 address;           // Faulting virtual address
    __u64 file_offset;       // Offset within the mmap'd file
    __u64 vma_start;         // VMA start address
    __u64 vma_end;           // VMA end address
    __u32 pid;               // Process ID
    __u32 tid;               // Thread ID
    __u32 event_type;        // Event type
    __u32 fault_flags;       // Page fault flags (read/write/etc)
    __u64 latency_ns;        // Time spent in fault handler (if available)
    __u8  is_major;          // Major fault (disk I/O) vs minor (in page cache)
};

// Ring buffer for events - sized for high throughput
struct {
    __uint(type, BPF_MAP_TYPE_RINGBUF);
    __uint(max_entries, 16 * 1024 * 1024);  // 16MB buffer
} events SEC(".maps");

// Track which VMAs belong to MDBX files
// Key: inode number, Value: 1 if we should trace
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 16);
    __type(key, __u64);    // inode
    __type(value, __u8);   // 1 = trace this inode
} tracked_inodes SEC(".maps");

// Track active VMAs for MDBX files
// Key: VMA start address, Value: file offset base
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 256);
    __type(key, __u64);    // vma->vm_start
    __type(value, __u64);  // file offset (vm_pgoff * PAGE_SIZE)
} vma_to_offset SEC(".maps");

// Configuration from userspace - renamed to avoid conflict with vmlinux.h
struct {
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __uint(max_entries, 1);
    __type(key, __u32);
    __type(value, __u32);  // target PID (0 = trace all)
} profiler_config SEC(".maps");

// Statistics counters
struct {
    __uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
    __uint(max_entries, 4);
    __type(key, __u32);
    __type(value, __u64);
} stats SEC(".maps");

#define STAT_TOTAL_FAULTS     0
#define STAT_MDBX_FAULTS      1
#define STAT_MAJOR_FAULTS     2
#define STAT_EVENTS_DROPPED   3

static __always_inline void inc_stat(__u32 idx) {
    __u64 *val = bpf_map_lookup_elem(&stats, &idx);
    if (val) {
        __sync_fetch_and_add(val, 1);
    }
}

// Check if we should trace this process
static __always_inline bool should_trace_pid(__u32 pid) {
    __u32 key = 0;
    __u32 *target_pid = bpf_map_lookup_elem(&profiler_config, &key);
    if (!target_pid || *target_pid == 0) {
        return true;  // Trace all if not configured
    }
    return pid == *target_pid;
}

// Get inode from file struct
static __always_inline __u64 get_file_inode(struct file *file) {
    if (!file) return 0;
    
    struct inode *inode = BPF_CORE_READ(file, f_inode);
    if (!inode) return 0;
    
    return BPF_CORE_READ(inode, i_ino);
}

// Trace page faults - this is the hot path
SEC("kprobe/handle_mm_fault")
int BPF_KPROBE(trace_page_fault, 
               struct vm_area_struct *vma,
               unsigned long address,
               unsigned int flags)
{
    __u32 pid = bpf_get_current_pid_tgid() >> 32;
    
    // Filter by PID first (fast path)
    if (!should_trace_pid(pid)) {
        return 0;
    }
    
    inc_stat(STAT_TOTAL_FAULTS);
    
    // Read VMA bounds
    __u64 vm_start = BPF_CORE_READ(vma, vm_start);
    __u64 vm_end = BPF_CORE_READ(vma, vm_end);
    
    // Check if this is a VMA we're tracking
    __u64 *file_offset_base = bpf_map_lookup_elem(&vma_to_offset, &vm_start);
    if (!file_offset_base) {
        // Not a tracked VMA - check if it's a new MDBX mmap
        struct file *file = BPF_CORE_READ(vma, vm_file);
        if (!file) return 0;
        
        __u64 inode = get_file_inode(file);
        __u8 *tracked = bpf_map_lookup_elem(&tracked_inodes, &inode);
        if (!tracked) {
            return 0;  // Not an MDBX file
        }
        
        // New VMA for tracked inode - register it
        __u64 pgoff = BPF_CORE_READ(vma, vm_pgoff);
        __u64 offset = pgoff * 4096;  // PAGE_SIZE
        bpf_map_update_elem(&vma_to_offset, &vm_start, &offset, BPF_ANY);
        file_offset_base = &offset;
    }
    
    inc_stat(STAT_MDBX_FAULTS);
    
    // Calculate file offset from virtual address
    __u64 file_offset = *file_offset_base + (address - vm_start);
    
    // Reserve space in ring buffer
    struct page_fault_event *e = bpf_ringbuf_reserve(&events, sizeof(*e), 0);
    if (!e) {
        inc_stat(STAT_EVENTS_DROPPED);
        return 0;
    }
    
    // Fill event
    e->timestamp_ns = bpf_ktime_get_ns();
    e->address = address;
    e->file_offset = file_offset;
    e->vma_start = vm_start;
    e->vma_end = vm_end;
    e->pid = pid;
    e->tid = bpf_get_current_pid_tgid() & 0xFFFFFFFF;
    e->event_type = EVENT_PAGE_FAULT;
    e->fault_flags = flags;
    e->latency_ns = 0;  // Will be filled by return probe
    e->is_major = 0;    // Will determine from flags
    
    bpf_ringbuf_submit(e, 0);
    
    return 0;
}

// Optional: trace mmap calls to detect new MDBX mappings
SEC("kprobe/do_mmap")
int BPF_KPROBE(trace_mmap,
               struct file *file,
               unsigned long addr,
               unsigned long len,
               unsigned long prot,
               unsigned long flags,
               unsigned long pgoff)
{
    if (!file) return 0;
    
    __u32 pid = bpf_get_current_pid_tgid() >> 32;
    if (!should_trace_pid(pid)) return 0;
    
    __u64 inode = get_file_inode(file);
    __u8 *tracked = bpf_map_lookup_elem(&tracked_inodes, &inode);
    if (!tracked) return 0;
    
    // Log the mmap event
    struct page_fault_event *e = bpf_ringbuf_reserve(&events, sizeof(*e), 0);
    if (!e) return 0;
    
    e->timestamp_ns = bpf_ktime_get_ns();
    e->address = addr;
    e->file_offset = pgoff * 4096;
    e->vma_start = addr;
    e->vma_end = addr + len;
    e->pid = pid;
    e->tid = bpf_get_current_pid_tgid() & 0xFFFFFFFF;
    e->event_type = EVENT_MMAP;
    e->fault_flags = flags;
    e->latency_ns = len;  // Repurpose for mmap length
    e->is_major = 0;
    
    bpf_ringbuf_submit(e, 0);
    
    return 0;
}

char LICENSE[] SEC("license") = "Dual BSD/GPL";
