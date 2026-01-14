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
./target/release/mdbx-profiler analyze \
    --input trace.jsonl
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
# generate html visualization (default)
./target/release/mdbx-profiler analyze --input trace.jsonl

# output raw json
./target/release/mdbx-profiler analyze --input trace.jsonl --format json

# output compact json for comparisons
./target/release/mdbx-profiler analyze --input trace.jsonl --format compact --label "my-trace"
```

options:
- `--input`: trace file to analyze (jsonl format)
- `--output`: output file (default: `<input>-viewer.html`)
- `--format`: output format - `html` (default), `json`, `compact`
- `--label`: label for compact export (used in comparisons)
- `--bucket-ms`: time bucket size for pattern analysis (default: 100)

the analyzer processes traces in streaming mode with constant memory usage (~500MB), so it can handle traces of any size (tested with 75GB+ files). progress is shown during analysis:

```
[████████████░░░░░░░░░░░░░░░░░░]  40.5% | 30.4GB/75.0GB | 85 MB/s | ETA: 8m 45s | 125M faults, 89M ops
```

## rpc method profiling

compare mdbx impact across different rpc methods:

```bash
./scripts/profile_methods.sh \
    --mdbx-path /data/reth/db/mdbx.dat \
    --methods "eth_getBalance,eth_call,trace_block" \
    --duration 300 \
    --concurrency 50
```

options:
- `--methods LIST`: comma-separated list of methods to test (default: all)
- `--duration SECS`: duration per method (default: 2700 = 45 min)
- `--concurrency N`: concurrent requests (default: 50)
- `--settle-time SECS`: wait time between tests for system to settle (default: 30)
- `--flush-caches`: flush OS page caches before each test (requires root)
- `--quick`: quick mode (~1 hour total: 4 min/test, 10s settle)
- `--pid PID`: reth process id (auto-detects if not specified)
- `--reth-binary PATH`: path to reth binary (for cursor tracing)
- `--rpc-url URL`: rpc endpoint (default: http://localhost:8545)
- `--metrics-url URL`: metrics endpoint (default: http://localhost:9001)
- `--output-dir DIR`: output directory (default: ./method_profiles)

available methods:
- `eth_getBalance`, `eth_getCode`, `eth_getStorageAt`, `eth_getTransactionCount`
- `eth_getProof`, `eth_getBlockByNumber`, `eth_getBlockReceipts`
- `eth_call`, `eth_estimateGas`
- `trace_transaction`, `trace_block`, `debug_traceTransaction`
- `ots_searchTransactions`, `metrics`

generates an interactive html comparison report showing:
- page fault counts and rates per method
- delta from baseline (idle) measurements
- table access breakdown by method


## how it works

see [docs/INTERNALS.md](docs/INTERNALS.md) for the full technical details on:
- ebpf probe architecture
- active operation tracking
- page fault enrichment
- viewer visualizations

## license

MIT OR Apache-2.0
