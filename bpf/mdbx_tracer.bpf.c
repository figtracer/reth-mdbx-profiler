// SPDX-License-Identifier: GPL-2.0 OR BSD-3-Clause
//
// eBPF program to trace MDBX page faults and cursor operations
//
// This traces:
// 1. Memory-mapped file access patterns (page faults) to understand I/O behavior
// 2. MDBX cursor operations (seeks, gets) to understand database access patterns
//
// The cursor tracing uses uprobes on libmdbx functions.

#include "vmlinux.h"
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_tracing.h>
#include <bpf/bpf_core_read.h>

// Maximum file path length we track
#define MAX_PATH_LEN 256

// Maximum key bytes to capture (first N bytes of the key)
#define MAX_KEY_SIZE 64

// Event types
#define EVENT_PAGE_FAULT     1
#define EVENT_MMAP           2
#define EVENT_CURSOR_GET     3
#define EVENT_CURSOR_PUT     4
#define EVENT_DIRECT_GET     5
#define EVENT_CURSOR_DEL     6
#define EVENT_TXN_BEGIN      7
#define EVENT_TXN_COMMIT     8
#define EVENT_TXN_ABORT      9

// MDBX cursor operations (from libmdbx mdbx.h MDBX_cursor_op enum)
// These are the numeric values for the cursor operations
#define MDBX_FIRST           0
#define MDBX_FIRST_DUP       1
#define MDBX_GET_BOTH        2
#define MDBX_GET_BOTH_RANGE  3
#define MDBX_GET_CURRENT     4
#define MDBX_GET_MULTIPLE    5
#define MDBX_LAST            6
#define MDBX_LAST_DUP        7
#define MDBX_NEXT            8
#define MDBX_NEXT_DUP        9
#define MDBX_NEXT_MULTIPLE   10
#define MDBX_NEXT_NODUP      11
#define MDBX_PREV            12
#define MDBX_PREV_DUP        13
#define MDBX_PREV_NODUP      14
#define MDBX_SET             15
#define MDBX_SET_KEY         16
#define MDBX_SET_RANGE       17
#define MDBX_PREV_MULTIPLE   18
#define MDBX_SET_LOWERBOUND  19
#define MDBX_SET_UPPERBOUND  20

// VM_FAULT flags from kernel (used in handle_mm_fault return value)
#define VM_FAULT_MAJOR   0x0004

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

// MDBX cursor operation event sent to userspace
// This captures mdbx_cursor_get/put/del calls with key information
struct cursor_event {
    __u64 timestamp_ns;      // Kernel timestamp
    __u32 pid;               // Process ID
    __u32 tid;               // Thread ID
    __u32 event_type;        // EVENT_CURSOR_GET, EVENT_CURSOR_PUT, EVENT_CURSOR_DEL
    __u32 cursor_op;         // MDBX cursor operation (MDBX_SET_RANGE, MDBX_NEXT, etc.)
    __u32 dbi;               // Database index (table identifier)
    __u32 key_size;          // Size of the key
    __u8  key_data[MAX_KEY_SIZE];  // First MAX_KEY_SIZE bytes of the key
    __s32 return_code;       // Return code from the operation (filled in uretprobe)
    __u32 value_size;        // Size of value (for put operations)
    __u64 latency_ns;        // Time spent in the operation
    __u32 write_flags;       // Write flags (UPSERT, APPEND, etc.) for put operations
    __u32 _pad2;             // Padding for alignment
};

// Transaction event sent to userspace
// This captures mdbx_txn_begin/commit/abort calls
struct txn_event {
    __u64 timestamp_ns;      // Kernel timestamp
    __u32 pid;               // Process ID
    __u32 tid;               // Thread ID
    __u32 event_type;        // EVENT_TXN_BEGIN, EVENT_TXN_COMMIT, EVENT_TXN_ABORT
    __u32 txn_flags;         // Transaction flags (MDBX_TXN_RDONLY=1, MDBX_TXN_READWRITE=0)
    __u64 txn_ptr;           // Transaction pointer (for correlation)
    __u64 parent_txn_ptr;    // Parent transaction pointer (0 if none)
    __u64 latency_ns;        // Time spent (for commit)
    __s32 return_code;       // Return code
    __u32 _pad;              // Padding for alignment
};

// Context for correlating kprobe entry with kretprobe return
struct fault_context {
    __u64 timestamp_ns;      // Entry timestamp for latency calculation
    __u64 address;           // Faulting address
    __u64 file_offset;       // File offset
    __u64 vma_start;         // VMA bounds
    __u64 vma_end;
    __u32 pid;
    __u32 tid;
    __u32 fault_flags;
    __u8  should_trace;      // Whether this fault should be traced
};

// Context for correlating uprobe entry with uretprobe return for cursor ops
struct cursor_context {
    __u64 timestamp_ns;      // Entry timestamp for latency calculation
    __u32 pid;
    __u32 tid;
    __u32 cursor_op;         // MDBX cursor operation
    __u32 dbi;               // Database index
    __u32 key_size;          // Key size
    __u32 value_size;        // Value size (for put operations)
    __u32 write_flags;       // Write flags (for put operations)
    __u8  key_data[MAX_KEY_SIZE];  // Key data
};

// Context for correlating uprobe entry with uretprobe return for cursor put ops
struct cursor_put_context {
    __u64 timestamp_ns;      // Entry timestamp for latency calculation
    __u32 pid;
    __u32 tid;
    __u32 dbi;               // Database index
    __u32 key_size;          // Key size
    __u32 value_size;        // Value size
    __u32 write_flags;       // Write flags (UPSERT, APPEND, etc.)
    __u8  key_data[MAX_KEY_SIZE];  // Key data
};

// Context for correlating uprobe entry with uretprobe return for cursor del ops
struct cursor_del_context {
    __u64 timestamp_ns;      // Entry timestamp for latency calculation
    __u32 pid;
    __u32 tid;
    __u32 dbi;               // Database index
    __u32 write_flags;       // Delete flags (CURRENT, NO_DUP_DATA)
};

// Context for transaction begin operations
struct txn_begin_context {
    __u64 timestamp_ns;      // Entry timestamp
    __u32 pid;
    __u32 tid;
    __u32 txn_flags;         // Transaction flags (RO/RW)
    __u64 parent_txn_ptr;    // Parent transaction pointer
    __u64 txn_ptr_ptr;       // Address of MDBX_txn** output parameter
};

// Context for transaction commit operations
struct txn_commit_context {
    __u64 timestamp_ns;      // Entry timestamp
    __u32 pid;
    __u32 tid;
    __u64 txn_ptr;           // Transaction pointer
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
    __uint(max_entries, 16);
    __type(key, __u32);
    __type(value, __u64);
} stats SEC(".maps");

// Per-task fault context for correlating kprobe entry with kretprobe return
// Key: tid (thread id), Value: fault_context
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 10240);
    __type(key, __u64);    // pid_tgid
    __type(value, struct fault_context);
} pending_faults SEC(".maps");

// Per-task cursor context for correlating uprobe entry with uretprobe return
// Key: tid (thread id), Value: cursor_context
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 10240);
    __type(key, __u64);    // pid_tgid
    __type(value, struct cursor_context);
} pending_cursors SEC(".maps");

// Map cursor address to DBI - populated by mdbx_cursor_open uprobe
// Key: cursor pointer (lower 64 bits), Value: DBI (u32)
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 10240);
    __type(key, __u64);    // cursor address
    __type(value, __u32);  // DBI
} cursor_to_dbi SEC(".maps");

// Context for cursor_open to get the cursor pointer from return probe
struct cursor_open_context {
    __u64 cursor_ptr_ptr;  // Address of MDBX_cursor** argument
    __u32 dbi;             // DBI being opened
};

// Per-task context for mdbx_cursor_open
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 1024);
    __type(key, __u64);    // pid_tgid
    __type(value, struct cursor_open_context);
} pending_cursor_opens SEC(".maps");

// Context for correlating uprobe entry with uretprobe return for direct get ops
// mdbx_get signature: int mdbx_get(const MDBX_txn *txn, MDBX_dbi dbi, 
//                                   const MDBX_val *key, MDBX_val *data)
struct direct_get_context {
    __u64 timestamp_ns;      // Entry timestamp for latency calculation
    __u32 pid;
    __u32 tid;
    __u32 dbi;               // Database index (passed directly as parameter)
    __u32 key_size;          // Key size
    __u8  key_data[MAX_KEY_SIZE];  // Key data
};

// Per-task context for mdbx_get
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 10240);
    __type(key, __u64);    // pid_tgid
    __type(value, struct direct_get_context);
} pending_direct_gets SEC(".maps");

// Per-task context for mdbx_cursor_put
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 10240);
    __type(key, __u64);    // pid_tgid
    __type(value, struct cursor_put_context);
} pending_cursor_puts SEC(".maps");

// Per-task context for mdbx_cursor_del
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 10240);
    __type(key, __u64);    // pid_tgid
    __type(value, struct cursor_del_context);
} pending_cursor_dels SEC(".maps");

// Per-task context for mdbx_txn_begin_ex
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 1024);
    __type(key, __u64);    // pid_tgid
    __type(value, struct txn_begin_context);
} pending_txn_begins SEC(".maps");

// Per-task context for mdbx_txn_commit_ex
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 1024);
    __type(key, __u64);    // pid_tgid
    __type(value, struct txn_commit_context);
} pending_txn_commits SEC(".maps");

#define STAT_TOTAL_FAULTS     0
#define STAT_MDBX_FAULTS      1
#define STAT_MAJOR_FAULTS     2
#define STAT_EVENTS_DROPPED   3
#define STAT_CURSOR_OPS       4
#define STAT_CURSOR_SEEKS     5
#define STAT_CURSOR_NEXTS     6
#define STAT_CURSOR_ERRORS    7
#define STAT_DIRECT_GETS      8
#define STAT_CURSOR_PUTS      9
#define STAT_CURSOR_DELS     10
#define STAT_TXN_BEGINS      11
#define STAT_TXN_COMMITS     12
#define STAT_TXN_ABORTS      13

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

// Trace page faults entry - save context for kretprobe
SEC("kprobe/handle_mm_fault")
int BPF_KPROBE(trace_page_fault, 
               struct vm_area_struct *vma,
               unsigned long address,
               unsigned int flags)
{
    __u64 pid_tgid = bpf_get_current_pid_tgid();
    __u32 pid = pid_tgid >> 32;
    
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
    __u64 offset_base_val = 0;
    
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
        offset_base_val = pgoff * 4096;  // PAGE_SIZE
        bpf_map_update_elem(&vma_to_offset, &vm_start, &offset_base_val, BPF_ANY);
    } else {
        offset_base_val = *file_offset_base;
    }
    
    inc_stat(STAT_MDBX_FAULTS);
    
    // Calculate file offset from virtual address
    __u64 file_offset = offset_base_val + (address - vm_start);
    
    // Save context for kretprobe to emit event with major fault info
    struct fault_context fctx = {
        .timestamp_ns = bpf_ktime_get_ns(),
        .address = address,
        .file_offset = file_offset,
        .vma_start = vm_start,
        .vma_end = vm_end,
        .pid = pid,
        .tid = (__u32)pid_tgid,
        .fault_flags = flags,
        .should_trace = 1,
    };
    bpf_map_update_elem(&pending_faults, &pid_tgid, &fctx, BPF_ANY);
    
    return 0;
}

// Trace page fault return - emit event with major fault info from return value
SEC("kretprobe/handle_mm_fault")
int BPF_KRETPROBE(trace_page_fault_ret, vm_fault_t ret)
{
    __u64 pid_tgid = bpf_get_current_pid_tgid();
    
    // Look up the saved context
    struct fault_context *fctx = bpf_map_lookup_elem(&pending_faults, &pid_tgid);
    if (!fctx || !fctx->should_trace) {
        return 0;
    }
    
    // Determine if this was a major fault from return value
    __u8 is_major = (ret & VM_FAULT_MAJOR) ? 1 : 0;
    
    if (is_major) {
        inc_stat(STAT_MAJOR_FAULTS);
    }
    
    // Calculate latency
    __u64 now = bpf_ktime_get_ns();
    __u64 latency = now - fctx->timestamp_ns;
    
    // Reserve space in ring buffer
    struct page_fault_event *e = bpf_ringbuf_reserve(&events, sizeof(*e), 0);
    if (!e) {
        inc_stat(STAT_EVENTS_DROPPED);
        bpf_map_delete_elem(&pending_faults, &pid_tgid);
        return 0;
    }
    
    // Fill event with saved context and return info
    e->timestamp_ns = fctx->timestamp_ns;
    e->address = fctx->address;
    e->file_offset = fctx->file_offset;
    e->vma_start = fctx->vma_start;
    e->vma_end = fctx->vma_end;
    e->pid = fctx->pid;
    e->tid = fctx->tid;
    e->event_type = EVENT_PAGE_FAULT;
    e->fault_flags = fctx->fault_flags;
    e->latency_ns = latency;
    e->is_major = is_major;
    
    bpf_ringbuf_submit(e, 0);
    
    // Clean up
    bpf_map_delete_elem(&pending_faults, &pid_tgid);
    
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

// ============================================================================
// MDBX Cursor Operation Tracing (uprobes)
// ============================================================================
//
// These uprobes attach to libmdbx functions to trace database operations.
// The main function we trace is mdbx_cursor_get which has this signature:
//
//   int mdbx_cursor_get(MDBX_cursor *cursor, MDBX_val *key, MDBX_val *data, 
//                       MDBX_cursor_op op);
//
// MDBX_val is a struct with:
//   struct MDBX_val { void *iov_base; size_t iov_len; };
//
// MDBX_cursor contains a reference to the DBI (database index).
// The cursor struct layout (from libmdbx source):
//   - The DBI is accessible via cursor->mc_dbi (offset varies by version)
//
// For simplicity, we capture:
//   - The cursor operation (op parameter)
//   - The key being sought (from key->iov_base, key->iov_len)
//   - The DBI from the cursor structure

// MDBX_val structure - matches libmdbx definition
struct mdbx_val {
    void *iov_base;
    __u64 iov_len;  // size_t
};

// Uprobe on mdbx_cursor_get entry
// Signature: int mdbx_cursor_get(MDBX_cursor *cursor, MDBX_val *key, 
//                                 MDBX_val *data, MDBX_cursor_op op)
SEC("uprobe/mdbx_cursor_get")
int BPF_UPROBE(trace_cursor_get, void *cursor, struct mdbx_val *key, 
               void *data, int op)
{
    __u64 pid_tgid = bpf_get_current_pid_tgid();
    __u32 pid = pid_tgid >> 32;
    
    // Filter by PID
    if (!should_trace_pid(pid)) {
        return 0;
    }
    
    inc_stat(STAT_CURSOR_OPS);
    
    // Track specific operation types
    if (op == MDBX_SET_RANGE || op == MDBX_SET || op == MDBX_SET_KEY ||
        op == MDBX_SET_LOWERBOUND || op == MDBX_SET_UPPERBOUND) {
        inc_stat(STAT_CURSOR_SEEKS);
    } else if (op == MDBX_NEXT || op == MDBX_NEXT_DUP || op == MDBX_NEXT_NODUP) {
        inc_stat(STAT_CURSOR_NEXTS);
    }
    
    // Build cursor context (named cctx to avoid conflict with BPF_UPROBE's ctx)
    struct cursor_context cctx = {
        .timestamp_ns = bpf_ktime_get_ns(),
        .pid = pid,
        .tid = (__u32)pid_tgid,
        .cursor_op = op,
        .dbi = 0,  // Will try to read from cursor struct
        .key_size = 0,
    };
    
    // Look up the DBI from cursor_to_dbi map (populated by mdbx_cursor_open uprobe)
    if (cursor) {
        __u64 cursor_addr = (__u64)cursor;
        __u32 *dbi_ptr = bpf_map_lookup_elem(&cursor_to_dbi, &cursor_addr);
        if (dbi_ptr) {
            cctx.dbi = *dbi_ptr;
        } else {
            // Cursor was opened before tracing started - try to read DBI from struct
            // MDBX_cursor layout (libmdbx 0.12+):
            //   offset 0:  int32_t signature
            //   offset 4:  int16_t top_and_flags  
            //   offset 6:  uint8_t checking
            //   offset 7:  uint8_t pad
            //   offset 8:  uint8_t* dbi_state
            //   offset 16: MDBX_txn* txn
            // 
            // MDBX_txn layout:
            //   offset 0:  int32_t signature
            //   offset 4:  uint32_t flags
            //   offset 8:  size_t n_dbi
            //   ... (varies by config)
            //   dbi_state is typically at offset 104-120 depending on build options
            //
            // DBI = cursor->dbi_state - cursor->txn->dbi_state
            // This is fragile, so we try it but fall back to a sentinel value
            
            __u64 cursor_dbi_state = 0;
            __u64 txn_ptr = 0;
            __u64 txn_dbi_state = 0;
            
            // Read cursor->dbi_state (offset 8)
            if (bpf_probe_read_user(&cursor_dbi_state, sizeof(cursor_dbi_state), 
                                    (void *)(cursor_addr + 8)) == 0 && cursor_dbi_state != 0) {
                // Read cursor->txn (offset 16)
                if (bpf_probe_read_user(&txn_ptr, sizeof(txn_ptr),
                                        (void *)(cursor_addr + 16)) == 0 && txn_ptr != 0) {
                    // Try common offsets for txn->dbi_state: 104, 112, 120, 88
                    // These vary based on libmdbx build options (MDBX_ENABLE_DBI_SPARSE, etc.)
                    #pragma unroll
                    for (int offset_idx = 0; offset_idx < 4; offset_idx++) {
                        __u64 try_offset = (offset_idx == 0) ? 104 : 
                                           (offset_idx == 1) ? 112 : 
                                           (offset_idx == 2) ? 120 : 88;
                        if (bpf_probe_read_user(&txn_dbi_state, sizeof(txn_dbi_state),
                                                (void *)(txn_ptr + try_offset)) == 0) {
                            // Validate: dbi_state should point into txn's dbi_state array
                            // and DBI should be reasonable (< 256)
                            if (txn_dbi_state != 0 && cursor_dbi_state >= txn_dbi_state) {
                                __u64 computed_dbi = cursor_dbi_state - txn_dbi_state;
                                if (computed_dbi < 256) {
                                    cctx.dbi = (__u32)computed_dbi;
                                    goto dbi_found;
                                }
                            }
                        }
                    }
                }
            }
            
            // Could not determine DBI - use sentinel value 0xFFFFFFFE
            // This will show up as "Unknown" in the analyzer
            cctx.dbi = 0xFFFFFFFE;
            dbi_found:;
        }
    }
    
    // Read key data if available (for seek operations)
    if (key) {
        struct mdbx_val key_val = {};
        if (bpf_probe_read_user(&key_val, sizeof(key_val), key) == 0) {
            cctx.key_size = key_val.iov_len;
            if (cctx.key_size > MAX_KEY_SIZE) {
                cctx.key_size = MAX_KEY_SIZE;
            }
            if (key_val.iov_base && cctx.key_size > 0) {
                bpf_probe_read_user(cctx.key_data, cctx.key_size, key_val.iov_base);
            }
        }
    }
    
    // Save context for uretprobe
    bpf_map_update_elem(&pending_cursors, &pid_tgid, &cctx, BPF_ANY);
    
    return 0;
}

// Uretprobe on mdbx_cursor_get return
SEC("uretprobe/mdbx_cursor_get")
int BPF_URETPROBE(trace_cursor_get_ret, int ret)
{
    __u64 pid_tgid = bpf_get_current_pid_tgid();
    
    // Look up the saved context (named cctx to avoid conflict with BPF_URETPROBE's ctx)
    struct cursor_context *cctx = bpf_map_lookup_elem(&pending_cursors, &pid_tgid);
    if (!cctx) {
        return 0;
    }
    
    // Track errors
    if (ret != 0) {
        inc_stat(STAT_CURSOR_ERRORS);
    }
    
    // Calculate latency
    __u64 now = bpf_ktime_get_ns();
    __u64 latency = now - cctx->timestamp_ns;
    
    // Reserve space in ring buffer for cursor event
    struct cursor_event *e = bpf_ringbuf_reserve(&events, sizeof(*e), 0);
    if (!e) {
        inc_stat(STAT_EVENTS_DROPPED);
        bpf_map_delete_elem(&pending_cursors, &pid_tgid);
        return 0;
    }
    
    // Fill event
    e->timestamp_ns = cctx->timestamp_ns;
    e->pid = cctx->pid;
    e->tid = cctx->tid;
    e->event_type = EVENT_CURSOR_GET;
    e->cursor_op = cctx->cursor_op;
    e->dbi = cctx->dbi;
    e->key_size = cctx->key_size;
    e->return_code = ret;
    e->value_size = 0;  // Not applicable for get
    e->latency_ns = latency;
    e->write_flags = 0;
    e->_pad2 = 0;
    
    // Copy key data
    // Use a loop that the verifier can understand
    #pragma unroll
    for (int i = 0; i < MAX_KEY_SIZE; i++) {
        e->key_data[i] = cctx->key_data[i];
    }
    
    bpf_ringbuf_submit(e, 0);
    
    // Clean up
    bpf_map_delete_elem(&pending_cursors, &pid_tgid);
    
    return 0;
}

// ============================================================================
// MDBX Cursor Open Tracing - captures DBI for each cursor
// ============================================================================
//
// Signature: int mdbx_cursor_open(MDBX_txn *txn, MDBX_dbi dbi, MDBX_cursor **cursor)
//
// We capture the DBI on entry and the cursor pointer on return,
// then store the mapping in cursor_to_dbi map.

SEC("uprobe/mdbx_cursor_open")
int BPF_UPROBE(trace_cursor_open, void *txn, __u32 dbi, void **cursor_ptr)
{
    __u64 pid_tgid = bpf_get_current_pid_tgid();
    __u32 pid = pid_tgid >> 32;
    
    if (!should_trace_pid(pid)) {
        return 0;
    }
    
    // Save context for uretprobe
    struct cursor_open_context octx = {
        .cursor_ptr_ptr = (__u64)cursor_ptr,
        .dbi = dbi,
    };
    bpf_map_update_elem(&pending_cursor_opens, &pid_tgid, &octx, BPF_ANY);
    
    return 0;
}

SEC("uretprobe/mdbx_cursor_open")
int BPF_URETPROBE(trace_cursor_open_ret, int ret)
{
    __u64 pid_tgid = bpf_get_current_pid_tgid();
    
    struct cursor_open_context *octx = bpf_map_lookup_elem(&pending_cursor_opens, &pid_tgid);
    if (!octx) {
        return 0;
    }
    
    // Only record if open succeeded
    if (ret == 0 && octx->cursor_ptr_ptr) {
        // Read the cursor pointer from the output parameter
        void *cursor = NULL;
        if (bpf_probe_read_user(&cursor, sizeof(cursor), (void *)octx->cursor_ptr_ptr) == 0 && cursor) {
            __u64 cursor_addr = (__u64)cursor;
            bpf_map_update_elem(&cursor_to_dbi, &cursor_addr, &octx->dbi, BPF_ANY);
        }
    }
    
    bpf_map_delete_elem(&pending_cursor_opens, &pid_tgid);
    return 0;
}

// Also track cursor close to clean up the map
// Signature: void mdbx_cursor_close(MDBX_cursor *cursor)
SEC("uprobe/mdbx_cursor_close")
int BPF_UPROBE(trace_cursor_close, void *cursor)
{
    __u64 pid_tgid = bpf_get_current_pid_tgid();
    __u32 pid = pid_tgid >> 32;
    
    if (!should_trace_pid(pid)) {
        return 0;
    }
    
    if (cursor) {
        __u64 cursor_addr = (__u64)cursor;
        bpf_map_delete_elem(&cursor_to_dbi, &cursor_addr);
    }
    
    return 0;
}

// ============================================================================
// MDBX Direct Get Tracing (uprobes)
// ============================================================================
//
// These uprobes attach to mdbx_get to trace direct key lookups.
// This is separate from cursor operations - it's used for single key lookups.
//
// Signature: int mdbx_get(const MDBX_txn *txn, MDBX_dbi dbi, 
//                          const MDBX_val *key, MDBX_val *data)
//
// Unlike cursor operations, the DBI is passed directly as a parameter,
// making attribution straightforward.

SEC("uprobe/mdbx_get")
int BPF_UPROBE(trace_direct_get, void *txn, __u32 dbi, struct mdbx_val *key, void *data)
{
    __u64 pid_tgid = bpf_get_current_pid_tgid();
    __u32 pid = pid_tgid >> 32;
    
    // Filter by PID
    if (!should_trace_pid(pid)) {
        return 0;
    }
    
    inc_stat(STAT_DIRECT_GETS);
    
    // Build direct get context
    struct direct_get_context dctx = {
        .timestamp_ns = bpf_ktime_get_ns(),
        .pid = pid,
        .tid = (__u32)pid_tgid,
        .dbi = dbi,
        .key_size = 0,
    };
    
    // Read key data if available
    if (key) {
        struct mdbx_val key_val = {};
        if (bpf_probe_read_user(&key_val, sizeof(key_val), key) == 0) {
            dctx.key_size = key_val.iov_len;
            if (dctx.key_size > MAX_KEY_SIZE) {
                dctx.key_size = MAX_KEY_SIZE;
            }
            if (key_val.iov_base && dctx.key_size > 0) {
                bpf_probe_read_user(dctx.key_data, dctx.key_size, key_val.iov_base);
            }
        }
    }
    
    // Save context for uretprobe
    bpf_map_update_elem(&pending_direct_gets, &pid_tgid, &dctx, BPF_ANY);
    
    return 0;
}

SEC("uretprobe/mdbx_get")
int BPF_URETPROBE(trace_direct_get_ret, int ret)
{
    __u64 pid_tgid = bpf_get_current_pid_tgid();
    
    // Look up the saved context
    struct direct_get_context *dctx = bpf_map_lookup_elem(&pending_direct_gets, &pid_tgid);
    if (!dctx) {
        return 0;
    }
    
    // Track errors
    if (ret != 0) {
        inc_stat(STAT_CURSOR_ERRORS);  // Reuse error counter
    }
    
    // Calculate latency
    __u64 now = bpf_ktime_get_ns();
    __u64 latency = now - dctx->timestamp_ns;
    
    // Reserve space in ring buffer for cursor event (reuse same struct)
    struct cursor_event *e = bpf_ringbuf_reserve(&events, sizeof(*e), 0);
    if (!e) {
        inc_stat(STAT_EVENTS_DROPPED);
        bpf_map_delete_elem(&pending_direct_gets, &pid_tgid);
        return 0;
    }
    
    // Fill event - use EVENT_DIRECT_GET type
    e->timestamp_ns = dctx->timestamp_ns;
    e->pid = dctx->pid;
    e->tid = dctx->tid;
    e->event_type = EVENT_DIRECT_GET;
    e->cursor_op = 0;  // Not applicable for direct get
    e->dbi = dctx->dbi;
    e->key_size = dctx->key_size;
    e->return_code = ret;
    e->value_size = 0;  // Not tracked for direct get
    e->latency_ns = latency;
    e->write_flags = 0;
    e->_pad2 = 0;
    
    // Copy key data
    #pragma unroll
    for (int i = 0; i < MAX_KEY_SIZE; i++) {
        e->key_data[i] = dctx->key_data[i];
    }
    
    bpf_ringbuf_submit(e, 0);
    
    // Clean up
    bpf_map_delete_elem(&pending_direct_gets, &pid_tgid);
    
    return 0;
}

// ============================================================================
// MDBX Cursor Put Tracing (uprobes)
// ============================================================================
//
// Signature: int mdbx_cursor_put(MDBX_cursor *cursor, MDBX_val *key,
//                                 MDBX_val *data, MDBX_put_flags_t flags)
//
// This traces cursor-based write operations which Reth uses heavily for:
// - upsert, insert, append, append_dup

SEC("uprobe/mdbx_cursor_put")
int BPF_UPROBE(trace_cursor_put, void *cursor, struct mdbx_val *key,
               struct mdbx_val *data, __u32 flags)
{
    __u64 pid_tgid = bpf_get_current_pid_tgid();
    __u32 pid = pid_tgid >> 32;

    if (!should_trace_pid(pid)) {
        return 0;
    }

    inc_stat(STAT_CURSOR_PUTS);

    struct cursor_put_context pctx = {
        .timestamp_ns = bpf_ktime_get_ns(),
        .pid = pid,
        .tid = (__u32)pid_tgid,
        .dbi = 0,
        .key_size = 0,
        .value_size = 0,
        .write_flags = flags,
    };

    // Look up the DBI from cursor_to_dbi map
    if (cursor) {
        __u64 cursor_addr = (__u64)cursor;
        __u32 *dbi_ptr = bpf_map_lookup_elem(&cursor_to_dbi, &cursor_addr);
        if (dbi_ptr) {
            pctx.dbi = *dbi_ptr;
        } else {
            pctx.dbi = 0xFFFFFFFE;  // Unknown cursor
        }
    }

    // Read key data
    if (key) {
        struct mdbx_val key_val = {};
        if (bpf_probe_read_user(&key_val, sizeof(key_val), key) == 0) {
            pctx.key_size = key_val.iov_len;
            if (pctx.key_size > MAX_KEY_SIZE) {
                pctx.key_size = MAX_KEY_SIZE;
            }
            if (key_val.iov_base && pctx.key_size > 0) {
                bpf_probe_read_user(pctx.key_data, pctx.key_size, key_val.iov_base);
            }
        }
    }

    // Read value size (not the actual data, just the size)
    if (data) {
        struct mdbx_val data_val = {};
        if (bpf_probe_read_user(&data_val, sizeof(data_val), data) == 0) {
            pctx.value_size = data_val.iov_len;
        }
    }

    bpf_map_update_elem(&pending_cursor_puts, &pid_tgid, &pctx, BPF_ANY);
    return 0;
}

SEC("uretprobe/mdbx_cursor_put")
int BPF_URETPROBE(trace_cursor_put_ret, int ret)
{
    __u64 pid_tgid = bpf_get_current_pid_tgid();

    struct cursor_put_context *pctx = bpf_map_lookup_elem(&pending_cursor_puts, &pid_tgid);
    if (!pctx) {
        return 0;
    }

    if (ret != 0) {
        inc_stat(STAT_CURSOR_ERRORS);
    }

    __u64 now = bpf_ktime_get_ns();
    __u64 latency = now - pctx->timestamp_ns;

    struct cursor_event *e = bpf_ringbuf_reserve(&events, sizeof(*e), 0);
    if (!e) {
        inc_stat(STAT_EVENTS_DROPPED);
        bpf_map_delete_elem(&pending_cursor_puts, &pid_tgid);
        return 0;
    }

    e->timestamp_ns = pctx->timestamp_ns;
    e->pid = pctx->pid;
    e->tid = pctx->tid;
    e->event_type = EVENT_CURSOR_PUT;
    e->cursor_op = 0;  // Not applicable for put
    e->dbi = pctx->dbi;
    e->key_size = pctx->key_size;
    e->return_code = ret;
    e->value_size = pctx->value_size;
    e->latency_ns = latency;
    e->write_flags = pctx->write_flags;
    e->_pad2 = 0;

    #pragma unroll
    for (int i = 0; i < MAX_KEY_SIZE; i++) {
        e->key_data[i] = pctx->key_data[i];
    }

    bpf_ringbuf_submit(e, 0);
    bpf_map_delete_elem(&pending_cursor_puts, &pid_tgid);
    return 0;
}

// ============================================================================
// MDBX Cursor Del Tracing (uprobes)
// ============================================================================
//
// Signature: int mdbx_cursor_del(MDBX_cursor *cursor, MDBX_put_flags_t flags)
//
// This traces cursor-based delete operations

SEC("uprobe/mdbx_cursor_del")
int BPF_UPROBE(trace_cursor_del, void *cursor, __u32 flags)
{
    __u64 pid_tgid = bpf_get_current_pid_tgid();
    __u32 pid = pid_tgid >> 32;

    if (!should_trace_pid(pid)) {
        return 0;
    }

    inc_stat(STAT_CURSOR_DELS);

    struct cursor_del_context dctx = {
        .timestamp_ns = bpf_ktime_get_ns(),
        .pid = pid,
        .tid = (__u32)pid_tgid,
        .dbi = 0,
        .write_flags = flags,
    };

    // Look up the DBI from cursor_to_dbi map
    if (cursor) {
        __u64 cursor_addr = (__u64)cursor;
        __u32 *dbi_ptr = bpf_map_lookup_elem(&cursor_to_dbi, &cursor_addr);
        if (dbi_ptr) {
            dctx.dbi = *dbi_ptr;
        } else {
            dctx.dbi = 0xFFFFFFFE;  // Unknown cursor
        }
    }

    bpf_map_update_elem(&pending_cursor_dels, &pid_tgid, &dctx, BPF_ANY);
    return 0;
}

SEC("uretprobe/mdbx_cursor_del")
int BPF_URETPROBE(trace_cursor_del_ret, int ret)
{
    __u64 pid_tgid = bpf_get_current_pid_tgid();

    struct cursor_del_context *dctx = bpf_map_lookup_elem(&pending_cursor_dels, &pid_tgid);
    if (!dctx) {
        return 0;
    }

    if (ret != 0) {
        inc_stat(STAT_CURSOR_ERRORS);
    }

    __u64 now = bpf_ktime_get_ns();
    __u64 latency = now - dctx->timestamp_ns;

    struct cursor_event *e = bpf_ringbuf_reserve(&events, sizeof(*e), 0);
    if (!e) {
        inc_stat(STAT_EVENTS_DROPPED);
        bpf_map_delete_elem(&pending_cursor_dels, &pid_tgid);
        return 0;
    }

    e->timestamp_ns = dctx->timestamp_ns;
    e->pid = dctx->pid;
    e->tid = dctx->tid;
    e->event_type = EVENT_CURSOR_DEL;
    e->cursor_op = 0;
    e->dbi = dctx->dbi;
    e->key_size = 0;  // No key for delete at current position
    e->return_code = ret;
    e->value_size = 0;
    e->latency_ns = latency;
    e->write_flags = dctx->write_flags;
    e->_pad2 = 0;

    // Zero out key_data
    #pragma unroll
    for (int i = 0; i < MAX_KEY_SIZE; i++) {
        e->key_data[i] = 0;
    }

    bpf_ringbuf_submit(e, 0);
    bpf_map_delete_elem(&pending_cursor_dels, &pid_tgid);
    return 0;
}

// ============================================================================
// MDBX Transaction Lifecycle Tracing (uprobes)
// ============================================================================
//
// These trace transaction begin/commit/abort to understand:
// - Transaction duration
// - Concurrent transactions (RO vs RW)
// - Thread to transaction mapping

// Signature: int mdbx_txn_begin_ex(MDBX_env *env, MDBX_txn *parent,
//                                   MDBX_txn_flags_t flags, MDBX_txn **txn, void *context)
SEC("uprobe/mdbx_txn_begin_ex")
int BPF_UPROBE(trace_txn_begin, void *env, void *parent, __u32 flags,
               void **txn_ptr, void *context)
{
    __u64 pid_tgid = bpf_get_current_pid_tgid();
    __u32 pid = pid_tgid >> 32;

    if (!should_trace_pid(pid)) {
        return 0;
    }

    inc_stat(STAT_TXN_BEGINS);

    struct txn_begin_context bctx = {
        .timestamp_ns = bpf_ktime_get_ns(),
        .pid = pid,
        .tid = (__u32)pid_tgid,
        .txn_flags = flags,
        .parent_txn_ptr = (__u64)parent,
        .txn_ptr_ptr = (__u64)txn_ptr,
    };

    bpf_map_update_elem(&pending_txn_begins, &pid_tgid, &bctx, BPF_ANY);
    return 0;
}

SEC("uretprobe/mdbx_txn_begin_ex")
int BPF_URETPROBE(trace_txn_begin_ret, int ret)
{
    __u64 pid_tgid = bpf_get_current_pid_tgid();

    struct txn_begin_context *bctx = bpf_map_lookup_elem(&pending_txn_begins, &pid_tgid);
    if (!bctx) {
        return 0;
    }

    __u64 now = bpf_ktime_get_ns();
    __u64 latency = now - bctx->timestamp_ns;

    // Read the transaction pointer from the output parameter
    __u64 txn_ptr = 0;
    if (ret == 0 && bctx->txn_ptr_ptr) {
        void *txn = NULL;
        if (bpf_probe_read_user(&txn, sizeof(txn), (void *)bctx->txn_ptr_ptr) == 0) {
            txn_ptr = (__u64)txn;
        }
    }

    struct txn_event *e = bpf_ringbuf_reserve(&events, sizeof(*e), 0);
    if (!e) {
        inc_stat(STAT_EVENTS_DROPPED);
        bpf_map_delete_elem(&pending_txn_begins, &pid_tgid);
        return 0;
    }

    e->timestamp_ns = bctx->timestamp_ns;
    e->pid = bctx->pid;
    e->tid = bctx->tid;
    e->event_type = EVENT_TXN_BEGIN;
    e->txn_flags = bctx->txn_flags;
    e->txn_ptr = txn_ptr;
    e->parent_txn_ptr = bctx->parent_txn_ptr;
    e->latency_ns = latency;
    e->return_code = ret;
    e->_pad = 0;

    bpf_ringbuf_submit(e, 0);
    bpf_map_delete_elem(&pending_txn_begins, &pid_tgid);
    return 0;
}

// Signature: int mdbx_txn_commit_ex(MDBX_txn *txn, MDBX_commit_latency *latency)
SEC("uprobe/mdbx_txn_commit_ex")
int BPF_UPROBE(trace_txn_commit, void *txn, void *latency)
{
    __u64 pid_tgid = bpf_get_current_pid_tgid();
    __u32 pid = pid_tgid >> 32;

    if (!should_trace_pid(pid)) {
        return 0;
    }

    inc_stat(STAT_TXN_COMMITS);

    struct txn_commit_context cctx = {
        .timestamp_ns = bpf_ktime_get_ns(),
        .pid = pid,
        .tid = (__u32)pid_tgid,
        .txn_ptr = (__u64)txn,
    };

    bpf_map_update_elem(&pending_txn_commits, &pid_tgid, &cctx, BPF_ANY);
    return 0;
}

SEC("uretprobe/mdbx_txn_commit_ex")
int BPF_URETPROBE(trace_txn_commit_ret, int ret)
{
    __u64 pid_tgid = bpf_get_current_pid_tgid();

    struct txn_commit_context *cctx = bpf_map_lookup_elem(&pending_txn_commits, &pid_tgid);
    if (!cctx) {
        return 0;
    }

    __u64 now = bpf_ktime_get_ns();
    __u64 latency = now - cctx->timestamp_ns;

    struct txn_event *e = bpf_ringbuf_reserve(&events, sizeof(*e), 0);
    if (!e) {
        inc_stat(STAT_EVENTS_DROPPED);
        bpf_map_delete_elem(&pending_txn_commits, &pid_tgid);
        return 0;
    }

    e->timestamp_ns = cctx->timestamp_ns;
    e->pid = cctx->pid;
    e->tid = cctx->tid;
    e->event_type = EVENT_TXN_COMMIT;
    e->txn_flags = 0;  // Not available at commit time
    e->txn_ptr = cctx->txn_ptr;
    e->parent_txn_ptr = 0;
    e->latency_ns = latency;
    e->return_code = ret;
    e->_pad = 0;

    bpf_ringbuf_submit(e, 0);
    bpf_map_delete_elem(&pending_txn_commits, &pid_tgid);
    return 0;
}

// Signature: int mdbx_txn_abort(MDBX_txn *txn)
// For abort, we don't need entry/return correlation - just emit on entry
SEC("uprobe/mdbx_txn_abort")
int BPF_UPROBE(trace_txn_abort, void *txn)
{
    __u64 pid_tgid = bpf_get_current_pid_tgid();
    __u32 pid = pid_tgid >> 32;

    if (!should_trace_pid(pid)) {
        return 0;
    }

    inc_stat(STAT_TXN_ABORTS);

    struct txn_event *e = bpf_ringbuf_reserve(&events, sizeof(*e), 0);
    if (!e) {
        inc_stat(STAT_EVENTS_DROPPED);
        return 0;
    }

    e->timestamp_ns = bpf_ktime_get_ns();
    e->pid = pid;
    e->tid = (__u32)pid_tgid;
    e->event_type = EVENT_TXN_ABORT;
    e->txn_flags = 0;
    e->txn_ptr = (__u64)txn;
    e->parent_txn_ptr = 0;
    e->latency_ns = 0;
    e->return_code = 0;
    e->_pad = 0;

    bpf_ringbuf_submit(e, 0);
    return 0;
}

char LICENSE[] SEC("license") = "Dual BSD/GPL";
