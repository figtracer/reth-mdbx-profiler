# internals

this document explains how the profiler bridges physical I/O events with logical database operations.

## architecture overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ RETH PROCESS                                                                │
│                                                                             │
│  mdbx_cursor_get(HashedStorages, key, SET_RANGE)                            │
│  │                                                                          │
│  ├─► uprobe fires → registers active_op for this thread                     │
│  │                                                                          │
│  │   [page fault occurs - kernel calls handle_mm_fault]                     │ 
│  │   │                                                                      │
│  │   └─► kprobe fires → looks up active_op → enriches fault event           │
│  │                                                                          │
│  └─► uretprobe fires → clears active_op, emits cursor_event                 │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ RING BUFFER (16MB)                                                          │
│                                                                             │
│  [page_fault_event with active_op context][cursor_event][page_fault]...     │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ USERSPACE                                                                   │
│                                                                             │
│  tracer: polls ring buffer → writes trace.jsonl                             │
│  analyzer: reads trace → generates html visualization                       │
└─────────────────────────────────────────────────────────────────────────────┘
```

## direct bpf attribution

the key innovation is tracking active operations per-thread in a bpf map:

```c
// bpf/mdbx_tracer.bpf.c

struct active_op {
    __u64 start_ns;
    __u32 dbi;               // table id
    __u32 op_type;           // CURSOR_GET, CURSOR_PUT, etc.
    __u32 cursor_op;         // SET_RANGE, NEXT, etc.
    __u8  key_prefix[16];    // first 16 bytes of key
};

struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 4096);
    __type(key, __u64);      // thread id (pid_tgid)
    __type(value, struct active_op);
} active_ops SEC(".maps");
```

### registration flow

when an mdbx operation starts, the uprobe registers the active operation:

```c
SEC("uprobe/mdbx_cursor_get")
int BPF_UPROBE(trace_cursor_get, void *cursor, struct mdbx_val *key, 
               void *data, int op)
{
    __u64 pid_tgid = bpf_get_current_pid_tgid();
    
    struct active_op aop = {
        .start_ns = bpf_ktime_get_ns(),
        .dbi = lookup_dbi(cursor),
        .op_type = EVENT_CURSOR_GET,
        .cursor_op = op,
    };
    copy_key_prefix(&aop, key);
    
    bpf_map_update_elem(&active_ops, &pid_tgid, &aop, BPF_ANY);
    // ... save context for uretprobe ...
}
```

### enrichment flow

when a page fault occurs, the kretprobe looks up the active operation:

```c
SEC("kretprobe/handle_mm_fault")
int BPF_KRETPROBE(trace_page_fault_ret, vm_fault_t ret)
{
    __u64 pid_tgid = bpf_get_current_pid_tgid();
    
    struct page_fault_event *e = bpf_ringbuf_reserve(&events, sizeof(*e), 0);
    // ... fill physical layer info (address, offset, major/minor) ...
    
    // look up active operation for this thread
    struct active_op *op = bpf_map_lookup_elem(&active_ops, &pid_tgid);
    if (op) {
        // direct attribution - 100% accurate
        e->active_dbi = op->dbi;
        e->active_op_type = op->op_type;
        e->active_cursor_op = op->cursor_op;
        memcpy(e->active_key_prefix, op->key_prefix, 16);
    } else {
        // no active op - fault between operations
        e->active_dbi = NO_ACTIVE_OP_DBI;
    }
    
    bpf_ringbuf_submit(e, 0);
}
```

### cleanup flow

when the mdbx operation completes, the uretprobe clears the active operation:

```c
SEC("uretprobe/mdbx_cursor_get")
int BPF_URETPROBE(trace_cursor_get_ret, int ret)
{
    __u64 pid_tgid = bpf_get_current_pid_tgid();
    
    // emit cursor_event with latency
    // ...
    
    // clear active operation
    bpf_map_delete_elem(&active_ops, &pid_tgid);
}
```

### result

page fault events carry full operation context:

```rust
// src/event.rs
pub struct PageFaultEvent {
    // physical layer
    pub timestamp_ns: u64,
    pub address: u64,
    pub file_offset: u64,
    pub is_major: u8,
    pub latency_ns: u64,
    
    // logical layer (from active_ops lookup)
    pub active_dbi: u32,           // table
    pub active_op_type: u32,       // operation type
    pub active_cursor_op: u32,     // cursor operation
    pub active_key_prefix: [u8; 16], // key prefix
}
```

## traced operations

all operations that can cause page faults are tracked:

| function | event type | tracked fields |
|----------|------------|----------------|
| `mdbx_cursor_get` | CURSOR_GET | dbi, cursor_op, key |
| `mdbx_cursor_put` | CURSOR_PUT | dbi, key, value_size, flags |
| `mdbx_cursor_del` | CURSOR_DEL | dbi, flags |
| `mdbx_get` | DIRECT_GET | dbi, key |
| `mdbx_put` | DIRECT_PUT | dbi, key, value_size, flags |
| `mdbx_del` | DIRECT_DEL | dbi, key |

## bpf maps

| map | type | purpose |
|-----|------|---------|
| `active_ops` | hash | tracks active mdbx operation per thread |
| `events` | ringbuf | event queue to userspace (16MB) |
| `tracked_inodes` | hash | mdbx file inodes to trace |
| `vma_to_offset` | hash | cached vma → file offset mappings |
| `cursor_to_dbi` | hash | cursor pointer → table id |
| `pending_faults` | hash | kprobe → kretprobe context |
| `pending_cursors` | hash | uprobe → uretprobe context |
| `profiler_config` | array | target pid filter |

## viewer

the html viewer shows three tabs:

### physical tab

- **metrics row**: duration, block range, fault counts, fault rate, major ratio
- **fault timeline**: page faults over time (drag to zoom, double-click to reset)
- **access heatmap**: 2D density of faults by time and file offset
- **access pattern**: sequential vs random ratio, top stride patterns
- **top threads**: threads causing the most page faults

### tables tab

- **fault distribution chart**: horizontal bar chart showing faults per table
- **attribution summary**: directly attributed vs uncorrelated counts
- **i/o impact table**: unified view with expandable rows showing:
  - faults and major faults per table
  - slow ops count and percentage
  - i/o time (cumulative time spent in slow operations)
  - top operation type causing faults
  - drill-down details: faults by operation, by cursor op, hot keys

### transactions tab

- **transaction metrics**: total, rate, RO/RW counts, commits/aborts
- **concurrency stats**: max and avg concurrent RO transactions
- **concurrency timeline**: concurrent RO transactions over time
- **rw commit latency timeline**: when and how long RW commits take
- **thread distribution**: which threads do transaction work

## attribution accuracy

with direct bpf attribution:
- **~97% of faults** are directly attributed to the exact operation
- **~3% uncorrelated** - faults between operations (readahead, internal mdbx work)

compared to the old timestamp correlation approach:
- only ~60% could be correlated
- statistical matching, not causal
- faults during cursor open/close weren't attributed

## performance

- kprobe overhead: ~100-500ns per fault
- uprobe overhead: ~200-800ns per operation
- ring buffer: lock-free, 16MB capacity
- filtering happens in kernel - only relevant events reach userspace

## limitations

- requires linux kernel 5.8+ with btf
- requires non-stripped reth binary for cursor tracing
- 4KB page size assumed (no huge page support)
- ring buffer can drop events under extreme load
