# reth-mdbx-profiler internals

this document explains how the profiler works in depth, focusing on the ebpf tracer and the overall architecture.

## overview

the profiler uses ebpf to trace memory-mapped file accesses in the linux kernel. when reth accesses its mdbx database, the kernel handles page faults for memory-mapped regions. we hook into these page fault handlers to capture every access pattern.

```
┌─────────────┐     ┌──────────────┐     ┌─────────────┐
│   reth      │────>│ linux kernel │────>│ ebpf probes │
│  (mdbx)     │     │ page faults  │     │             │
└─────────────┘     └──────────────┘     └──────┬──────┘
                                                │
                                                v
                                         ┌──────────────┐
                                         │ ring buffer  │
                                         └──────┬───────┘
                                                │
                                                v
                                         ┌──────────────┐
                                         │  userspace   │
                                         │  collector   │
                                         └──────────────┘
```

## ebpf tracer (bpf/mdbx_tracer.bpf.c)

### what is ebpf?

ebpf is a linux kernel technology that allows running sandboxed programs in kernel space. these programs are verified by the kernel for safety (no infinite loops, bounded memory access) and can attach to various kernel events like syscalls, tracepoints, and function calls.

### probe types used

#### 1. kprobe/kretprobe on `handle_mm_fault`

the main tracing mechanism. `handle_mm_fault` is the kernel function called whenever a page fault occurs for a user-space memory access.

```c
SEC("kprobe/handle_mm_fault")
int BPF_KPROBE(trace_page_fault, 
               struct vm_area_struct *vma,
               unsigned long address,
               unsigned int flags)
```

**parameters:**
- `vma`: virtual memory area struct - contains info about the memory mapping
- `address`: the faulting virtual address
- `flags`: fault flags (read/write/etc)

**what we extract:**
- `vma->vm_start`, `vma->vm_end`: bounds of the memory mapping
- `vma->vm_file`: the file being mapped (if any)
- `vma->vm_pgoff`: page offset within the file
- `vma->vm_file->f_inode->i_ino`: inode number to identify the mdbx file

the kretprobe captures the return value which tells us if it was a **major fault** (required disk i/o) or **minor fault** (page was in cache):

```c
SEC("kretprobe/handle_mm_fault")
int BPF_KRETPROBE(trace_page_fault_ret, vm_fault_t ret)
{
    // VM_FAULT_MAJOR (0x0004) indicates disk I/O was required
    __u8 is_major = (ret & VM_FAULT_MAJOR) ? 1 : 0;
}
```

#### 2. kprobe on `do_mmap`

optional tracing of new memory mappings to detect when mdbx maps new regions:

```c
SEC("kprobe/do_mmap")
int BPF_KPROBE(trace_mmap,
               struct file *file,
               unsigned long addr,
               unsigned long len,
               ...)
```

#### 3. uprobes for cursor operations

user-space probes that attach to libmdbx functions to trace database operations:

```c
SEC("uprobe/mdbx_cursor_get")
int BPF_UPROBE(trace_cursor_get, void *cursor, struct mdbx_val *key, 
               void *data, int op)
```

these require the reth binary to have symbols (not stripped).

### bpf maps

maps are the primary data structures for ebpf programs to store state and communicate with userspace.

#### ring buffer (`events`)

```c
struct {
    __uint(type, BPF_MAP_TYPE_RINGBUF);
    __uint(max_entries, 16 * 1024 * 1024);  // 16MB
} events SEC(".maps");
```

a lock-free, multi-producer single-consumer queue. events are written by the bpf program and read by userspace. 16mb provides enough buffer for high-throughput tracing.

#### tracked inodes (`tracked_inodes`)

```c
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 16);
    __type(key, __u64);    // inode number
    __type(value, __u8);   // 1 = trace this file
} tracked_inodes SEC(".maps");
```

userspace registers the mdbx.dat file's inode here. the bpf program only traces page faults for files in this map.

#### vma tracking (`vma_to_offset`)

```c
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 256);
    __type(key, __u64);    // vma->vm_start
    __type(value, __u64);  // file offset base
} vma_to_offset SEC(".maps");
```

caches the mapping from virtual memory areas to file offsets. when we see a new vma for a tracked inode, we calculate its file offset base (`vm_pgoff * PAGE_SIZE`) and store it here.

#### pending contexts

```c
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 10240);
    __type(key, __u64);    // pid_tgid
    __type(value, struct fault_context);
} pending_faults SEC(".maps");
```

correlates kprobe entry with kretprobe return. since bpf programs are stateless between calls, we save context on entry and retrieve it on return using the thread id as key.

#### statistics (`stats`)

```c
struct {
    __uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
    __uint(max_entries, 8);
    __type(key, __u32);
    __type(value, __u64);
} stats SEC(".maps");
```

per-cpu counters for statistics. percpu avoids lock contention - each cpu has its own counter array, userspace sums them.

### event structures

#### page fault event

```c
struct page_fault_event {
    __u64 timestamp_ns;      // bpf_ktime_get_ns()
    __u64 address;           // faulting virtual address
    __u64 file_offset;       // offset within mdbx.dat
    __u64 vma_start;         // vma bounds for context
    __u64 vma_end;
    __u32 pid;               // process id
    __u32 tid;               // thread id
    __u32 event_type;        // 1=page_fault, 2=mmap
    __u32 fault_flags;       // read/write/etc
    __u64 latency_ns;        // time in fault handler
    __u8  is_major;          // major (disk) vs minor (cache)
};
```

#### cursor event

```c
struct cursor_event {
    __u64 timestamp_ns;
    __u32 pid;
    __u32 tid;
    __u32 event_type;        // 3=cursor_get, 4=cursor_put
    __u32 cursor_op;         // MDBX_SET_RANGE, MDBX_NEXT, etc.
    __u32 dbi;               // database index (table id)
    __u32 key_size;
    __u8  key_data[64];      // first 64 bytes of lookup key
    __s32 return_code;       // 0=success, -30798=not_found
    __u64 latency_ns;
};
```

### filtering logic

the bpf program filters events efficiently in kernel space:

```c
static __always_inline bool should_trace_pid(__u32 pid) {
    __u32 key = 0;
    __u32 *target_pid = bpf_map_lookup_elem(&profiler_config, &key);
    if (!target_pid || *target_pid == 0) {
        return true;  // trace all if not configured
    }
    return pid == *target_pid;
}
```

1. **pid filter**: only trace the target reth process
2. **inode filter**: only trace page faults on mdbx.dat
3. **vma caching**: skip expensive file lookups for known vmas

### calculating file offset

the key insight is converting virtual addresses to file offsets:

```c
// vma->vm_pgoff is the page offset within the file
__u64 pgoff = BPF_CORE_READ(vma, vm_pgoff);
offset_base_val = pgoff * 4096;  // PAGE_SIZE

// file_offset = base + (virtual_addr - vma_start)
__u64 file_offset = offset_base_val + (address - vm_start);
```

this tells us exactly which byte of mdbx.dat was accessed, which we later map to mdbx tables.

## userspace components

### main.rs (tracer)

1. **loads bpf object**: compiled bpf program from `mdbx_tracer.bpf.o`
2. **configures maps**: sets target pid, registers mdbx inode
3. **attaches probes**: kprobe/kretprobe on `handle_mm_fault`
4. **polls ring buffer**: reads events and writes to jsonl file

```rust
let mut ring_builder = RingBufferBuilder::new();
ring_builder.add(&events_map, move |data: &[u8]| {
    // deserialize event from raw bytes
    let event: PageFaultEvent = unsafe { 
        std::ptr::read_unaligned(data.as_ptr() as *const PageFaultEvent) 
    };
    // write as json line
    writeln!(writer, "{}", serde_json::to_string(&event)?);
    0
})?;
```

### event.rs

defines rust structs matching the bpf event layouts. must be `#[repr(C)]` with exact same memory layout:

```rust
#[repr(C)]
pub struct PageFaultEvent {
    pub timestamp_ns: u64,
    pub address: u64,
    pub file_offset: u64,
    // ... must match C struct exactly
}
```

also provides:
- `dbi_to_table_name()`: maps mdbx database index to reth table name
- `CursorOp`: enum for mdbx cursor operations with helper methods

### analyzer.rs

standalone tool that processes trace files:

1. parses jsonl events
2. loads mdbx metadata (optional, via `mdbx_stat`)
3. generates statistics and visualizations
4. outputs html viewer or csv/json

### mdbx_metadata.rs

handles mdbx file structure and table attribution:

#### mdbx file layout

```
page 0-1:  meta pages (alternating for atomic updates)
page 2:    free list (garbage collector)
page 3+:   b+ tree nodes and data for all tables
```

#### table attribution

since mdbx doesn't store per-page table ownership in the file header, we use:

1. **mdbx_stat**: external tool that reads mdbx internal structures
2. **proportional attribution**: distribute faults based on table sizes
3. **heuristics**: use access patterns to guess table

```rust
pub fn run_mdbx_stat(path: &Path) -> Option<Vec<MdbxStatOutput>> {
    let output = Command::new("mdbx_stat")
        .arg("-a")
        .arg(path)
        .output()?;
    parse_mdbx_stat_output(&output.stdout)
}
```

#### dbi (database index) mapping

mdbx assigns dbi numbers dynamically when tables are opened. reth opens tables in a specific order:

```rust
// dbi 0: @main (mdbx internal)
// dbi 1: @free (free list)
// dbi 2: CanonicalHeaders
// dbi 3: HeaderTerminalDifficulties
// ...
// dbi 21: AccountsTrie
// dbi 22: StoragesTrie
```

### viewer/mod.rs

generates interactive html visualization:

1. **timeline**: page faults over time, bucketed by 100ms
2. **heatmap**: 2d grid of time vs file offset
3. **table breakdown**: faults per mdbx table
4. **thread distribution**: which threads cause most faults
5. **hot pages**: most frequently accessed pages
6. **pattern analysis**: sequential vs random access ratio
7. **prefetch analysis**: predictability score

the html is self-contained with embedded javascript and css.

## data flow

```
1. reth accesses mdbx.dat via mmap'd memory
   
2. cpu triggers page fault (page not in memory or not present)
   
3. kernel calls handle_mm_fault()
   
4. kprobe fires, bpf program runs:
   - checks pid filter
   - checks if file inode is tracked
   - calculates file offset from vma
   - saves context to pending_faults map
   
5. kernel handles fault (may do disk i/o for major fault)
   
6. kretprobe fires:
   - retrieves saved context
   - checks return value for major/minor
   - reserves space in ring buffer
   - writes event
   
7. userspace polls ring buffer:
   - deserializes events
   - writes to jsonl file
   
8. analyzer processes trace:
   - loads mdbx metadata
   - computes statistics
   - generates visualization
```

## mdbx cursor operations

when `--trace-cursors` is enabled, we also trace mdbx api calls:

### mdbx_cursor_get

```c
int mdbx_cursor_get(MDBX_cursor *cursor, MDBX_val *key, 
                    MDBX_val *data, MDBX_cursor_op op);
```

cursor operations we track:
- `MDBX_SET_RANGE` (17): seek to key >= given key (b+ tree traversal)
- `MDBX_NEXT` (8): move to next entry (sequential scan)
- `MDBX_FIRST` (0): move to first entry
- `MDBX_SET` (15): seek to exact key

### dbi tracking

we capture which table each cursor operates on:

```c
SEC("uprobe/mdbx_cursor_open")
int BPF_UPROBE(trace_cursor_open, void *txn, __u32 dbi, void **cursor_ptr)
{
    // save dbi for this cursor pointer
}

SEC("uretprobe/mdbx_cursor_open")
int BPF_URETPROBE(trace_cursor_open_ret, int ret)
{
    // read cursor pointer from output param
    // map cursor address -> dbi
}
```

when we later see `mdbx_cursor_get`, we look up the cursor address to find its dbi.

## performance considerations

### bpf overhead

- kprobes add ~100-500ns per call
- ring buffer writes are lock-free
- per-cpu stats avoid cache contention

### ring buffer sizing

16mb buffer at 10k events/sec = ~1.6 seconds of buffering. if userspace falls behind, oldest events are dropped (tracked in `STAT_EVENTS_DROPPED`).

### filtering in kernel

all filtering happens in bpf before events reach userspace:
- wrong pid? return early
- wrong inode? return early
- no ring buffer space? increment dropped counter

## limitations

### 4kb page size assumption

the tracer assumes standard 4kb pages:

```c
offset_base_val = pgoff * 4096;  // hardcoded PAGE_SIZE
```

huge pages (2mb, 1gb) would need different handling.

### btf requirement

the tracer uses co-re (compile once, run everywhere) which requires btf (bpf type format) in the kernel:

```
/sys/kernel/btf/vmlinux must exist
```

this provides type information so the bpf program can read kernel structures correctly across different kernel versions.

### symbol requirement for cursor tracing

uprobe attachment needs symbol addresses:

```rust
fn find_symbol_offset(binary: &Path, symbol: &str) -> Option<u64> {
    let output = Command::new("nm").arg(binary).output()?;
    // parse nm output for symbol address
}
```

stripped binaries won't work for cursor tracing.

### ring buffer drops

under extreme load, the ring buffer may fill before userspace can drain it. the `STAT_EVENTS_DROPPED` counter tracks this.

## building the bpf program

the bpf program is compiled with clang:

```bash
clang -g -O2 -target bpf \
    -I/path/to/vmlinux.h \
    -c bpf/mdbx_tracer.bpf.c \
    -o target/release/mdbx_tracer.bpf.o
```

`vmlinux.h` is generated from the running kernel's btf:

```bash
bpftool btf dump file /sys/kernel/btf/vmlinux format c > bpf/vmlinux.h
```

this header contains all kernel type definitions needed for reading kernel structures.
