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
# generate html reports and compact exports
# ============================================================================
echo "generating reports..."

for trace in baseline rpc_stress metrics_stress; do
    if [ -f "$SESSION_DIR/${trace}.jsonl" ]; then
        echo "  analyzing ${trace}..."
        # generate individual html
        "$ANALYZER" --input "$SESSION_DIR/${trace}.jsonl" --mdbx-path "$MDBX_PATH" \
            --output "$SESSION_DIR/${trace}.html" 2>&1 || echo "    html generation failed"
        # generate compact json for comparison
        "$ANALYZER" --input "$SESSION_DIR/${trace}.jsonl" --mdbx-path "$MDBX_PATH" \
            --format compact --label "$trace" 2>/dev/null > "$SESSION_DIR/${trace}.json"
        if [ -s "$SESSION_DIR/${trace}.json" ]; then
            echo "    ${trace}.json created ($(wc -c < "$SESSION_DIR/${trace}.json") bytes)"
        else
            echo "    ${trace}.json is empty or failed"
        fi
    else
        echo "  ${trace}.jsonl not found, skipping"
    fi
done

# ============================================================================
# generate comparison html
# ============================================================================
echo "generating comparison report..."

# collect all compact jsons into array file (safer than shell variable)
COMPARISON_JSON="$SESSION_DIR/comparison_data.json"
echo "[" > "$COMPARISON_JSON"
first=true
for trace in baseline rpc_stress metrics_stress; do
    json="$SESSION_DIR/${trace}.json"
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

# read comparison data
COMPARISON_DATA=$(cat "$COMPARISON_JSON")

# generate comparison html
cat > "$SESSION_DIR/comparison.html" << 'HTMLEOF'
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>MDBX Profile Comparison</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            background: #000;
            color: #e4e4e7;
            padding: 24px;
            line-height: 1.5;
        }
        .container { max-width: 1400px; margin: 0 auto; }
        h1 { color: #3b82f6; margin-bottom: 8px; }
        .subtitle { color: #71717a; margin-bottom: 24px; }

        .comparison-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(350px, 1fr));
            gap: 16px;
            margin-bottom: 24px;
        }

        .profile-card {
            background: #12121a;
            border-radius: 10px;
            border: 1px solid #1e1e2a;
            overflow: hidden;
        }
        .profile-card.baseline { border-top: 3px solid #22c55e; }
        .profile-card.rpc_stress { border-top: 3px solid #a855f7; }
        .profile-card.metrics_stress { border-top: 3px solid #f87171; }

        .card-header {
            padding: 16px;
            background: #0a0a0f;
            border-bottom: 1px solid #1e1e2a;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }
        .card-title { font-size: 16px; font-weight: 600; }
        .card-badge {
            font-size: 11px;
            padding: 4px 8px;
            border-radius: 4px;
            font-weight: 500;
        }
        .badge-baseline { background: #22c55e20; color: #22c55e; }
        .badge-rpc { background: #a855f720; color: #a855f7; }
        .badge-metrics { background: #f8717120; color: #f87171; }

        .card-body { padding: 16px; }

        .metric-grid {
            display: grid;
            grid-template-columns: repeat(2, 1fr);
            gap: 12px;
        }
        .metric {
            background: #0a0a0f;
            padding: 12px;
            border-radius: 6px;
        }
        .metric-label {
            font-size: 11px;
            color: #71717a;
            text-transform: uppercase;
            margin-bottom: 4px;
        }
        .metric-value {
            font-size: 20px;
            font-weight: 700;
            color: #3b82f6;
        }
        .metric-value.major { color: #f87171; }
        .metric-value.good { color: #22c55e; }

        .delta {
            font-size: 12px;
            margin-left: 8px;
            padding: 2px 6px;
            border-radius: 4px;
        }
        .delta.up { background: #f8717120; color: #f87171; }
        .delta.down { background: #22c55e20; color: #22c55e; }

        .section { margin-bottom: 24px; }
        .section-title {
            font-size: 14px;
            color: #3b82f6;
            text-transform: uppercase;
            margin-bottom: 12px;
            padding-bottom: 8px;
            border-bottom: 1px solid #1e1e2a;
        }

        .table-comparison {
            width: 100%;
            border-collapse: collapse;
            font-size: 13px;
        }
        .table-comparison th,
        .table-comparison td {
            padding: 10px 12px;
            text-align: left;
            border-bottom: 1px solid #1e1e2a;
        }
        .table-comparison th {
            background: #0a0a0f;
            color: #71717a;
            font-weight: 500;
            text-transform: uppercase;
            font-size: 11px;
        }
        .table-comparison td {
            font-variant-numeric: tabular-nums;
        }
        .table-comparison tr:hover { background: #1a1a24; }

        .bar-container {
            display: flex;
            align-items: center;
            gap: 8px;
        }
        .bar {
            height: 8px;
            border-radius: 4px;
            transition: width 0.3s;
        }
        .bar.baseline { background: #22c55e; }
        .bar.rpc { background: #a855f7; }
        .bar.metrics { background: #f87171; }
    </style>
</head>
<body>
    <div class="container">
        <h1>MDBX Profile Comparison</h1>
        <p class="subtitle">baseline vs stressed workload analysis</p>

        <div id="comparison-content"></div>
    </div>

    <script>
HTMLEOF

# inject the data
echo "const DATA = $COMPARISON_DATA;" >> "$SESSION_DIR/comparison.html"

cat >> "$SESSION_DIR/comparison.html" << 'HTMLEOF2'

        const fmt = n => {
            if (n === null || n === undefined) return '-';
            if (n >= 1e6) return (n/1e6).toFixed(1) + 'M';
            if (n >= 1e3) return (n/1e3).toFixed(1) + 'K';
            return n.toFixed ? n.toFixed(0) : n;
        };

        const fmtPct = n => (n * 100).toFixed(1) + '%';
        const fmtDur = s => s < 60 ? s.toFixed(1) + 's' : (s/60).toFixed(1) + 'm';

        function getDelta(current, baseline) {
            if (!baseline || baseline === 0) return null;
            const pct = ((current - baseline) / baseline * 100);
            return pct;
        }

        function deltaHtml(current, baseline, inverse = false) {
            const delta = getDelta(current, baseline);
            if (delta === null) return '';
            const isUp = delta > 0;
            const cls = inverse ? (isUp ? 'down' : 'up') : (isUp ? 'up' : 'down');
            const sign = isUp ? '+' : '';
            return `<span class="delta ${cls}">${sign}${delta.toFixed(0)}%</span>`;
        }

        function getBadgeClass(label) {
            if (label === 'baseline') return 'badge-baseline';
            if (label === 'rpc_stress') return 'badge-rpc';
            if (label === 'metrics_stress') return 'badge-metrics';
            return '';
        }

        // Check if data loaded
        if (!DATA || !Array.isArray(DATA) || DATA.length === 0) {
            document.getElementById('comparison-content').innerHTML =
                '<p style="color: #f87171; margin-bottom: 16px;">No profile data found. Check that the traces were captured and analyzed correctly.</p>' +
                '<p style="color: #71717a;">Make sure the .json files were generated in the session directory.</p>';
        } else {

        // Find baseline for delta calculations
        const baseline = DATA.find(d => d && d.label === 'baseline');

        // Generate comparison cards
        let html = '<div class="comparison-grid">';

        for (const profile of DATA) {
            const pf = profile.page_faults;
            const base = baseline?.page_faults;
            const isBaseline = profile.label === 'baseline';

            html += `
            <div class="profile-card ${profile.label}">
                <div class="card-header">
                    <span class="card-title">${profile.label.replace('_', ' ')}</span>
                    <span class="card-badge ${getBadgeClass(profile.label)}">${fmtDur(profile.trace.duration_secs)}</span>
                </div>
                <div class="card-body">
                    <div class="metric-grid">
                        <div class="metric">
                            <div class="metric-label">Page Faults</div>
                            <div class="metric-value">${fmt(pf.total)}${isBaseline ? '' : deltaHtml(pf.total, base?.total)}</div>
                        </div>
                        <div class="metric">
                            <div class="metric-label">Major (Disk)</div>
                            <div class="metric-value major">${fmt(pf.major)}${isBaseline ? '' : deltaHtml(pf.major, base?.major)}</div>
                        </div>
                        <div class="metric">
                            <div class="metric-label">Fault Rate</div>
                            <div class="metric-value">${fmt(pf.rate_per_sec)}/s${isBaseline ? '' : deltaHtml(pf.rate_per_sec, base?.rate_per_sec)}</div>
                        </div>
                        <div class="metric">
                            <div class="metric-label">Major Ratio</div>
                            <div class="metric-value ${pf.major_ratio > 0.5 ? 'major' : ''}">${fmtPct(pf.major_ratio)}</div>
                        </div>
                        <div class="metric">
                            <div class="metric-label">Sequential</div>
                            <div class="metric-value good">${fmtPct(pf.sequential_ratio)}</div>
                        </div>
                        <div class="metric">
                            <div class="metric-label">Unique Pages</div>
                            <div class="metric-value">${fmt(pf.unique_pages)}</div>
                        </div>
                    </div>
                </div>
            </div>`;
        }
        html += '</div>';

        // Table comparison
        if (DATA.length > 0 && DATA[0].tables) {
            html += `
            <div class="section">
                <div class="section-title">Top Tables by Faults</div>
                <table class="table-comparison">
                    <thead>
                        <tr>
                            <th>Table</th>`;

            for (const profile of DATA) {
                html += `<th>${profile.label.replace('_', ' ')}</th>`;
            }
            html += '</tr></thead><tbody>';

            // Collect all table names
            const allTables = new Set();
            for (const profile of DATA) {
                for (const t of profile.tables || []) {
                    allTables.add(t.name);
                }
            }

            // Find max for bar scaling
            let maxFaults = 0;
            for (const profile of DATA) {
                for (const t of profile.tables || []) {
                    if (t.faults > maxFaults) maxFaults = t.faults;
                }
            }

            // Sort tables by total faults across all profiles
            const tableOrder = [...allTables].map(name => {
                let total = 0;
                for (const profile of DATA) {
                    const t = (profile.tables || []).find(x => x.name === name);
                    if (t) total += t.faults;
                }
                return { name, total };
            }).sort((a, b) => b.total - a.total).slice(0, 15);

            for (const { name } of tableOrder) {
                html += `<tr><td>${name}</td>`;
                for (const profile of DATA) {
                    const t = (profile.tables || []).find(x => x.name === name);
                    const faults = t ? t.faults : 0;
                    const major = t ? t.major_faults : 0;
                    const barWidth = maxFaults > 0 ? (faults / maxFaults * 100) : 0;
                    const barClass = profile.label === 'baseline' ? 'baseline' :
                                    profile.label === 'rpc_stress' ? 'rpc' : 'metrics';
                    html += `<td>
                        <div class="bar-container">
                            <div class="bar ${barClass}" style="width: ${barWidth}%; min-width: ${faults > 0 ? 4 : 0}px;"></div>
                            <span>${fmt(faults)} (${fmt(major)} major)</span>
                        </div>
                    </td>`;
                }
                html += '</tr>';
            }

            html += '</tbody></table></div>';
        }

        document.getElementById('comparison-content').innerHTML = html;
        } // end else (data exists)
    </script>
</body>
</html>
HTMLEOF2

echo "      comparison.html generated"

# ============================================================================
# summary
# ============================================================================
echo ""
echo "========================================"
echo "complete"
echo "========================================"
echo ""
echo "individual reports:"
for html in "$SESSION_DIR"/baseline.html "$SESSION_DIR"/rpc_stress.html "$SESSION_DIR"/metrics_stress.html; do
    if [ -f "$html" ]; then
        echo "  file://$html"
    fi
done
echo ""
echo "comparison report:"
echo "  file://$SESSION_DIR/comparison.html"
