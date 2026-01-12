#!/usr/bin/env bash
#
# Profile Workload Orchestrator
#
# This script orchestrates profiling sessions by:
# 1. Optionally capturing a baseline profile (node idle)
# 2. Running a stress workload while capturing a profile
# 3. Generating comparison reports
#
# Supports both samply (macOS/Linux) and the mdbx-profiler for different views.
#
# Usage:
#   ./profile_workload.sh [options]
#
# Options:
#   --workload TYPE     Workload type: rpc_state, rpc_mixed, metrics, all (default: metrics)
#   --duration SECS     Duration per test in seconds (default: 30)
#   --baseline          Capture baseline profile first (idle node)
#   --rpc-url URL       RPC endpoint (default: http://localhost:8545)
#   --metrics-url URL   Metrics endpoint (default: http://localhost:9001)
#   --pid PID           Process ID to profile (auto-detects reth if not specified)
#   --output-dir DIR    Output directory (default: ./profiles)
#   --concurrency N     Concurrent workers for stress test (default: 20)
#   --samply            Use samply for CPU profiling
#   --mdbx              Use mdbx-profiler for I/O profiling
#   --mdbx-path PATH    Path to MDBX data file (for mdbx-profiler)
#
# Examples:
#   # Profile metrics endpoint stress with samply
#   ./profile_workload.sh --workload metrics --duration 30 --baseline --samply --pid 12345
#
#   # Profile RPC stress with mdbx-profiler
#   ./profile_workload.sh --workload rpc_state --mdbx --mdbx-path /data/reth/db/mdbx.dat
#
#   # Full profiling session
#   ./profile_workload.sh --workload all --baseline --samply --duration 60

set -euo pipefail

# Defaults
WORKLOAD="metrics"
DURATION=30
BASELINE=false
RPC_URL="http://localhost:8545"
METRICS_URL="http://localhost:9001"
PID=""
OUTPUT_DIR="./profiles"
CONCURRENCY=20
USE_SAMPLY=false
USE_MDBX=false
MDBX_PATH=""

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
MAGENTA='\033[0;35m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --workload)
            WORKLOAD="$2"
            shift 2
            ;;
        --duration)
            DURATION="$2"
            shift 2
            ;;
        --baseline)
            BASELINE=true
            shift
            ;;
        --rpc-url)
            RPC_URL="$2"
            shift 2
            ;;
        --metrics-url)
            METRICS_URL="$2"
            shift 2
            ;;
        --pid)
            PID="$2"
            shift 2
            ;;
        --output-dir)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        --concurrency)
            CONCURRENCY="$2"
            shift 2
            ;;
        --samply)
            USE_SAMPLY=true
            shift
            ;;
        --mdbx)
            USE_MDBX=true
            shift
            ;;
        --mdbx-path)
            MDBX_PATH="$2"
            shift 2
            ;;
        --help|-h)
            head -50 "$0" | grep "^#" | cut -c3-
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

log() {
    echo -e "${BLUE}[$(date +%H:%M:%S)]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

success() {
    echo -e "${GREEN}[OK]${NC} $1"
}

section() {
    echo ""
    echo -e "${MAGENTA}========================================${NC}"
    echo -e "${MAGENTA} $1${NC}"
    echo -e "${MAGENTA}========================================${NC}"
}

# Create output directory
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
SESSION_DIR="$OUTPUT_DIR/session_${TIMESTAMP}"
mkdir -p "$SESSION_DIR"

log "Session directory: $SESSION_DIR"

# Auto-detect reth PID if not specified
if [ -z "$PID" ]; then
    log "Auto-detecting reth process..."
    PID=$(pgrep -x reth 2>/dev/null | head -1 || echo "")
    if [ -z "$PID" ]; then
        error "Could not find reth process. Specify --pid manually."
        exit 1
    fi
    success "Found reth process: PID $PID"
fi

# Verify process exists
if ! kill -0 "$PID" 2>/dev/null; then
    error "Process $PID does not exist"
    exit 1
fi

# Check profiler availability
if $USE_SAMPLY; then
    if ! command -v samply &> /dev/null; then
        error "samply not found. Install with: cargo install samply"
        exit 1
    fi
    success "samply available"
fi

if $USE_MDBX; then
    MDBX_PROFILER="$SCRIPT_DIR/../target/release/mdbx-profiler"
    if [ ! -x "$MDBX_PROFILER" ]; then
        log "Building mdbx-profiler..."
        (cd "$SCRIPT_DIR/.." && cargo build --release)
    fi

    if [ -z "$MDBX_PATH" ]; then
        error "MDBX path required for mdbx-profiler. Use --mdbx-path"
        exit 1
    fi

    if [ ! -f "$MDBX_PATH" ]; then
        error "MDBX file not found: $MDBX_PATH"
        exit 1
    fi
    success "mdbx-profiler available"
fi

# Function to run samply profile
run_samply_profile() {
    local name="$1"
    local duration="$2"
    local output_file="$SESSION_DIR/${name}.json"

    log "Starting samply profile: $name (${duration}s)"

    # samply record attaches to PID and records for duration
    timeout $((duration + 5)) samply record \
        --pid "$PID" \
        --duration "$duration" \
        --save-only \
        --output "$output_file" 2>&1 || {
        # timeout returns 124 on timeout, which is expected
        if [ $? -ne 124 ]; then
            error "samply failed"
            return 1
        fi
    }

    if [ -f "$output_file" ]; then
        success "Profile saved: $output_file"
        return 0
    else
        error "Profile not saved"
        return 1
    fi
}

# Function to run mdbx-profiler
run_mdbx_profile() {
    local name="$1"
    local duration="$2"
    local output_file="$SESSION_DIR/${name}.jsonl"

    log "Starting mdbx-profiler trace: $name (${duration}s)"

    "$MDBX_PROFILER" trace \
        --pid "$PID" \
        --mdbx-path "$MDBX_PATH" \
        --output "$output_file" \
        --duration "${duration}s" \
        --trace-cursors 2>&1 &

    local profiler_pid=$!

    # Wait for duration
    sleep $((duration + 2))

    # Kill if still running
    kill -INT $profiler_pid 2>/dev/null || true
    wait $profiler_pid 2>/dev/null || true

    if [ -f "$output_file" ]; then
        success "Trace saved: $output_file"
        return 0
    else
        error "Trace not saved"
        return 1
    fi
}

# Function to run workload
run_workload() {
    local workload_type="$1"
    local duration="$2"
    local output_prefix="$SESSION_DIR/workload_${workload_type}"

    case "$workload_type" in
        rpc_state)
            log "Running RPC state_unbounded stress test..."
            "$SCRIPT_DIR/rpc_stress.sh" state_unbounded "$duration" "$RPC_URL" "$CONCURRENCY" "$SESSION_DIR" &
            ;;
        rpc_execution)
            log "Running RPC state_execution stress test..."
            "$SCRIPT_DIR/rpc_stress.sh" state_execution "$duration" "$RPC_URL" "$CONCURRENCY" "$SESSION_DIR" &
            ;;
        rpc_trace)
            log "Running RPC debug_trace stress test..."
            "$SCRIPT_DIR/rpc_stress.sh" debug_trace "$duration" "$RPC_URL" "$CONCURRENCY" "$SESSION_DIR" &
            ;;
        rpc_mixed)
            log "Running RPC mixed stress test..."
            "$SCRIPT_DIR/rpc_stress.sh" mixed "$duration" "$RPC_URL" "$CONCURRENCY" "$SESSION_DIR" &
            ;;
        metrics)
            log "Running metrics endpoint stress test..."
            "$SCRIPT_DIR/metrics_stress.sh" "$duration" "$CONCURRENCY" "$METRICS_URL" "$SESSION_DIR" &
            ;;
        *)
            error "Unknown workload: $workload_type"
            return 1
            ;;
    esac

    return 0
}

# ============================================================================
# Main profiling session
# ============================================================================

section "Profiling Session"
log "Workload:    $WORKLOAD"
log "Duration:    ${DURATION}s per test"
log "Baseline:    $BASELINE"
log "Target PID:  $PID"
log "Concurrency: $CONCURRENCY"
log "Profilers:   $([ $USE_SAMPLY = true ] && echo 'samply')$([ $USE_MDBX = true ] && echo ' mdbx-profiler')"

# Validate workload scripts exist
for script in rpc_stress.sh metrics_stress.sh; do
    if [ ! -x "$SCRIPT_DIR/$script" ]; then
        chmod +x "$SCRIPT_DIR/$script" 2>/dev/null || {
            error "Script not executable: $SCRIPT_DIR/$script"
            exit 1
        }
    fi
done

# ============================================================================
# Phase 1: Baseline profile (optional)
# ============================================================================

if $BASELINE; then
    section "Phase 1: Baseline Profile (Idle Node)"
    log "Capturing baseline with no workload for ${DURATION}s..."

    if $USE_SAMPLY; then
        run_samply_profile "baseline_samply" "$DURATION" &
        SAMPLY_PID=$!
    fi

    if $USE_MDBX; then
        run_mdbx_profile "baseline_mdbx" "$DURATION" &
        MDBX_PID=$!
    fi

    # Wait for profilers
    if $USE_SAMPLY; then wait $SAMPLY_PID 2>/dev/null || true; fi
    if $USE_MDBX; then wait $MDBX_PID 2>/dev/null || true; fi

    success "Baseline profiles captured"
    sleep 2
fi

# ============================================================================
# Phase 2: Stressed profiles
# ============================================================================

run_stressed_profile() {
    local workload_name="$1"

    section "Stressed Profile: $workload_name"

    # Start profilers
    if $USE_SAMPLY; then
        run_samply_profile "stressed_${workload_name}_samply" "$DURATION" &
        SAMPLY_PID=$!
    fi

    if $USE_MDBX; then
        run_mdbx_profile "stressed_${workload_name}_mdbx" "$DURATION" &
        MDBX_PID=$!
    fi

    # Give profilers a moment to attach
    sleep 1

    # Start workload
    run_workload "$workload_name" "$DURATION"
    WORKLOAD_PID=$!

    # Wait for everything
    wait $WORKLOAD_PID 2>/dev/null || true
    if $USE_SAMPLY; then wait $SAMPLY_PID 2>/dev/null || true; fi
    if $USE_MDBX; then wait $MDBX_PID 2>/dev/null || true; fi

    success "Stressed profile complete: $workload_name"
    sleep 2
}

case "$WORKLOAD" in
    metrics)
        run_stressed_profile "metrics"
        ;;
    rpc_state)
        run_stressed_profile "rpc_state"
        ;;
    rpc_execution)
        run_stressed_profile "rpc_execution"
        ;;
    rpc_trace)
        run_stressed_profile "rpc_trace"
        ;;
    rpc_mixed)
        run_stressed_profile "rpc_mixed"
        ;;
    all)
        run_stressed_profile "metrics"
        run_stressed_profile "rpc_state"
        run_stressed_profile "rpc_mixed"
        ;;
    *)
        error "Unknown workload: $WORKLOAD"
        exit 1
        ;;
esac

# ============================================================================
# Phase 3: Generate summary
# ============================================================================

section "Session Summary"

# List all generated files
log "Generated files:"
find "$SESSION_DIR" -type f | sort | while read -r file; do
    size=$(ls -lh "$file" | awk '{print $5}')
    echo "  $file ($size)"
done

# Generate session manifest
cat > "$SESSION_DIR/manifest.json" << EOF
{
    "session_id": "$TIMESTAMP",
    "target_pid": $PID,
    "workload": "$WORKLOAD",
    "duration_per_test_secs": $DURATION,
    "concurrency": $CONCURRENCY,
    "baseline_captured": $BASELINE,
    "profilers": {
        "samply": $USE_SAMPLY,
        "mdbx": $USE_MDBX
    },
    "endpoints": {
        "rpc": "$RPC_URL",
        "metrics": "$METRICS_URL"
    },
    "mdbx_path": "$MDBX_PATH",
    "files": $(find "$SESSION_DIR" -type f -name "*.json" -o -name "*.jsonl" | jq -R -s -c 'split("\n") | map(select(length > 0))')
}
EOF

success "Session manifest: $SESSION_DIR/manifest.json"

# Instructions for viewing
echo ""
log "Next steps:"
if $USE_SAMPLY; then
    echo "  View samply profiles:"
    find "$SESSION_DIR" -name "*_samply.json" | while read -r f; do
        echo "    samply load '$f'"
    done
fi

if $USE_MDBX; then
    echo "  Analyze mdbx traces:"
    find "$SESSION_DIR" -name "*_mdbx.jsonl" | while read -r f; do
        echo "    mdbx-profiler analyze --input '$f' --format summary"
    done
fi

echo ""
echo "  Generate comparison HTML:"
echo "    ./compare_profiles.sh $SESSION_DIR"

success "Profiling session complete!"
