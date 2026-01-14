# CPU Profiling Implementation Plan

## Overview

Add CPU profiling capabilities to the MDBX profiler to complement the existing page fault analysis. This will help identify whether performance bottlenecks are due to I/O (page faults) or CPU-bound operations within MDBX.

## Goals

1. Capture CPU time spent in MDBX operations
2. Correlate CPU usage with page fault data
3. Identify hot functions within MDBX cursor operations
4. Distinguish between I/O-bound vs CPU-bound tables/operations

## Implementation Approach

### Option A: eBPF-based CPU Sampling (Recommended)

Use eBPF `perf_event` programs to sample CPU stack traces during MDBX operations.

**Pros:**
- Low overhead sampling
- No code modification to reth needed
- Can capture kernel + userspace stacks
- Integrates well with existing eBPF infrastructure

**Cons:**
- Requires stack frame pointers or DWARF info for accurate stacks
- Sampling may miss short operations

### Option B: uprobes with Timestamps

Extend existing uprobes to measure wall-clock time vs actual CPU time using `bpf_ktime_get_ns()`.

**Pros:**
- Simpler implementation
- Exact measurements per operation
- Already have uprobe infrastructure

**Cons:**
- Higher overhead for very frequent operations
- Doesn't show where within a function time is spent

### Option C: Hybrid Approach (Recommended for Phase 1)

Combine uprobe timing with periodic CPU sampling:
1. Use uprobes to track operation start/end and measure duration
2. Use perf sampling to identify hot spots within operations
3. Correlate the two data sources in analysis

## Detailed Design

### Phase 1: Operation-Level CPU Time

#### BPF Changes

```c
// Add to existing cursor operation tracking
struct cursor_cpu_stats {
    u64 total_cpu_ns;      // Total CPU time in this operation type
    u64 count;             // Number of operations
    u64 max_cpu_ns;        // Max single operation CPU time
    u64 min_cpu_ns;        // Min single operation CPU time
};

// Per-operation CPU tracking (keyed by operation + table)
BPF_HASH(cursor_cpu_map, struct op_table_key, struct cursor_cpu_stats);

// Track CPU time using task's cpu_time or cputime accounting
// On operation start: record current CPU time
// On operation end: calculate delta
```

#### New Metrics to Capture

For each cursor operation:
- **Wall time**: Already captured (start_ns to end_ns)
- **CPU time**: Time actually executing on CPU (exclude I/O wait)
- **I/O wait time**: Wall time - CPU time (approximation)

#### Data Structures

```rust
// New fields in CursorOpStats
pub struct CursorOpStats {
    // Existing fields...
    pub total_cpu_time_us: u64,
    pub avg_cpu_time_us: f64,
    pub cpu_to_wall_ratio: f64,  // <1 means I/O bound, ~1 means CPU bound
}

// New per-table CPU summary
pub struct TableCpuProfile {
    pub table_name: String,
    pub total_cpu_time_ms: f64,
    pub total_wall_time_ms: f64,
    pub cpu_efficiency: f64,      // cpu_time / wall_time
    pub is_io_bound: bool,        // cpu_efficiency < 0.5
    pub is_cpu_bound: bool,       // cpu_efficiency > 0.8
}
```

### Phase 2: Function-Level Profiling

#### Stack Sampling

```c
// Perf event program for CPU sampling
SEC("perf_event")
int sample_cpu_stack(struct bpf_perf_event_data *ctx) {
    // Only sample if we're in an active MDBX operation
    u64 pid_tgid = bpf_get_current_pid_tgid();
    struct active_op *op = bpf_map_lookup_elem(&active_ops, &pid_tgid);
    if (!op) return 0;
    
    // Capture stack trace
    struct stack_sample sample = {};
    sample.table_id = op->table_id;
    sample.cursor_op = op->cursor_op;
    sample.stack_id = bpf_get_stackid(ctx, &stack_traces, BPF_F_USER_STACK);
    sample.timestamp = bpf_ktime_get_ns();
    
    bpf_perf_event_output(ctx, &cpu_samples, BPF_F_CURRENT_CPU, &sample, sizeof(sample));
    return 0;
}
```

#### Symbol Resolution

- Use `/proc/<pid>/maps` to find library load addresses
- Parse ELF symbols from libmdbx.so
- Build address-to-function mapping for stack traces

### Phase 3: Viewer Integration

#### New UI Components

1. **CPU/IO Ratio Column in Tables tab**
   - Add column showing CPU efficiency per table
   - Color code: Red (CPU bound), Blue (I/O bound), Green (balanced)

2. **CPU Flamegraph** (optional, stretch goal)
   - Aggregate stack samples into flamegraph data
   - Interactive SVG or canvas-based visualization

3. **Operation CPU Breakdown**
   - Bar chart: CPU time vs I/O wait time per operation type
   - Helps identify which operations benefit from more RAM vs faster CPU

#### New Metrics Display

```
┌─────────────────────────────────────────────────────────────┐
│ CPU Profile Summary                                          │
├─────────────────────────────────────────────────────────────┤
│ Total CPU Time: 45.2s    Total Wall Time: 2748.6s           │
│ Overall CPU Efficiency: 1.6%                                 │
│ Bottleneck: I/O (page faults)                               │
├─────────────────────────────────────────────────────────────┤
│ Table             CPU Time    Wall Time    Efficiency        │
│ HashedAccounts    12.3s       747.0s       1.6% (I/O)       │
│ StoragesTrie      8.1s        512.4s       1.6% (I/O)       │
│ AccountsTrie      5.2s        340.4s       1.5% (I/O)       │
└─────────────────────────────────────────────────────────────┘
```

## Implementation Steps

### Step 1: Basic CPU Time Tracking
- [ ] Add CPU time fields to BPF maps
- [ ] Capture CPU time on operation entry/exit using `bpf_ktime_get_ns()` with thread CPU tracking
- [ ] Add CPU stats to Rust data structures
- [ ] Display basic CPU metrics in viewer

### Step 2: CPU Efficiency Analysis
- [ ] Calculate CPU-to-wall-time ratios
- [ ] Add CPU efficiency column to Tables tab
- [ ] Add summary showing I/O bound vs CPU bound classification
- [ ] Add recommendation based on bottleneck type

### Step 3: Per-Operation CPU Breakdown
- [ ] Track CPU time per operation type (GET, PUT, DEL, etc.)
- [ ] Show CPU time breakdown in operation details
- [ ] Identify CPU-heavy operations vs I/O-heavy operations

### Step 4: Stack Sampling (Optional/Future)
- [ ] Add perf_event BPF program for sampling
- [ ] Implement stack trace collection
- [ ] Add symbol resolution for libmdbx
- [ ] Build flamegraph visualization

## Technical Considerations

### Measuring CPU Time in BPF

**Challenge**: BPF doesn't have direct access to per-thread CPU time.

**Solutions**:

1. **Use `bpf_ktime_get_ns()` pairs**: Measures wall time, not CPU time. Simple but doesn't distinguish I/O wait.

2. **Read from task_struct**: Access `task->utime` and `task->stime` via BPF. Requires CO-RE and careful struct access.

3. **Use cgroup CPU accounting**: If process is in a cgroup, can read CPU stats.

4. **Estimate from page faults**: If we know a page fault occurred, estimate I/O time based on storage latency (~100us SSD, ~10ms HDD).

**Recommended approach for Phase 1**: 
Use wall time from `bpf_ktime_get_ns()` and subtract estimated I/O time based on major page faults detected. Each major fault adds ~100-500us for SSD.

```c
// Approximate CPU time
cpu_time_ns = wall_time_ns - (major_faults * ESTIMATED_IO_LATENCY_NS);
```

### Overhead Considerations

- CPU sampling at 99Hz adds ~1-2% overhead
- Per-operation timing already exists, CPU tracking adds minimal overhead
- Stack trace collection is expensive, sample sparingly

### Compatibility

- Requires Linux 5.x+ for full BPF features
- Stack sampling requires frame pointers or BPF_F_USER_STACK support
- Symbol resolution needs debug symbols or .symtab in libmdbx

## Success Criteria

1. Can identify whether a table is I/O-bound or CPU-bound
2. Can show CPU time breakdown per operation type
3. Overhead remains under 5% during profiling
4. Results help users decide: more RAM vs faster CPU vs faster storage

## Future Enhancements

- Lock contention tracking (mutex wait time)
- Memory allocation profiling within MDBX
- Cross-reference with Linux perf data
- Integration with external profilers (perf, flamegraph tools)
