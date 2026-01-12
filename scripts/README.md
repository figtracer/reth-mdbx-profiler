# stress testing scripts

scripts for reproducing and analyzing blocking I/O issues in reth using mdbx-profiler.

## what these do

- capture mdbx-profiler traces under different workloads
- compare baseline (idle) vs stressed I/O patterns
- generate html reports for each scenario

## quick start

```bash
# run full comparison: baseline vs rpc stress vs metrics stress
./profile_compare.sh \
    --mdbx-path /data/reth/db/mdbx.dat \
    --reth-binary /usr/local/bin/reth \
    --duration 30

# or run stress tests individually
./metrics_stress.sh 30 10 http://localhost:9001
./rpc_stress.sh mixed 30 http://localhost:8545 20
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

### profile_compare.sh

runs mdbx-profiler under three conditions and generates comparison html:

1. **baseline** - idle node, no workload
2. **rpc_stress** - while hammering RPC endpoints  
3. **metrics_stress** - while hammering /metrics endpoint

```bash
./profile_compare.sh [options]
```

options:
- `--duration SECS`: duration per profile (default: 30)
- `--pid PID`: reth process id (auto-detects)
- `--mdbx-path PATH`: path to mdbx.dat (required)
- `--reth-binary PATH`: path to reth binary (for cursor tracing)
- `--rpc-url URL`: rpc endpoint (default: http://localhost:8545)
- `--metrics-url URL`: metrics endpoint (default: http://localhost:9001)
- `--output-dir DIR`: output directory (default: ./profiles)
- `--concurrency N`: stress test concurrency (default: 10)

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

## reth source references

- `crates/storage/db/src/implementation/mdbx/mod.rs:265-341` - db.report_metrics()
- `crates/storage/provider/src/providers/static_file/manager.rs:477-523` - sfp.report_metrics()
- `crates/node/metrics/src/hooks.rs:44-49` - default hooks
- `crates/node/metrics/src/server.rs` - metrics server
