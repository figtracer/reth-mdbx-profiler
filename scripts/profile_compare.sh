#!/usr/bin/env bash
#
# profile and compare different workloads using mdbx-profiler
#
# captures three profiles:
#   1. baseline - idle node, no workload
#   2. rpc_stress - while hammering RPC endpoints
#   3. metrics_stress - while hammering /metrics endpoint
#
# then generates comparison html
#
# usage:
#   ./profile_compare.sh [options]
#
# options:
#   --duration SECS     duration per profile (default: 30)
#   --pid PID           reth process id (auto-detects if not specified)
#   --mdbx-path PATH    path to mdbx.dat (required)
#   --reth-binary PATH  path to reth binary (for cursor tracing)
#   --rpc-url URL       rpc endpoint (default: http://localhost:8545)
#   --metrics-url URL   metrics endpoint (default: http://localhost:9001)
#   --output-dir DIR    output directory (default: ./profiles)
#   --concurrency N     stress test concurrency (default: 10)
#
# example:
#   ./profile_compare.sh \
#       --mdbx-path /data/reth/db/mdbx.dat \
#       --reth-binary /usr/local/bin/reth \
#       --duration 30

set -euo pipefail

# defaults
DURATION=30
PID=""
MDBX_PATH=""
RETH_BINARY=""
RPC_URL="http://localhost:8545"
METRICS_URL="http://localhost:9001"
OUTPUT_DIR="./profiles"
CONCURRENCY=10

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROFILER="$SCRIPT_DIR/../target/release/mdbx-profiler"
ANALYZER="$SCRIPT_DIR/../target/release/mdbx-trace-analyzer"

# parse args
while [[ $# -gt 0 ]]; do
    case $1 in
        --duration) DURATION="$2"; shift 2 ;;
        --pid) PID="$2"; shift 2 ;;
        --mdbx-path) MDBX_PATH="$2"; shift 2 ;;
        --reth-binary) RETH_BINARY="$2"; shift 2 ;;
        --rpc-url) RPC_URL="$2"; shift 2 ;;
        --metrics-url) METRICS_URL="$2"; shift 2 ;;
        --output-dir) OUTPUT_DIR="$2"; shift 2 ;;
        --concurrency) CONCURRENCY="$2"; shift 2 ;;
        --help|-h) head -30 "$0" | grep "^#" | cut -c3-; exit 0 ;;
        *) echo "unknown option: $1"; exit 1 ;;
    esac
done

# validate
if [ -z "$MDBX_PATH" ]; then
    echo "error: --mdbx-path is required"
    exit 1
fi

if [ ! -f "$MDBX_PATH" ]; then
    echo "error: mdbx file not found: $MDBX_PATH"
    exit 1
fi

# auto-detect pid
if [ -z "$PID" ]; then
    # try reth first
    PID=$(pgrep -x reth 2>/dev/null | head -1 || echo "")

    # if not found and reth-binary specified, try that binary name
    if [ -z "$PID" ] && [ -n "$RETH_BINARY" ]; then
        BINARY_NAME=$(basename "$RETH_BINARY")
        PID=$(pgrep -x "$BINARY_NAME" 2>/dev/null | head -1 || echo "")
    fi

    if [ -z "$PID" ]; then
        echo "error: could not find reth process. use --pid"
        exit 1
    fi
    echo "detected pid: $PID"
fi

# check profiler exists
if [ ! -x "$PROFILER" ]; then
    echo "building mdbx-profiler..."
    (cd "$SCRIPT_DIR/.." && cargo build --release)
fi

# create output dir
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
SESSION_DIR="$OUTPUT_DIR/compare_${TIMESTAMP}"
mkdir -p "$SESSION_DIR"

echo ""
echo "========================================"
echo "mdbx profile comparison"
echo "========================================"
echo "duration:    ${DURATION}s per profile"
echo "pid:         $PID"
echo "mdbx:        $MDBX_PATH"
echo "output:      $SESSION_DIR"
echo "========================================"
echo ""

# build profiler command
PROFILER_CMD="$PROFILER trace --pid $PID --mdbx-path $MDBX_PATH --duration ${DURATION}s"
if [ -n "$RETH_BINARY" ]; then
    PROFILER_CMD="$PROFILER_CMD --trace-cursors --reth-binary $RETH_BINARY"
fi

# ============================================================================
# profile 1: baseline (no workload)
# ============================================================================
echo "[1/3] capturing baseline profile (idle node)..."
echo "      waiting ${DURATION}s with no workload"

$PROFILER_CMD --output "$SESSION_DIR/baseline.jsonl"

echo "      baseline complete"
echo ""
sleep 2

# ============================================================================
# profile 2: rpc stress
# ============================================================================
echo "[2/3] capturing rpc stress profile..."
echo "      starting rpc stress test in background"

# start stress test in background
"$SCRIPT_DIR/rpc_stress.sh" mixed "$DURATION" "$RPC_URL" "$CONCURRENCY" "$SESSION_DIR" > "$SESSION_DIR/rpc_stress.log" 2>&1 &
STRESS_PID=$!

# give stress test a moment to start
sleep 1

# run profiler
$PROFILER_CMD --output "$SESSION_DIR/rpc_stress.jsonl"

# wait for stress test to finish
wait $STRESS_PID 2>/dev/null || true

echo "      rpc stress complete"
echo ""
sleep 2

# ============================================================================
# profile 3: metrics stress
# ============================================================================
echo "[3/3] capturing metrics stress profile..."
echo "      starting metrics stress test in background"

# start stress test in background
"$SCRIPT_DIR/metrics_stress.sh" "$DURATION" "$CONCURRENCY" "$METRICS_URL" "$SESSION_DIR" > "$SESSION_DIR/metrics_stress.log" 2>&1 &
STRESS_PID=$!

# give stress test a moment to start
sleep 1

# run profiler
$PROFILER_CMD --output "$SESSION_DIR/metrics_stress.jsonl"

# wait for stress test to finish
wait $STRESS_PID 2>/dev/null || true

echo "      metrics stress complete"
echo ""

# ============================================================================
# generate html reports
# ============================================================================
echo "generating html reports..."

for trace in baseline rpc_stress metrics_stress; do
    if [ -f "$SESSION_DIR/${trace}.jsonl" ]; then
        echo "  analyzing ${trace}..."
        "$ANALYZER" --input "$SESSION_DIR/${trace}.jsonl" --mdbx-path "$MDBX_PATH" \
            --output "$SESSION_DIR/${trace}.html" 2>/dev/null || true
    fi
done

# ============================================================================
# summary
# ============================================================================
echo ""
echo "========================================"
echo "complete"
echo "========================================"
echo ""
echo "profiles:"
ls -lh "$SESSION_DIR"/*.jsonl 2>/dev/null | awk '{print "  " $NF " (" $5 ")"}'
echo ""
echo "html reports:"
ls -lh "$SESSION_DIR"/*.html 2>/dev/null | awk '{print "  " $NF " (" $5 ")"}'
echo ""
echo "open in browser:"
for html in "$SESSION_DIR"/*.html; do
    echo "  file://$html"
done
echo ""
echo "compare the three reports to see I/O pattern differences:"
echo "  - baseline: normal node operation"
echo "  - rpc_stress: under RPC load (state reads, eth_call, traces)"
echo "  - metrics_stress: under /metrics hammering (db.report_metrics blocking)"
