# reth-mdbx-profiler

ebpf-based profiler for analyzing mdbx page fault patterns and cursor operations in reth.

## what it does

- traces page faults on mdbx memory-mapped regions
- traces mdbx cursor operations (seeks, gets, navigation, puts)
- correlates page faults with cursor operations to attribute faults to specific tables
- generates interactive html visualizations

## requirements

- linux kernel 5.8+ with btf enabled
- root access
- reth node with mdbx database

## setup

```bash
cargo build --release
```

## commands

### trace

trace page faults and cursor operations on mdbx regions.

```bash
# using process name (recommended - survives process restarts)
./target/release/mdbx-profiler trace \
    --process-name reth \
    --mdbx-path /data/reth/db/mdbx.dat \
    --duration 60s \
    --output trace.jsonl \
    --trace-cursors \
    --reth-binary /path/to/reth

# using pid (if you know it won't restart)
./target/release/mdbx-profiler trace \
    --pid $(pgrep reth) \
    --mdbx-path /data/reth/db/mdbx.dat \
    --duration 30s \
    --output trace.jsonl
```

options:
- `--pid`: target process id (use this OR --process-name)
- `--process-name`: process name to trace (e.g., "reth"). automatically detects restarts and updates tracking
- `--mdbx-path`: path to mdbx.dat file
- `--duration`: how long to trace (e.g., 30s, 5m)
- `--output`: output file (default: trace.jsonl)
- `--trace-cursors`: also trace cursor operations (required for accurate table attribution)
- `--reth-binary`: path to reth binary (required for cursor tracing)

### analyze

analyze a trace file.

```bash
./target/release/mdbx-profiler analyze --input trace.jsonl --format summary
```

formats: `summary`, `csv`, `json`, `logs`

## web viewer

generate interactive html visualizations from traces:

```bash
./target/release/mdbx-trace-analyzer \
    --input trace.jsonl \
    --mdbx-path /data/reth/db/mdbx.dat
```

the analyzer runs on macos/linux without ebpf - collect traces on your node and analyze locally.

## license

MIT OR Apache-2.0
