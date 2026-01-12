#!/usr/bin/env bash
#
# Metrics Endpoint Stress Test
#
# This script hammers the /metrics endpoint to reproduce the blocking I/O issue
# documented in the reth GitHub issue.
#
# The Problem (from issue analysis):
# On every /metrics request, reth executes these BLOCKING operations synchronously:
#
# 1. db.report_metrics() - Opens MDBX read transaction, iterates ALL tables:
#    - For each table in Tables::ALL: open_db() + db_stat()
#    - Reads freelist() and stat()
#    - This contends with write transactions on a syncing node
#    Source: crates/storage/db/src/implementation/mdbx/mod.rs:265-341
#
# 2. sfp.report_metrics() - Enumerates all static file segments:
#    - iter_static_files() over all segments
#    - For each segment: opens jar provider, gets row counts
#    - Calls metadata() on 4 paths per segment
#    Source: crates/storage/provider/src/providers/static_file/manager.rs:477-523
#
# 3. Default hooks:
#    - Collector::default().collect() - process metrics
#    - collect_memory_stats() - jemalloc epoch + stats
#    - collect_io_stats() - /proc/self/io reads
#    Source: crates/node/metrics/src/hooks.rs:44-49
#
# Usage:
#   ./metrics_stress.sh [duration_secs] [concurrency] [metrics_url]
#
# Examples:
#   ./metrics_stress.sh 30 10 http://localhost:9001/metrics
#   ./metrics_stress.sh 60 20

set -euo pipefail

DURATION="${1:-30}"
CONCURRENCY="${2:-10}"
METRICS_URL="${3:-http://localhost:9001}"
OUTPUT_DIR="${4:-./stress_results}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

mkdir -p "$OUTPUT_DIR"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
LOG_FILE="$OUTPUT_DIR/metrics_stress_${TIMESTAMP}.log"
STATS_FILE="$OUTPUT_DIR/metrics_stress_${TIMESTAMP}_stats.json"
LATENCY_FILE="$OUTPUT_DIR/metrics_stress_${TIMESTAMP}_latencies.txt"

# Counters
COUNTER_FILE=$(mktemp)
ERROR_FILE=$(mktemp)
echo "0" > "$COUNTER_FILE"
echo "0" > "$ERROR_FILE"

cleanup() {
    rm -f "$COUNTER_FILE" "$ERROR_FILE" 2>/dev/null || true
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

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1" | tee -a "$LOG_FILE"
}

# Check dependencies
if ! command -v curl &> /dev/null; then
    error "curl is required"
    exit 1
fi

# Test metrics endpoint connection
log "Testing metrics endpoint at $METRICS_URL..."
RESPONSE=$(curl -s --max-time 10 -o /dev/null -w "%{http_code}" "$METRICS_URL" 2>&1) || {
    error "Cannot connect to metrics endpoint at $METRICS_URL"
    echo ""
    echo "Make sure reth is running with metrics enabled:"
    echo "  reth node --metrics 0.0.0.0:9001"
    exit 1
}

if [ "$RESPONSE" != "200" ]; then
    error "Metrics endpoint returned HTTP $RESPONSE"
    exit 1
fi
success "Metrics endpoint responding"

# Check if we can see the expensive metrics
log "Checking for expensive metrics (db.*, static_files.*)..."
METRICS_SAMPLE=$(curl -s --max-time 10 "$METRICS_URL" 2>&1)

if echo "$METRICS_SAMPLE" | grep -q "db_table_size"; then
    success "Found MDBX database metrics (db_table_size)"
else
    warn "MDBX database metrics not found - db.report_metrics() may not be registered"
fi

if echo "$METRICS_SAMPLE" | grep -q "static_files_segment"; then
    success "Found static file metrics (static_files_segment)"
else
    warn "Static file metrics not found - sfp.report_metrics() may not be registered"
fi

# Count how many db tables are being reported
DB_TABLE_COUNT=$(echo "$METRICS_SAMPLE" | grep -c "db_table_size" || echo "0")
log "Database tables being reported: $DB_TABLE_COUNT"

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

# Worker function - hammers metrics endpoint
metrics_worker() {
    local end_time=$1
    local worker_id=$2

    while [ $(date +%s) -lt $end_time ]; do
        # Measure request latency
        local start_ms=$(date +%s%3N)

        local status
        status=$(curl -s --max-time 30 -o /dev/null -w "%{http_code}" "$METRICS_URL" 2>&1) || status="000"

        local end_ms=$(date +%s%3N)
        local latency_ms=$((end_ms - start_ms))

        inc_counter

        if [ "$status" != "200" ]; then
            inc_errors
        fi

        # Record latency
        echo "$latency_ms" >> "$LATENCY_FILE"

        # Minimal delay - we want to stress test the blocking I/O
        sleep 0.01
    done
}

# ============================================================================
# Main execution
# ============================================================================

log "========================================"
log "Metrics Endpoint Stress Test"
log "========================================"
log "Duration:    ${DURATION}s"
log "Concurrency: $CONCURRENCY workers"
log "Metrics URL: $METRICS_URL"
log "Output:      $LOG_FILE"
log "========================================"
log ""
log "${CYAN}Target: Blocking I/O in metrics collection${NC}"
log "  - db.report_metrics(): MDBX read txn + db_stat() on ALL tables"
log "  - sfp.report_metrics(): Static file enumeration + metadata()"
log "  - collect_memory_stats(): jemalloc epoch advance"
log ""

# Clear latency file
> "$LATENCY_FILE"

START_TIME=$(date +%s)
END_TIME=$((START_TIME + DURATION))

log "Starting $CONCURRENCY concurrent workers..."

# Launch workers
PIDS=()
for i in $(seq 1 $CONCURRENCY); do
    metrics_worker $END_TIME $i &
    PIDS+=($!)
done

# Progress reporting
while [ $(date +%s) -lt $END_TIME ]; do
    sleep 5
    local current_count=$(cat "$COUNTER_FILE")
    local current_errors=$(cat "$ERROR_FILE")
    local elapsed=$(($(date +%s) - START_TIME))
    local rps=$((current_count / (elapsed > 0 ? elapsed : 1)))

    # Calculate current latency stats
    if [ -s "$LATENCY_FILE" ]; then
        local avg_latency=$(awk '{ sum += $1; count++ } END { if(count>0) printf "%.0f", sum/count; else print "0" }' "$LATENCY_FILE")
        local max_latency=$(sort -n "$LATENCY_FILE" | tail -1)
        log "Progress: $current_count reqs, ${rps}/s, avg=${avg_latency}ms, max=${max_latency}ms, errors=$current_errors"
    else
        log "Progress: $current_count requests, $current_errors errors, ${rps} req/s"
    fi
done

# Wait for all workers
for pid in "${PIDS[@]}"; do
    wait "$pid" 2>/dev/null || true
done

# Calculate final statistics
FINAL_COUNT=$(cat "$COUNTER_FILE")
FINAL_ERRORS=$(cat "$ERROR_FILE")
ELAPSED=$(($(date +%s) - START_TIME))
RPS=$((FINAL_COUNT / (ELAPSED > 0 ? ELAPSED : 1)))

# Latency statistics
if [ -s "$LATENCY_FILE" ]; then
    LATENCY_AVG=$(awk '{ sum += $1; count++ } END { if(count>0) printf "%.1f", sum/count; else print "0" }' "$LATENCY_FILE")
    LATENCY_MIN=$(sort -n "$LATENCY_FILE" | head -1)
    LATENCY_MAX=$(sort -n "$LATENCY_FILE" | tail -1)

    # Percentiles
    TOTAL_LINES=$(wc -l < "$LATENCY_FILE")
    P50_LINE=$((TOTAL_LINES * 50 / 100))
    P95_LINE=$((TOTAL_LINES * 95 / 100))
    P99_LINE=$((TOTAL_LINES * 99 / 100))

    LATENCY_P50=$(sort -n "$LATENCY_FILE" | sed -n "${P50_LINE}p" || echo "0")
    LATENCY_P95=$(sort -n "$LATENCY_FILE" | sed -n "${P95_LINE}p" || echo "0")
    LATENCY_P99=$(sort -n "$LATENCY_FILE" | sed -n "${P99_LINE}p" || echo "0")
else
    LATENCY_AVG="0"
    LATENCY_MIN="0"
    LATENCY_MAX="0"
    LATENCY_P50="0"
    LATENCY_P95="0"
    LATENCY_P99="0"
fi

log "========================================"
log "Stress Test Complete"
log "========================================"
log "Total requests: $FINAL_COUNT"
log "Errors:         $FINAL_ERRORS"
log "Duration:       ${ELAPSED}s"
log "Throughput:     ${RPS} req/s"
log ""
log "Latency (ms):"
log "  Min:  ${LATENCY_MIN}ms"
log "  Avg:  ${LATENCY_AVG}ms"
log "  P50:  ${LATENCY_P50}ms"
log "  P95:  ${LATENCY_P95}ms"
log "  P99:  ${LATENCY_P99}ms"
log "  Max:  ${LATENCY_MAX}ms"
log "========================================"

# Interpretation
if [ "${LATENCY_P99:-0}" -gt 1000 ]; then
    warn "P99 latency > 1s indicates significant blocking I/O"
fi

if [ "${LATENCY_MAX:-0}" -gt 5000 ]; then
    warn "Max latency > 5s indicates severe blocking - metrics collection is stalling"
fi

# Write JSON summary
cat > "$STATS_FILE" << EOF
{
    "test_type": "metrics_stress",
    "target": "$METRICS_URL",
    "duration_secs": $ELAPSED,
    "concurrency": $CONCURRENCY,
    "total_requests": $FINAL_COUNT,
    "errors": $FINAL_ERRORS,
    "requests_per_sec": $RPS,
    "latency_ms": {
        "min": ${LATENCY_MIN:-0},
        "avg": ${LATENCY_AVG:-0},
        "p50": ${LATENCY_P50:-0},
        "p95": ${LATENCY_P95:-0},
        "p99": ${LATENCY_P99:-0},
        "max": ${LATENCY_MAX:-0}
    },
    "db_tables_reported": $DB_TABLE_COUNT,
    "timestamp": "$TIMESTAMP",
    "issue_context": {
        "description": "Tests blocking I/O in /metrics endpoint",
        "affected_code": [
            "crates/storage/db/src/implementation/mdbx/mod.rs:265-341 (db.report_metrics)",
            "crates/storage/provider/src/providers/static_file/manager.rs:477-523 (sfp.report_metrics)",
            "crates/node/metrics/src/hooks.rs:44-49 (default hooks)"
        ]
    }
}
EOF

success "Stats written to $STATS_FILE"
success "Latencies written to $LATENCY_FILE"
