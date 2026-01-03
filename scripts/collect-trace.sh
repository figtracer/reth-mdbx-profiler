#!/bin/bash
# Collect a trace from a running application node
# Usage: ./collect-trace.sh [duration_seconds] [output_file]

set -e

DURATION=${1:-30}
OUTPUT=${2:-"trace-$(date +%Y%m%d-%H%M%S).jsonl"}

echo "=== application MDBX Trace Collector ==="
echo

# Check root
if [ "$EUID" -ne 0 ]; then
    echo "Please run as root (needed for eBPF)"
    exit 1
fi

# Find application (or blob-exex which is a application-based ExEx)
RETH_PID=$(pgrep -x app || pgrep -f "app node" || pgrep -x blob-exex || pgrep -f "blob-exex node" || echo "")
if [ -z "$RETH_PID" ]; then
    echo "Error: application process not found"
    echo "Looking for: app, blob-exex, or similar..."
    ps aux | grep -E "app|exex" | grep -v grep || true
    exit 1
fi
echo "application PID: $RETH_PID"

# Find MDBX path
MDBX_PATH=$(grep -oE "/[^ ]+mdbx\.dat" /proc/$RETH_PID/maps 2>/dev/null | head -1 || echo "")
if [ -z "$MDBX_PATH" ]; then
    echo "Error: MDBX file not found in process maps"
    echo "Trying common paths..."

    for path in /home/ubuntu/app_data/db/mdbx.dat /data/app/db/mdbx.dat /var/lib/app/db/mdbx.dat ~/.local/share/app/mainnet/db/mdbx.dat; do
        if [ -f "$path" ]; then
            MDBX_PATH="$path"
            break
        fi
    done
fi

if [ -z "$MDBX_PATH" ]; then
    echo "Error: Could not find MDBX data file"
    exit 1
fi
echo "MDBX path: $MDBX_PATH"

# Get binary path
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROFILER="$SCRIPT_DIR/../target/release/mdbx-profiler"

if [ ! -x "$PROFILER" ]; then
    echo "Profiler not built. Building..."
    cd "$SCRIPT_DIR/.."
    cargo build --release
fi

echo
echo "Starting trace for ${DURATION}s..."
echo "Output: $OUTPUT"
echo

$PROFILER trace \
    --pid "$RETH_PID" \
    --mdbx-path "$MDBX_PATH" \
    --output "$OUTPUT" \
    --duration "${DURATION}s" \
    --stats-interval 5

echo
echo "Trace complete: $OUTPUT"
echo "File size: $(ls -lh "$OUTPUT" | awk '{print $5}')"
echo
echo "To analyze: ./scripts/analyze-trace.sh $OUTPUT"
