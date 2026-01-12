#!/usr/bin/env bash
#
# RPC Stress Test for Reth MDBX Profiling
#
# Generates sustained RPC traffic to stress test specific endpoint categories.
# Based on actual reth RPC implementation analysis.
#
# Key findings from reth codebase:
# - eth_getBalance, eth_getCode, eth_getStorageAt: UNBOUNDED (no semaphore)
# - eth_call, eth_estimateGas: Protected by blocking_io_request_semaphore
# - debug_traceTransaction: Protected by BlockingTaskGuard (lower limit)
#
# Usage:
#   ./rpc_stress.sh [category] [duration_secs] [rpc_url] [concurrency]
#
# Categories:
#   state_unbounded  - eth_getBalance, eth_getStorageAt (NO semaphore protection)
#   state_execution  - eth_call, eth_estimateGas (semaphore protected)
#   debug_trace      - debug_traceTransaction (heavily protected, expensive)
#   mixed            - Realistic mix of all categories
#
# Examples:
#   ./rpc_stress.sh state_unbounded 30 http://localhost:8545 50
#   ./rpc_stress.sh mixed 60

set -euo pipefail

CATEGORY="${1:-mixed}"
DURATION="${2:-30}"
RPC_URL="${3:-http://localhost:8545}"
CONCURRENCY="${4:-20}"
OUTPUT_DIR="${5:-./stress_results}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

mkdir -p "$OUTPUT_DIR"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
LOG_FILE="$OUTPUT_DIR/rpc_stress_${CATEGORY}_${TIMESTAMP}.log"
STATS_FILE="$OUTPUT_DIR/rpc_stress_${CATEGORY}_${TIMESTAMP}_stats.json"

# Counters (using temp files for subprocess communication)
COUNTER_FILE=$(mktemp)
ERROR_FILE=$(mktemp)
echo "0" > "$COUNTER_FILE"
echo "0" > "$ERROR_FILE"

cleanup() {
    rm -f "$COUNTER_FILE" "$ERROR_FILE" 2>/dev/null || true
    # Kill any background jobs
    jobs -p | xargs -r kill 2>/dev/null || true
}
trap cleanup EXIT

log() {
    echo -e "${BLUE}[$(date +%H:%M:%S)]${NC} $1" | tee -a "$LOG_FILE"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1" | tee -a "$LOG_FILE"
}

success() {
    echo -e "${GREEN}[OK]${NC} $1" | tee -a "$LOG_FILE"
}

# Check dependencies
if ! command -v curl &> /dev/null; then
    error "curl is required"
    exit 1
fi

# Test RPC connection
log "Testing RPC connection to $RPC_URL..."
BLOCK_RESPONSE=$(curl -s --max-time 5 -X POST -H "Content-Type: application/json" \
    --data '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
    "$RPC_URL" 2>&1) || {
    error "Cannot connect to RPC endpoint at $RPC_URL"
    exit 1
}

if echo "$BLOCK_RESPONSE" | grep -q '"error"'; then
    error "RPC error: $BLOCK_RESPONSE"
    exit 1
fi

BLOCK_HEX=$(echo "$BLOCK_RESPONSE" | grep -o '"result":"[^"]*"' | cut -d'"' -f4)
BLOCK_NUM=$((BLOCK_HEX))
success "Connected. Current block: $BLOCK_NUM"

# Well-known contract addresses for realistic queries
# These are mainnet addresses - adjust for other networks
CONTRACTS=(
    "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"  # WETH
    "0xdAC17F958D2ee523a2206206994597C13D831ec7"  # USDT
    "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"  # USDC
    "0x6B175474E89094C44Da98b954EesadFD72257c9f"  # DAI (note: typo in original, keeping for reference)
    "0x1f9840a85d5aF5bf1D1762F925BDADdC4201F984"  # UNI
    "0x514910771AF9Ca656af840dff83E8264EcF986CA"  # LINK
    "0x7Fc66500c84A76Ad7e9c93437bFc5Ac33E2DDaE9"  # AAVE
    "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599"  # WBTC
    "0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D"  # Uniswap V2 Router
    "0xE592427A0AEce92De3Edee1F18E0157C05861564"  # Uniswap V3 Router
)

# Storage slots commonly accessed
STORAGE_SLOTS=("0x0" "0x1" "0x2" "0x3" "0x4" "0x5" "0x6" "0x7" "0x8" "0x9" "0xa" "0xb")

# Increment counter atomically
inc_counter() {
    local current
    current=$(cat "$COUNTER_FILE")
    echo $((current + 1)) > "$COUNTER_FILE"
}

inc_errors() {
    local current
    current=$(cat "$ERROR_FILE")
    echo $((current + 1)) > "$ERROR_FILE"
}

# RPC call helper - silent, just counts success/failure
rpc_call() {
    local method="$1"
    local params="$2"

    local result
    result=$(curl -s --max-time 10 -X POST -H "Content-Type: application/json" \
        --data "{\"jsonrpc\":\"2.0\",\"method\":\"$method\",\"params\":$params,\"id\":1}" \
        "$RPC_URL" 2>&1)

    inc_counter

    if echo "$result" | grep -q '"error"'; then
        inc_errors
        return 1
    fi
    return 0
}

# ============================================================================
# WORKLOAD: state_unbounded
# These endpoints have NO semaphore protection in reth!
# - eth_getBalance: direct state lookup
# - eth_getCode: bytecode retrieval
# - eth_getStorageAt: storage slot read
# Target: MDBX read contention under high parallelism
# ============================================================================
workload_state_unbounded() {
    local end_time=$1

    while [ $(date +%s) -lt $end_time ]; do
        local addr="${CONTRACTS[$((RANDOM % ${#CONTRACTS[@]}))]}"
        local slot="${STORAGE_SLOTS[$((RANDOM % ${#STORAGE_SLOTS[@]}))]}"
        # Use recent blocks for hot state, older blocks for cold state
        local block_offset=$((RANDOM % 1000))
        local block="0x$(printf '%x' $((BLOCK_NUM - block_offset)))"

        case $((RANDOM % 4)) in
            0) rpc_call "eth_getBalance" "[\"$addr\", \"$block\"]" ;;
            1) rpc_call "eth_getCode" "[\"$addr\", \"$block\"]" ;;
            2) rpc_call "eth_getStorageAt" "[\"$addr\", \"$slot\", \"$block\"]" ;;
            3) rpc_call "eth_getTransactionCount" "[\"$addr\", \"$block\"]" ;;
        esac

        # Minimal delay - we want to stress test
        sleep 0.001
    done
}

# ============================================================================
# WORKLOAD: state_execution
# These endpoints ARE protected by blocking_io_request_semaphore
# - eth_call: EVM execution
# - eth_estimateGas: gas estimation
# Target: Semaphore exhaustion, EVM + state access
# ============================================================================
workload_state_execution() {
    local end_time=$1

    while [ $(date +%s) -lt $end_time ]; do
        local addr="${CONTRACTS[$((RANDOM % ${#CONTRACTS[@]}))]}"
        local block="0x$(printf '%x' $((BLOCK_NUM - RANDOM % 100)))"

        # balanceOf(address) call - common ERC20 pattern
        # Selector: 0x70a08231
        local target_addr="${CONTRACTS[$((RANDOM % ${#CONTRACTS[@]}))]}"
        local call_data="0x70a08231000000000000000000000000${target_addr:2}"

        case $((RANDOM % 3)) in
            0)
                # eth_call - protected by semaphore
                rpc_call "eth_call" "[{\"to\":\"$addr\",\"data\":\"$call_data\"}, \"$block\"]"
                ;;
            1)
                # eth_estimateGas - protected by semaphore
                rpc_call "eth_estimateGas" "[{\"to\":\"$addr\",\"data\":\"$call_data\"}]"
                ;;
            2)
                # eth_createAccessList - also protected
                rpc_call "eth_createAccessList" "[{\"to\":\"$addr\",\"data\":\"$call_data\"}, \"$block\"]"
                ;;
        esac

        sleep 0.01
    done
}

# ============================================================================
# WORKLOAD: debug_trace
# These are the MOST expensive operations
# Protected by BlockingTaskGuard with very low limit (typically 4-10)
# - debug_traceTransaction: full tx replay with tracing
# - debug_traceBlock: replay entire block
# Target: Trace queue exhaustion, full state reconstruction
# ============================================================================
workload_debug_trace() {
    local end_time=$1

    # First, get some recent transaction hashes to trace
    local block_hex="0x$(printf '%x' $((BLOCK_NUM - 1)))"
    local block_data
    block_data=$(curl -s -X POST -H "Content-Type: application/json" \
        --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getBlockByNumber\",\"params\":[\"$block_hex\", false],\"id\":0}" \
        "$RPC_URL")

    # Extract transaction hashes
    local tx_hashes=()
    while IFS= read -r line; do
        tx_hashes+=("$line")
    done < <(echo "$block_data" | grep -oE '0x[a-fA-F0-9]{64}' | head -20)

    if [ ${#tx_hashes[@]} -eq 0 ]; then
        log "Warning: No transactions found in recent blocks for tracing"
        return
    fi

    while [ $(date +%s) -lt $end_time ]; do
        local tx_hash="${tx_hashes[$((RANDOM % ${#tx_hashes[@]}))]}"
        local trace_block="0x$(printf '%x' $((BLOCK_NUM - RANDOM % 10)))"

        case $((RANDOM % 3)) in
            0)
                # debug_traceTransaction with callTracer
                rpc_call "debug_traceTransaction" "[\"$tx_hash\", {\"tracer\": \"callTracer\"}]"
                ;;
            1)
                # debug_traceTransaction with prestateTracer
                rpc_call "debug_traceTransaction" "[\"$tx_hash\", {\"tracer\": \"prestateTracer\"}]"
                ;;
            2)
                # trace_transaction (parity style)
                rpc_call "trace_transaction" "[\"$tx_hash\"]"
                ;;
        esac

        # Longer delay - these are expensive
        sleep 0.1
    done
}

# ============================================================================
# WORKLOAD: mixed
# Realistic traffic pattern combining all types
# Weights based on typical node usage
# ============================================================================
workload_mixed() {
    local end_time=$1

    # Get some tx hashes for tracing
    local block_hex="0x$(printf '%x' $((BLOCK_NUM - 1)))"
    local block_data
    block_data=$(curl -s -X POST -H "Content-Type: application/json" \
        --data "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getBlockByNumber\",\"params\":[\"$block_hex\", false],\"id\":0}" \
        "$RPC_URL")

    local tx_hashes=()
    while IFS= read -r line; do
        tx_hashes+=("$line")
    done < <(echo "$block_data" | grep -oE '0x[a-fA-F0-9]{64}' | head -20)

    while [ $(date +%s) -lt $end_time ]; do
        local addr="${CONTRACTS[$((RANDOM % ${#CONTRACTS[@]}))]}"
        local slot="${STORAGE_SLOTS[$((RANDOM % ${#STORAGE_SLOTS[@]}))]}"
        local block="0x$(printf '%x' $((BLOCK_NUM - RANDOM % 1000)))"
        local call_data="0x70a08231000000000000000000000000${addr:2}"

        # Weighted random: 60% state reads, 30% execution, 10% trace
        local weight=$((RANDOM % 100))

        if [ $weight -lt 60 ]; then
            # State reads (unbounded) - most common
            case $((RANDOM % 4)) in
                0) rpc_call "eth_getBalance" "[\"$addr\", \"$block\"]" ;;
                1) rpc_call "eth_getCode" "[\"$addr\", \"latest\"]" ;;
                2) rpc_call "eth_getStorageAt" "[\"$addr\", \"$slot\", \"$block\"]" ;;
                3) rpc_call "eth_getTransactionCount" "[\"$addr\", \"latest\"]" ;;
            esac
            sleep 0.001
        elif [ $weight -lt 90 ]; then
            # Execution (semaphore protected)
            case $((RANDOM % 2)) in
                0) rpc_call "eth_call" "[{\"to\":\"$addr\",\"data\":\"$call_data\"}, \"$block\"]" ;;
                1) rpc_call "eth_estimateGas" "[{\"to\":\"$addr\",\"data\":\"$call_data\"}]" ;;
            esac
            sleep 0.01
        else
            # Trace (heavily protected) - least common
            if [ ${#tx_hashes[@]} -gt 0 ]; then
                local tx_hash="${tx_hashes[$((RANDOM % ${#tx_hashes[@]}))]}"
                rpc_call "debug_traceTransaction" "[\"$tx_hash\", {\"tracer\": \"callTracer\"}]"
            fi
            sleep 0.1
        fi
    done
}

# ============================================================================
# Main execution
# ============================================================================

log "========================================"
log "RPC Stress Test"
log "========================================"
log "Category:    $CATEGORY"
log "Duration:    ${DURATION}s"
log "Concurrency: $CONCURRENCY workers"
log "RPC URL:     $RPC_URL"
log "Output:      $LOG_FILE"
log "========================================"

START_TIME=$(date +%s)
END_TIME=$((START_TIME + DURATION))

# Select workload function
case "$CATEGORY" in
    state_unbounded)
        WORKLOAD_FUNC="workload_state_unbounded"
        log "Testing UNBOUNDED state queries (no semaphore protection)"
        log "Target: MDBX read contention"
        ;;
    state_execution)
        WORKLOAD_FUNC="workload_state_execution"
        log "Testing EVM execution endpoints (semaphore protected)"
        log "Target: blocking_io_request_semaphore exhaustion"
        ;;
    debug_trace)
        WORKLOAD_FUNC="workload_debug_trace"
        log "Testing debug/trace endpoints (heavily protected)"
        log "Target: BlockingTaskGuard exhaustion"
        ;;
    mixed)
        WORKLOAD_FUNC="workload_mixed"
        log "Testing mixed realistic workload"
        log "Target: Overall system behavior"
        ;;
    *)
        error "Unknown category: $CATEGORY"
        echo "Valid categories: state_unbounded, state_execution, debug_trace, mixed"
        exit 1
        ;;
esac

log "Starting $CONCURRENCY concurrent workers..."

# Launch workers
PIDS=()
for i in $(seq 1 $CONCURRENCY); do
    $WORKLOAD_FUNC $END_TIME &
    PIDS+=($!)
done

# Progress reporting
while [ $(date +%s) -lt $END_TIME ]; do
    sleep 5
    local current_count=$(cat "$COUNTER_FILE")
    local current_errors=$(cat "$ERROR_FILE")
    local elapsed=$(($(date +%s) - START_TIME))
    local rps=$((current_count / (elapsed > 0 ? elapsed : 1)))
    log "Progress: $current_count requests, $current_errors errors, ${rps} req/s"
done

# Wait for all workers
for pid in "${PIDS[@]}"; do
    wait "$pid" 2>/dev/null || true
done

# Final stats
FINAL_COUNT=$(cat "$COUNTER_FILE")
FINAL_ERRORS=$(cat "$ERROR_FILE")
ELAPSED=$(($(date +%s) - START_TIME))
RPS=$((FINAL_COUNT / (ELAPSED > 0 ? ELAPSED : 1)))
ERROR_RATE=$(echo "scale=2; $FINAL_ERRORS * 100 / ($FINAL_COUNT + 1)" | bc 2>/dev/null || echo "0")

log "========================================"
log "Stress Test Complete"
log "========================================"
log "Total requests: $FINAL_COUNT"
log "Errors:         $FINAL_ERRORS ($ERROR_RATE%)"
log "Duration:       ${ELAPSED}s"
log "Throughput:     ${RPS} req/s"
log "========================================"

# Write JSON summary
cat > "$STATS_FILE" << EOF
{
    "category": "$CATEGORY",
    "duration_secs": $ELAPSED,
    "concurrency": $CONCURRENCY,
    "total_requests": $FINAL_COUNT,
    "errors": $FINAL_ERRORS,
    "requests_per_sec": $RPS,
    "rpc_url": "$RPC_URL",
    "start_block": $BLOCK_NUM,
    "timestamp": "$TIMESTAMP"
}
EOF

success "Stats written to $STATS_FILE"
