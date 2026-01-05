# reth-mdbx-profiler

ebpf-based profiler for analyzing mdbx page fault patterns in reth.

## requirements

- linux kernel 5.8+ with btf enabled
- root access
- reth node with mdbx database

## setup

```bash
sudo ./scripts/setup-node.sh
cargo build --release
```

## commands

### trace

trace page faults on mdbx regions.

```bash
sudo ./target/release/mdbx-profiler trace \
    --pid $(pgrep reth) \
    --mdbx-path /data/reth/db/mdbx.dat \
    --duration 30s \
    --output trace.jsonl
```

options:
- `--pid`: target process id
- `--mdbx-path`: path to mdbx.dat file
- `--duration`: how long to trace (e.g., 30s, 5m)
- `--output`: output file (default: trace.jsonl)
- `--trace-cursors`: also trace cursor operations
- `--reth-binary`: path to reth binary (for cursor tracing)

### trace-cursors

trace mdbx cursor operations and page faults.

```bash
sudo ./target/release/mdbx-profiler trace-cursors \
    --pid $(pgrep reth) \
    --binary /path/to/reth \
    --duration 30s \
    --print-logs
```

options:
- `--pid`: target process id
- `--binary`: path to reth binary
- `--duration`: how long to trace
- `--output`: output file (default: cursor-trace.jsonl)
- `--print-logs`: print events to stdout in log format

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

## known limitations

- only works with 4kb page sizes (standard on most linux systems)
- requires btf support in the kernel (`/sys/kernel/btf/vmlinux` must exist)
- cursor tracing requires symbols in the reth binary (not stripped)
- page fault tracing may miss faults during very high load due to ring buffer drops
- table attribution relies on mdbx file structure and may not work with non-standard configurations

## license

MIT OR Apache-2.0
