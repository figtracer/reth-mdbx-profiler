# profiling scripts

scripts for profiling MDBX page fault patterns in reth under different RPC workloads.

## profile_methods.sh

profiles individual RPC methods in isolation to compare their MDBX impact. runs each method with concurrent stress, captures page faults, then generates a comparison HTML report.

### usage

```bash
./profile_methods.sh [options]
```

### options

| option | description | default |
|--------|-------------|---------|
| `--methods LIST` | comma-separated list of methods to test | all |
| `--duration SECS` | duration per method | 2700 (45 min) |
| `--concurrency N` | concurrent requests | 50 |
| `--pid PID` | reth process ID | auto-detect |
| `--mdbx-path PATH` | path to mdbx.dat | required |
| `--reth-binary PATH` | path to reth binary (for cursor tracing) | - |
| `--rpc-url URL` | RPC endpoint | http://localhost:8545 |
| `--metrics-url URL` | metrics endpoint | http://localhost:9001 |
| `--output-dir DIR` | output directory | ./method_profiles |
| `--settle-time SECS` | wait time between tests for system to settle | 30 |
| `--flush-caches` | flush OS page caches before each test (requires root) | false |
| `--baseline-runs N` | number of baseline runs for variance estimation | 3 |
| `--quick` | quick mode (~1 hour total: 4 min/test, 1 baseline, 10s settle) | false |

### available methods

- `eth_getBalance`, `eth_getCode`, `eth_getStorageAt`, `eth_getTransactionCount`
- `eth_getProof`, `eth_getBlockByNumber`, `eth_getBlockReceipts`
- `eth_call`, `eth_estimateGas`
- `trace_transaction`, `trace_block`, `debug_traceTransaction`
- `ots_searchTransactions`
- `metrics`

### example

```bash
# profile specific methods
./profile_methods.sh \
    --mdbx-path /data/reth/db/mdbx.dat \
    --methods "eth_getBalance,eth_getProof,trace_block,metrics" \
    --duration 300

# profile all methods with defaults
./profile_methods.sh --mdbx-path /data/reth/db/mdbx.dat
```

### output

generates `method_profiles/methods_<timestamp>/`:
- `comparison.html` - interactive comparison report
- `<method>.jsonl` - raw trace data per method
- `<method>.json` - analyzed compact data per method
