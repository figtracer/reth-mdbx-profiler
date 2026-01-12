#!/usr/bin/env bash
#
# Profile Comparison Report Generator
#
# Generates an HTML comparison report from a profiling session.
# Compares baseline vs stressed workload metrics and profiles.
#
# Usage:
#   ./compare_profiles.sh SESSION_DIR [OUTPUT_FILE]
#
# Example:
#   ./compare_profiles.sh ./profiles/session_20240115_143022

set -euo pipefail

SESSION_DIR="${1:-.}"
OUTPUT_FILE="${2:-$SESSION_DIR/comparison_report.html}"

if [ ! -d "$SESSION_DIR" ]; then
    echo "Error: Session directory not found: $SESSION_DIR"
    exit 1
fi

# Collect all stats files
STATS_FILES=$(find "$SESSION_DIR" -name "*_stats.json" -type f 2>/dev/null | sort)

if [ -z "$STATS_FILES" ]; then
    echo "Error: No stats files found in $SESSION_DIR"
    exit 1
fi

echo "Found stats files:"
echo "$STATS_FILES" | while read -r f; do echo "  $f"; done

# Read manifest if available
MANIFEST_FILE="$SESSION_DIR/manifest.json"
if [ -f "$MANIFEST_FILE" ]; then
    SESSION_ID=$(jq -r '.session_id // "unknown"' "$MANIFEST_FILE")
    TARGET_PID=$(jq -r '.target_pid // "unknown"' "$MANIFEST_FILE")
    WORKLOAD=$(jq -r '.workload // "unknown"' "$MANIFEST_FILE")
else
    SESSION_ID=$(basename "$SESSION_DIR")
    TARGET_PID="unknown"
    WORKLOAD="unknown"
fi

# Generate HTML
cat > "$OUTPUT_FILE" << 'HTMLHEAD'
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Profile Comparison Report</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            background: #0a0a0f;
            color: #e4e4e7;
            line-height: 1.6;
            padding: 24px;
        }
        .container { max-width: 1200px; margin: 0 auto; }
        h1 {
            color: #3b82f6;
            margin-bottom: 8px;
            font-size: 28px;
        }
        .subtitle {
            color: #71717a;
            margin-bottom: 24px;
        }
        .meta {
            display: flex;
            gap: 24px;
            margin-bottom: 32px;
            padding: 16px;
            background: #12121a;
            border-radius: 8px;
        }
        .meta-item {
            display: flex;
            flex-direction: column;
        }
        .meta-label {
            font-size: 11px;
            color: #71717a;
            text-transform: uppercase;
        }
        .meta-value {
            font-size: 16px;
            color: #a1a1aa;
            font-weight: 500;
        }
        .section {
            margin-bottom: 32px;
        }
        .section-title {
            font-size: 18px;
            color: #3b82f6;
            margin-bottom: 16px;
            padding-bottom: 8px;
            border-bottom: 1px solid #1e1e2a;
        }
        .card-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(350px, 1fr));
            gap: 16px;
        }
        .card {
            background: #12121a;
            border-radius: 10px;
            border: 1px solid #1e1e2a;
            padding: 20px;
        }
        .card-header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 16px;
        }
        .card-title {
            font-size: 14px;
            font-weight: 600;
            color: #e4e4e7;
        }
        .card-badge {
            font-size: 11px;
            padding: 4px 8px;
            border-radius: 4px;
            font-weight: 500;
        }
        .badge-baseline {
            background: #22c55e20;
            color: #22c55e;
        }
        .badge-stressed {
            background: #f8717120;
            color: #f87171;
        }
        .badge-metrics {
            background: #3b82f620;
            color: #3b82f6;
        }
        .badge-rpc {
            background: #a855f720;
            color: #a855f7;
        }
        .stats-grid {
            display: grid;
            grid-template-columns: repeat(2, 1fr);
            gap: 12px;
        }
        .stat {
            padding: 12px;
            background: #0a0a0f;
            border-radius: 6px;
        }
        .stat-label {
            font-size: 11px;
            color: #71717a;
            text-transform: uppercase;
            margin-bottom: 4px;
        }
        .stat-value {
            font-size: 20px;
            font-weight: 700;
            color: #3b82f6;
        }
        .stat-value.error { color: #f87171; }
        .stat-value.success { color: #22c55e; }
        .stat-value.warning { color: #fbbf24; }
        .latency-bars {
            margin-top: 12px;
        }
        .latency-bar {
            display: flex;
            align-items: center;
            margin-bottom: 8px;
        }
        .latency-label {
            width: 40px;
            font-size: 12px;
            color: #71717a;
        }
        .latency-track {
            flex: 1;
            height: 8px;
            background: #1e1e2a;
            border-radius: 4px;
            overflow: hidden;
            margin: 0 12px;
        }
        .latency-fill {
            height: 100%;
            border-radius: 4px;
            transition: width 0.3s;
        }
        .latency-fill.p50 { background: #22c55e; }
        .latency-fill.p95 { background: #fbbf24; }
        .latency-fill.p99 { background: #f87171; }
        .latency-fill.max { background: #dc2626; }
        .latency-value {
            width: 70px;
            text-align: right;
            font-size: 12px;
            color: #a1a1aa;
            font-variant-numeric: tabular-nums;
        }
        .comparison-table {
            width: 100%;
            border-collapse: collapse;
            margin-top: 16px;
        }
        .comparison-table th,
        .comparison-table td {
            padding: 12px 16px;
            text-align: left;
            border-bottom: 1px solid #1e1e2a;
        }
        .comparison-table th {
            font-size: 12px;
            color: #71717a;
            text-transform: uppercase;
            font-weight: 500;
        }
        .comparison-table td {
            font-variant-numeric: tabular-nums;
        }
        .delta {
            font-size: 12px;
            padding: 2px 6px;
            border-radius: 4px;
            margin-left: 8px;
        }
        .delta.positive {
            background: #22c55e20;
            color: #22c55e;
        }
        .delta.negative {
            background: #f8717120;
            color: #f87171;
        }
        .issue-context {
            margin-top: 24px;
            padding: 16px;
            background: #1e1e2a;
            border-radius: 8px;
            border-left: 4px solid #f87171;
        }
        .issue-context h3 {
            color: #f87171;
            font-size: 14px;
            margin-bottom: 12px;
        }
        .issue-context ul {
            margin-left: 20px;
            color: #a1a1aa;
            font-size: 13px;
        }
        .issue-context li {
            margin-bottom: 4px;
        }
        .issue-context code {
            background: #0a0a0f;
            padding: 2px 6px;
            border-radius: 4px;
            font-family: monospace;
            font-size: 12px;
        }
        .profile-links {
            margin-top: 16px;
        }
        .profile-link {
            display: inline-block;
            padding: 8px 16px;
            background: #3b82f6;
            color: white;
            text-decoration: none;
            border-radius: 6px;
            font-size: 13px;
            margin-right: 8px;
            margin-bottom: 8px;
        }
        .profile-link:hover {
            background: #2563eb;
        }
        .profile-link.secondary {
            background: #1e1e2a;
            color: #a1a1aa;
        }
    </style>
</head>
<body>
    <div class="container">
        <h1>Profile Comparison Report</h1>
        <p class="subtitle">Stress test analysis for blocking I/O investigation</p>

        <div class="meta">
HTMLHEAD

# Add meta information
cat >> "$OUTPUT_FILE" << EOF
            <div class="meta-item">
                <span class="meta-label">Session ID</span>
                <span class="meta-value">$SESSION_ID</span>
            </div>
            <div class="meta-item">
                <span class="meta-label">Target PID</span>
                <span class="meta-value">$TARGET_PID</span>
            </div>
            <div class="meta-item">
                <span class="meta-label">Workload</span>
                <span class="meta-value">$WORKLOAD</span>
            </div>
            <div class="meta-item">
                <span class="meta-label">Generated</span>
                <span class="meta-value">$(date '+%Y-%m-%d %H:%M:%S')</span>
            </div>
        </div>
EOF

# Add workload results section
cat >> "$OUTPUT_FILE" << 'EOF'
        <div class="section">
            <h2 class="section-title">Workload Results</h2>
            <div class="card-grid">
EOF

# Process each stats file and generate cards
echo "$STATS_FILES" | while read -r stats_file; do
    if [ -z "$stats_file" ]; then continue; fi

    filename=$(basename "$stats_file")

    # Determine test type from filename
    if echo "$filename" | grep -q "metrics"; then
        test_type="Metrics Endpoint"
        badge_class="badge-metrics"
    elif echo "$filename" | grep -q "rpc"; then
        test_type="RPC Stress"
        badge_class="badge-rpc"
    else
        test_type="Unknown"
        badge_class="badge-stressed"
    fi

    # Extract stats from JSON
    if [ -f "$stats_file" ]; then
        total_requests=$(jq -r '.total_requests // 0' "$stats_file")
        errors=$(jq -r '.errors // 0' "$stats_file")
        duration=$(jq -r '.duration_secs // 0' "$stats_file")
        rps=$(jq -r '.requests_per_sec // 0' "$stats_file")
        concurrency=$(jq -r '.concurrency // "N/A"' "$stats_file")

        # Latency stats (if available)
        latency_min=$(jq -r '.latency_ms.min // "N/A"' "$stats_file")
        latency_avg=$(jq -r '.latency_ms.avg // "N/A"' "$stats_file")
        latency_p50=$(jq -r '.latency_ms.p50 // "N/A"' "$stats_file")
        latency_p95=$(jq -r '.latency_ms.p95 // "N/A"' "$stats_file")
        latency_p99=$(jq -r '.latency_ms.p99 // "N/A"' "$stats_file")
        latency_max=$(jq -r '.latency_ms.max // "N/A"' "$stats_file")

        # Category (for RPC tests)
        category=$(jq -r '.category // ""' "$stats_file")
        if [ -n "$category" ] && [ "$category" != "null" ]; then
            test_type="RPC: $category"
        fi

        # Calculate error rate
        if [ "$total_requests" -gt 0 ]; then
            error_rate=$(echo "scale=1; $errors * 100 / $total_requests" | bc 2>/dev/null || echo "0")
        else
            error_rate="0"
        fi

        # Generate card HTML
        cat >> "$OUTPUT_FILE" << CARD_EOF
                <div class="card">
                    <div class="card-header">
                        <span class="card-title">$test_type</span>
                        <span class="card-badge $badge_class">$filename</span>
                    </div>
                    <div class="stats-grid">
                        <div class="stat">
                            <div class="stat-label">Total Requests</div>
                            <div class="stat-value">$total_requests</div>
                        </div>
                        <div class="stat">
                            <div class="stat-label">Throughput</div>
                            <div class="stat-value">$rps/s</div>
                        </div>
                        <div class="stat">
                            <div class="stat-label">Errors</div>
                            <div class="stat-value$([ "$errors" -gt 0 ] && echo ' error')">$errors ($error_rate%)</div>
                        </div>
                        <div class="stat">
                            <div class="stat-label">Duration</div>
                            <div class="stat-value">${duration}s</div>
                        </div>
                    </div>
CARD_EOF

        # Add latency bars if metrics test
        if [ "$latency_p50" != "N/A" ] && [ "$latency_p50" != "null" ]; then
            # Calculate bar widths (normalize to max)
            max_val="$latency_max"
            if [ "$max_val" = "0" ] || [ "$max_val" = "N/A" ]; then max_val=1; fi

            p50_width=$(echo "scale=0; $latency_p50 * 100 / $max_val" | bc 2>/dev/null || echo "50")
            p95_width=$(echo "scale=0; $latency_p95 * 100 / $max_val" | bc 2>/dev/null || echo "75")
            p99_width=$(echo "scale=0; $latency_p99 * 100 / $max_val" | bc 2>/dev/null || echo "90")

            cat >> "$OUTPUT_FILE" << LATENCY_EOF
                    <div class="latency-bars">
                        <div class="latency-bar">
                            <span class="latency-label">P50</span>
                            <div class="latency-track">
                                <div class="latency-fill p50" style="width: ${p50_width}%"></div>
                            </div>
                            <span class="latency-value">${latency_p50}ms</span>
                        </div>
                        <div class="latency-bar">
                            <span class="latency-label">P95</span>
                            <div class="latency-track">
                                <div class="latency-fill p95" style="width: ${p95_width}%"></div>
                            </div>
                            <span class="latency-value">${latency_p95}ms</span>
                        </div>
                        <div class="latency-bar">
                            <span class="latency-label">P99</span>
                            <div class="latency-track">
                                <div class="latency-fill p99" style="width: ${p99_width}%"></div>
                            </div>
                            <span class="latency-value">${latency_p99}ms</span>
                        </div>
                        <div class="latency-bar">
                            <span class="latency-label">Max</span>
                            <div class="latency-track">
                                <div class="latency-fill max" style="width: 100%"></div>
                            </div>
                            <span class="latency-value">${latency_max}ms</span>
                        </div>
                    </div>
LATENCY_EOF
        fi

        echo "                </div>" >> "$OUTPUT_FILE"
    fi
done

# Close card grid and section
cat >> "$OUTPUT_FILE" << 'EOF'
            </div>
        </div>
EOF

# Add issue context section (for metrics tests)
cat >> "$OUTPUT_FILE" << 'EOF'
        <div class="section">
            <div class="issue-context">
                <h3>Blocking I/O Context</h3>
                <p style="margin-bottom: 12px; color: #a1a1aa;">
                    The metrics endpoint triggers synchronous blocking I/O on every request:
                </p>
                <ul>
                    <li><code>db.report_metrics()</code> - Opens MDBX read txn, calls db_stat() on ALL tables</li>
                    <li><code>sfp.report_metrics()</code> - Enumerates static files, calls metadata() on each</li>
                    <li><code>collect_memory_stats()</code> - jemalloc epoch advance</li>
                </ul>
                <p style="margin-top: 12px; color: #71717a; font-size: 12px;">
                    High P99/Max latencies indicate contention with the node's main workload.
                    See: crates/storage/db/src/implementation/mdbx/mod.rs:265-341
                </p>
            </div>
        </div>
EOF

# Add profile links if samply files exist
SAMPLY_FILES=$(find "$SESSION_DIR" -name "*_samply.json" -type f 2>/dev/null | sort)
if [ -n "$SAMPLY_FILES" ]; then
    cat >> "$OUTPUT_FILE" << 'EOF'
        <div class="section">
            <h2 class="section-title">CPU Profiles (samply)</h2>
            <p style="color: #71717a; margin-bottom: 16px;">
                Open these profiles with: <code>samply load &lt;file&gt;</code>
            </p>
            <div class="profile-links">
EOF

    echo "$SAMPLY_FILES" | while read -r f; do
        fname=$(basename "$f")
        echo "                <a href=\"$fname\" class=\"profile-link secondary\">$fname</a>" >> "$OUTPUT_FILE"
    done

    cat >> "$OUTPUT_FILE" << 'EOF'
            </div>
        </div>
EOF
fi

# Close HTML
cat >> "$OUTPUT_FILE" << 'EOF'
    </div>
</body>
</html>
EOF

echo ""
echo "Comparison report generated: $OUTPUT_FILE"
echo "Open in browser: file://$OUTPUT_FILE"
