# reth-mdbx-profiler

ebpf-based profiler for analyzing mdbx page fault patterns and cursor operations in reth.

## what it does

- traces page faults on mdbx memory-mapped regions with **direct operation attribution**
- traces mdbx cursor operations (seeks, gets, navigation, puts, deletes)
- shows which tables, operations, and keys cause the most disk I/O
- generates interactive html visualizations

## requirements

- linux kernel 5.8+ with btf enabled
- root access
- reth node

## quick start

```bash
# build
cargo build --release

# trace (with cursor operations for full attribution)
./target/release/mdbx-profiler trace \
    --process-name reth \
    --mdbx-path /data/reth/db/mdbx.dat \
    --duration 60s \
    --output trace.jsonl \
    --trace-cursors \
    --reth-binary /path/to/reth

# generate visualization
./target/release/mdbx-trace-analyzer \
    --input trace.jsonl \
    --mdbx-path /data/reth/db/mdbx.dat
```

## commands

### trace

```bash
# recommended: use process name (survives restarts)
./target/release/mdbx-profiler trace \
    --process-name reth \
    --mdbx-path /data/reth/db/mdbx.dat \
    --duration 60s \
    --output trace.jsonl \
    --trace-cursors \
    --reth-binary /path/to/reth

# alternative: use pid directly
./target/release/mdbx-profiler trace \
    --pid $(pgrep reth) \
    --mdbx-path /data/reth/db/mdbx.dat \
    --duration 30s \
    --output trace.jsonl
```

options:
- `--pid`: target process id (use this OR --process-name)
- `--process-name`: process name to trace (auto-detects restarts)
- `--mdbx-path`: path to mdbx.dat file
- `--duration`: how long to trace (e.g., 30s, 5m)
- `--output`: output file (default: trace.jsonl)
- `--trace-cursors`: trace cursor operations (required for attribution)
- `--reth-binary`: path to reth binary (required for cursor tracing)

### analyze

```bash
./target/release/mdbx-profiler analyze --input trace.jsonl --format summary
```

formats: `summary`, `csv`, `json`, `logs`

### web viewer

```bash
./target/release/mdbx-trace-analyzer \
    --input trace.jsonl \
    --mdbx-path /data/reth/db/mdbx.dat
```

the analyzer runs on macos/linux without ebpf - collect traces on your node and analyze locally.

## how it works

see [INTERNALS.md](INTERNALS.md) for the full technical details on:
- ebpf probe architecture
- active operation tracking
- page fault enrichment
- viewer visualizations

## license

MIT OR Apache-2.0
