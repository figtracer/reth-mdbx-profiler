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

## the table attribution problem

mdbx stores all tables in a single file (mdbx.dat) as interleaved b+ tree pages. unlike databases with separate files per table, you **cannot** map a file offset directly to a table - page 1000 might belong to HashedStorages, page 1001 to AccountsTrie.

### the solution: timestamp correlation

we correlate page faults with cursor operations using **thread id + timestamp matching**:

```
┌─────────────────────────────────────────────────────────────────┐
│ thread 1234                                                     │
│                                                                 │
│  cursor_get(HashedStorages) ─────────────────────>              │
│  [start]                    [fault!]              [end]         │
│  t=1000                     t=1050               t=1200         │
│                                                                 │
│  the fault at t=1050 is attributed to HashedStorages            │
└─────────────────────────────────────────────────────────────────┘
```

for each page fault, we find cursor operations on the **same thread** where:

```
cursor_start <= fault_timestamp <= cursor_start + latency
```

this gives us accurate per-table attribution for faults that occur during cursor operations.

### correlation rate

typically 50-70% of faults can be correlated. the remainder are faults from the same process that occur outside cursor get operation windows:

- **cursor open/close**: we only time `mdbx_cursor_get`, not setup/teardown. faults during `mdbx_cursor_open` aren't correlated
- **transaction begin/commit**: mdbx transaction operations touch pages but we don't trace them
- **write operations**: we trace reads (`mdbx_cursor_get`, `mdbx_get`) but not writes (`mdbx_cursor_put`, `mdbx_put`)
- **between cursor operations**: faults while reth processes results between cursor calls
- **kernel readahead**: when reth touches a page, linux may prefetch nearby pages asynchronously

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

SEC("uprobe/mdbx_cursor_open")
int BPF_UPROBE(trace_cursor_open, void *txn, __u32 dbi, void **cursor_ptr)

SEC("uprobe/mdbx_get")
int BPF_UPROBE(trace_direct_get, void *txn, __u32 dbi, struct mdbx_val *key, void *data)
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

#### cursor to dbi mapping (`cursor_to_dbi`)

```c
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 10240);
    __type(key, __u64);    // cursor pointer address
    __type(value, __u32);  // dbi (table id)
} cursor_to_dbi SEC(".maps");
```

maps cursor pointers to their table id. populated by `mdbx_cursor_open` uprobe, used by `mdbx_cursor_get` to know which table is being accessed.

#### profiler config (`profiler_config`)

```c
struct {
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __uint(max_entries, 1);
    __type(key, __u32);
    __type(value, __u32);  // target PID (0 = trace all)
} profiler_config SEC(".maps");
```

stores the target pid. updated dynamically when using `--process-name` and the process restarts.

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
    __u32 event_type;        // 3=cursor_get, 4=cursor_put, 5=direct_get
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

this tells us exactly which byte of mdbx.dat was accessed.

## userspace components

### main.rs (tracer)

1. **loads bpf object**: compiled bpf program from `mdbx_tracer.bpf.o`
2. **configures maps**: sets target pid, registers mdbx inode
3. **attaches probes**: kprobe/kretprobe on `handle_mm_fault`, uprobes on mdbx functions
4. **polls ring buffer**: reads events and writes to jsonl file
5. **monitors process**: if using `--process-name`, detects restarts and updates pid

```rust
// process restart detection
if !is_process_running(current_pid) {
    info!("Process exited. Waiting for restart...");
    current_pid = 0;
    update_target_pid(&mut obj, 0)?;
    clear_cursor_to_dbi_map(&mut obj)?;  // clear stale cursor mappings
}

let pids = find_pids_by_name(&name);
if !pids.is_empty() {
    current_pid = pids[0];
    update_target_pid(&mut obj, current_pid)?;
    info!("Process restarted with PID {}. Tracing resumed.", current_pid);
}
```

### viewer/mod.rs

generates interactive html visualization with:

1. **timeline**: page faults and cursor ops over time
2. **heatmap**: 2d grid of time vs file offset
3. **table breakdown**: faults per mdbx table (correlated)
4. **cursor operations**: ops by table, by type, latency distribution
5. **slow operations**: operations >100μs, likely page faults
6. **slow keys**: frequently accessed keys with high latency
7. **thread distribution**: which threads cause most faults
8. **pattern analysis**: sequential vs random access ratio

#### correlation implementation

```rust
pub fn correlate_faults_with_cursors(
    page_faults: &[&PageFaultEvent],
    cursor_events: &[CursorEvent],
) -> FaultCorrelation {
    // build index of cursor operations by thread id
    // for each cursor: (start_time, end_time, dbi, table_name)
    let mut cursor_windows_by_tid: HashMap<u32, Vec<(u64, u64, u32, String)>> = HashMap::new();
    
    for cursor in cursor_events {
        let start_time = cursor.timestamp_ns;
        let end_time = cursor.timestamp_ns + cursor.latency_ns;
        cursor_windows_by_tid
            .entry(cursor.tid)
            .or_default()
            .push((start_time, end_time, cursor.dbi, table_name));
    }
    
    // sort by start time for binary search
    for windows in cursor_windows_by_tid.values_mut() {
        windows.sort_by_key(|w| w.0);
    }
    
    // correlate each fault
    for fault in page_faults {
        if let Some(windows) = cursor_windows_by_tid.get(&fault.tid) {
            // binary search for matching window
            for (start, end, dbi, table_name) in windows {
                if fault.timestamp_ns >= start && fault.timestamp_ns <= end {
                    // fault occurred during this cursor operation
                    correlated_faults.entry(table_name).or_insert((0, 0)).0 += 1;
                    break;
                }
            }
        }
    }
}
```

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
   - writes event to ring buffer
   
7. simultaneously, uprobes trace cursor operations:
   - mdbx_cursor_open: maps cursor pointer -> dbi
   - mdbx_cursor_get: records operation with dbi, key, latency
   - mdbx_get: records direct get with dbi, key, latency
   
8. userspace polls ring buffer:
   - deserializes events
   - writes to jsonl file
   
9. analyzer processes trace:
   - correlates page faults with cursor ops by tid + timestamp
   - generates visualization with accurate table attribution
```

## mdbx cursor operations

when `--trace-cursors` is enabled, we trace mdbx api calls:

### mdbx_cursor_get

```c
int mdbx_cursor_get(MDBX_cursor *cursor, MDBX_val *key, 
                    MDBX_val *data, MDBX_cursor_op op);
```

cursor operations we track:
- `MDBX_SET_RANGE` (17): seek to key >= given key (b+ tree traversal)
- `MDBX_SET` (15): seek to exact key
- `MDBX_GET_BOTH_RANGE` (3): seek in dupsort table
- `MDBX_NEXT` (8): move to next entry (sequential scan)
- `MDBX_PREV` (12): move to previous entry
- `MDBX_FIRST` (0): move to first entry

### mdbx_get (direct lookup)

```c
int mdbx_get(MDBX_txn *txn, MDBX_dbi dbi, MDBX_val *key, MDBX_val *data);
```

direct key lookup without cursor. we trace these as `event_type=5` (DIRECT_GET).

### dbi tracking

we capture which table each cursor operates on:

```c
SEC("uprobe/mdbx_cursor_open")
int BPF_UPROBE(trace_cursor_open, void *txn, __u32 dbi, void **cursor_ptr)
{
    // save dbi and cursor_ptr location for return probe
    struct cursor_open_context ctx = { .dbi = dbi, .cursor_ptr = cursor_ptr };
    bpf_map_update_elem(&pending_cursor_opens, &pid_tgid, &ctx, BPF_ANY);
}

SEC("uretprobe/mdbx_cursor_open")
int BPF_URETPROBE(trace_cursor_open_ret, int ret)
{
    // read cursor pointer from output param
    // map cursor address -> dbi
    void *cursor;
    bpf_probe_read_user(&cursor, sizeof(cursor), ctx->cursor_ptr);
    bpf_map_update_elem(&cursor_to_dbi, &cursor, &ctx->dbi, BPF_ANY);
}
```

when we later see `mdbx_cursor_get`, we look up the cursor address to find its dbi.

### pre-trace cursors

cursors opened before tracing started won't be in our `cursor_to_dbi` map. these show as "Unknown (pre-trace cursors)" in the output. using `--process-name` and restarting the node after the profiler starts avoids this issue.

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

### correlation coverage

not all page faults can be correlated. we only trace `mdbx_cursor_get` and `mdbx_get` - faults during other operations (cursor open/close, transaction begin/commit, writes) won't be attributed to tables.

expect 50-70% correlation rate in typical workloads. this can be improved by adding tracing for write operations and transaction management.

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
