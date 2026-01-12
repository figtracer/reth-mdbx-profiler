# stress testing scripts

scripts for reproducing and analyzing blocking I/O issues in reth.

## what these do

- generate targeted RPC and metrics endpoint traffic
- capture profiles during stress workloads
- produce comparison reports (baseline vs stressed)

## quick start

```bash
# stress test the metrics endpoint (blocking I/O issue)
./metrics_stress.sh 30 10 http://localhost:9001

# stress test RPC state queries (unbounded, no semaphore)
./rpc_stress.sh state_unbounded 30 http://localhost:8545 20

# full profiling session with samply
./profile_workload.sh \
    --workload metrics \
    --duration 30 \
    --baseline \
    --samply \
    --pid $(pgrep reth)

# generate comparison report
./compare_profiles.sh ./profiles/session_*
```

## scripts

### metrics_stress.sh

hammers `/metrics` to trigger blocking I/O:
- `db.report_metrics()` - mdbx read txn, iterates all tables
- `sfp.report_metrics()` - static file enumeration
- `collect_memory_stats()` - jemalloc stats

```bash
./metrics_stress.sh [duration_secs] [concurrency] [metrics_url]

# 30 seconds, 10 concurrent workers
./metrics_stress.sh 30 10 http://localhost:9001
```

outputs latency percentiles (p50/p95/p99/max) showing blocking impact.

### rpc_stress.sh

generates RPC traffic by category:

| category | endpoints | protection | target |
|----------|-----------|------------|--------|
| `state_unbounded` | eth_getBalance, eth_getStorageAt | **none** | mdbx contention |
| `state_execution` | eth_call, eth_estimateGas | semaphore | semaphore exhaustion |
| `debug_trace` | debug_traceTransaction | low-limit guard | queue exhaustion |
| `mixed` | all above | mixed | realistic workload |

```bash
./rpc_stress.sh [category] [duration_secs] [rpc_url] [concurrency]

# unbounded state queries, 50 workers
./rpc_stress.sh state_unbounded 30 http://localhost:8545 50
```

### profile_workload.sh

orchestrates complete profiling sessions.

```bash
./profile_workload.sh [options]
```

options:
- `--workload TYPE`: metrics, rpc_state, rpc_mixed, all
- `--duration SECS`: duration per test (default: 30)
- `--baseline`: capture baseline profile first (idle node)
- `--samply`: use samply for CPU profiling
- `--mdbx`: use mdbx-profiler for I/O profiling
- `--mdbx-path PATH`: path to mdbx.dat
- `--pid PID`: target process (auto-detects reth)
- `--concurrency N`: concurrent workers (default: 20)

### compare_profiles.sh

generates html comparison report from a profiling session.

```bash
./compare_profiles.sh SESSION_DIR [OUTPUT_FILE]
```

## the blocking I/O problem

every `/metrics` request triggers synchronous blocking operations:

```
/metrics request
  └─> hook()
      ├─> db.report_metrics()           # BLOCKING
      │   └─> begin_ro_txn()
      │       └─> for table in ALL:
      │           └─> open_db() + db_stat()
      │
      ├─> sfp.report_metrics()          # BLOCKING
      │   └─> iter_static_files()
      │       └─> for segment:
      │           └─> open_jar() + metadata()
      │
      └─> collect_memory_stats()        # BLOCKING
          └─> jemalloc epoch advance
```

this contends with write transactions on a syncing node.

## interpreting results

### metrics stress

| metric | good | warning | critical |
|--------|------|---------|----------|
| p50 | < 100ms | 100-500ms | > 500ms |
| p95 | < 500ms | 500ms-2s | > 2s |
| p99 | < 1s | 1-5s | > 5s |
| max | < 2s | 2-10s | > 10s |

high latencies = blocking I/O contention with node workload.

### rpc stress

- `state_unbounded`: high throughput, may cause mdbx contention
- `state_execution`: limited by `blocking_io_request_semaphore`
- `debug_trace`: very low throughput (BlockingTaskGuard, ~4-10 concurrent)

## with profilers

### samply (cpu)

```bash
samply record --pid $(pgrep reth) --duration 30 &
./metrics_stress.sh 30 10

samply load ./profile.json
```

### mdbx-profiler (I/O)

```bash
sudo ../target/release/mdbx-profiler trace \
    --pid $(pgrep reth) \
    --mdbx-path /data/reth/db/mdbx.dat \
    --duration 30s &

./metrics_stress.sh 30 10

../target/release/mdbx-profiler analyze --input trace.jsonl --format summary
```

## reth source references

- `crates/storage/db/src/implementation/mdbx/mod.rs:265-341` - db.report_metrics()
- `crates/storage/provider/src/providers/static_file/manager.rs:477-523` - sfp.report_metrics()
- `crates/node/metrics/src/hooks.rs:44-49` - default hooks
- `crates/node/metrics/src/server.rs` - metrics server
