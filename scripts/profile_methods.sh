#!/usr/bin/env bash
#
# Profile individual RPC methods and compare their MDBX impact
#
# Runs each method in isolation, profiles page faults, then generates
# a comparison showing which methods cause the most database load.
#
# Usage:
#   ./profile_methods.sh [options]
#
# Options:
#   --methods LIST      Comma-separated list of methods to test (default: all)
#   --duration SECS     Duration per method (default: 2700 = 45 minutes)
#   --concurrency N     Number of concurrent requests (default: 50)
#   --pid PID           Reth process ID (auto-detects if not specified)
#   --mdbx-path PATH    Path to mdbx.dat (required)
#   --reth-binary PATH  Path to reth binary (for cursor tracing)
#   --rpc-url URL       RPC endpoint (default: http://localhost:8545)
#   --metrics-url URL   Metrics endpoint (default: http://localhost:9001)
#   --output-dir DIR    Output directory (default: ./method_profiles)
#   --settle-time SECS  Time to wait between tests for system to settle (default: 30)
#   --flush-caches      Flush OS page caches before each test (requires root)
#   --baseline-runs N   Number of baseline runs for variance estimation (default: 3)
#   --quick             Quick mode: ~1 hour total (4 min/test, 1 baseline, 10s settle)
#
# Available methods:
#   eth_getBalance, eth_getCode, eth_getStorageAt, eth_getTransactionCount,
#   eth_getProof, eth_getBlockByNumber, eth_getBlockReceipts, eth_call,
#   eth_estimateGas, trace_transaction, trace_block, debug_traceTransaction,
#   ots_searchTransactions, metrics
#
# Example:
#   ./profile_methods.sh \
#       --mdbx-path /data/reth/db/mdbx.dat \
#       --methods "eth_getBalance,eth_getProof,trace_block,metrics" \
#       --duration 300

set -euo pipefail

# Defaults
METHODS="eth_getBalance,eth_getStorageAt,eth_getCode,eth_getProof,eth_getBlockReceipts,eth_call,trace_transaction,trace_block,debug_traceTransaction,metrics"
DURATION=2700
CONCURRENCY=50
QUICK=false
PID=""
MDBX_PATH=""
RETH_BINARY=""
RPC_URL="http://localhost:8545"
METRICS_URL="http://localhost:9001"
OUTPUT_DIR="./method_profiles"
SETTLE_TIME=30
FLUSH_CACHES=false
BASELINE_RUNS=3

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROFILER="$SCRIPT_DIR/../target/release/mdbx-profiler"
ANALYZER="$SCRIPT_DIR/../target/release/mdbx-trace-analyzer"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

# Parse args
while [[ $# -gt 0 ]]; do
    case $1 in
        --methods) METHODS="$2"; shift 2 ;;
        --duration) DURATION="$2"; shift 2 ;;
        --concurrency) CONCURRENCY="$2"; shift 2 ;;
        --pid) PID="$2"; shift 2 ;;
        --mdbx-path) MDBX_PATH="$2"; shift 2 ;;
        --reth-binary) RETH_BINARY="$2"; shift 2 ;;
        --rpc-url) RPC_URL="$2"; shift 2 ;;
        --metrics-url) METRICS_URL="$2"; shift 2 ;;
        --output-dir) OUTPUT_DIR="$2"; shift 2 ;;
        --settle-time) SETTLE_TIME="$2"; shift 2 ;;
        --flush-caches) FLUSH_CACHES=true; shift ;;
        --baseline-runs) BASELINE_RUNS="$2"; shift 2 ;;
        --quick) QUICK=true; shift ;;
        --help|-h)
            head -40 "$0" | grep "^#" | cut -c3-
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# Quick mode: 1 hour total (~4 min per test)
if [ "$QUICK" = true ]; then
    DURATION=240
    BASELINE_RUNS=1
    SETTLE_TIME=10
    echo -e "${YELLOW}Quick mode: 4 min/test, 1 baseline, 10s settle (~1 hour total)${NC}"
fi

# Validate
if [ -z "$MDBX_PATH" ]; then
    echo "Error: --mdbx-path is required"
    exit 1
fi

if [ ! -f "$MDBX_PATH" ]; then
    echo "Error: mdbx file not found: $MDBX_PATH"
    exit 1
fi

# Auto-detect PID
if [ -z "$PID" ]; then
    # Try reth binary name if specified
    if [ -n "$RETH_BINARY" ]; then
        BINARY_NAME=$(basename "$RETH_BINARY")
        PID=$(pgrep -x "$BINARY_NAME" 2>/dev/null | head -1 || echo "")
    fi
    # Fall back to default 'reth' name
    if [ -z "$PID" ]; then
        PID=$(pgrep -x reth 2>/dev/null | head -1 || echo "")
    fi
    if [ -z "$PID" ]; then
        echo "Error: could not find reth process. use --pid"
        exit 1
    fi
    echo "Detected PID: $PID"
fi

# Build profiler if needed
if [ ! -x "$PROFILER" ]; then
    echo "Building mdbx-profiler..."
    (cd "$SCRIPT_DIR/.." && cargo build --release)
fi

# Create output dir
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
SESSION_DIR="$OUTPUT_DIR/methods_${TIMESTAMP}"
mkdir -p "$SESSION_DIR"

# Convert methods to array
IFS=',' read -ra METHOD_LIST <<< "$METHODS"

echo ""
echo -e "${CYAN}========================================"
echo "MDBX Method Profiler"
echo -e "========================================${NC}"
echo "Methods:     ${METHOD_LIST[*]}"
echo "Duration:    ${DURATION}s per method"
echo "Concurrency: $CONCURRENCY"
echo "Settle time: ${SETTLE_TIME}s between tests"
echo "Flush cache: $FLUSH_CACHES"
echo "Baseline runs: $BASELINE_RUNS"
echo "PID:         $PID"
echo "MDBX:        $MDBX_PATH"
echo "Output:      $SESSION_DIR"
echo "========================================"
echo ""

# Build profiler command
PROFILER_CMD="$PROFILER trace --pid $PID --mdbx-path $MDBX_PATH --duration ${DURATION}s"
if [ -n "$RETH_BINARY" ]; then
    PROFILER_CMD="$PROFILER_CMD --trace-cursors --reth-binary $RETH_BINARY"
fi

# Get current block number
BLOCK_RESPONSE=$(curl -s --max-time 5 -X POST -H "Content-Type: application/json" \
    --data '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
    "$RPC_URL" 2>&1) || { echo "Cannot connect to RPC"; exit 1; }

BLOCK_HEX=$(echo "$BLOCK_RESPONSE" | grep -o '"result":"[^"]*"' | cut -d'"' -f4)
BLOCK_NUM=$((BLOCK_HEX))
echo "Current block: $BLOCK_NUM"

# Contract addresses for testing
CONTRACTS=(
    "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"  # WETH
    "0xdAC17F958D2ee523a2206206994597C13D831ec7"  # USDT
    "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"  # USDC
    "0x1f9840a85d5aF5bf1D1762F925BDADdC4201F984"  # UNI
)

# Get some tx hashes for trace methods
echo "Fetching transaction hashes for tracing..."
TX_HASHES=()
for offset in 1 2 3; do
    block_hex="0x$(printf '%x' $((BLOCK_NUM - offset)))"
    block_data=$(curl -s -X POST -H "Content-Type: application/json" \
        --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getBlockByNumber\",\"params\":[\"$block_hex\", false],\"id\":0}" \
        "$RPC_URL")
    while IFS= read -r line; do
        TX_HASHES+=("$line")
    done < <(echo "$block_data" | grep -oE '0x[a-fA-F0-9]{64}' | tail -n +2 | head -10)
done
echo "Found ${#TX_HASHES[@]} transactions for tracing"

# Get block hash for debug methods
BLOCK_HASH=$(curl -s -X POST -H "Content-Type: application/json" \
    --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getBlockByNumber\",\"params\":[\"0x$(printf '%x' $((BLOCK_NUM - 1)))\", false],\"id\":1}" \
    "$RPC_URL" | grep -o '"hash":"0x[a-fA-F0-9]\{64\}"' | head -1 | cut -d'"' -f4)

# ============================================================================
# Method stress functions - each hammers a single method type
# ============================================================================

stress_eth_getBalance() {
    local end_time=$1
    while [ $(date +%s) -lt $end_time ]; do
        local addr="${CONTRACTS[$((RANDOM % ${#CONTRACTS[@]}))]}"
        local block="0x$(printf '%x' $((BLOCK_NUM - RANDOM % 1000)))"
        curl -s --max-time 10 -X POST -H "Content-Type: application/json" \
            --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getBalance\",\"params\":[\"$addr\", \"$block\"],\"id\":1}" \
            "$RPC_URL" > /dev/null
        sleep 0.001
    done
}

stress_eth_getStorageAt() {
    local end_time=$1
    local slots=("0x0" "0x1" "0x2" "0x3" "0x4" "0x5")
    while [ $(date +%s) -lt $end_time ]; do
        local addr="${CONTRACTS[$((RANDOM % ${#CONTRACTS[@]}))]}"
        local slot="${slots[$((RANDOM % ${#slots[@]}))]}"
        local block="0x$(printf '%x' $((BLOCK_NUM - RANDOM % 1000)))"
        curl -s --max-time 10 -X POST -H "Content-Type: application/json" \
            --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getStorageAt\",\"params\":[\"$addr\", \"$slot\", \"$block\"],\"id\":1}" \
            "$RPC_URL" > /dev/null
        sleep 0.001
    done
}

stress_eth_getCode() {
    local end_time=$1
    while [ $(date +%s) -lt $end_time ]; do
        local addr="${CONTRACTS[$((RANDOM % ${#CONTRACTS[@]}))]}"
        local block="0x$(printf '%x' $((BLOCK_NUM - RANDOM % 1000)))"
        curl -s --max-time 10 -X POST -H "Content-Type: application/json" \
            --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getCode\",\"params\":[\"$addr\", \"$block\"],\"id\":1}" \
            "$RPC_URL" > /dev/null
        sleep 0.001
    done
}

stress_eth_getTransactionCount() {
    local end_time=$1
    while [ $(date +%s) -lt $end_time ]; do
        local addr="${CONTRACTS[$((RANDOM % ${#CONTRACTS[@]}))]}"
        local block="0x$(printf '%x' $((BLOCK_NUM - RANDOM % 1000)))"
        curl -s --max-time 10 -X POST -H "Content-Type: application/json" \
            --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getTransactionCount\",\"params\":[\"$addr\", \"$block\"],\"id\":1}" \
            "$RPC_URL" > /dev/null
        sleep 0.001
    done
}

stress_eth_getProof() {
    local end_time=$1
    while [ $(date +%s) -lt $end_time ]; do
        local addr="${CONTRACTS[$((RANDOM % ${#CONTRACTS[@]}))]}"
        local block="0x$(printf '%x' $((BLOCK_NUM - RANDOM % 100)))"
        curl -s --max-time 60 -X POST -H "Content-Type: application/json" \
            --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getProof\",\"params\":[\"$addr\", [\"0x0\"], \"$block\"],\"id\":1}" \
            "$RPC_URL" > /dev/null
        sleep 0.05
    done
}

stress_eth_getBlockByNumber() {
    local end_time=$1
    while [ $(date +%s) -lt $end_time ]; do
        local block="0x$(printf '%x' $((BLOCK_NUM - RANDOM % 1000)))"
        curl -s --max-time 10 -X POST -H "Content-Type: application/json" \
            --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getBlockByNumber\",\"params\":[\"$block\", true],\"id\":1}" \
            "$RPC_URL" > /dev/null
        sleep 0.01
    done
}

stress_eth_getBlockReceipts() {
    local end_time=$1
    while [ $(date +%s) -lt $end_time ]; do
        local block="0x$(printf '%x' $((BLOCK_NUM - RANDOM % 1000)))"
        curl -s --max-time 60 -X POST -H "Content-Type: application/json" \
            --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getBlockReceipts\",\"params\":[\"$block\"],\"id\":1}" \
            "$RPC_URL" > /dev/null
        sleep 0.05
    done
}

stress_eth_call() {
    local end_time=$1
    while [ $(date +%s) -lt $end_time ]; do
        local addr="${CONTRACTS[$((RANDOM % ${#CONTRACTS[@]}))]}"
        local target="${CONTRACTS[$((RANDOM % ${#CONTRACTS[@]}))]}"
        local block="0x$(printf '%x' $((BLOCK_NUM - RANDOM % 100)))"
        # balanceOf(address)
        local call_data="0x70a08231000000000000000000000000${target:2}"
        curl -s --max-time 30 -X POST -H "Content-Type: application/json" \
            --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_call\",\"params\":[{\"to\":\"$addr\",\"data\":\"$call_data\"}, \"$block\"],\"id\":1}" \
            "$RPC_URL" > /dev/null
        sleep 0.01
    done
}

stress_eth_estimateGas() {
    local end_time=$1
    while [ $(date +%s) -lt $end_time ]; do
        local addr="${CONTRACTS[$((RANDOM % ${#CONTRACTS[@]}))]}"
        local target="${CONTRACTS[$((RANDOM % ${#CONTRACTS[@]}))]}"
        local call_data="0x70a08231000000000000000000000000${target:2}"
        curl -s --max-time 30 -X POST -H "Content-Type: application/json" \
            --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_estimateGas\",\"params\":[{\"to\":\"$addr\",\"data\":\"$call_data\"}],\"id\":1}" \
            "$RPC_URL" > /dev/null
        sleep 0.01
    done
}

stress_trace_transaction() {
    local end_time=$1
    while [ $(date +%s) -lt $end_time ]; do
        if [ ${#TX_HASHES[@]} -gt 0 ]; then
            local tx_hash="${TX_HASHES[$((RANDOM % ${#TX_HASHES[@]}))]}"
            curl -s --max-time 60 -X POST -H "Content-Type: application/json" \
                --data "{\"jsonrpc\":\"2.0\",\"method\":\"trace_transaction\",\"params\":[\"$tx_hash\"],\"id\":1}" \
                "$RPC_URL" > /dev/null
        fi
        sleep 0.1
    done
}

stress_trace_block() {
    local end_time=$1
    while [ $(date +%s) -lt $end_time ]; do
        local block="0x$(printf '%x' $((BLOCK_NUM - RANDOM % 10)))"
        curl -s --max-time 120 -X POST -H "Content-Type: application/json" \
            --data "{\"jsonrpc\":\"2.0\",\"method\":\"trace_block\",\"params\":[\"$block\"],\"id\":1}" \
            "$RPC_URL" > /dev/null
        sleep 0.5
    done
}

stress_debug_traceTransaction() {
    local end_time=$1
    while [ $(date +%s) -lt $end_time ]; do
        if [ ${#TX_HASHES[@]} -gt 0 ]; then
            local tx_hash="${TX_HASHES[$((RANDOM % ${#TX_HASHES[@]}))]}"
            curl -s --max-time 60 -X POST -H "Content-Type: application/json" \
                --data "{\"jsonrpc\":\"2.0\",\"method\":\"debug_traceTransaction\",\"params\":[\"$tx_hash\", {\"tracer\": \"callTracer\"}],\"id\":1}" \
                "$RPC_URL" > /dev/null
        fi
        sleep 0.1
    done
}

stress_ots_searchTransactions() {
    local end_time=$1
    while [ $(date +%s) -lt $end_time ]; do
        local addr="${CONTRACTS[$((RANDOM % ${#CONTRACTS[@]}))]}"
        curl -s --max-time 60 -X POST -H "Content-Type: application/json" \
            --data "{\"jsonrpc\":\"2.0\",\"method\":\"ots_searchTransactionsBefore\",\"params\":[\"$addr\", $BLOCK_NUM, 25],\"id\":1}" \
            "$RPC_URL" > /dev/null
        sleep 0.1
    done
}

stress_metrics() {
    local end_time=$1
    while [ $(date +%s) -lt $end_time ]; do
        curl -s --max-time 30 "$METRICS_URL" > /dev/null
        sleep 0.01
    done
}

# Counter for requests
REQUEST_COUNTS=()

# ============================================================================
# Helper: Flush caches and wait for system to settle
# ============================================================================

flush_and_settle() {
    local reason="$1"
    echo -e "  ${CYAN}Preparing for $reason...${NC}"

    # Flush OS page caches if enabled and running as root
    if [ "$FLUSH_CACHES" = true ]; then
        if [ "$(id -u)" = "0" ]; then
            echo "    Flushing OS page caches..."
            sync
            echo 3 > /proc/sys/vm/drop_caches 2>/dev/null || true
        else
            echo -e "    ${YELLOW}Warning: --flush-caches requires root, skipping${NC}"
        fi
    fi

    # Wait for system to settle
    echo "    Waiting ${SETTLE_TIME}s for system to settle..."
    sleep "$SETTLE_TIME"
}

# ============================================================================
# Capture baseline profiles (idle, no load) - multiple runs for variance
# ============================================================================

echo ""
echo -e "${YELLOW}[0/${#METHOD_LIST[@]}] Capturing baseline profiles (no load)${NC}"
echo "Will run $BASELINE_RUNS baseline measurements for variance estimation"

for run in $(seq 1 $BASELINE_RUNS); do
    echo ""
    echo -e "  ${CYAN}Baseline run $run/$BASELINE_RUNS${NC}"

    flush_and_settle "baseline run $run"

    echo "  Profiling for ${DURATION}s..."
    $PROFILER_CMD --output "$SESSION_DIR/baseline_${run}.jsonl" 2>&1 | tee "$SESSION_DIR/baseline_${run}_profiler.log"

    echo -e "  ${GREEN}Baseline run $run complete${NC}"
done

# Use the last baseline run as the primary baseline for comparison
cp "$SESSION_DIR/baseline_${BASELINE_RUNS}.jsonl" "$SESSION_DIR/baseline.jsonl"
echo ""
echo -e "${GREEN}Baseline capture complete ($BASELINE_RUNS runs)${NC}"

# ============================================================================
# Profile each method
# ============================================================================

TOTAL_METHODS=${#METHOD_LIST[@]}
CURRENT=0

for method in "${METHOD_LIST[@]}"; do
    CURRENT=$((CURRENT + 1))
    echo ""
    echo -e "${BLUE}[$CURRENT/$TOTAL_METHODS] Profiling: $method${NC}"
    echo "Duration: ${DURATION}s with $CONCURRENCY workers"

    # Determine stress function
    case "$method" in
        eth_getBalance) STRESS_FUNC="stress_eth_getBalance" ;;
        eth_getStorageAt) STRESS_FUNC="stress_eth_getStorageAt" ;;
        eth_getCode) STRESS_FUNC="stress_eth_getCode" ;;
        eth_getTransactionCount) STRESS_FUNC="stress_eth_getTransactionCount" ;;
        eth_getProof) STRESS_FUNC="stress_eth_getProof" ;;
        eth_getBlockByNumber) STRESS_FUNC="stress_eth_getBlockByNumber" ;;
        eth_getBlockReceipts) STRESS_FUNC="stress_eth_getBlockReceipts" ;;
        eth_call) STRESS_FUNC="stress_eth_call" ;;
        eth_estimateGas) STRESS_FUNC="stress_eth_estimateGas" ;;
        trace_transaction) STRESS_FUNC="stress_trace_transaction" ;;
        trace_block) STRESS_FUNC="stress_trace_block" ;;
        debug_traceTransaction) STRESS_FUNC="stress_debug_traceTransaction" ;;
        ots_searchTransactions) STRESS_FUNC="stress_ots_searchTransactions" ;;
        metrics) STRESS_FUNC="stress_metrics" ;;
        *)
            echo "Unknown method: $method, skipping"
            continue
            ;;
    esac

    # Flush caches and settle before this method
    flush_and_settle "$method"

    # Start profiler FIRST (before stress workers)
    # This ensures we capture all activity from the start
    $PROFILER_CMD --output "$SESSION_DIR/${method}.jsonl" 2>&1 | tee "$SESSION_DIR/${method}_profiler.log" &
    PROFILER_PID=$!

    # Give profiler a moment to initialize BPF hooks
    sleep 2

    # Now start stress workers - calculate end time AFTER profiler is ready
    START_TIME=$(date +%s)
    # Stress workers run for slightly less than DURATION to ensure they finish
    # before profiler stops, so we capture clean shutdown
    STRESS_DURATION=$((DURATION - 5))
    END_TIME=$((START_TIME + STRESS_DURATION))

    STRESS_PIDS=()
    for i in $(seq 1 $CONCURRENCY); do
        $STRESS_FUNC $END_TIME &
        STRESS_PIDS+=($!)
    done

    echo "  Stress workers started ($CONCURRENCY concurrent)"

    # Wait for profiler (it will exit after duration)
    wait $PROFILER_PID 2>/dev/null || true

    # Kill any remaining stress workers (they should have finished already)
    for pid in "${STRESS_PIDS[@]}"; do
        kill $pid 2>/dev/null || true
    done
    wait 2>/dev/null || true

    echo -e "${GREEN}  $method complete${NC}"
done

# ============================================================================
# Analyze all profiles and generate comparison
# ============================================================================

echo ""
echo -e "${CYAN}Analyzing profiles...${NC}"

# Analyze all baseline runs
for run in $(seq 1 $BASELINE_RUNS); do
    if [ -f "$SESSION_DIR/baseline_${run}.jsonl" ]; then
        echo "  Analyzing baseline run ${run}..."
        "$ANALYZER" --input "$SESSION_DIR/baseline_${run}.jsonl" --mdbx-path "$MDBX_PATH" \
            --format compact --label "baseline_${run}" 2>/dev/null > "$SESSION_DIR/baseline_${run}.json"
    fi
done

# Use last baseline as the primary comparison baseline
if [ -f "$SESSION_DIR/baseline.jsonl" ]; then
    echo "  Analyzing primary baseline..."
    "$ANALYZER" --input "$SESSION_DIR/baseline.jsonl" --mdbx-path "$MDBX_PATH" \
        --format compact --label "baseline" 2>/dev/null > "$SESSION_DIR/baseline.json"
fi

for method in "${METHOD_LIST[@]}"; do
    if [ -f "$SESSION_DIR/${method}.jsonl" ]; then
        echo "  Analyzing ${method}..."
        # Generate compact JSON for comparison
        "$ANALYZER" --input "$SESSION_DIR/${method}.jsonl" --mdbx-path "$MDBX_PATH" \
            --format compact --label "$method" 2>/dev/null > "$SESSION_DIR/${method}.json"
    fi
done

# ============================================================================
# Generate comparison HTML
# ============================================================================

echo "Generating comparison report..."

# Collect all compact JSONs (baseline first, then methods)
COMPARISON_JSON="$SESSION_DIR/comparison_data.json"
echo "[" > "$COMPARISON_JSON"
first=true

# Add baseline first
if [ -f "$SESSION_DIR/baseline.json" ] && [ -s "$SESSION_DIR/baseline.json" ]; then
    first=false
    cat "$SESSION_DIR/baseline.json" >> "$COMPARISON_JSON"
fi

# Add all methods
for method in "${METHOD_LIST[@]}"; do
    json="$SESSION_DIR/${method}.json"
    if [ -f "$json" ] && [ -s "$json" ]; then
        if [ "$first" = true ]; then
            first=false
        else
            echo "," >> "$COMPARISON_JSON"
        fi
        cat "$json" >> "$COMPARISON_JSON"
    fi
done
echo "]" >> "$COMPARISON_JSON"

COMPARISON_DATA=$(cat "$COMPARISON_JSON")

# Generate HTML
cat > "$SESSION_DIR/comparison.html" << 'HTMLEOF'
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>RPC Method MDBX Comparison</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            background: #0a0a0f;
            color: #e4e4e7;
            padding: 24px;
            line-height: 1.5;
        }
        .container { max-width: 1600px; margin: 0 auto; }
        h1 { color: #3b82f6; margin-bottom: 8px; }
        .subtitle { color: #71717a; margin-bottom: 24px; }

        .summary-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
            gap: 12px;
            margin-bottom: 32px;
        }

        .method-card {
            background: #12121a;
            border-radius: 8px;
            border: 1px solid #1e1e2a;
            padding: 16px;
            cursor: pointer;
            transition: border-color 0.2s;
        }
        .method-card:hover { border-color: #3b82f6; }
        .method-card.worst { border-left: 3px solid #f87171; }
        .method-card.best { border-left: 3px solid #22c55e; }

        .method-name {
            font-size: 14px;
            font-weight: 600;
            color: #3b82f6;
            margin-bottom: 8px;
            font-family: monospace;
        }

        .method-stats {
            display: grid;
            grid-template-columns: repeat(2, 1fr);
            gap: 8px;
        }
        .stat {
            background: #0a0a0f;
            padding: 8px;
            border-radius: 4px;
        }
        .stat-label {
            font-size: 10px;
            color: #71717a;
            text-transform: uppercase;
        }
        .stat-value {
            font-size: 16px;
            font-weight: 700;
            color: #e4e4e7;
        }
        .stat-value.bad { color: #f87171; }
        .stat-value.good { color: #22c55e; }

        .section { margin-bottom: 32px; }
        .section-title {
            font-size: 16px;
            color: #3b82f6;
            margin-bottom: 16px;
            padding-bottom: 8px;
            border-bottom: 1px solid #1e1e2a;
        }

        .ranking-table {
            width: 100%;
            border-collapse: collapse;
            font-size: 13px;
        }
        .ranking-table th,
        .ranking-table td {
            padding: 12px;
            text-align: left;
            border-bottom: 1px solid #1e1e2a;
        }
        .ranking-table th {
            background: #0a0a0f;
            color: #71717a;
            font-weight: 500;
            text-transform: uppercase;
            font-size: 11px;
        }
        .ranking-table tr:hover { background: #1a1a24; }
        .ranking-table .rank {
            font-weight: 700;
            width: 40px;
        }
        .ranking-table .rank.top { color: #f87171; }
        .ranking-table .method { font-family: monospace; color: #3b82f6; }

        .bar-cell {
            display: flex;
            align-items: center;
            gap: 8px;
        }
        .bar {
            height: 8px;
            border-radius: 4px;
            background: #3b82f6;
            transition: width 0.3s;
        }
        .bar.major { background: #f87171; }

        .table-breakdown {
            margin-top: 24px;
        }
        .table-breakdown th:first-child { width: 200px; }

        .baseline-banner {
            background: linear-gradient(135deg, #1a1a2e 0%, #16213e 100%);
            border: 1px solid #3b82f6;
            border-radius: 8px;
            padding: 16px 20px;
            margin-bottom: 24px;
        }
        .baseline-title {
            font-size: 14px;
            font-weight: 600;
            color: #3b82f6;
            margin-bottom: 8px;
        }
        .baseline-stats {
            display: flex;
            gap: 24px;
            font-size: 13px;
            color: #e4e4e7;
            margin-bottom: 8px;
        }
        .baseline-note {
            font-size: 11px;
            color: #71717a;
        }
        .stat-delta {
            font-size: 10px;
            margin-top: 4px;
        }
        .stat-delta.delta-bad { color: #f87171; }
        .stat-delta.delta-good { color: #22c55e; }

        .delta-bar-bad { background: #f87171; }
        .delta-bar-good { background: #22c55e; }
        .delta-text-bad { color: #f87171; }
        .delta-text-good { color: #22c55e; }
    </style>
</head>
<body>
    <div class="container">
        <h1>RPC Method MDBX Impact</h1>
        <p class="subtitle">Comparing page fault patterns across different RPC methods</p>

        <div id="content"></div>
    </div>

    <script>
HTMLEOF

echo "const DATA = $COMPARISON_DATA;" >> "$SESSION_DIR/comparison.html"

cat >> "$SESSION_DIR/comparison.html" << 'HTMLEOF2'

        const fmt = n => {
            if (n === null || n === undefined) return '-';
            if (n >= 1e6) return (n/1e6).toFixed(1) + 'M';
            if (n >= 1e3) return (n/1e3).toFixed(1) + 'K';
            return n.toFixed ? n.toFixed(0) : n;
        };

        const fmtPct = n => (n * 100).toFixed(1) + '%';

        const fmtDelta = n => {
            if (n === null || n === undefined) return '-';
            const prefix = n > 0 ? '+' : '';
            if (Math.abs(n) >= 1e6) return prefix + (n/1e6).toFixed(1) + 'M';
            if (Math.abs(n) >= 1e3) return prefix + (n/1e3).toFixed(1) + 'K';
            return prefix + (n.toFixed ? n.toFixed(0) : n);
        };

        if (!DATA || !Array.isArray(DATA) || DATA.length === 0) {
            document.getElementById('content').innerHTML =
                '<p style="color: #f87171;">No profile data found.</p>';
        } else {

        // Extract baseline and methods
        const baseline = DATA.find(d => d.label === 'baseline');
        const methods = DATA.filter(d => d.label !== 'baseline');

        // Sort methods by delta from baseline (worst first)
        const sorted = [...methods].sort((a, b) => {
            const deltaA = baseline ? (a.page_faults.total - baseline.page_faults.total) : a.page_faults.total;
            const deltaB = baseline ? (b.page_faults.total - baseline.page_faults.total) : b.page_faults.total;
            return deltaB - deltaA;
        });

        const maxFaults = sorted[0]?.page_faults?.total || 1;
        const maxMajor = Math.max(...methods.map(d => d.page_faults.major));
        const maxRate = Math.max(...methods.map(d => d.page_faults.rate_per_sec));
        const maxDelta = baseline ? Math.max(...methods.map(d => d.page_faults.total - baseline.page_faults.total)) : maxFaults;

        // Find best/worst
        const worstMethod = sorted[0]?.label;
        const bestMethod = sorted[sorted.length - 1]?.label;

        let html = '';

        // Baseline info banner
        if (baseline) {
            const bpf = baseline.page_faults;
            html += `
            <div class="baseline-banner">
                <div class="baseline-title">Baseline (Idle)</div>
                <div class="baseline-stats">
                    <span>Total: ${fmt(bpf.total)}</span>
                    <span>Major: ${fmt(bpf.major)}</span>
                    <span>Rate: ${fmt(bpf.rate_per_sec)}/s</span>
                </div>
                <div class="baseline-note">Delta values below show additional load caused by each method</div>
            </div>`;
        }

        // Summary cards
        html += '<div class="summary-grid">';
        for (const profile of sorted) {
            const pf = profile.page_faults;
            const isWorst = profile.label === worstMethod;
            const isBest = profile.label === bestMethod;
            const cardClass = isWorst ? 'worst' : (isBest ? 'best' : '');

            // Calculate deltas from baseline
            const deltaTotal = baseline ? (pf.total - baseline.page_faults.total) : pf.total;
            const deltaMajor = baseline ? (pf.major - baseline.page_faults.major) : pf.major;
            const deltaRate = baseline ? (pf.rate_per_sec - baseline.page_faults.rate_per_sec) : pf.rate_per_sec;

            html += `
            <div class="method-card ${cardClass}">
                <div class="method-name">${profile.label}</div>
                <div class="method-stats">
                    <div class="stat">
                        <div class="stat-label">Total Faults</div>
                        <div class="stat-value">${fmt(pf.total)}</div>
                        ${baseline ? `<div class="stat-delta ${deltaTotal > 0 ? 'delta-bad' : 'delta-good'}">${fmtDelta(deltaTotal)} vs baseline</div>` : ''}
                    </div>
                    <div class="stat">
                        <div class="stat-label">Major (Disk)</div>
                        <div class="stat-value ${pf.major_ratio > 0.4 ? 'bad' : ''}">${fmt(pf.major)}</div>
                        ${baseline ? `<div class="stat-delta ${deltaMajor > 0 ? 'delta-bad' : 'delta-good'}">${fmtDelta(deltaMajor)} vs baseline</div>` : ''}
                    </div>
                    <div class="stat">
                        <div class="stat-label">Fault Rate</div>
                        <div class="stat-value">${fmt(pf.rate_per_sec)}/s</div>
                        ${baseline ? `<div class="stat-delta ${deltaRate > 0 ? 'delta-bad' : 'delta-good'}">${fmtDelta(deltaRate)}/s vs baseline</div>` : ''}
                    </div>
                    <div class="stat">
                        <div class="stat-label">Major Ratio</div>
                        <div class="stat-value ${pf.major_ratio > 0.4 ? 'bad' : 'good'}">${fmtPct(pf.major_ratio)}</div>
                    </div>
                </div>
            </div>`;
        }
        html += '</div>';

        // Ranking table
        html += `
        <div class="section">
            <div class="section-title">Method Ranking by Page Faults ${baseline ? '(sorted by delta from baseline)' : ''}</div>
            <table class="ranking-table">
                <thead>
                    <tr>
                        <th>#</th>
                        <th>Method</th>
                        <th>Total Faults</th>
                        ${baseline ? '<th>Delta from Baseline</th>' : ''}
                        <th>Major Faults</th>
                        <th>Fault Rate</th>
                        <th>Major Ratio</th>
                        <th>Unique Pages</th>
                    </tr>
                </thead>
                <tbody>`;

        sorted.forEach((profile, idx) => {
            const pf = profile.page_faults;
            const barWidth = (pf.total / maxFaults * 100);
            const majorBarWidth = (pf.major / maxMajor * 100);
            const deltaTotal = baseline ? (pf.total - baseline.page_faults.total) : 0;
            const deltaBarWidth = baseline && maxDelta > 0 ? (Math.abs(deltaTotal) / maxDelta * 100) : 0;

            html += `
                <tr>
                    <td class="rank ${idx < 3 ? 'top' : ''}">${idx + 1}</td>
                    <td class="method">${profile.label}</td>
                    <td>
                        <div class="bar-cell">
                            <div class="bar" style="width: ${barWidth}%; min-width: 4px;"></div>
                            <span>${fmt(pf.total)}</span>
                        </div>
                    </td>
                    ${baseline ? `
                    <td>
                        <div class="bar-cell">
                            <div class="bar ${deltaTotal > 0 ? 'delta-bar-bad' : 'delta-bar-good'}" style="width: ${deltaBarWidth}%; min-width: 4px;"></div>
                            <span class="${deltaTotal > 0 ? 'delta-text-bad' : 'delta-text-good'}">${fmtDelta(deltaTotal)}</span>
                        </div>
                    </td>` : ''}
                    <td>
                        <div class="bar-cell">
                            <div class="bar major" style="width: ${majorBarWidth}%; min-width: 4px;"></div>
                            <span>${fmt(pf.major)}</span>
                        </div>
                    </td>
                    <td>${fmt(pf.rate_per_sec)}/s</td>
                    <td>${fmtPct(pf.major_ratio)}</td>
                    <td>${fmt(pf.unique_pages)}</td>
                </tr>`;
        });

        html += '</tbody></table></div>';

        // Table breakdown - which tables each method hits
        if (DATA[0]?.tables) {
            html += `
            <div class="section table-breakdown">
                <div class="section-title">Table Access by Method</div>
                <table class="ranking-table">
                    <thead>
                        <tr>
                            <th>Table</th>`;

            for (const profile of sorted) {
                html += `<th>${profile.label}</th>`;
            }
            html += '</tr></thead><tbody>';

            // Collect all tables
            const allTables = new Set();
            for (const profile of DATA) {
                for (const t of profile.tables || []) {
                    allTables.add(t.name);
                }
            }

            // Find max for scaling
            let maxTableFaults = 0;
            for (const profile of DATA) {
                for (const t of profile.tables || []) {
                    if (t.faults > maxTableFaults) maxTableFaults = t.faults;
                }
            }

            // Sort tables by total faults across all methods
            const tableOrder = [...allTables].map(name => {
                let total = 0;
                for (const profile of DATA) {
                    const t = (profile.tables || []).find(x => x.name === name);
                    if (t) total += t.faults;
                }
                return { name, total };
            }).sort((a, b) => b.total - a.total).slice(0, 20);

            for (const { name } of tableOrder) {
                html += `<tr><td style="font-family: monospace;">${name}</td>`;
                for (const profile of sorted) {
                    const t = (profile.tables || []).find(x => x.name === name);
                    const faults = t ? t.faults : 0;
                    const major = t ? t.major_faults : 0;
                    const barWidth = maxTableFaults > 0 ? (faults / maxTableFaults * 100) : 0;
                    html += `<td>
                        <div class="bar-cell">
                            <div class="bar" style="width: ${barWidth}%; min-width: ${faults > 0 ? 2 : 0}px;"></div>
                            <span>${fmt(faults)}</span>
                        </div>
                    </td>`;
                }
                html += '</tr>';
            }

            html += '</tbody></table></div>';
        }

        document.getElementById('content').innerHTML = html;
        }
    </script>
</body>
</html>
HTMLEOF2

echo ""
echo -e "${GREEN}========================================"
echo "Complete"
echo -e "========================================${NC}"
echo ""
echo "Comparison report:"
echo "  file://$SESSION_DIR/comparison.html"
echo ""
echo "Individual profiles:"
for method in "${METHOD_LIST[@]}"; do
    if [ -f "$SESSION_DIR/${method}.jsonl" ]; then
        echo "  $method: $SESSION_DIR/${method}.jsonl"
    fi
done
