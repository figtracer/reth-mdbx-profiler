#!/bin/bash
# Analyze a collected trace
# Usage: ./analyze-trace.sh <trace_file> [format]

set -e

INPUT=${1:-"trace.jsonl"}
FORMAT=${2:-"summary"}

if [ ! -f "$INPUT" ]; then
    echo "Error: Trace file not found: $INPUT"
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ANALYZER="$SCRIPT_DIR/../target/release/app-trace-analyzer"

if [ ! -x "$ANALYZER" ]; then
    echo "Analyzer not built. Building..."
    cd "$SCRIPT_DIR/.."
    cargo build --release --bin app-trace-analyzer
fi

echo "Analyzing: $INPUT"
echo "Format: $FORMAT"
echo

$ANALYZER --input "$INPUT" --format "$FORMAT"
