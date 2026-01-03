# reth-mdbx-profiler

eBPF-based profiler for analyzing MDBX page fault patterns in Reth.

traces memory-mapped file accesses to understand database I/O behavior and identify optimization opportunities for state root computation, trie traversal, and block execution.

## features

- **page fault tracing**: captures every page fault in MDBX memory-mapped regions with major/minor fault detection
- **table attribution**: maps page faults to Reth MDBX tables (AccountsTrie, StoragesTrie, PlainAccountState, etc.)
- **web-based viewer**: generates self-contained HTML visualizations with interactive charts and heatmaps
- **access pattern analysis**: identifies sequential vs random access patterns
- **prefetch opportunity detection**: analyzes if prefetching could improve performance
- **thread distribution**: shows which threads cause the most I/O

## requirements

- Linux kernel 5.8+ (for ring buffer support)
- BTF enabled (`/sys/kernel/btf/vmlinux` exists)
- root access (for eBPF)
- Reth node with MDBX database

## quick start

### 1. setup

```bash
# run setup script (installs dependencies, generates vmlinux.h)
sudo ./scripts/setup-node.sh

# build the profiler
cargo build --release
```

### 2. collect a trace

```bash
# find the reth process and mdbx path
./target/release/mdbx-profiler find-mdbx --pid $(pgrep reth)

# trace for 30 seconds
sudo ./target/release/mdbx-profiler trace \
    --pid $(pgrep reth) \
    --mdbx-path /data/reth/db/mdbx.dat \
    --duration 30s \
    --output trace.jsonl
```

### 3. analyze the trace

```bash
# generate interactive HTML viewer
./target/release/mdbx-trace-analyzer --input trace.jsonl --mdbx-path ../reth_data/db/mdbx.dat

# or export as CSV
./target/release/mdbx-trace-analyzer --input trace.jsonl --mdbx-path ../reth_data/db/mdbx.dat --format csv
```

the analyzer runs on macOS/Linux and doesn't require eBPF - you can collect traces on your node and analyze them locally.

## web viewer

the HTML viewer includes:

- **summary dashboard**: total faults, major/minor ratio, fault rate, duration
- **timeline heatmap**: interactive 2D view of time vs file offset
- **table breakdown**: pie/bar charts showing faults per MDBX table
- **thread distribution**: which threads cause the most faults
- **hot pages table**: sortable table of most-accessed pages
- **access pattern analysis**: sequential vs random ratio, stride distribution
- **prefetch opportunity score**: prediction hit rate and locality analysis

## project structure

```
reth-mdbx-profiler/
├── bpf/
│   └── mdbx_tracer.bpf.c   # eBPF probes (kprobe/kretprobe on handle_mm_fault)
├── src/
│   ├── main.rs             # profiler CLI (Linux only, requires eBPF)
│   ├── analyzer.rs         # trace analyzer with web viewer
│   ├── event.rs            # shared event types
│   ├── mdbx.rs             # MDBX file detection
│   ├── mdbx_metadata.rs    # table attribution
│   └── viewer/             # HTML viewer generation
└── scripts/
    └── setup-node.sh       # install dependencies
```

## license

MIT OR Apache-2.0
