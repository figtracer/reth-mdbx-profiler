# reth-mdbx-profiler internals

this document explains how the profiler works in depth, focusing on the ebpf tracer and the overall architecture.

## overview

the profiler uses ebpf to trace memory-mapped file accesses in the linux kernel. when reth accesses its mdbx database, the kernel handles page faults for memory-mapped regions. we hook into these page fault handlers to capture every access pattern.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ 1. RETH PROCESS                                                             │
│                                                                             │
│    reth accesses mdbx.dat via mmap'd memory                                 │
│    → cpu triggers page fault (page not in tlb or not present)               │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ 2. KERNEL PAGE FAULT HANDLER                                                │
│                                                                             │
│    handle_mm_fault(vma, address, flags)                                     │
│    │                                                                        │
│    ├─► kprobe fires                                                         │
│    │   • check pid filter                                                   │
│    │   • check inode filter                                                 │
│    │   • calculate file offset                                              │
│    │   • save context to pending_faults                                     │
│    │                                                                        │
│    ├─► kernel handles fault                                                 │
│    │   • minor: map page from cache                                         │
│    │   • major: read from disk, then map                                    │
│    │                                                                        │
│    └─► kretprobe fires                                                      │
│        • retrieve saved context                                             │
│        • check VM_FAULT_MAJOR bit                                           │
│        • calculate latency                                                  │
│        • emit page_fault_event to ring buffer                               │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ 3. MDBX CURSOR OPERATIONS (concurrent)                                      │
│                                                                             │
│    mdbx_cursor_open(txn, dbi, &cursor)                                      │
│    │                                                                        │
│    ├─► uprobe: save (cursor_ptr_ptr, dbi)                                   │
│    └─► uretprobe: map cursor_addr → dbi                                     │
│                                                                             │
│    mdbx_cursor_get(cursor, key, data, op)           [READ OPERATIONS]       │
│    │                                                                        │
│    ├─► uprobe: lookup dbi, save context                                     │
│    │   • reads key data via bpf_probe_read_user                             │
│    │   • page faults may occur here! ◄──────────────────────────────────────┤
│    │                                                                        │
│    └─► uretprobe: emit cursor_event to ring buffer                          │
│                                                                             │
│    mdbx_cursor_put(cursor, key, data, flags)        [WRITE OPERATIONS]      │
│    │                                                                        │
│    ├─► uprobe: lookup dbi, capture key + value_size + flags                 │
│    │   • page faults during b-tree traversal and page splits               │
│    │                                                                        │
│    └─► uretprobe: emit cursor_event with event_type=PUT                     │
│                                                                             │
│    mdbx_cursor_del(cursor, flags)                   [DELETE OPERATIONS]     │
│    │                                                                        │
│    ├─► uprobe: lookup dbi, capture flags                                    │
│    └─► uretprobe: emit cursor_event with event_type=DEL                     │
│                                                                             │
│    mdbx_cursor_close(cursor)                                                │
│    └─► uprobe: remove cursor from cursor_to_dbi map                         │  
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ 4. RING BUFFER (16MB)                                                       │
│                                                                             │
│    [event][event][event][event][event]...                                   │
│    ▲                                  │                                     │
│    │ bpf writes                       │ userspace reads                     │
│    │ (lock-free)                      ▼                                     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ 5. USERSPACE TRACER                                                         │
│                                                                             │
│    loop {                                                                   │
│        ring.poll(100ms)                                                     │
│        for event in new_events {                                            │
│            let json = serde_json::to_string(&event)?;                       │
│            writeln!(file, "{}", json)?;                                     │
│        }                                                                    │
│        check_process_restart();                                             │
│    }                                                                        │
│                                                                             │
│    output: trace.jsonl                                                      │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ 6. ANALYZER (post-processing)                                               │
│                                                                             │
│    • load events from jsonl                                                 │
│    • correlate faults with cursor ops (tid + timestamp)                     │
│    • calculate statistics                                                   │
│    • generate html visualization                                            │
│                                                                             │
│    output: trace.html                                                       │
└─────────────────────────────────────────────────────────────────────────────┘
```

## viewer visualizations

the html viewer has three tabs: overview, cursor ops, and mdbx txns. each visualization serves a specific purpose for understanding database performance.

### overview tab

#### fault timeline (interactive)
- **what it shows**: page faults over time during the trace
- **x-axis**: time since trace start (seconds/minutes)
- **y-axis**: number of faults per time bucket
- **interaction**: drag to zoom into a time range, double-click to reset
- **what to look for**: spikes indicate I/O-heavy periods (block execution, trie computation)

#### access heatmap (interactive)
- **what it shows**: 2D density of page faults by time and file offset
- **x-axis**: time during trace
- **y-axis**: file offset in mdbx.dat (GB)
- **color**: blue gradient - darker = fewer faults, brighter = more faults
- **interaction**: hover to see exact time, offset, and fault count
- **what to look for**: 
  - horizontal bands = sequential access to a file region
  - scattered dots = random access (bad for performance)
  - bright spots = hot regions causing many faults

#### page faults by table
- **what it shows**: which tables caused the most page faults
- **columns**: table name, total faults, major faults, percentage
- **note**: faults are attributed via timestamp correlation with cursor ops

#### access patterns
- **sequential vs random**: ratio of sequential (stride ≤4 pages) vs random access
- **top stride patterns**: common access strides (e.g., "sequential-forward", "random-jump")
- **top threads**: which threads cause the most faults

### cursor ops tab

requires `--trace-cursors` flag during tracing.

#### metrics row
- **total ops**: total cursor operations traced
- **ops/sec**: operation throughput
- **avg/p50/p95/p99 latency**: operation latency distribution
- **seeks vs navigation**: seek ops traverse B+ tree, navigation ops move to adjacent keys

#### operations by type
- **what it shows**: breakdown of cursor operations (SET_RANGE, NEXT, PREV, etc.)
- **what to look for**: high seek ratio = random access pattern, high NEXT ratio = sequential scan

#### operations by table
- **what it shows**: which tables have the most cursor activity
- **what to look for**: tables with high ops but low faults = good cache hit rate

#### slow operations (>100μs)
- **what it shows**: operations that took >100μs - likely caused page faults
- **columns**: table, slow count, total count, %, avg/max latency, time lost
- **what to look for**: tables with high slow % are causing disk I/O

#### hot keys
- **what it shows**: specific keys that are frequently slow
- **what to look for**: repeated slow access to same key = cache thrashing

### mdbx txns tab

requires `--trace-cursors` flag during tracing.

#### metrics row
- **total txns**: transaction count (begin events)
- **txns/sec**: transaction throughput
- **RO/RW**: read-only vs read-write transaction counts
- **commits/aborts**: how transactions ended (RO txns typically "abort" - this is normal)

#### concurrency chart (interactive)
- **what it shows**: concurrent read-only transactions over time
- **x-axis**: time during trace
- **y-axis**: number of concurrent RO transactions
- **interaction**: drag to zoom, double-click to reset
- **what to look for**: high concurrency = good parallelism, drops = writer blocking readers

#### rw commit latency timeline (interactive)
- **what it shows**: when RW commits happen and how long they take
- **x-axis**: time during trace (seconds)
- **y-axis**: commit latency (milliseconds)
- **each bar**: one RW commit at that point in time
- **interaction**: drag to zoom, double-click to reset
- **what to look for**:
  - regular spacing = block-by-block commits (~12s apart on mainnet)
  - tall bars = slow commits (large write batches or disk pressure)
  - clusters = multiple commits in quick succession

#### thread distribution
- **what it shows**: which threads are doing transaction work
- **what to look for**: RW transactions should be on dedicated writer thread

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
- **write operations**: page faults during `mdbx_cursor_put` and `mdbx_cursor_del` are now correlated (see write operation tracing below)
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

SEC("uprobe/mdbx_cursor_put")
int BPF_UPROBE(trace_cursor_put, void *cursor, struct mdbx_val *key,
               struct mdbx_val *data, __u32 flags)

SEC("uprobe/mdbx_cursor_del")
int BPF_UPROBE(trace_cursor_del, void *cursor, __u32 flags)
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

// similar maps for cursor operations:
} pending_cursors SEC(".maps");      // for mdbx_cursor_get
} pending_cursor_puts SEC(".maps");  // for mdbx_cursor_put
} pending_cursor_dels SEC(".maps");  // for mdbx_cursor_del
} pending_direct_gets SEC(".maps");  // for mdbx_get
```

correlates kprobe/uprobe entry with kretprobe/uretprobe return. since bpf programs are stateless between calls, we save context on entry and retrieve it on return using the thread id as key.

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
    __u32 event_type;        // 3=cursor_get, 4=cursor_put, 5=direct_get, 6=cursor_del
    __u32 cursor_op;         // MDBX_SET_RANGE, MDBX_NEXT, etc. (for get ops)
    __u32 dbi;               // database index (table id)
    __u32 key_size;
    __u8  key_data[64];      // first 64 bytes of lookup key
    __s32 return_code;       // 0=success, -30798=not_found
    __u32 value_size;        // size of value (for put operations)
    __u64 latency_ns;
    __u32 write_flags;       // MDBX_UPSERT, MDBX_APPEND, etc. (for put/del ops)
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

#### block range extraction

the viewer displays which ethereum blocks were processed during the trace. this is determined by looking at **write operations** to block-keyed tables:

```rust
/// tables where writes indicate block processing
const BLOCK_WRITE_DBIS: &[u32] = &[
    2,  // CanonicalHeaders - Key: BlockNumber (u64)
    6,  // BlockBodyIndices - Key: BlockNumber (u64)
    18, // AccountChangeSets - Key: BlockNumber (u64)
    19, // StorageChangeSets - Key: BlockNumberAddress (block_number || address)
];
```

**why writes only?** during block processing, reth reads historical data spanning thousands of blocks (for state lookups, history queries, etc.) but only writes to the current block being processed. using read operations would show the entire accessed range, not the blocks actually synced.

block numbers are extracted from keys using reth's encoding:

```rust
fn extract_block_from_key(dbi: u32, key_data: &[u8], key_size: u32) -> Option<u64> {
    if !BLOCK_WRITE_DBIS.contains(&dbi) {
        return None;
    }
    // block number is big-endian u64 in first 8 bytes
    let block = u64::from_be_bytes(key_data[0..8]);
    
    // sanity check: reject unreasonable values
    if block > 50_000_000 {
        return None;
    }
    Some(block)
}
```

the viewer then shows:
- **min block**: lowest block number written during trace
- **max block**: highest block number written during trace  
- **block count**: `max - min + 1` (approximate blocks processed)

**dbi to table mapping**: the dbi numbers must match reth's table order (defined in `tables!` macro in `db-api/src/tables/mod.rs`). if reth reorders tables, the mapping needs updating.

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
   - mdbx_cursor_get: records read operation with dbi, key, latency
   - mdbx_get: records direct get with dbi, key, latency
   - mdbx_cursor_put: records write operation with dbi, key, value_size, flags, latency
   - mdbx_cursor_del: records delete operation with dbi, flags, latency
   
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

## mdbx write operations

write operations are traced to understand database mutation patterns and correlate page faults during writes.

### mdbx_cursor_put

```c
int mdbx_cursor_put(MDBX_cursor *cursor, MDBX_val *key, 
                    MDBX_val *data, MDBX_put_flags_t flags);
```

cursor-based write operations. reth uses this heavily for:
- `MDBX_UPSERT` (0): insert or update
- `MDBX_NOOVERWRITE` (16): insert only, fail if key exists
- `MDBX_APPEND` (0x20000): append to end (optimization for sequential inserts)
- `MDBX_APPENDDUP` (0x40000): append duplicate in dupsort table

we capture:
- the key being written (first 64 bytes)
- value size (not content, to limit overhead)
- write flags to understand the operation type
- latency to identify slow writes (b-tree splits, page allocations)

### mdbx_cursor_del

```c
int mdbx_cursor_del(MDBX_cursor *cursor, MDBX_put_flags_t flags);
```

deletes the current key/value pair. flags include:
- `MDBX_NODUPDATA` (32): remove all duplicates for key (dupsort tables)
- `MDBX_CURRENT` (64): delete at current cursor position

this is used during pruning operations where reth removes old history.

### write operation tracking

```c
SEC("uprobe/mdbx_cursor_put")
int BPF_UPROBE(trace_cursor_put, void *cursor, struct mdbx_val *key,
               struct mdbx_val *data, __u32 flags)
{
    // lookup DBI from cursor_to_dbi map (same as cursor_get)
    // capture key data and value size
    // save context for uretprobe
}

SEC("uretprobe/mdbx_cursor_put")
int BPF_URETPROBE(trace_cursor_put_ret, int ret)
{
    // calculate latency
    // emit cursor_event with event_type=EVENT_CURSOR_PUT
}
```

### write patterns in reth

typical write patterns observed:

1. **state updates**: `PlainAccountState`, `PlainStorageState` updates after block execution
2. **hashed state**: `HashedAccounts`, `HashedStorages` populated before trie computation
3. **trie updates**: `AccountsTrie`, `StoragesTrie` branch node insertions
4. **history**: `AccountsHistory`, `StoragesHistory` append-only inserts
5. **changesets**: `AccountChangeSets`, `StorageChangeSets` batch inserts per block

### write latency characteristics

write latency is typically higher than reads due to:
- **copy-on-write**: mdbx copies pages before modification
- **b-tree splits**: inserting into full pages triggers splits
- **page allocation**: new pages allocated from freelist or file growth
- **dirty page tracking**: pages marked dirty for eventual commit

high write latency (>1ms) often indicates:
- page splits in hot tables
- large value insertions (bytecodes)
- freelist exhaustion requiring file growth

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

not all page faults can be correlated. we trace:
- `mdbx_cursor_get` and `mdbx_get` (reads)
- `mdbx_cursor_put` and `mdbx_cursor_del` (writes)

faults during other operations (cursor open/close, transaction begin/commit) won't be attributed to tables.

expect 70-90% correlation rate in typical workloads with write tracing enabled. uncorrelated faults are typically from:
- transaction management overhead
- cursor setup/teardown
- kernel readahead prefetching

## transaction lifecycle tracing

the profiler traces mdbx transaction lifecycle operations to understand concurrency patterns:

### traced functions

```c
SEC("uprobe/mdbx_txn_begin_ex")
int BPF_UPROBE(trace_txn_begin, void *env, void *parent, unsigned int flags, void **txn, void *ctx)

SEC("uretprobe/mdbx_txn_begin_ex") 
int BPF_URETPROBE(trace_txn_begin_ret, int ret)

SEC("uprobe/mdbx_txn_commit_ex")
int BPF_UPROBE(trace_txn_commit, void *txn, void *latency)

SEC("uretprobe/mdbx_txn_commit_ex")
int BPF_URETPROBE(trace_txn_commit_ret, int ret)

SEC("uprobe/mdbx_txn_abort")
int BPF_UPROBE(trace_txn_abort, void *txn)

SEC("uretprobe/mdbx_txn_abort")
int BPF_URETPROBE(trace_txn_abort_ret, int ret)
```

### transaction event structure

```c
struct txn_event {
    __u64 timestamp_ns;
    __u32 pid;
    __u32 tid;
    __u32 event_type;      // 7=BEGIN, 8=COMMIT, 9=ABORT
    __u32 txn_flags;       // MDBX_TXN_RDONLY=0x20000, MDBX_TXN_READWRITE=0
    __u64 txn_ptr;         // transaction pointer (for correlation)
    __u64 parent_txn_ptr;  // for nested transactions (0 if none)
    __u64 latency_ns;      // commit/abort latency
    __s32 return_code;
};
```

### ro vs rw transaction detection

mdbx uses `MDBX_TXN_RDONLY = 0x20000` for read-only transactions:

```c
// in txn_begin uprobe
__u32 txn_flags = flags;
bool is_ro = (flags & 0x20000) != 0;

// store flags for later lookup at commit/abort time
bpf_map_update_elem(&active_txn_flags, &txn_ptr, &txn_flags, BPF_ANY);
```

### transaction correlation

transactions are correlated using `(txn_ptr, tid)` as a key since mdbx reuses transaction pointers:

```rust
// active_txns: (txn_ptr, tid) -> (start_time, is_ro)
let mut active_txns: HashMap<(u64, u32), (u64, bool)> = HashMap::new();

// on BEGIN: insert into map
active_txns.insert((event.txn_ptr, event.tid), (event.timestamp_ns, is_ro));

// on COMMIT/ABORT: lookup and create timeline entry
if let Some((start_ts, is_ro)) = active_txns.remove(&(event.txn_ptr, event.tid)) {
    timeline_entries.push(TxnTimelineEntry { ... });
}
```

### reth's transaction patterns

analysis of reth's mdbx usage reveals:

1. **read-only transactions dominate**: vast majority (>99%) are RO transactions
2. **ro transactions are "aborted"**: ro txns use `mdbx_txn_abort()` to close (not commit) - this is normal and efficient
3. **few rw transactions**: typically one writer thread doing batch commits
4. **high rw commit latency**: ~100-150ms average for rw commits (actual disk flush)
5. **near-zero ro "commit" latency**: ro commits are just cleanup, ~3-4μs

### visualization

the transaction gantt chart shows:
- **rw threads at top**: always displayed regardless of transaction count
- **top ro threads by activity**: busiest reader threads
- **color coding**:
  - bright green: ro commit
  - dark green: ro closed (abort)
  - red: rw commit  
  - dark red: rw abort

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
