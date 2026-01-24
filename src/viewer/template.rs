//! HTML template generation for the web viewer

use super::ViewerData;

/// Generate the complete HTML file with embedded data and JavaScript
pub fn generate_html(data: &ViewerData) -> String {
    let data_json = serde_json::to_string(data).unwrap_or_else(|_| "{}".to_string());

    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>MDBX Trace</title>
    <!-- uPlot for interactive charts -->
    <link rel="stylesheet" href="https://unpkg.com/uplot@1.6.30/dist/uPlot.min.css">
    <script src="https://unpkg.com/uplot@1.6.30/dist/uPlot.iife.min.js"></script>
    <!-- Plotly for heatmap -->
    <script src="https://cdn.plot.ly/plotly-2.35.2.min.js" charset="utf-8"></script>
    <style>
{css}
    </style>
</head>
<body>
    <div id="app">
        <nav class="tabs">
            <button class="tab active" data-tab="overview">Overview</button>
            <button class="tab" data-tab="tables">Tables</button>
            <button class="tab" data-tab="resources">Resources</button>
            <button class="export-btn" id="export-compact-btn" title="Download JSON for analysis">Export</button>
        </nav>

        <main>
            <!-- OVERVIEW TAB -->
            <section id="overview" class="panel active">
                <!-- Top metrics row -->
                <div class="metrics-row">
                    <div class="metric">
                        <span class="metric-value" id="duration"></span>
                        <span class="metric-label">Duration</span>
                    </div>
                    <div class="metric" id="block-range-metric" style="display:none;">
                        <span class="metric-value" id="block-range"></span>
                        <span class="metric-label">Blocks</span>
                    </div>
                    <div class="metric">
                        <span class="metric-value" id="total-faults"></span>
                        <span class="metric-label">Page Faults</span>
                    </div>
                    <div class="metric">
                        <span class="metric-value major" id="major-faults"></span>
                        <span class="metric-label">Major (Disk)</span>
                    </div>
                    <div class="metric">
                        <span class="metric-value minor" id="minor-faults"></span>
                        <span class="metric-label">Minor (Cache)</span>
                    </div>
                    <div class="metric">
                        <span class="metric-value" id="fault-rate"></span>
                        <span class="metric-label">Faults/sec</span>
                    </div>
                    <div class="metric">
                        <span class="metric-value" id="major-ratio"></span>
                        <span class="metric-label">Major Ratio</span>
                    </div>
                </div>

                <!-- Access Heatmap (full width, larger) - on top for correlation with timeline -->
                <div class="card" style="margin-bottom: 16px;">
                    <div class="card-header">Access Heatmap <span class="axis-hint">(drag to zoom, dbl-click to reset)</span></div>
                    <div class="card-body heatmap-container" style="position: relative;">
                        <div id="heatmap-plotly" style="width: 100%; height: 100%;"></div>
                    </div>
                </div>

                <!-- Fault Timeline (full width) -->
                <div class="card" style="margin-bottom: 16px;">
                    <div class="card-header">Fault Timeline <span class="axis-hint">(drag to zoom, dbl-click to reset)</span></div>
                    <div class="card-body uplot-container" id="timeline-chart">
                    </div>
                </div>

                <!-- Access Patterns -->
                <div class="two-col">
                    <div class="card">
                        <div class="card-header">Access Pattern</div>
                        <div class="card-body">
                            <div class="pattern-section">
                                <div class="pattern-bar">
                                    <span class="pattern-label">Sequential</span>
                                    <div class="bar-container">
                                        <div class="bar sequential" id="seq-bar"></div>
                                    </div>
                                    <span class="pattern-value" id="seq-ratio"></span>
                                </div>
                                <div class="pattern-bar">
                                    <span class="pattern-label">Random</span>
                                    <div class="bar-container">
                                        <div class="bar random" id="rand-bar"></div>
                                    </div>
                                    <span class="pattern-value" id="rand-ratio"></span>
                                </div>
                            </div>
                            <div class="stride-section" id="stride-section" style="margin-top: 16px;">
                                <div class="section-title">Top Stride Patterns</div>
                                <div id="stride-list"></div>
                            </div>
                        </div>
                    </div>
                    <div class="card">
                        <div class="card-header">Page Type Distribution</div>
                        <div class="card-body" style="padding: 16px;">
                            <div class="donut-container" style="display: flex; align-items: center; gap: 24px;">
                                <canvas id="overview-page-type-donut" width="180" height="180"></canvas>
                                <div class="donut-legend" id="overview-page-type-legend"></div>
                            </div>
                        </div>
                    </div>
                </div>
            </section>

            <!-- TABLES TAB -->
            <section id="tables" class="panel">
                <div id="tables-no-data" class="no-data" style="display:none;">
                    No table data. Run with <code>--trace-cursors</code> for full attribution.
                </div>

                <div id="tables-content">
                    <!-- Summary metrics row -->
                    <div class="metrics-row">
                        <div class="metric">
                            <span class="metric-value" id="tables-total-faults">0</span>
                            <span class="metric-label">Total Faults</span>
                        </div>
                        <div class="metric">
                            <span class="metric-value" id="tables-io-time">0ms</span>
                            <span class="metric-label">I/O Time Lost</span>
                        </div>
                        <div class="metric">
                            <span class="metric-value" id="tables-hottest">-</span>
                            <span class="metric-label">Hottest Table</span>
                        </div>
                        <div class="metric">
                            <span class="metric-value" id="tables-count">0</span>
                            <span class="metric-label">Tables Traced</span>
                        </div>
                    </div>

                    <!-- Attribution badge -->
                    <div class="attribution-header" id="tables-attribution-header">
                        <span class="card-badge direct-badge">Direct BPF Attribution</span>
                        <span class="attribution-summary" id="tables-attribution-summary"></span>
                    </div>

                    <!-- CPU Profile Summary -->
                    <div class="cpu-profile-banner" id="cpu-profile-banner" style="display:none;">
                        <span class="cpu-profile-bottleneck" id="cpu-bottleneck">-</span>
                        <span class="cpu-profile-detail" id="cpu-profile-detail"></span>
                    </div>

                    <!-- Fault distribution and I/O breakdown side by side -->
                    <div class="two-col" id="fault-dist-row" style="display:none;">
                        <div class="card" id="fault-dist-card">
                            <div class="card-header">
                                Fault Distribution
                                <span class="chart-legend">
                                    <span class="legend-item"><span class="legend-swatch minor-swatch"></span>Minor</span>
                                    <span class="legend-item"><span class="legend-swatch major-swatch"></span>Major</span>
                                </span>
                            </div>
                            <div class="card-body" style="padding: 12px 16px;">
                                <canvas id="fault-dist-chart"></canvas>
                            </div>
                        </div>
                        <div class="card" id="io-breakdown-card">
                            <div class="card-header">I/O Time Breakdown</div>
                            <div class="card-body" style="padding: 12px 16px;">
                                <canvas id="io-time-chart"></canvas>
                            </div>
                        </div>
                    </div>

                    <!-- Detailed table with expandable rows showing top 5 operations -->
                    <div class="card full-width">
                        <div class="card-header">
                            Tables
                            <span class="card-hint">Click row to expand details</span>
                        </div>
                        <div class="card-body compact-table-container" style="max-height: none; padding: 0;">
                            <table class="compact-table expandable-table" id="unified-tables">
                                <thead>
                                    <tr>
                                        <th style="width:30px;"></th>
                                        <th>Table</th>
                                        <th>Faults</th>
                                        <th>Major %</th>
                                        <th>B:L Ratio</th>
                                        <th>Working Set</th>
                                        <th>Reuse %</th>
                                        <th>I/O Time</th>
                                        <th>CPU %</th>
                                    </tr>
                                </thead>
                                <tbody></tbody>
                            </table>
                        </div>
                    </div>
                </div>
            </section>

            <!-- RESOURCES TAB - Memory, Transactions, Threads -->
            <section id="resources" class="panel">
                <div id="resources-content">
                    <!-- Memory Section -->
                    <div class="section-header">Memory & Working Set</div>

                    <div id="memory-no-data" class="no-data" style="display:none;">
                        No working set data available.
                    </div>

                    <div id="memory-content">
                        <!-- Memory summary banner -->
                        <div class="memory-summary-banner" id="memory-summary-banner">
                            <span id="memory-summary-text"></span>
                        </div>

                        <!-- Memory metrics -->
                        <div class="metrics-row">
                            <div class="metric">
                                <span class="metric-value" id="mem-unique-pages">0</span>
                                <span class="metric-label">Unique Pages</span>
                            </div>
                            <div class="metric">
                                <span class="metric-value" id="mem-working-set">0 GB</span>
                                <span class="metric-label">Working Set</span>
                            </div>
                            <div class="metric">
                                <span class="metric-value" id="mem-reuse-ratio">0%</span>
                                <span class="metric-label">Page Reuse</span>
                            </div>
                            <div class="metric">
                                <span class="metric-value" id="mem-avg-accesses">0</span>
                                <span class="metric-label">Avg Accesses/Page</span>
                            </div>
                        </div>

                        <!-- Access count distribution -->
                        <div class="card">
                            <div class="card-header">Pages by Access Count</div>
                            <div class="card-body">
                                <div class="access-count-chart" id="access-count-chart"></div>
                            </div>
                        </div>
                    </div>

                    <!-- Transactions Section -->
                    <div class="section-header" style="margin-top: 24px;">Transactions</div>

                    <div id="txn-no-data" class="no-data" style="display:none;">
                        No transaction data available.
                    </div>

                    <div id="txn-content">
                        <div class="metrics-row">
                            <div class="metric">
                                <span class="metric-value" id="txn-total"></span>
                                <span class="metric-label">Total Txns</span>
                            </div>
                            <div class="metric">
                                <span class="metric-value" id="txn-rate"></span>
                                <span class="metric-label">Txns/sec</span>
                            </div>
                            <div class="metric">
                                <span class="metric-value minor" id="txn-ro"></span>
                                <span class="metric-label">Read-Only</span>
                            </div>
                            <div class="metric">
                                <span class="metric-value major" id="txn-rw"></span>
                                <span class="metric-label">Read-Write</span>
                            </div>
                            <div class="metric">
                                <span class="metric-value" id="txn-commits"></span>
                                <span class="metric-label">Commits</span>
                            </div>
                            <div class="metric">
                                <span class="metric-value" id="txn-aborts"></span>
                                <span class="metric-label">Aborts</span>
                            </div>
                        </div>

                        <div class="two-col">
                            <div class="card">
                                <div class="card-header">Concurrency</div>
                                <div class="card-body">
                                    <div class="concurrency-stats">
                                        <div class="conc-stat">
                                            <span class="conc-label">Max RO</span>
                                            <span class="conc-value" id="txn-max-ro"></span>
                                        </div>
                                        <div class="conc-stat">
                                            <span class="conc-label">Max RW</span>
                                            <span class="conc-value" id="txn-max-rw"></span>
                                        </div>
                                        <div class="conc-stat">
                                            <span class="conc-label">Avg RO</span>
                                            <span class="conc-value" id="txn-avg-ro"></span>
                                        </div>
                                    </div>
                                    <div class="uplot-container" id="txn-concurrency-chart">
                                    </div>
                                </div>
                            </div>
                            <div class="card">
                                <div class="card-header">RW Commit Latency (ms)</div>
                                <div class="card-body">
                                    <div class="latency-stats" style="margin-bottom: 16px;">
                                        <div class="lat-stat">
                                            <span class="lat-label">AVG</span>
                                            <span class="lat-value" id="txn-avg-latency"></span>
                                        </div>
                                        <div class="lat-stat">
                                            <span class="lat-label">P95</span>
                                            <span class="lat-value" id="txn-p95-latency"></span>
                                        </div>
                                        <div class="lat-stat">
                                            <span class="lat-label">P99</span>
                                            <span class="lat-value major" id="txn-p99-latency"></span>
                                        </div>
                                        <div class="lat-stat">
                                            <span class="lat-label">MAX</span>
                                            <span class="lat-value major" id="txn-max-latency"></span>
                                        </div>
                                    </div>
                                    <div class="uplot-container" id="txn-latency-chart" style="height: 180px;">
                                    </div>
                                </div>
                            </div>
                        </div>
                    </div>

                    <!-- Threads Section -->
                    <div class="section-header" style="margin-top: 24px;">Threads</div>

                    <div class="two-col">
                        <div class="card">
                            <div class="card-header">Page Faults by Thread</div>
                            <div class="card-body compact-table-container" style="max-height: 300px; padding: 0;">
                                <table class="compact-table" id="threads-faults-table">
                                    <thead>
                                        <tr>
                                            <th>Thread</th>
                                            <th>Faults</th>
                                            <th>% of Total</th>
                                        </tr>
                                    </thead>
                                    <tbody></tbody>
                                </table>
                            </div>
                        </div>
                        <div class="card">
                            <div class="card-header">Transactions by Thread</div>
                            <div class="card-body compact-table-container" style="max-height: 300px; padding: 0;">
                                <table class="compact-table" id="threads-txn-table">
                                    <thead>
                                        <tr>
                                            <th>Thread</th>
                                            <th>Total</th>
                                            <th>RO</th>
                                            <th>RW</th>
                                            <th>Commits</th>
                                        </tr>
                                    </thead>
                                    <tbody></tbody>
                                </table>
                            </div>
                        </div>
                    </div>
                </div>
            </section>
        </main>
    </div>

    <script>
        const DATA = {data_json};
        {javascript}
    </script>
</body>
</html>
"##,
        css = CSS,
        data_json = data_json,
        javascript = JAVASCRIPT
    )
}

const CSS: &str = r##"
* { margin: 0; padding: 0; box-sizing: border-box; }

body {
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    background: #000000;
    color: #e4e4e7;
    font-size: 14px;
    line-height: 1.5;
}

#app {
    max-width: 1600px;
    margin: 0 auto;
    padding: 16px 24px;
}

/* Tabs */
.tabs {
    display: flex;
    gap: 6px;
    margin-bottom: 16px;
    background: #12121a;
    padding: 6px;
    border-radius: 10px;
}

.tab {
    padding: 10px 20px;
    background: transparent;
    border: none;
    color: #71717a;
    cursor: pointer;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 500;
    transition: all 0.15s;
}

.tab:hover { background: #1a1a24; color: #a1a1aa; }
.tab.active { background: #3b82f6; color: #fff; }

.export-btn {
    margin-left: auto;
    padding: 10px 18px;
    background: #059669;
    color: #fff;
    border: none;
    border-radius: 8px;
    cursor: pointer;
    font-size: 13px;
    font-weight: 600;
}
.export-btn:hover { background: #10b981; }

/* Panels */
.panel { display: none; }
.panel.active { display: block; }

/* Metrics row */
.metrics-row {
    display: flex;
    gap: 12px;
    margin-bottom: 16px;
    flex-wrap: wrap;
}

.metric {
    background: #12121a;
    padding: 14px 18px;
    border-radius: 8px;
    text-align: center;
    min-width: 110px;
    flex: 1;
}

.metric-value {
    display: block;
    font-size: 20px;
    font-weight: 700;
    color: #3b82f6;
}
.metric-value.major { color: #f87171; }
.metric-value.minor { color: #34d399; }

.metric-label {
    display: block;
    font-size: 11px;
    color: #71717a;
    text-transform: uppercase;
    margin-top: 4px;
    letter-spacing: 0.5px;
}

/* Section headers for Resources tab */
.section-header {
    font-size: 14px;
    font-weight: 600;
    color: #3b82f6;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: 12px;
    padding-bottom: 8px;
    border-bottom: 1px solid #1e1e2a;
}

/* Two column layout */
.two-col {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
    margin-bottom: 16px;
}

@media (max-width: 1000px) {
    .two-col { grid-template-columns: 1fr; }
}

/* Cards */
.card {
    background: #12121a;
    border-radius: 10px;
    border: 1px solid #1e1e2a;
    overflow: hidden;
}

.card.full-width { grid-column: 1 / -1; }

.card-header {
    padding: 12px 16px;
    background: #000000;
    font-size: 12px;
    font-weight: 600;
    color: #a1a1aa;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    border-bottom: 1px solid #1e1e2a;
}

.card-badge {
    font-size: 11px;
    padding: 3px 8px;
    border-radius: 4px;
    background: #22c55e20;
    color: #22c55e;
    font-weight: 500;
}

.card-badge.direct-badge {
    background: #3b82f620;
    color: #3b82f6;
}

.attribution-summary {
    font-size: 13px;
    color: #d4d4d8;
}

.attribution-header {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 16px;
    padding: 12px 16px;
    background: #12121a;
    border-radius: 8px;
}

.card-hint {
    font-size: 11px;
    color: #52525b;
    font-weight: 400;
    text-transform: none;
    margin-left: auto;
}

/* Chart legend (inline in header) */
.chart-legend {
    display: flex;
    gap: 16px;
    font-size: 11px;
    font-weight: 400;
    text-transform: none;
}

.legend-item {
    display: flex;
    align-items: center;
    gap: 6px;
    color: #a1a1aa;
}

.legend-swatch {
    width: 12px;
    height: 12px;
    border-radius: 2px;
}

.legend-swatch.minor-swatch { background: #60a5fa; }
.legend-swatch.major-swatch { background: #f87171; }

/* Expandable table rows */
.expandable-table tbody tr.table-row {
    cursor: pointer;
    transition: background 0.15s;
}

.expandable-table tbody tr.table-row:hover {
    background: #1a1a24;
}

.expandable-table .expand-icon {
    display: inline-block;
    width: 16px;
    text-align: center;
    color: #52525b;
    transition: transform 0.2s;
}

.expandable-table tr.table-row.expanded .expand-icon {
    transform: rotate(90deg);
    color: #3b82f6;
}

.expandable-table tr.details-row {
    background: #0a0a10;
}

.expandable-table tr.details-row td {
    padding: 0;
    border-bottom: 2px solid #2a2a3a;
}

.expandable-table tr.details-row.hidden {
    display: none;
}

.details-content {
    padding: 16px 24px;
    display: grid;
    grid-template-columns: 2fr 1fr;
    gap: 20px;
}

.details-section {
    background: #12121a;
    border-radius: 6px;
    padding: 12px;
}

.details-section-title {
    font-size: 11px;
    font-weight: 600;
    color: #3b82f6;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: 8px;
}

.details-list {
    font-size: 12px;
}

.details-item {
    display: flex;
    justify-content: space-between;
    padding: 4px 0;
    border-bottom: 1px solid #1e1e2a;
}

.details-item:last-child {
    border-bottom: none;
}

.details-item-name {
    color: #a1a1aa;
}

.details-item-value {
    color: #e4e4e7;
    font-variant-numeric: tabular-nums;
}

.details-item-value.major {
    color: #f87171;
}

.io-time {
    color: #60a5fa;
    font-weight: 500;
}

/* CPU efficiency coloring: I/O bound (low CPU%) = blue, CPU bound (high CPU%) = orange */
.io-bound {
    color: #60a5fa;
    font-weight: 500;
}
.cpu-bound {
    color: #f59e0b;
    font-weight: 500;
}

/* CPU Profile banner - matches attribution-header style */
.cpu-profile-banner {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 16px;
    padding: 12px 16px;
    background: #12121a;
    border-radius: 8px;
}
.cpu-profile-bottleneck {
    font-size: 11px;
    font-weight: 600;
    padding: 4px 8px;
    border-radius: 4px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    background: #3b82f620;
    color: #3b82f6;
}
.cpu-profile-bottleneck.io-bound {
    background: #3b82f620;
    color: #3b82f6;
}
.cpu-profile-bottleneck.cpu-bound {
    background: #f59e0b20;
    color: #f59e0b;
}
.cpu-profile-detail {
    font-size: 13px;
    color: #d4d4d8;
}

.axis-hint {
    font-size: 10px;
    color: #52525b;
    font-weight: 400;
    text-transform: none;
}

.card-body {
    padding: 16px;
}

/* Chart containers */
.chart-container {
    position: relative;
    height: 220px;
}

.chart-container canvas {
    width: 100% !important;
    height: 100% !important;
}

/* Heatmap container - larger for better visualization */
.heatmap-container {
    position: relative;
    height: 400px;
}

.heatmap-container canvas {
    width: 100% !important;
    height: 100% !important;
    cursor: crosshair;
}

/* Heatmap selection box for zoom */
.heatmap-selection {
    position: absolute;
    background: rgba(99, 102, 241, 0.15);
    border: 1px solid rgba(99, 102, 241, 0.5);
    pointer-events: none;
    z-index: 10;
}

/* uPlot styling */
.uplot-container {
    position: relative;
    height: 220px;
    width: 100%;
}

.uplot {
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif !important;
}

.u-legend {
    display: none !important;
}

.u-select {
    background: rgba(99, 102, 241, 0.15) !important;
}

.u-cursor-x, .u-cursor-y {
    border-color: rgba(99, 102, 241, 0.5) !important;
}

.chart-tooltip {
    position: absolute;
    background: #1e1e2a;
    border: 1px solid #3b82f6;
    border-radius: 6px;
    padding: 8px 12px;
    font-size: 12px;
    color: #e4e4e7;
    pointer-events: none;
    z-index: 100;
    white-space: nowrap;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
}

.chart-tooltip-label {
    color: #71717a;
    font-size: 10px;
    text-transform: uppercase;
    margin-bottom: 4px;
}

.chart-tooltip-value {
    color: #60a5fa;
    font-weight: 600;
    font-size: 14px;
}

/* Compact tables */
.compact-table-container {
    overflow-x: auto;
    overflow-y: auto;
    max-height: 450px;
}

/* Remove padding from card-body when it contains a table, let table cells handle padding */
.card-body.compact-table-container {
    padding: 0;
}

.compact-table {
    width: 100%;
    border-collapse: separate;
    border-spacing: 0;
    font-size: 13px;
}

.compact-table th,
.compact-table td {
    padding: 10px 16px;
    text-align: left;
    border-bottom: 1px solid #1e1e2a;
    white-space: nowrap;
}

.compact-table th:first-child,
.compact-table td:first-child {
    padding-left: 20px;
}

.compact-table th:last-child,
.compact-table td:last-child {
    padding-right: 20px;
}

.compact-table thead th {
    position: sticky;
    top: 0;
    z-index: 10;
    background: #12121a;
    font-weight: 600;
    color: #3b82f6;
    border-bottom: 2px solid #2a2a3a;
}

.compact-table tbody tr:hover { background: #1a1a24; }

.compact-table td { font-variant-numeric: tabular-nums; }

/* Pattern section */
.pattern-section {
    margin-bottom: 20px;
}

.pattern-bar {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 10px;
}

.pattern-label {
    width: 80px;
    font-size: 13px;
    color: #a1a1aa;
    font-weight: 500;
}

.bar-container {
    flex: 1;
    height: 12px;
    background: #1e1e2a;
    border-radius: 6px;
    overflow: hidden;
}

.bar {
    height: 100%;
    border-radius: 6px;
    transition: width 0.3s;
}

.bar.sequential { background: linear-gradient(90deg, #3b82f6, #60a5fa); }
.bar.random { background: linear-gradient(90deg, #4f46e5, #818cf8); }

.pattern-value {
    width: 55px;
    text-align: right;
    font-size: 13px;
    font-weight: 600;
    color: #e4e4e7;
}

/* Section titles */
.section-title {
    font-size: 11px;
    font-weight: 600;
    color: #3b82f6;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: 10px;
    padding-bottom: 6px;
    border-bottom: 1px solid #1e1e2a;
}

/* Stride section */
.stride-section {
    margin-bottom: 20px;
}

.stride-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 6px 0;
    font-size: 12px;
    color: #a1a1aa;
}

.stride-item .stride-type {
    color: #60a5fa;
    font-weight: 500;
}

.stride-item .stride-count {
    color: #71717a;
}

/* Threads section */
.threads-section {
    font-size: 13px;
}

.thread-item {
    display: flex;
    justify-content: space-between;
    padding: 8px 0;
    border-bottom: 1px solid #1e1e2a;
    color: #a1a1aa;
}

.thread-item:last-child { border-bottom: none; }

/* Concurrency stats */
.concurrency-stats, .latency-stats {
    display: flex;
    gap: 24px;
    margin-bottom: 16px;
    flex-wrap: wrap;
}

.conc-stat, .lat-stat {
    text-align: center;
}

.conc-label, .lat-label {
    display: block;
    font-size: 11px;
    color: #71717a;
    text-transform: uppercase;
    margin-bottom: 4px;
}

.conc-value, .lat-value {
    display: block;
    font-size: 24px;
    font-weight: 700;
    color: #3b82f6;
}

.lat-value.major { color: #f87171; }

/* No data state */
.no-data {
    text-align: center;
    padding: 60px;
    color: #71717a;
    font-size: 15px;
}

.no-data code {
    background: #1e1e2a;
    padding: 4px 8px;
    border-radius: 4px;
    font-family: monospace;
    color: #a1a1aa;
}

/* Scrollbar */
::-webkit-scrollbar { width: 8px; height: 8px; }
::-webkit-scrollbar-track { background: #000000; }
::-webkit-scrollbar-thumb { background: #2a2a3a; border-radius: 4px; }
::-webkit-scrollbar-thumb:hover { background: #3a3a4a; }

/* ============================================
   B+ TREE TAB STYLES
   ============================================ */

/* Page type color tokens */
:root {
    --color-branch: #f59e0b;
    --color-leaf: #22c55e;
    --color-overflow: #8b5cf6;
    --color-meta: #6366f1;
    --color-unknown: #71717a;
}

.donut-container {
    position: relative;
    width: 220px;
    height: 220px;
}

.donut-center {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    text-align: center;
}

.donut-total {
    display: block;
    font-size: 28px;
    font-weight: 700;
    color: #fff;
}

.donut-label {
    display: block;
    font-size: 11px;
    color: #71717a;
    text-transform: uppercase;
    letter-spacing: 0.5px;
}

.donut-legend {
    display: flex;
    flex-direction: column;
    gap: 8px;
}

.legend-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
}

.legend-dot {
    width: 12px;
    height: 12px;
    border-radius: 50%;
}

.legend-dot.branch { background: var(--color-branch); }
.legend-dot.leaf { background: var(--color-leaf); }
.legend-dot.overflow { background: var(--color-overflow); }
.legend-dot.meta { background: var(--color-meta); }
.legend-dot.unknown { background: var(--color-unknown); }

.legend-name {
    color: #a1a1aa;
    min-width: 70px;
}

.legend-value {
    color: #fff;
    font-weight: 600;
}

.legend-pct {
    color: #71717a;
    font-size: 12px;
}

/* Per-table tree bars */
.table-tree-bar {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 12px;
}

.table-tree-name {
    width: 200px;
    font-size: 13px;
    color: #e4e4e7;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
}

.table-tree-bar-container {
    flex: 1;
    height: 24px;
    background: #0a0a0f;
    border-radius: 4px;
    overflow: hidden;
    display: flex;
}

.table-tree-segment {
    height: 100%;
    transition: width 0.3s ease;
}

.table-tree-segment.branch {
    background: var(--color-branch);
}

.table-tree-segment.leaf {
    background: var(--color-leaf);
}

.table-tree-segment.overflow {
    background: var(--color-overflow);
}

.table-tree-stats {
    width: 200px;
    display: flex;
    gap: 16px;
    font-size: 12px;
}

.table-tree-stat {
    display: flex;
    gap: 4px;
}

.table-tree-stat-label {
    color: #71717a;
}

.table-tree-stat-value {
    color: #e4e4e7;
    font-weight: 500;
}

.table-tree-stat-value.branch {
    color: var(--color-branch);
}

.table-tree-stat-value.leaf {
    color: var(--color-leaf);
}

.tree-card {
    background: #1a1a24;
    border: 1px solid #27272a;
    border-radius: 8px;
    padding: 12px;
    width: 180px;
    cursor: pointer;
    transition: all 0.2s ease;
}

.tree-card:hover {
    border-color: #3f3f46;
    transform: translateY(-2px);
}

.tree-card-header {
    font-size: 12px;
    font-weight: 600;
    color: #e4e4e7;
    margin-bottom: 8px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
}

.tree-diagram {
    height: 80px;
    position: relative;
    margin-bottom: 8px;
}

.tree-card-stats {
    display: flex;
    justify-content: space-between;
    font-size: 11px;
}

.tree-card-stat {
    text-align: center;
}

.tree-card-stat-value {
    font-weight: 600;
    color: #e4e4e7;
}

.tree-card-stat-label {
    color: #71717a;
    font-size: 10px;
}

/* Tree tooltip */
.tree-tooltip {
    position: fixed;
    background: #1a1a24;
    border: 1px solid #3f3f46;
    border-radius: 8px;
    padding: 12px;
    font-size: 12px;
    z-index: 1000;
    pointer-events: none;
    max-width: 280px;
    box-shadow: 0 4px 12px rgba(0,0,0,0.5);
}

.tree-tooltip-header {
    font-weight: 600;
    color: #e4e4e7;
    margin-bottom: 8px;
    border-bottom: 1px solid #27272a;
    padding-bottom: 6px;
}

.tree-tooltip-row {
    display: flex;
    justify-content: space-between;
    margin-bottom: 4px;
}

.tree-tooltip-label {
    color: #71717a;
}

.tree-tooltip-value {
    color: #e4e4e7;
    font-weight: 500;
}

/* Sort controls for table comparison */
.sort-controls {
    font-size: 12px;
}

.sort-btn {
    background: #27272a;
    border: 1px solid #3f3f46;
    color: #a1a1aa;
    padding: 4px 10px;
    border-radius: 4px;
    cursor: pointer;
    transition: all 0.2s ease;
}

.sort-btn:hover {
    background: #3f3f46;
    color: #e4e4e7;
}

.sort-btn.active {
    background: var(--color-branch);
    border-color: var(--color-branch);
    color: #000;
}

/* Distribution bar in table comparison */
.dist-bar {
    display: flex;
    height: 16px;
    border-radius: 4px;
    overflow: hidden;
    background: #27272a;
}

.dist-bar-segment {
    height: 100%;
    transition: width 0.3s ease;
}

.dist-bar-segment.branch {
    background: var(--color-branch);
}

.dist-bar-segment.leaf {
    background: var(--color-leaf);
}

.dist-bar-segment.overflow {
    background: var(--color-overflow);
}

/* Efficiency score color coding */
.efficiency-high { color: #22c55e; }
.efficiency-medium { color: #f59e0b; }
.efficiency-low { color: #ef4444; }

/* Block analysis table enhancements */
#block-analysis-table tbody tr:hover {
    background: #1f1f2a;
}

.tables-touched {
    font-size: 10px;
    color: #71717a;
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}

@media (max-width: 1000px) {
    .btree-hero {
        flex-direction: column;
    }
    .btree-hero-chart {
        flex-direction: column;
        align-items: center;
    }
    .table-tree-bar {
        flex-direction: column;
        align-items: flex-start;
    }
    .table-tree-name,
    .table-tree-stats {
        width: 100%;
    }
}

/* Memory tab styles */
.memory-summary-banner {
    background: #12121a;
    border-radius: 8px;
    padding: 12px 16px;
    margin-bottom: 16px;
    font-size: 13px;
    line-height: 1.6;
    color: #d4d4d8;
}

.access-count-chart {
    min-height: 200px;
    margin-bottom: 16px;
}

.time-wss-stats {
    display: flex;
    gap: 24px;
    padding: 12px;
}

.time-wss-stat {
    text-align: center;
}

.time-wss-stat-value {
    font-size: 20px;
    font-weight: 700;
    color: #3b82f6;
}

.time-wss-stat-label {
    font-size: 12px;
    color: #71717a;
}

.bar-chart-container {
    display: flex;
    flex-direction: column;
    gap: 8px;
}

.bar-chart-row {
    display: flex;
    align-items: center;
    gap: 10px;
}

.bar-chart-label {
    width: 140px;
    font-size: 12px;
    color: #a1a1aa;
    text-align: right;
    flex-shrink: 0;
}

.bar-chart-bar-container {
    flex: 1;
    height: 24px;
    background: #1a1a24;
    border-radius: 4px;
    overflow: hidden;
    position: relative;
}

.bar-chart-bar {
    height: 100%;
    border-radius: 4px;
    transition: width 0.3s ease;
}

.bar-chart-bar.access-count { background: linear-gradient(90deg, #06b6d4, #22d3ee); }

.bar-chart-value {
    width: 70px;
    font-size: 12px;
    color: #e4e4e7;
    font-weight: 600;
}
"##;

const JAVASCRIPT: &str = r##"
// Track all uPlot instances for resize handling
const uplotInstances = [];

// uPlot interactive chart helper
function createUPlotChart(container, data, opts = {}) {
    const el = typeof container === 'string' ? document.getElementById(container) : container;
    if (!el || !data.length) return null;

    const rect = el.getBoundingClientRect();
    const width = rect.width || 400;
    const height = rect.height || 200;

    // Build time series: timestamps and values
    const durationSecs = opts.durationSecs || data.length;
    const timestamps = data.map((_, i) => i * (durationSecs / data.length));

    const uplotData = [timestamps, data];

    const color = opts.color || '#3b82f6';

    const uplotOpts = {
        width,
        height,
        // Padding to align with heatmap: { t: 25, r: 20, b: 45, l: 63 }
        padding: [25, 20, 0, 0], // top, right, bottom (handled by axis), left (handled by axis)
        cursor: {
            show: true,
            x: true,
            y: true,
            drag: { x: true, y: false, setScale: true }
        },
        select: {
            show: true,
        },
        scales: {
            x: { time: false },
            y: { auto: true }
        },
        axes: [
            {
                stroke: '#71717a',
                grid: { stroke: '#1e1e2a', width: 1 },
                ticks: { stroke: '#1e1e2a', width: 1 },
                font: '11px sans-serif',
                size: 45, // Bottom axis height to match heatmap
                values: (u, vals) => vals.map(v => {
                    if (v < 60) return v.toFixed(0) + 's';
                    return (v / 60).toFixed(1) + 'm';
                })
            },
            {
                stroke: '#71717a',
                grid: { stroke: '#1e1e2a', width: 1 },
                ticks: { stroke: '#1e1e2a', width: 1 },
                font: '11px sans-serif',
                size: 63, // Left axis width to match heatmap
                values: (u, vals) => vals.map(v => {
                    if (v >= 1e6) return (v/1e6).toFixed(1) + 'M';
                    if (v >= 1e3) return (v/1e3).toFixed(1) + 'K';
                    return v.toFixed(0);
                })
            }
        ],
        series: [
            {},
            {
                stroke: color,
                width: 2,
                fill: color + '30',
                label: opts.label || 'Value',
                points: { show: false }
            }
        ],
        hooks: {
            setSelect: [
                u => {
                    if (u.select.width > 0) {
                        const min = u.posToVal(u.select.left, 'x');
                        const max = u.posToVal(u.select.left + u.select.width, 'x');
                        u.setScale('x', { min, max });
                    }
                    u.setSelect({ width: 0, height: 0 }, false);
                }
            ]
        }
    };

    // Clear container and create chart
    el.innerHTML = '';
    const uplot = new uPlot(uplotOpts, uplotData, el);

    // Double-click to reset zoom
    el.addEventListener('dblclick', () => {
        uplot.setScale('x', { min: timestamps[0], max: timestamps[timestamps.length - 1] });
    });

    // Track for resize
    uplotInstances.push({ uplot, el, height });

    return uplot;
}

// uPlot scatter chart for commit latency timeline
function createCommitTimelineChart(container, commitData, opts = {}) {
    const el = typeof container === 'string' ? document.getElementById(container) : container;
    if (!el || !commitData.length) return null;

    const rect = el.getBoundingClientRect();
    const width = rect.width || 400;
    const height = rect.height || 140;

    // Extract timestamps (seconds) and latencies (ms)
    const timestamps = commitData.map(p => p.time_secs);
    const latencies = commitData.map(p => p.latency_ms);

    const uplotData = [timestamps, latencies];

    const color = opts.color || '#3b82f6';

    const uplotOpts = {
        width,
        height,
        cursor: {
            show: true,
            x: true,
            y: true,
            drag: { x: true, y: false, setScale: true }
        },
        select: {
            show: true,
        },
        scales: {
            x: { time: false },
            y: { auto: true }
        },
        axes: [
            {
                stroke: '#71717a',
                grid: { stroke: '#1e1e2a', width: 1 },
                ticks: { stroke: '#1e1e2a', width: 1 },
                font: '11px sans-serif',
                values: (u, vals) => vals.map(v => {
                    if (v < 60) return v.toFixed(0) + 's';
                    return (v / 60).toFixed(1) + 'm';
                })
            },
            {
                stroke: '#71717a',
                grid: { stroke: '#1e1e2a', width: 1 },
                ticks: { stroke: '#1e1e2a', width: 1 },
                font: '11px sans-serif',
                size: 50,
                values: (u, vals) => vals.map(v => {
                    if (v === 0) return '0';
                    if (v < 1) return v.toFixed(1);
                    if (v < 1000) return v.toFixed(0);
                    return (v / 1000).toFixed(1) + 's';
                })
            }
        ],
        series: [
            {},
            {
                stroke: color,
                width: 0,
                fill: color + '80',
                label: 'Commit Latency',
                paths: (u, seriesIdx, idx0, idx1) => {
                    // Draw vertical bars from 0 to value
                    const s = u.series[seriesIdx];
                    const xdata = u.data[0];
                    const ydata = u.data[seriesIdx];

                    let path = new Path2D();
                    const barWidth = Math.max(2, Math.min(8, u.bbox.width / ydata.length * 0.6));

                    for (let i = idx0; i <= idx1; i++) {
                        const x = u.valToPos(xdata[i], 'x', true);
                        const y0 = u.valToPos(0, 'y', true);
                        const y1 = u.valToPos(ydata[i], 'y', true);

                        path.rect(x - barWidth/2, y1, barWidth, y0 - y1);
                    }

                    return { stroke: path, fill: path };
                },
                points: { show: false }
            }
        ],
        hooks: {
            setSelect: [
                u => {
                    if (u.select.width > 0) {
                        const min = u.posToVal(u.select.left, 'x');
                        const max = u.posToVal(u.select.left + u.select.width, 'x');
                        u.setScale('x', { min, max });
                    }
                    u.setSelect({ width: 0, height: 0 }, false);
                }
            ]
        }
    };

    // Clear container and create chart
    el.innerHTML = '';
    const uplot = new uPlot(uplotOpts, uplotData, el);

    // Double-click to reset zoom
    el.addEventListener('dblclick', () => {
        uplot.setScale('x', { min: timestamps[0], max: timestamps[timestamps.length - 1] });
    });

    // Track for resize
    uplotInstances.push({ uplot, el, height });

    return uplot;
}

// Resize handler for all uPlot charts
let resizeTimeout;
window.addEventListener('resize', () => {
    clearTimeout(resizeTimeout);
    resizeTimeout = setTimeout(() => {
        uplotInstances.forEach(({ uplot, el, height }) => {
            const rect = el.getBoundingClientRect();
            if (rect.width > 0) {
                uplot.setSize({ width: rect.width, height: height });
            }
        });
    }, 100);
});

// Canvas-based Chart class for bar charts, histograms, and heatmaps
class Chart {
    constructor(canvas) {
        this.canvas = canvas;
        this.ctx = canvas.getContext('2d');
    }

    resize() {
        const rect = this.canvas.getBoundingClientRect();
        if (rect.width === 0) return false;
        this.canvas.width = rect.width * 2;
        this.canvas.height = rect.height * 2;
        this.ctx.scale(2, 2);
        this.w = rect.width;
        this.h = rect.height;
        return true;
    }

    clear() { this.ctx.clearRect(0, 0, this.w, this.h); }

    drawHorizontalBar(labels, values, color = '#6366f1') {
        if (!this.resize() || !values.length) return;
        this.clear();

        const pad = { t: 10, r: 60, b: 10, l: 120 };
        const w = this.w - pad.l - pad.r;
        const h = this.h - pad.t - pad.b;
        const max = Math.max(...values) * 1.1 || 1;
        const barH = Math.min(24, (h / values.length) * 0.7);
        const gap = (h / values.length) - barH;

        values.forEach((v, i) => {
            const barW = (v / max) * w;
            const x = pad.l;
            const y = pad.t + i * (barH + gap) + gap / 2;

            // Bar
            this.ctx.fillStyle = color;
            this.ctx.fillRect(x, y, barW, barH);

            // Label (left)
            this.ctx.fillStyle = '#a1a1aa';
            this.ctx.font = '11px sans-serif';
            this.ctx.textAlign = 'right';
            const labelText = labels[i].length > 16 ? labels[i].substring(0, 14) + '...' : labels[i];
            this.ctx.fillText(labelText, pad.l - 8, y + barH / 2 + 4);

            // Value (right)
            this.ctx.fillStyle = '#71717a';
            this.ctx.textAlign = 'left';
            this.ctx.fillText(this.fmtAxisVal(v), x + barW + 8, y + barH / 2 + 4);
        });
    }

    drawHeatmap(data, opts = {}) {
        if (!this.resize()) return;
        const { time_buckets, offset_buckets, data: cells, max_count, min_offset_gb, max_offset_gb, min_time_ms, max_time_ms } = data;

        const pad = { t: 20, r: 20, b: 40, l: 60 };
        const w = this.w - pad.l - pad.r;
        const h = this.h - pad.t - pad.b;
        const cellW = w / time_buckets;
        const cellH = h / offset_buckets;

        // Draw cells
        for (let t = 0; t < time_buckets; t++) {
            for (let o = 0; o < offset_buckets; o++) {
                const idx = t * offset_buckets + o;
                const intensity = max_count > 0 ? cells[idx] / max_count : 0;
                const x = pad.l + t * cellW;
                const y = pad.t + (offset_buckets - 1 - o) * cellH;

                this.ctx.fillStyle = this.heatColor(intensity);
                this.ctx.fillRect(x, y, cellW + 1, cellH + 1);
            }
        }

        // Y-axis labels (file offset in GB)
        this.ctx.fillStyle = '#71717a';
        this.ctx.font = '10px sans-serif';
        this.ctx.textAlign = 'right';
        const offsetRange = max_offset_gb - min_offset_gb;
        for (let i = 0; i <= 4; i++) {
            const gb = min_offset_gb + offsetRange * (1 - i / 4);
            const y = pad.t + (h / 4) * i;
            this.ctx.fillText(gb.toFixed(0) + 'GB', pad.l - 8, y + 4);
        }

        // X-axis labels (time)
        this.ctx.textAlign = 'center';
        const timeRange = max_time_ms - min_time_ms;
        for (let i = 0; i <= 4; i++) {
            const ms = min_time_ms + timeRange * (i / 4);
            const x = pad.l + (w / 4) * i;
            const label = ms < 60000 ? (ms/1000).toFixed(0) + 's' : (ms/60000).toFixed(1) + 'm';
            this.ctx.fillText(label, x, this.h - pad.b + 16);
        }

        // Axis titles
        this.ctx.fillStyle = '#52525b';
        this.ctx.font = '10px sans-serif';
        this.ctx.textAlign = 'center';
        this.ctx.fillText('Time', pad.l + w / 2, this.h - 5);

        this.ctx.save();
        this.ctx.translate(12, pad.t + h / 2);
        this.ctx.rotate(-Math.PI / 2);
        this.ctx.fillText('File Offset', 0, 0);
        this.ctx.restore();
    }

    heatColor(i) {
        if (i === 0) return '#000000';
        // Smooth gradient: black -> dark blue -> blue -> cyan
        // Use power curve for better low-end visibility
        const t = Math.pow(i, 0.6);
        const r = Math.floor(t * 96);
        const g = Math.floor(t * 165 + (1 - t) * 20);
        const b = Math.floor(t * 255 + (1 - t) * 40);
        return `rgb(${r}, ${g}, ${b})`;
    }

    fmtAxisVal(n) {
        if (n >= 1e6) return (n/1e6).toFixed(1) + 'M';
        if (n >= 1e3) return (n/1e3).toFixed(1) + 'K';
        return n.toFixed(0);
    }
}

// Initialize Plotly heatmap - handles zoom, pan, and tooltips correctly
function initPlotlyHeatmap(container, data) {
    const { time_buckets, offset_buckets, data: cells, max_count, 
            min_offset_gb, max_offset_gb, min_time_ms, max_time_ms,
            cell_attribution } = data;

    // Build attribution lookup for custom hover text
    const attrLookup = {};
    if (cell_attribution) {
        cell_attribution.forEach(a => { attrLookup[a.cell] = a.tables; });
    }

    // Convert flat array to 2D array for Plotly (offset rows, time columns)
    // Plotly expects z[y][x] format
    const z = [];
    const customData = [];  // For storing attribution info
    for (let o = 0; o < offset_buckets; o++) {
        const row = [];
        const customRow = [];
        for (let t = 0; t < time_buckets; t++) {
            const idx = t * offset_buckets + o;
            row.push(cells[idx] || 0);
            // Store cell index for attribution lookup
            customRow.push(idx);
        }
        z.push(row);
        customData.push(customRow);
    }

    // Generate axis labels
    const timeRange = max_time_ms - min_time_ms;
    const offsetRange = max_offset_gb - min_offset_gb;

    const xLabels = [];
    for (let t = 0; t < time_buckets; t++) {
        const time = min_time_ms + (t + 0.5) * timeRange / time_buckets;
        xLabels.push(time < 60000 
            ? (time / 1000).toFixed(2) + 's'
            : (time / 60000).toFixed(2) + 'm');
    }

    const yLabels = [];
    for (let o = 0; o < offset_buckets; o++) {
        const offset = min_offset_gb + (o + 0.5) * offsetRange / offset_buckets;
        yLabels.push(offset.toFixed(2) + ' GB');
    }

    // Custom hover template with attribution
    const hovertemplate = 
        '<b>Time:</b> %{x}<br>' +
        '<b>Offset:</b> %{y}<br>' +
        '<b>Faults:</b> %{z}<br>' +
        '<extra></extra>';

    const trace = {
        z: z,
        x: xLabels,
        y: yLabels,
        customdata: customData,
        type: 'heatmap',
        colorscale: [
            [0, '#000000'],
            [0.1, '#1a1a3e'],
            [0.25, '#14147a'],
            [0.5, '#1478b4'],
            [0.75, '#3cfff0'],
            [1, '#ffff50']
        ],
        hovertemplate: hovertemplate,
        showscale: true,
        colorbar: {
            title: 'Faults',
            titleside: 'right',
            tickfont: { color: '#a1a1aa', size: 10 },
            titlefont: { color: '#a1a1aa', size: 11 }
        }
    };

    const layout = {
        paper_bgcolor: '#18181b',
        plot_bgcolor: '#000000',
        font: { color: '#a1a1aa', family: '-apple-system, BlinkMacSystemFont, sans-serif' },
        margin: { l: 70, r: 80, t: 20, b: 50 },
        xaxis: {
            title: 'Time',
            titlefont: { size: 12 },
            tickfont: { size: 10 },
            tickangle: 0,
            nticks: 8,
            gridcolor: '#27272a',
            linecolor: '#3f3f46'
        },
        yaxis: {
            title: 'File Offset',
            titlefont: { size: 12 },
            tickfont: { size: 10 },
            nticks: 8,
            gridcolor: '#27272a',
            linecolor: '#3f3f46'
        },
        dragmode: 'zoom'
    };

    const config = {
        responsive: true,
        displayModeBar: true,
        modeBarButtonsToRemove: ['lasso2d', 'select2d', 'autoScale2d'],
        displaylogo: false,
        doubleClick: 'reset'
    };

    Plotly.newPlot(container, [trace], layout, config);

    // Add custom hover handler for attribution data
    container.on('plotly_hover', function(eventData) {
        if (!eventData.points || !eventData.points[0]) return;
        const pt = eventData.points[0];
        const cellIdx = pt.customdata;
        const tables = attrLookup[cellIdx];
        
        if (tables && tables.length > 0) {
            // Update the hover label with attribution info
            const hoverText = document.querySelector('.hovertext');
            if (hoverText) {
                let attrHtml = '<tspan x="0" dy="1.2em" style="font-weight:bold">Top Tables:</tspan>';
                tables.forEach(([name, total, major]) => {
                    const majorPct = total > 0 ? Math.round(major / total * 100) : 0;
                    attrHtml += '<tspan x="0" dy="1.1em">' + name + ': ' + total + ' (' + majorPct + '% major)</tspan>';
                });
            }
        }
    });
}

// Horizontal bar chart for fault distribution (dynamically sized)
function drawFaultDistChart(canvas, labels, totalFaults, majorFaults) {
    const ctx = canvas.getContext('2d');
    const container = canvas.parentElement;
    const containerWidth = container ? container.offsetWidth : 0;
    const rect = canvas.getBoundingClientRect();
    // Use container width, fallback to rect width, minimum 500px to fit labels
    const w = Math.max(500, containerWidth > 0 ? containerWidth : (rect.width > 0 ? rect.width : 500));

    // Dynamic height based on number of bars
    const barHeight = 22;
    const barGap = 12;
    const pad = { t: 12, r: 80, b: 12, l: 160 };
    const chartH = labels.length * (barHeight + barGap) - barGap;
    const totalH = chartH + pad.t + pad.b;

    // Set canvas size
    canvas.style.width = w + 'px';
    canvas.style.height = totalH + 'px';
    canvas.width = w * 2;
    canvas.height = totalH * 2;
    ctx.scale(2, 2);
    const chartW = w - pad.l - pad.r;

    const max = Math.max(...totalFaults) * 1.1 || 1;

    ctx.clearRect(0, 0, w, totalH);

    labels.forEach((label, i) => {
        const total = totalFaults[i];
        const major = majorFaults[i];

        const totalBarW = (total / max) * chartW;
        const minorBarW = ((total - major) / max) * chartW;
        const y = pad.t + i * (barHeight + barGap);

        // Draw minor faults (blue) first, then major (red) stacked after
        ctx.fillStyle = '#60a5fa';
        ctx.fillRect(pad.l, y, minorBarW, barHeight);

        if (major > 0) {
            ctx.fillStyle = '#f87171';
            ctx.fillRect(pad.l + minorBarW, y, totalBarW - minorBarW, barHeight);
        }

        // Label (left)
        ctx.fillStyle = '#e4e4e7';
        ctx.font = '12px -apple-system, BlinkMacSystemFont, sans-serif';
        ctx.textAlign = 'right';
        ctx.fillText(label, pad.l - 12, y + barHeight / 2 + 4);

        // Value (right of bar)
        ctx.fillStyle = '#a1a1aa';
        ctx.textAlign = 'left';
        const fmtVal = n => n >= 1e6 ? (n/1e6).toFixed(1)+'M' : n >= 1e3 ? (n/1e3).toFixed(1)+'K' : n.toFixed(0);
        ctx.fillText(fmtVal(total), pad.l + totalBarW + 10, y + barHeight / 2 + 4);
    });
}

// Horizontal bar chart for I/O time breakdown (dynamically sized)
function drawIOTimeChart(canvas, labels, timeMs, slowOps) {
    const ctx = canvas.getContext('2d');
    const container = canvas.parentElement;
    const containerWidth = container ? container.offsetWidth : 0;
    const rect = canvas.getBoundingClientRect();
    // Use container width, fallback to rect width, minimum 500px to fit labels
    const w = Math.max(500, containerWidth > 0 ? containerWidth : (rect.width > 0 ? rect.width : 500));

    // Dynamic height based on number of bars
    const barHeight = 22;
    const barGap = 12;
    const pad = { t: 12, r: 70, b: 12, l: 160 };
    const chartH = labels.length * (barHeight + barGap) - barGap;
    const totalH = chartH + pad.t + pad.b;

    // Set canvas size
    canvas.style.width = w + 'px';
    canvas.style.height = totalH + 'px';
    canvas.width = w * 2;
    canvas.height = totalH * 2;
    ctx.scale(2, 2);

    const chartW = w - pad.l - pad.r;

    const max = Math.max(...timeMs) * 1.1 || 1;

    ctx.clearRect(0, 0, w, totalH);

    labels.forEach((label, i) => {
        const time = timeMs[i];
        const ops = slowOps[i];

        const barW = (time / max) * chartW;
        const y = pad.t + i * (barHeight + barGap);

        // Blue to purple gradient based on position (matches app theme)
        const t = i / (labels.length - 1 || 1);
        const r = Math.floor(96 + t * (129 - 96));    // 60 -> 81 (hex)
        const g = Math.floor(165 + t * (140 - 165));  // a5 -> 8c
        const b = Math.floor(250 + t * (248 - 250));  // fa -> f8
        ctx.fillStyle = `rgb(${r}, ${g}, ${b})`;
        ctx.fillRect(pad.l, y, barW, barHeight);

        // Label (left)
        ctx.fillStyle = '#e4e4e7';
        ctx.font = '12px -apple-system, BlinkMacSystemFont, sans-serif';
        ctx.textAlign = 'right';
        ctx.fillText(label, pad.l - 12, y + barHeight / 2 + 4);

        // Value (right of bar) - show time in readable format
        ctx.fillStyle = '#a1a1aa';
        ctx.textAlign = 'left';
        const timeStr = time >= 1000 ? (time / 1000).toFixed(1) + 's' : time.toFixed(0) + 'ms';
        ctx.fillText(timeStr, pad.l + barW + 10, y + barHeight / 2 + 4);
    });
}

// Utilities
const fmt = n => n >= 1e6 ? (n/1e6).toFixed(1)+'M' : n >= 1e3 ? (n/1e3).toFixed(1)+'K' : n.toFixed(0);
const fmtBlock = n => n.toLocaleString();  // Full block numbers with commas
const fmtDur = s => s < 60 ? s.toFixed(1)+'s' : s < 3600 ? (s/60).toFixed(1)+'m' : (s/3600).toFixed(1)+'h';
const fmtLat = us => us >= 1000 ? (us/1000).toFixed(1)+'ms' : us.toFixed(0)+'μs';

// Tab switching
document.querySelectorAll('.tab').forEach(tab => {
    tab.addEventListener('click', () => {
        document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
        document.querySelectorAll('.panel').forEach(p => p.classList.remove('active'));
        tab.classList.add('active');
        document.getElementById(tab.dataset.tab).classList.add('active');
        initTab(tab.dataset.tab);
        // Resize charts after tab switch (container may have been hidden)
        setTimeout(() => {
            uplotInstances.forEach(({ uplot, el, height }) => {
                const rect = el.getBoundingClientRect();
                if (rect.width > 0) {
                    uplot.setSize({ width: rect.width, height: height });
                }
            });
        }, 50);
    });
});

document.getElementById('export-compact-btn').addEventListener('click', () => {
    // Build compact export (removes large arrays, keeps key metrics)
    // Helper to get measured depth for a table from cursor stats
    const getTableDepthStats = (tableName) => {
        const tableStats = DATA.cursor_data?.table_stats?.find(t => t.name === tableName);
        if (!tableStats) return null;
        // If we have per-table depth tracking in the future, add it here
        return null;
    };

    const compact = {
        _meta: {
            export_version: 2,
            description: "MDBX profiler trace - optimized for LLM analysis",
            generated_at: new Date().toISOString()
        },
        analysis_summary: {
            top_bottleneck_tables: DATA.unified_tables.slice(0, 5).map(t => t.name),
            total_page_faults: DATA.summary.page_faults,
            major_fault_pct: DATA.summary.major_fault_ratio,
            tree_depth: DATA.cursor_data?.summary?.tree_depth_stats?.ops_with_depth_data > 0 ? {
                max_observed: DATA.cursor_data.summary.tree_depth_stats.max_depth_observed,
                avg: DATA.cursor_data.summary.tree_depth_stats.avg_depth,
                ops_measured: DATA.cursor_data.summary.tree_depth_stats.ops_with_depth_data
            } : null,
            branch_leaf_ratio: DATA.page_type_stats?.traversal_to_data_ratio || null,
            io_bound: DATA.summary.major_fault_ratio > 50,
            high_traversal_overhead: (DATA.page_type_stats?.traversal_to_data_ratio || 0) > 1.0
        },
        trace: {
            duration_secs: DATA.summary.duration_secs,
            total_events: DATA.summary.total_events,
            file_size_gb: DATA.summary.file_size_gb,
            block_range: DATA.summary.block_range
        },
        page_faults: {
            total: DATA.summary.page_faults,
            major: DATA.summary.major_faults,
            minor: DATA.summary.minor_faults,
            major_ratio: DATA.summary.major_fault_ratio,
            rate_per_sec: DATA.summary.fault_rate_per_sec,
            unique_pages: DATA.summary.unique_pages,
            sequential_ratio: DATA.patterns.sequential_ratio,
            random_ratio: DATA.patterns.random_ratio,
            top_strides: DATA.patterns.top_strides
        },
        tables: DATA.unified_tables.map(t => {
            // Find tree traversal stats for this table
            const treeStats = DATA.tree_traversal?.tables?.find(tt => tt.name === t.name);
            return {
                name: t.name,
                faults: t.faults,
                major_faults: t.major_faults,
                fault_pct: t.fault_percentage,
                slow_ops: t.slow_ops,
                time_lost_ms: t.time_lost_ms,
                top_op: t.top_operation,
                faults_by_op: t.details.faults_by_op,
                // B+ tree traversal stats
                branch_faults: treeStats?.branch_faults || 0,
                leaf_faults: treeStats?.leaf_faults || 0,
                branch_leaf_ratio: treeStats?.branch_leaf_ratio || null
            };
        }),
        threads: DATA.threads.slice(0, 10),
        cursor_ops: DATA.cursor_data.has_data ? {
            total: DATA.cursor_data.summary.total_ops,
            rate_per_sec: DATA.cursor_data.summary.op_rate_per_sec,
            seek_ratio: DATA.cursor_data.summary.seek_ratio,
            latency_avg_us: DATA.cursor_data.summary.avg_latency_us,
            latency_p95_us: DATA.cursor_data.summary.p95_latency_us,
            latency_p99_us: DATA.cursor_data.summary.p99_latency_us,
            by_operation: DATA.cursor_data.operations.slice(0, 10),
            by_table: DATA.cursor_data.table_stats.slice(0, 15).map(t => ({
                name: t.name,
                ops: t.ops,
                pct: t.percentage,
                avg_latency_us: t.avg_latency_us,
                p95_latency_us: t.p95_latency_us
            })),
            slow_tables: DATA.cursor_data.slow_ops_by_table.slice(0, 10).map(s => ({
                table: s.table,
                slow_ops: s.slow_op_count,
                time_lost_ms: s.total_slow_time_ms,
                by_op: s.by_operation.slice(0, 5)
            })),
            slow_keys: DATA.cursor_data.slow_keys.slice(0, 20),
            tree_depth_stats: DATA.cursor_data.summary.tree_depth_stats ? {
                ops_with_depth_data: DATA.cursor_data.summary.tree_depth_stats.ops_with_depth_data,
                max_depth_observed: DATA.cursor_data.summary.tree_depth_stats.max_depth_observed,
                avg_depth: DATA.cursor_data.summary.tree_depth_stats.avg_depth,
                depth_histogram: DATA.cursor_data.summary.tree_depth_stats.depth_histogram,
                // Per-table depth stats (sorted by avg depth descending)
                by_table: DATA.cursor_data.summary.tree_depth_stats.by_table?.slice(0, 20).map(t => ({
                    table: t.table_name,
                    ops: t.ops_count,
                    max_depth: t.max_depth,
                    avg_depth: t.avg_depth,
                    avg_faults: t.avg_faults,
                    avg_latency_us: t.avg_latency_us
                })) || [],
                // Per-operation depth stats (sorted by avg depth descending)
                by_operation: DATA.cursor_data.summary.tree_depth_stats.by_operation?.map(op => ({
                    operation: op.operation,
                    is_seek: op.is_seek,
                    ops: op.ops_count,
                    max_depth: op.max_depth,
                    avg_depth: op.avg_depth,
                    avg_faults: op.avg_faults
                })) || []
            } : null
        } : null,
        transactions: DATA.txn_data.has_data ? {
            total: DATA.txn_data.summary.begin_count,
            rate_per_sec: DATA.txn_data.summary.txn_rate_per_sec,
            ro_count: DATA.txn_data.summary.ro_count,
            rw_count: DATA.txn_data.summary.rw_count,
            commits: DATA.txn_data.summary.commit_count,
            aborts: DATA.txn_data.summary.abort_count,
            commit_latency_avg_us: DATA.txn_data.summary.avg_commit_latency_us,
            commit_latency_p95_us: DATA.txn_data.summary.p95_commit_latency_us,
            commit_latency_p99_us: DATA.txn_data.summary.p99_commit_latency_us,
            commit_latency_max_us: DATA.txn_data.summary.max_commit_latency_us,
            concurrency: {
                max_ro: DATA.txn_data.concurrency.max_concurrent_ro,
                max_rw: DATA.txn_data.concurrency.max_concurrent_rw,
                avg_ro: DATA.txn_data.concurrency.avg_concurrent_ro
            },
            top_threads: DATA.txn_data.thread_stats.slice(0, 10)
        } : null,
        attribution: DATA.direct_fault_attribution.has_data ? {
            directly_attributed: DATA.direct_fault_attribution.directly_attributed_count,
            timestamp_fallback: DATA.direct_fault_attribution.timestamp_fallback_count,
            uncorrelated: DATA.direct_fault_attribution.uncorrelated_count,
            by_op_type: DATA.direct_fault_attribution.faults_by_op_type,
            by_cursor_op: DATA.direct_fault_attribution.faults_by_cursor_op
        } : null,
        // B+ Tree Visualization data (BTREE_VIZ_PLAN)
        btree: DATA.btree_viz && DATA.btree_viz.has_data ? {
            traversal_efficiency_score: DATA.btree_viz.traversal_efficiency_score,
            tree_depth_estimates: DATA.btree_viz.tree_depth_estimates,
            operation_page_types: DATA.btree_viz.operation_page_types.slice(0, 10),
            attribution_stats: DATA.btree_viz.attribution_stats ? {
                total_faults: DATA.btree_viz.attribution_stats.total_faults,
                batch_attributed_faults: DATA.btree_viz.attribution_stats.batch_attributed_faults,
                block_attributed_faults: DATA.btree_viz.attribution_stats.block_attributed_faults,
                unattributed_faults: DATA.btree_viz.attribution_stats.unattributed_faults,
                batch_attribution_pct: DATA.btree_viz.attribution_stats.batch_attribution_pct,
                block_attribution_pct: DATA.btree_viz.attribution_stats.block_attribution_pct,
                rw_commits_detected: DATA.btree_viz.attribution_stats.rw_commits_detected,
                blocks_with_writes: DATA.btree_viz.attribution_stats.blocks_with_writes
            } : null,
            batch_analysis: DATA.btree_viz.batch_analysis ? DATA.btree_viz.batch_analysis.map(b => ({
                batch_index: b.batch_index,
                first_block: b.first_block,
                last_block: b.last_block,
                block_count: b.block_count,
                total_faults: b.total_faults,
                branch_faults: b.branch_faults,
                leaf_faults: b.leaf_faults,
                major_faults: b.major_faults,
                io_time_us: b.io_time_us,
                tables_touched: b.tables_touched,
                start_time_ns: b.start_time_ns,
                end_time_ns: b.end_time_ns,
                commit_latency_us: b.commit_latency_us
            })) : [],
            block_analysis: DATA.btree_viz.block_analysis.slice(0, 20).map(b => ({
                block: b.block_number,
                total: b.total_faults,
                branch: b.branch_faults,
                leaf: b.leaf_faults,
                major: b.major_faults,
                io_time_us: b.io_time_us,
                tables: b.tables_touched
            })),
            tree_traversal: DATA.tree_traversal && DATA.tree_traversal.has_data ?
                DATA.tree_traversal.tables.slice(0, 15).map(t => ({
                    name: t.name,
                    total_faults: t.total_faults,
                    branch_faults: t.branch_faults,
                    leaf_faults: t.leaf_faults,
                    branch_leaf_ratio: t.branch_leaf_ratio
                })) : null
        } : null,
        page_types: DATA.page_type_stats && DATA.page_type_stats.has_data ? {
            total_faults: DATA.page_type_stats.total_faults,
            traversal_to_data_ratio: DATA.page_type_stats.traversal_to_data_ratio,
            by_type: DATA.page_type_stats.by_type
        } : null
    };

    const json = JSON.stringify(compact, null, 2);
    const blob = new Blob([json], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = 'mdbx-trace.json';
    a.click();
});

// Tab initialization
const initialized = {};

function initTab(name) {
    if (initialized[name]) return;
    initialized[name] = true;

    if (name === 'overview') initOverview();
    else if (name === 'tables') initTables();
    else if (name === 'resources') initResources();
}

function initOverview() {
    const s = DATA.summary;

    // Metrics
    document.getElementById('duration').textContent = fmtDur(s.duration_secs);
    document.getElementById('total-faults').textContent = fmt(s.page_faults);
    document.getElementById('major-faults').textContent = fmt(s.major_faults);
    document.getElementById('minor-faults').textContent = fmt(s.minor_faults);
    document.getElementById('fault-rate').textContent = fmt(s.fault_rate_per_sec);
    document.getElementById('major-ratio').textContent = (s.major_fault_ratio * 100).toFixed(1) + '%';

    // Block range - show full block numbers
    if (s.block_range) {
        document.getElementById('block-range-metric').style.display = 'block';
        document.getElementById('block-range').textContent =
            fmtBlock(s.block_range.min_block) + ' → ' + fmtBlock(s.block_range.max_block);
    }

    // Timeline (uPlot interactive)
    if (DATA.timeline.length) {
        createUPlotChart('timeline-chart', DATA.timeline.map(t => t.faults), {
            durationSecs: s.duration_secs,
            color: '#3b82f6',
            label: 'Faults'
        });
    }

    // Plotly Heatmap with built-in zoom/pan/tooltips
    if (DATA.heatmap.data.length) {
        const container = document.getElementById('heatmap-plotly');
        initPlotlyHeatmap(container, DATA.heatmap);
    }

    // Access patterns
    const seqPct = (DATA.patterns.sequential_ratio * 100);
    const randPct = (DATA.patterns.random_ratio * 100);
    document.getElementById('seq-bar').style.width = seqPct + '%';
    document.getElementById('rand-bar').style.width = randPct + '%';
    document.getElementById('seq-ratio').textContent = seqPct.toFixed(1) + '%';
    document.getElementById('rand-ratio').textContent = randPct.toFixed(1) + '%';

    // Stride patterns
    const strideList = document.getElementById('stride-list');
    if (DATA.patterns.top_strides && DATA.patterns.top_strides.length > 0) {
        DATA.patterns.top_strides.slice(0, 5).forEach(stride => {
            const div = document.createElement('div');
            div.className = 'stride-item';
            div.innerHTML = `
                <span><span class="stride-type">${stride.pattern_type}</span> (${stride.stride_pages} pages)</span>
                <span class="stride-count">${fmt(stride.count)} (${stride.percentage.toFixed(1)}%)</span>
            `;
            strideList.appendChild(div);
        });
    } else {
        document.getElementById('stride-section').style.display = 'none';
    }

    // Page type donut in overview
    const pt = DATA.page_type_stats;
    if (pt && pt.has_data && pt.by_type && pt.by_type.length > 0) {
        const canvas = document.getElementById('overview-page-type-donut');
        const ctx = canvas.getContext('2d');
        const colors = {
            'Branch': '#f59e0b',
            'Leaf': '#22c55e',
            'Overflow': '#8b5cf6',
            'Meta': '#6366f1',
            'Unknown': '#71717a'
        };

        // Sort by total_faults descending (highest first)
        const sortedTypes = [...pt.by_type].sort((a, b) => b.total_faults - a.total_faults);

        const total = sortedTypes.reduce((sum, t) => sum + t.total_faults, 0);
        let startAngle = -Math.PI / 2;
        const cx = canvas.width / 2;
        const cy = canvas.height / 2;
        const outerR = Math.min(cx, cy) - 10;
        const innerR = outerR * 0.6;

        sortedTypes.forEach(t => {
            if (t.total_faults === 0) return;
            const sliceAngle = (t.total_faults / total) * Math.PI * 2;
            ctx.beginPath();
            ctx.arc(cx, cy, outerR, startAngle, startAngle + sliceAngle);
            ctx.arc(cx, cy, innerR, startAngle + sliceAngle, startAngle, true);
            ctx.closePath();
            ctx.fillStyle = colors[t.page_type] || colors['Unknown'];
            ctx.fill();
            startAngle += sliceAngle;
        });

        // Legend (sorted by total_faults descending)
        const legend = document.getElementById('overview-page-type-legend');
        sortedTypes.filter(t => t.total_faults > 0).forEach(t => {
            const row = document.createElement('div');
            row.className = 'legend-row';
            row.innerHTML = `
                <span class="legend-dot" style="background: ${colors[t.page_type] || colors['Unknown']}"></span>
                <span class="legend-name">${t.page_type}</span>
                <span class="legend-value">${fmt(t.total_faults)}</span>
                <span class="legend-pct">${t.percentage.toFixed(1)}%</span>
            `;
            legend.appendChild(row);
        });
    }
}

// Reth table source links for navigation
const RETH_TABLE_SOURCES = {
    'CanonicalHeaders': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'HeaderTerminalDifficulties': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'HeaderNumbers': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'Headers': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'BlockBodyIndices': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'BlockOmmers': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'BlockWithdrawals': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'Transactions': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'TransactionSenders': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'TransactionHashNumbers': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'TransactionBlocks': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'Receipts': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'PlainStorageState': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'PlainAccountState': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'Bytecodes': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'AccountChangeSets': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'StorageChangeSets': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'HashedAccounts': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'HashedStorages': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'AccountsTrie': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'StoragesTrie': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'SyncStage': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'SyncStageProgress': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'PruneCheckpoints': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'StageCheckpoints': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
    'StageCheckpointProgresses': { github_url: 'https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/models/mod.rs' },
};

function initResources() {
    // Initialize Memory section
    initResourcesMemory();

    // Initialize Transactions section
    initResourcesTxns();

    // Initialize Threads section
    initResourcesThreads();
}

function initResourcesMemory() {
    const ws = DATA.working_set;

    if (!ws || !ws.has_data) {
        document.getElementById('memory-no-data').style.display = 'block';
        document.getElementById('memory-content').style.display = 'none';
        return;
    }

    // Summary banner
    document.getElementById('memory-summary-text').textContent = ws.summary_text;

    // Key metrics
    document.getElementById('mem-unique-pages').textContent = fmt(ws.total_unique_pages);
    const workingSetGB = (ws.total_unique_pages * 4096 / 1e9).toFixed(2);
    document.getElementById('mem-working-set').textContent = workingSetGB + ' GB';
    document.getElementById('mem-reuse-ratio').textContent = (ws.reuse_ratio * 100).toFixed(1) + '%';
    document.getElementById('mem-avg-accesses').textContent = ws.avg_accesses_per_page.toFixed(2);

    // Access count distribution (how many pages have 1x, 2x, etc. accesses)
    if (ws.access_count_distribution && ws.access_count_distribution.length > 0) {
        const chartEl = document.getElementById('access-count-chart');
        let html = '<div class="bar-chart-container">';

        const maxPct = Math.max(...ws.access_count_distribution.map(b => b.percentage));
        ws.access_count_distribution.forEach(bucket => {
            const barWidth = maxPct > 0 ? (bucket.percentage / maxPct * 100) : 0;
            html += `
                <div class="bar-chart-row">
                    <span class="bar-chart-label">${bucket.label}</span>
                    <div class="bar-chart-bar-container">
                        <div class="bar-chart-bar access-count" style="width: ${barWidth}%"></div>
                    </div>
                    <span class="bar-chart-value">${bucket.percentage.toFixed(1)}%</span>
                </div>
            `;
        });
        html += '</div>';
        chartEl.innerHTML = html;
    }

}

function initTables() {
    const unified = DATA.unified_tables;
    const dfa = DATA.direct_fault_attribution;

    if (!unified || unified.length === 0) {
        document.getElementById('tables-no-data').style.display = 'block';
        document.getElementById('tables-content').style.display = 'none';
        return;
    }

    // Populate summary cards
    const totalFaults = unified.reduce((sum, t) => sum + t.faults, 0);
    const totalIO = unified.reduce((sum, t) => sum + t.time_lost_ms, 0);
    const hottest = unified[0];

    document.getElementById('tables-total-faults').textContent = fmt(totalFaults);
    document.getElementById('tables-io-time').textContent = totalIO >= 1000
        ? (totalIO / 1000).toFixed(1) + 's'
        : totalIO.toFixed(0) + 'ms';
    document.getElementById('tables-hottest').textContent = hottest ? hottest.name : '-';
    document.getElementById('tables-count').textContent = unified.length;

    // Attribution summary
    if (dfa && dfa.has_data) {
        const total = dfa.directly_attributed_count + dfa.timestamp_fallback_count + dfa.uncorrelated_count;
        const directPct = (dfa.directly_attributed_count / total * 100).toFixed(1);
        document.getElementById('tables-attribution-summary').innerHTML =
            `<strong>${fmt(dfa.directly_attributed_count)}</strong> directly attributed (${directPct}%) · ` +
            `${fmt(dfa.uncorrelated_count)} uncorrelated`;
    }

    // CPU Profile summary
    const cpu = DATA.cpu_profile;
    if (cpu && cpu.has_data) {
        document.getElementById('cpu-profile-banner').style.display = 'flex';

        const bottleneckEl = document.getElementById('cpu-bottleneck');
        bottleneckEl.textContent = cpu.bottleneck;
        bottleneckEl.className = 'cpu-profile-bottleneck' +
            (cpu.cpu_efficiency < 0.5 ? ' io-bound' : (cpu.cpu_efficiency > 0.8 ? ' cpu-bound' : ''));

        const fmtTime = (ms) => ms >= 1000 ? (ms / 1000).toFixed(1) + 's' : ms.toFixed(0) + 'ms';
        document.getElementById('cpu-profile-detail').textContent =
            `CPU: ${fmtTime(cpu.total_cpu_time_ms)} · I/O Wait: ${fmtTime(cpu.total_io_wait_ms)} · Efficiency: ${(cpu.cpu_efficiency * 100).toFixed(1)}%`;
    }

    // Draw fault distribution and I/O time charts
    if (unified.length > 0) {
        document.getElementById('fault-dist-row').style.display = 'grid';

        // Defer chart drawing to ensure panel is visible and has dimensions
        // Use requestAnimationFrame to wait for layout, then draw
        requestAnimationFrame(() => {
            requestAnimationFrame(() => {
                // Fault distribution chart (top 8 by faults, sorted descending)
                const faultCanvas = document.getElementById('fault-dist-chart');
                if (faultCanvas) {
                    const topByFaults = [...unified]
                        .sort((a, b) => b.faults - a.faults)
                        .slice(0, 8);
                    drawFaultDistChart(faultCanvas,
                        topByFaults.map(t => t.name),
                        topByFaults.map(t => t.faults),
                        topByFaults.map(t => t.major_faults)
                    );
                }

                // I/O time chart (top 8 by time lost, filtered to those with actual I/O time)
                const ioCanvas = document.getElementById('io-time-chart');
                if (ioCanvas) {
                    const topByIO = unified.filter(t => t.time_lost_ms > 0)
                        .sort((a, b) => b.time_lost_ms - a.time_lost_ms)
                        .slice(0, 8);
                    if (topByIO.length > 0) {
                        drawIOTimeChart(ioCanvas,
                            topByIO.map(t => t.name),
                            topByIO.map(t => t.time_lost_ms),
                            topByIO.map(t => t.slow_ops)
                        );
                    }
                }
            });
        });
    }

    // Build unified table with expandable rows
    const tbody = document.querySelector('#unified-tables tbody');

    unified.forEach((t, idx) => {
        // Main row
        const tr = document.createElement('tr');
        tr.className = 'table-row';
        tr.dataset.idx = idx;

        const ioTime = t.time_lost_ms >= 1000
            ? (t.time_lost_ms / 1000).toFixed(1) + 's'
            : t.time_lost_ms.toFixed(0) + 'ms';

        const majorPct = t.faults > 0 ? (t.major_faults / t.faults * 100).toFixed(1) : '0.0';

        // Get B:L ratio from tree_traversal data
        const treeTable = DATA.tree_traversal?.tables?.find(tt => tt.name === t.name);
        const blRatio = treeTable ? treeTable.branch_leaf_ratio.toFixed(2) : '-';

        // Get working set from working_set data
        const wsTable = DATA.working_set?.per_table?.find(wt => wt.name === t.name);
        const workingSet = wsTable ? fmt(wsTable.unique_pages) : '-';
        const reusePct = wsTable ? (wsTable.reuse_ratio * 100).toFixed(1) + '%' : '-';

        // CPU efficiency: higher = more CPU bound, lower = more I/O bound
        const cpuPct = (t.cpu_efficiency * 100).toFixed(1);
        const cpuClass = t.is_io_bound ? 'io-bound' : (t.cpu_efficiency > 0.8 ? 'cpu-bound' : '');

        tr.innerHTML = `
            <td><span class="expand-icon">▶</span></td>
            <td>${t.name}</td>
            <td>${fmt(t.faults)}</td>
            <td>${majorPct}%</td>
            <td>${blRatio}</td>
            <td>${workingSet}</td>
            <td>${reusePct}</td>
            <td class="io-time">${t.time_lost_ms > 0 ? ioTime : '-'}</td>
            <td class="${cpuClass}">${t.total_wall_time_ms > 0 ? cpuPct + '%' : '-'}</td>
        `;
        tbody.appendChild(tr);

        // Details row (hidden by default)
        const detailsTr = document.createElement('tr');
        detailsTr.className = 'details-row hidden';
        detailsTr.dataset.idx = idx;

        let detailsHtml = '<td colspan="9"><div class="details-content">';

        // Top 5 Operations by Faults - Primary section
        detailsHtml += '<div class="details-section"><div class="details-section-title">Top Operations by Page Faults</div><div class="details-list">';
        if (t.details.faults_by_op && t.details.faults_by_op.length > 0) {
            t.details.faults_by_op.slice(0, 5).forEach((op, i) => {
                const opPct = t.faults > 0 ? (op.faults / t.faults * 100).toFixed(1) : 0;
                const majorPct = op.faults > 0 ? (op.major_faults / op.faults * 100).toFixed(0) : 0;
                detailsHtml += `<div class="details-item" style="padding: 6px 0;">
                    <span class="details-item-name" style="font-weight: 500;">${i + 1}. ${op.operation}</span>
                    <span class="details-item-value">${fmt(op.faults)} (${opPct}%) <span class="major" style="font-size: 11px;">${majorPct}% major</span></span>
                </div>`;
            });
        } else {
            detailsHtml += '<div class="details-item"><span class="details-item-name" style="color:#52525b;">No operations traced</span></div>';
        }
        detailsHtml += '</div></div>';

        // Hot Keys
        detailsHtml += '<div class="details-section"><div class="details-section-title">Slow Keys</div><div class="details-list">';
        if (t.details.hot_keys && t.details.hot_keys.length > 0) {
            t.details.hot_keys.slice(0, 5).forEach(key => {
                const keyShort = key.key_hex.length > 24 ? key.key_hex.substring(0, 22) + '...' : key.key_hex;
                const avgMs = (key.avg_latency_us / 1000).toFixed(1);
                detailsHtml += `<div class="details-item">
                    <span class="details-item-name" style="font-family:monospace;font-size:11px;">${keyShort}</span>
                    <span class="details-item-value">${fmt(key.slow_count)} slow, ${avgMs}ms</span>
                </div>`;
            });
        } else {
            detailsHtml += '<div class="details-item"><span class="details-item-name" style="color:#52525b;">No slow keys</span></div>';
        }
        detailsHtml += '</div></div>';

        detailsHtml += '</div></td>';
        detailsTr.innerHTML = detailsHtml;
        tbody.appendChild(detailsTr);

        // Click handler to expand/collapse
        tr.addEventListener('click', () => {
            const isExpanded = tr.classList.contains('expanded');

            // Collapse all other rows
            document.querySelectorAll('#unified-tables .table-row.expanded').forEach(r => {
                r.classList.remove('expanded');
            });
            document.querySelectorAll('#unified-tables .details-row').forEach(r => {
                r.classList.add('hidden');
            });

            // Toggle this row
            if (!isExpanded) {
                tr.classList.add('expanded');
                detailsTr.classList.remove('hidden');
            }
        });
    });
}

// ============================================
// B+ TREE TAB (Enhanced - BTREE_VIZ_PLAN)
// ============================================

function initBTree() {
    const pt = DATA.page_type_stats;
    const tt = DATA.tree_traversal;
    const bv = DATA.btree_viz;

    // Check if we have data
    if (!pt || !pt.has_data) {
        document.getElementById('btree-no-data').style.display = 'block';
        document.getElementById('btree-content').style.display = 'none';
        return;
    }

    document.getElementById('btree-no-data').style.display = 'none';
    document.getElementById('btree-content').style.display = 'block';

    // Update metrics row
    const branchFaults = pt.by_type.find(t => t.page_type === 'Branch');
    const leafFaults = pt.by_type.find(t => t.page_type === 'Leaf');
    document.getElementById('branch-faults').textContent = fmt(branchFaults ? branchFaults.total_faults : 0);
    document.getElementById('leaf-faults').textContent = fmt(leafFaults ? leafFaults.total_faults : 0);
    document.getElementById('traversal-ratio').textContent = pt.traversal_to_data_ratio.toFixed(2);
    document.getElementById('page-type-total').textContent = fmt(pt.total_faults);

    // Traversal efficiency score (from btree_viz)
    if (bv && bv.has_data) {
        const effEl = document.getElementById('traversal-efficiency');
        const eff = bv.traversal_efficiency_score;
        effEl.textContent = eff.toFixed(0) + '%';
        effEl.className = 'metric-value ' + (eff >= 70 ? 'efficiency-high' : eff >= 40 ? 'efficiency-medium' : 'efficiency-low');
    }

    // Measured tree depth stats (from BPF per-operation tracking)
    const depthStats = DATA.cursor_data?.summary?.tree_depth_stats;
    if (depthStats && depthStats.ops_with_depth_data > 0) {
        document.getElementById('measured-depth-section').style.display = 'block';
        document.getElementById('measured-max-depth').textContent = depthStats.max_depth_observed;
        document.getElementById('measured-avg-depth').textContent = depthStats.avg_depth.toFixed(2);
        document.getElementById('measured-ops-count').textContent = fmt(depthStats.ops_with_depth_data);

        // Draw depth histogram chart
        if (depthStats.depth_histogram && depthStats.depth_histogram.length > 0) {
            drawDepthHistogram(depthStats.depth_histogram);
            fillDepthTable(depthStats.depth_histogram);
        }

        // Fill per-table depth stats
        if (depthStats.by_table && depthStats.by_table.length > 0) {
            fillDepthByTable(depthStats.by_table);
        }

        // Fill per-operation depth stats
        if (depthStats.by_operation && depthStats.by_operation.length > 0) {
            fillDepthByOperation(depthStats.by_operation);
        }
    }

    // Page type donut chart
    drawPageTypeDonut(pt);

    // Legend
    const legendEl = document.getElementById('page-type-legend');
    legendEl.innerHTML = pt.by_type.map(t => {
        const cssClass = t.page_type.toLowerCase();
        return `
            <div class="legend-row">
                <span class="legend-dot ${cssClass}"></span>
                <span class="legend-name">${t.page_type}</span>
                <span class="legend-value">${fmt(t.total_faults)}</span>
                <span class="legend-pct">(${t.percentage.toFixed(1)}%)</span>
            </div>
        `;
    }).join('');

    // Phase 2: Tree structure visualization
    if (tt && tt.has_data && tt.tables.length) {
        drawTreeStructureCards(tt, bv);
    }

    // Phase 3: Operation to page type chart
    if (bv && bv.has_data && bv.operation_page_types.length) {
        drawOperationPageTypeChart(bv.operation_page_types);
    }

    // Phase 4: Per-batch analysis (more accurate than per-block)
    if (bv && bv.has_data && bv.batch_analysis && bv.batch_analysis.length) {
        document.getElementById('block-analysis-section').style.display = 'block';
        drawBatchAnalysis(bv.batch_analysis);

        // Show attribution stats if available
        if (bv.attribution_stats) {
            const stats = bv.attribution_stats;
            document.getElementById('batch-attribution-stats').style.display = 'block';
            document.getElementById('batch-attribution-pct').textContent = stats.batch_attribution_pct.toFixed(1) + '%';
            document.getElementById('batch-rw-commits').textContent = stats.rw_commits_detected;
            document.getElementById('batch-blocks-count').textContent = stats.blocks_with_writes;
            document.getElementById('batch-attributed-faults').textContent = fmt(stats.batch_attributed_faults);
            document.getElementById('batch-unattributed-faults').textContent = fmt(stats.unattributed_faults);

            // Color code the attribution percentage
            const pctEl = document.getElementById('batch-attribution-pct');
            if (stats.batch_attribution_pct >= 90) {
                pctEl.style.color = '#22c55e'; // green
            } else if (stats.batch_attribution_pct >= 70) {
                pctEl.style.color = '#3b82f6'; // blue
            } else if (stats.batch_attribution_pct >= 50) {
                pctEl.style.color = '#f59e0b'; // amber
            } else {
                pctEl.style.color = '#ef4444'; // red
            }
        }
    } else if (bv && bv.has_data && bv.block_analysis && bv.block_analysis.length) {
        // Fallback to block analysis if no batch data
        document.getElementById('block-analysis-section').style.display = 'block';
        drawBlockAnalysis(bv.block_analysis);
    }

    // Phase 5: Comparative table view
    if (tt && tt.has_data && tt.tables.length) {
        drawTableComparison(tt, bv);
        initTableComparisonSort(tt, bv);
    }
}

// Phase 2: Draw mini tree structure cards for each table
function drawTreeStructureCards(tt, bv) {
    const container = document.getElementById('tree-structure-container');
    if (!container) return;

    // Get depth estimates map
    const depthMap = {};
    if (bv && bv.tree_depth_estimates) {
        bv.tree_depth_estimates.forEach(d => {
            depthMap[d.table_name] = d;
        });
    }

    // Take top 10 tables by faults
    const tables = tt.tables.slice(0, 10);

    container.innerHTML = tables.map(table => {
        const depth = depthMap[table.name];
        const estDepth = depth ? depth.estimated_depth.toFixed(1) : '?';

        return `
            <div class="tree-card" data-table="${table.name}">
                <div class="tree-card-header" title="${table.name}">${table.name}</div>
                <canvas class="tree-diagram" data-branch="${table.branch_faults}" data-leaf="${table.leaf_faults}" data-ratio="${table.branch_leaf_ratio}" width="156" height="80"></canvas>
                <div class="tree-card-stats">
                    <div class="tree-card-stat">
                        <div class="tree-card-stat-value">${table.branch_leaf_ratio.toFixed(2)}</div>
                        <div class="tree-card-stat-label">B:L</div>
                    </div>
                    <div class="tree-card-stat">
                        <div class="tree-card-stat-value">~${estDepth}</div>
                        <div class="tree-card-stat-label">Depth</div>
                    </div>
                    <div class="tree-card-stat">
                        <div class="tree-card-stat-value">${fmtK(table.total_faults)}</div>
                        <div class="tree-card-stat-label">Faults</div>
                    </div>
                </div>
            </div>
        `;
    }).join('');

    // Draw mini tree diagrams on each canvas
    container.querySelectorAll('.tree-diagram').forEach(canvas => {
        drawMiniTree(canvas);
    });

    // Add tooltips
    addTreeCardTooltips(container, tt, depthMap);
}

// Draw a mini B+ tree diagram on canvas
function drawMiniTree(canvas) {
    const ctx = canvas.getContext('2d');
    const dpr = window.devicePixelRatio || 1;
    const w = 156;
    const h = 80;

    canvas.width = w * dpr;
    canvas.height = h * dpr;
    canvas.style.width = w + 'px';
    canvas.style.height = h + 'px';
    ctx.scale(dpr, dpr);

    const ratio = parseFloat(canvas.dataset.ratio) || 0;

    // Estimate levels from ratio (1-4 levels)
    const levels = Math.min(4, Math.max(1, Math.round(1 + ratio * 2)));

    const branchColor = '#f59e0b';
    const leafColor = '#22c55e';
    const lineColor = '#3f3f46';

    ctx.clearRect(0, 0, w, h);

    // Draw tree structure based on levels
    const nodeRadius = 6;
    const levelHeight = (h - 20) / levels;

    for (let level = 0; level < levels; level++) {
        const y = 10 + level * levelHeight;
        const isLeaf = level === levels - 1;
        const nodesAtLevel = Math.pow(2, level);
        const spacing = w / (nodesAtLevel + 1);

        ctx.fillStyle = isLeaf ? leafColor : branchColor;

        for (let i = 0; i < nodesAtLevel; i++) {
            const x = spacing * (i + 1);

            // Draw connecting lines to children
            if (!isLeaf && level < levels - 1) {
                const childSpacing = w / (nodesAtLevel * 2 + 1);
                const childY = y + levelHeight;
                ctx.strokeStyle = lineColor;
                ctx.lineWidth = 1;
                ctx.beginPath();
                ctx.moveTo(x, y + nodeRadius);
                ctx.lineTo(childSpacing * (i * 2 + 1), childY - nodeRadius);
                ctx.stroke();
                ctx.beginPath();
                ctx.moveTo(x, y + nodeRadius);
                ctx.lineTo(childSpacing * (i * 2 + 2), childY - nodeRadius);
                ctx.stroke();
            }

            // Draw node
            ctx.beginPath();
            ctx.arc(x, y, nodeRadius, 0, Math.PI * 2);
            ctx.fill();
        }
    }
}

// Add hover tooltips to tree cards
function addTreeCardTooltips(container, tt, depthMap) {
    let tooltip = document.querySelector('.tree-tooltip');
    if (!tooltip) {
        tooltip = document.createElement('div');
        tooltip.className = 'tree-tooltip';
        tooltip.style.display = 'none';
        document.body.appendChild(tooltip);
    }

    container.querySelectorAll('.tree-card').forEach(card => {
        const tableName = card.dataset.table;
        const table = tt.tables.find(t => t.name === tableName);
        const depth = depthMap[tableName];

        card.addEventListener('mouseenter', (e) => {
            if (!table) return;

            const majorPct = table.total_faults > 0 ? ((table.branch_faults + table.leaf_faults) > 0 ? '~' : '0') : '0';

            tooltip.innerHTML = `
                <div class="tree-tooltip-header">${table.name}</div>
                <div class="tree-tooltip-row">
                    <span class="tree-tooltip-label">Branch Faults:</span>
                    <span class="tree-tooltip-value" style="color: #f59e0b">${fmt(table.branch_faults)}</span>
                </div>
                <div class="tree-tooltip-row">
                    <span class="tree-tooltip-label">Leaf Faults:</span>
                    <span class="tree-tooltip-value" style="color: #22c55e">${fmt(table.leaf_faults)}</span>
                </div>
                <div class="tree-tooltip-row">
                    <span class="tree-tooltip-label">Overflow:</span>
                    <span class="tree-tooltip-value" style="color: #8b5cf6">${fmt(table.overflow_faults)}</span>
                </div>
                <div class="tree-tooltip-row">
                    <span class="tree-tooltip-label">B:L Ratio:</span>
                    <span class="tree-tooltip-value">${table.branch_leaf_ratio.toFixed(3)}</span>
                </div>
                ${depth ? `
                <div class="tree-tooltip-row">
                    <span class="tree-tooltip-label">Est. Depth:</span>
                    <span class="tree-tooltip-value">~${depth.estimated_depth.toFixed(1)} levels (${depth.confidence})</span>
                </div>
                ` : ''}
                <div class="tree-tooltip-row" style="margin-top: 8px; padding-top: 8px; border-top: 1px solid #27272a;">
                    <span class="tree-tooltip-label">Avg traversal:</span>
                    <span class="tree-tooltip-value">${table.branch_leaf_ratio.toFixed(1)} branches/leaf</span>
                </div>
            `;
            tooltip.style.display = 'block';
        });

        card.addEventListener('mousemove', (e) => {
            tooltip.style.left = (e.clientX + 15) + 'px';
            tooltip.style.top = (e.clientY + 15) + 'px';
        });

        card.addEventListener('mouseleave', () => {
            tooltip.style.display = 'none';
        });
    });
}

// Phase 3: Draw operation to page type horizontal bar chart
function drawOperationPageTypeChart(opPageTypes) {
    const canvas = document.getElementById('op-page-type-chart');
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    const dpr = window.devicePixelRatio || 1;

    const containerWidth = canvas.parentElement.offsetWidth - 32;
    const barHeight = 24;
    const labelWidth = 100;
    const padding = 8;
    const numBars = Math.min(opPageTypes.length, 8);
    const h = numBars * (barHeight + padding) + 40;

    canvas.width = containerWidth * dpr;
    canvas.height = h * dpr;
    canvas.style.width = containerWidth + 'px';
    canvas.style.height = h + 'px';
    ctx.scale(dpr, dpr);

    ctx.clearRect(0, 0, containerWidth, h);

    // Find max total for scaling
    const maxTotal = Math.max(...opPageTypes.map(o => o.branch_faults + o.leaf_faults + o.overflow_faults), 1);
    const barMaxWidth = containerWidth - labelWidth - 80;

    // Draw bars
    opPageTypes.slice(0, numBars).forEach((op, i) => {
        const y = i * (barHeight + padding) + 10;
        const total = op.branch_faults + op.leaf_faults + op.overflow_faults;

        // Label
        ctx.fillStyle = '#e4e4e7';
        ctx.font = '12px -apple-system, BlinkMacSystemFont, sans-serif';
        ctx.textAlign = 'left';
        ctx.textBaseline = 'middle';
        ctx.fillText(op.cursor_op, 0, y + barHeight / 2);

        // Stacked bar
        const scale = barMaxWidth / maxTotal;
        let x = labelWidth;

        // Branch segment
        if (op.branch_faults > 0) {
            const segWidth = op.branch_faults * scale;
            ctx.fillStyle = '#f59e0b';
            ctx.fillRect(x, y, segWidth, barHeight);
            x += segWidth;
        }

        // Leaf segment
        if (op.leaf_faults > 0) {
            const segWidth = op.leaf_faults * scale;
            ctx.fillStyle = '#22c55e';
            ctx.fillRect(x, y, segWidth, barHeight);
            x += segWidth;
        }

        // Overflow segment
        if (op.overflow_faults > 0) {
            const segWidth = op.overflow_faults * scale;
            ctx.fillStyle = '#8b5cf6';
            ctx.fillRect(x, y, segWidth, barHeight);
            x += segWidth;
        }

        // Total count label
        ctx.fillStyle = '#71717a';
        ctx.textAlign = 'left';
        ctx.fillText(fmtK(total), x + 8, y + barHeight / 2);
    });

    // Legend at bottom
    const legendY = h - 20;
    ctx.font = '11px -apple-system, BlinkMacSystemFont, sans-serif';

    ctx.fillStyle = '#f59e0b';
    ctx.fillRect(labelWidth, legendY, 12, 12);
    ctx.fillStyle = '#a1a1aa';
    ctx.fillText('Branch', labelWidth + 16, legendY + 9);

    ctx.fillStyle = '#22c55e';
    ctx.fillRect(labelWidth + 80, legendY, 12, 12);
    ctx.fillStyle = '#a1a1aa';
    ctx.fillText('Leaf', labelWidth + 96, legendY + 9);

    ctx.fillStyle = '#8b5cf6';
    ctx.fillRect(labelWidth + 140, legendY, 12, 12);
    ctx.fillStyle = '#a1a1aa';
    ctx.fillText('Overflow', labelWidth + 156, legendY + 9);
}

// Phase 4: Draw batch analysis section (more accurate than block analysis)
function drawBatchAnalysis(batchAnalysis) {
    // Sort by batch index (chronological order)
    const sorted = [...batchAnalysis].sort((a, b) => a.batch_index - b.batch_index);

    // Draw histogram
    drawBatchHistogram(sorted);

    // Populate table
    const tbody = document.querySelector('#block-analysis-table tbody');
    if (!tbody) return;

    tbody.innerHTML = sorted.map(batch => {
        const majorPct = batch.total_faults > 0 ? ((batch.major_faults / batch.total_faults) * 100).toFixed(0) : '0';
        const blockRange = batch.block_count > 0
            ? (batch.first_block === batch.last_block
                ? batch.first_block.toLocaleString()
                : `${batch.first_block.toLocaleString()} - ${batch.last_block.toLocaleString()}`)
            : '-';
        const commitMs = (batch.commit_latency_us / 1000).toFixed(1);

        return `
            <tr>
                <td>#${batch.batch_index + 1}</td>
                <td title="${batch.block_count} blocks">${blockRange}</td>
                <td>${fmt(batch.total_faults)}</td>
                <td style="color: #f59e0b">${fmt(batch.branch_faults)}</td>
                <td style="color: #22c55e">${fmt(batch.leaf_faults)}</td>
                <td>${majorPct}%</td>
                <td>${(batch.io_time_us / 1000).toFixed(1)}ms</td>
                <td>${commitMs}ms</td>
            </tr>
        `;
    }).join('');
}

function drawBatchHistogram(batches) {
    const canvas = document.getElementById('block-histogram-canvas');
    if (!canvas || batches.length === 0) return;

    const ctx = canvas.getContext('2d');
    const dpr = window.devicePixelRatio || 1;
    const container = canvas.parentElement;
    const w = container.offsetWidth;
    const h = 180;

    canvas.width = w * dpr;
    canvas.height = h * dpr;
    canvas.style.width = w + 'px';
    canvas.style.height = h + 'px';
    ctx.scale(dpr, dpr);

    ctx.clearRect(0, 0, w, h);

    const padding = { top: 20, right: 20, bottom: 30, left: 50 };
    const chartW = w - padding.left - padding.right;
    const chartH = h - padding.top - padding.bottom;

    // Find max faults for y-axis
    const maxFaults = Math.max(...batches.map(b => b.total_faults), 1);

    // Draw axes
    ctx.strokeStyle = '#3f3f46';
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(padding.left, padding.top);
    ctx.lineTo(padding.left, h - padding.bottom);
    ctx.lineTo(w - padding.right, h - padding.bottom);
    ctx.stroke();

    // Draw bars
    const barWidth = Math.max(4, (chartW / batches.length) - 2);

    batches.forEach((batch, i) => {
        const x = padding.left + (i / batches.length) * chartW + 1;
        const barH = (batch.total_faults / maxFaults) * chartH;
        const y = h - padding.bottom - barH;

        // Color by major fault ratio (red = more major/disk, green = more minor/cache)
        const majorRatio = batch.total_faults > 0 ? batch.major_faults / batch.total_faults : 0;
        const r = Math.round(34 + majorRatio * (239 - 34));
        const g = Math.round(197 - majorRatio * (197 - 68));
        const b_color = Math.round(94 - majorRatio * (94 - 68));
        ctx.fillStyle = `rgb(${r}, ${g}, ${b_color})`;

        ctx.fillRect(x, y, barWidth, barH);
    });

    // Y-axis labels
    ctx.fillStyle = '#71717a';
    ctx.font = '10px -apple-system, BlinkMacSystemFont, sans-serif';
    ctx.textAlign = 'right';
    ctx.fillText(fmtK(maxFaults), padding.left - 5, padding.top + 5);
    ctx.fillText('0', padding.left - 5, h - padding.bottom + 5);

    // X-axis labels (batch numbers)
    ctx.textAlign = 'center';
    if (batches.length > 0) {
        ctx.fillText('Batch #1', padding.left + 30, h - 10);
        ctx.fillText('Batch #' + batches.length, w - padding.right - 30, h - 10);
    }
}

// Phase 4: Draw block analysis section (fallback)
function drawBlockAnalysis(blockAnalysis) {
    // Sort by block number for both histogram and table
    const sorted = [...blockAnalysis].sort((a, b) => a.block_number - b.block_number);

    // Draw histogram
    drawBlockHistogram(sorted);

    // Populate table (sorted by block number for chronological view)
    const tbody = document.querySelector('#block-analysis-table tbody');
    if (!tbody) return;

    tbody.innerHTML = sorted.slice(0, 20).map(block => {
        const majorPct = block.total_faults > 0 ? ((block.major_faults / block.total_faults) * 100).toFixed(0) : '0';
        const tables = block.tables_touched.slice(0, 3).join(', ') + (block.tables_touched.length > 3 ? '...' : '');

        return `
            <tr>
                <td>${block.block_number.toLocaleString()}</td>
                <td>${fmt(block.total_faults)}</td>
                <td style="color: #f59e0b">${fmt(block.branch_faults)}</td>
                <td style="color: #22c55e">${fmt(block.leaf_faults)}</td>
                <td>${majorPct}%</td>
                <td>${(block.io_time_us / 1000).toFixed(1)}ms</td>
                <td class="tables-touched" title="${block.tables_touched.join(', ')}">${tables}</td>
            </tr>
        `;
    }).join('');
}

function drawBlockHistogram(blocks) {
    const canvas = document.getElementById('block-histogram-canvas');
    if (!canvas || blocks.length === 0) return;

    const ctx = canvas.getContext('2d');
    const dpr = window.devicePixelRatio || 1;
    const container = canvas.parentElement;
    const w = container.offsetWidth;
    const h = 180;

    canvas.width = w * dpr;
    canvas.height = h * dpr;
    canvas.style.width = w + 'px';
    canvas.style.height = h + 'px';
    ctx.scale(dpr, dpr);

    ctx.clearRect(0, 0, w, h);

    const padding = { top: 20, right: 20, bottom: 30, left: 50 };
    const chartW = w - padding.left - padding.right;
    const chartH = h - padding.top - padding.bottom;

    // Find max faults for y-axis
    const maxFaults = Math.max(...blocks.map(b => b.total_faults), 1);

    // Draw axes
    ctx.strokeStyle = '#3f3f46';
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(padding.left, padding.top);
    ctx.lineTo(padding.left, h - padding.bottom);
    ctx.lineTo(w - padding.right, h - padding.bottom);
    ctx.stroke();

    // Draw bars
    const barWidth = Math.max(2, (chartW / blocks.length) - 1);

    blocks.forEach((block, i) => {
        const x = padding.left + (i / blocks.length) * chartW;
        const barH = (block.total_faults / maxFaults) * chartH;
        const y = h - padding.bottom - barH;

        // Color by major fault ratio
        const majorRatio = block.total_faults > 0 ? block.major_faults / block.total_faults : 0;
        const r = Math.round(34 + majorRatio * (239 - 34));
        const g = Math.round(197 - majorRatio * (197 - 68));
        const b = Math.round(94 - majorRatio * (94 - 68));
        ctx.fillStyle = `rgb(${r}, ${g}, ${b})`;

        ctx.fillRect(x, y, barWidth, barH);
    });

    // Y-axis labels
    ctx.fillStyle = '#71717a';
    ctx.font = '10px -apple-system, BlinkMacSystemFont, sans-serif';
    ctx.textAlign = 'right';
    ctx.fillText(fmtK(maxFaults), padding.left - 5, padding.top + 5);
    ctx.fillText('0', padding.left - 5, h - padding.bottom + 5);

    // X-axis labels
    ctx.textAlign = 'center';
    if (blocks.length > 0) {
        ctx.fillText(blocks[0].block_number.toLocaleString(), padding.left + 30, h - 10);
        ctx.fillText(blocks[blocks.length - 1].block_number.toLocaleString(), w - padding.right - 30, h - 10);
    }
}

// Phase 5: Draw comparative table view
function drawTableComparison(tt, bv) {
    const tbody = document.querySelector('#table-comparison tbody');
    if (!tbody) return;

    // Get depth estimates map
    const depthMap = {};
    if (bv && bv.tree_depth_estimates) {
        bv.tree_depth_estimates.forEach(d => {
            depthMap[d.table_name] = d;
        });
    }

    renderTableComparisonRows(tbody, tt.tables, depthMap);
}

function renderTableComparisonRows(tbody, tables, depthMap) {
    tbody.innerHTML = tables.slice(0, 20).map(table => {
        const depth = depthMap[table.name];
        const estDepth = depth ? depth.estimated_depth.toFixed(1) : '-';
        const total = table.branch_faults + table.leaf_faults + table.overflow_faults;
        const branchPct = total > 0 ? (table.branch_faults / total) * 100 : 0;
        const leafPct = total > 0 ? (table.leaf_faults / total) * 100 : 0;
        const overflowPct = total > 0 ? (table.overflow_faults / total) * 100 : 0;

        // Estimate major % (not directly available, approximate from ratio)
        const majorEstPct = table.branch_leaf_ratio > 1 ? '~40%' : table.branch_leaf_ratio > 0.5 ? '~25%' : '~15%';

        return `
            <tr>
                <td title="${table.name}">${table.name}</td>
                <td>${table.branch_leaf_ratio.toFixed(2)}</td>
                <td>${estDepth}</td>
                <td>${fmt(table.total_faults)}</td>
                <td style="color: #f59e0b">${fmt(table.branch_faults)}</td>
                <td style="color: #22c55e">${fmt(table.leaf_faults)}</td>
                <td>${majorEstPct}</td>
                <td>
                    <div class="dist-bar">
                        <div class="dist-bar-segment branch" style="width: ${branchPct}%"></div>
                        <div class="dist-bar-segment leaf" style="width: ${leafPct}%"></div>
                        <div class="dist-bar-segment overflow" style="width: ${overflowPct}%"></div>
                    </div>
                </td>
            </tr>
        `;
    }).join('');
}

function initTableComparisonSort(tt, bv) {
    const buttons = document.querySelectorAll('.sort-btn');

    // Get depth estimates map
    const depthMap = {};
    if (bv && bv.tree_depth_estimates) {
        bv.tree_depth_estimates.forEach(d => {
            depthMap[d.table_name] = d;
        });
    }

    buttons.forEach(btn => {
        btn.addEventListener('click', () => {
            buttons.forEach(b => b.classList.remove('active'));
            btn.classList.add('active');

            const sortBy = btn.dataset.sort;
            let sorted = [...tt.tables];

            switch (sortBy) {
                case 'ratio':
                    sorted.sort((a, b) => b.branch_leaf_ratio - a.branch_leaf_ratio);
                    break;
                case 'depth':
                    sorted.sort((a, b) => {
                        const da = depthMap[a.name]?.estimated_depth || 0;
                        const db = depthMap[b.name]?.estimated_depth || 0;
                        return db - da;
                    });
                    break;
                default: // faults
                    sorted.sort((a, b) => b.total_faults - a.total_faults);
            }

            const tbody = document.querySelector('#table-comparison tbody');
            renderTableComparisonRows(tbody, sorted, depthMap);
        });
    });
}

function drawPageTypeDonut(pt) {
    const canvas = document.getElementById('page-type-donut');
    const ctx = canvas.getContext('2d');
    const dpr = window.devicePixelRatio || 1;

    canvas.width = 220 * dpr;
    canvas.height = 220 * dpr;
    ctx.scale(dpr, dpr);

    const centerX = 110;
    const centerY = 110;
    const outerRadius = 100;
    const innerRadius = 65;

    const colors = {
        'Branch': '#f59e0b',
        'Leaf': '#22c55e',
        'Overflow': '#8b5cf6',
        'Meta': '#6366f1',
        'Unknown': '#71717a'
    };

    let startAngle = -Math.PI / 2;

    pt.by_type.forEach(item => {
        const sliceAngle = (item.percentage / 100) * 2 * Math.PI;
        const endAngle = startAngle + sliceAngle;

        ctx.beginPath();
        ctx.arc(centerX, centerY, outerRadius, startAngle, endAngle);
        ctx.arc(centerX, centerY, innerRadius, endAngle, startAngle, true);
        ctx.closePath();
        ctx.fillStyle = colors[item.page_type] || '#71717a';
        ctx.fill();

        startAngle = endAngle;
    });
}

// Draw depth histogram bar chart
function drawDepthHistogram(histogram) {
    const canvas = document.getElementById('depth-histogram-chart');
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    const dpr = window.devicePixelRatio || 1;

    const width = canvas.parentElement.clientWidth || 400;
    const height = 180;

    canvas.width = width * dpr;
    canvas.height = height * dpr;
    canvas.style.width = width + 'px';
    canvas.style.height = height + 'px';
    ctx.scale(dpr, dpr);

    // Clear
    ctx.fillStyle = '#18181b';
    ctx.fillRect(0, 0, width, height);

    if (!histogram.length) return;

    const padding = { top: 20, right: 20, bottom: 30, left: 50 };
    const chartWidth = width - padding.left - padding.right;
    const chartHeight = height - padding.top - padding.bottom;

    const maxCount = Math.max(...histogram.map(h => h.count));
    const barWidth = Math.min(40, (chartWidth / histogram.length) - 4);

    // Draw bars
    histogram.forEach((bucket, i) => {
        const barHeight = (bucket.count / maxCount) * chartHeight;
        const x = padding.left + (i * (chartWidth / histogram.length)) + (chartWidth / histogram.length - barWidth) / 2;
        const y = padding.top + chartHeight - barHeight;

        // Bar gradient based on depth
        const hue = Math.max(0, 120 - bucket.depth * 20); // Green to red as depth increases
        ctx.fillStyle = `hsl(${hue}, 70%, 50%)`;
        ctx.fillRect(x, y, barWidth, barHeight);

        // Depth label below bar
        ctx.fillStyle = '#a1a1aa';
        ctx.font = '11px system-ui';
        ctx.textAlign = 'center';
        ctx.fillText(bucket.depth.toString(), x + barWidth / 2, height - 10);
    });

    // Y-axis labels
    ctx.fillStyle = '#71717a';
    ctx.font = '10px system-ui';
    ctx.textAlign = 'right';
    ctx.fillText(fmtK(maxCount), padding.left - 5, padding.top + 10);
    ctx.fillText('0', padding.left - 5, height - padding.bottom);

    // X-axis label
    ctx.textAlign = 'center';
    ctx.fillText('Tree Depth (levels)', width / 2, height - 2);
}

// Fill depth distribution table
function fillDepthTable(histogram) {
    const tbody = document.querySelector('#depth-distribution-table tbody');
    if (!tbody) return;

    tbody.innerHTML = histogram.map(bucket => `
        <tr>
            <td><strong>${bucket.depth}</strong></td>
            <td>${fmt(bucket.count)}</td>
            <td>${bucket.percentage.toFixed(1)}%</td>
            <td>${bucket.avg_faults.toFixed(1)}</td>
            <td>${bucket.avg_latency_us.toFixed(0)}us</td>
        </tr>
    `).join('');
}

// Fill per-table depth stats table
function fillDepthByTable(tables) {
    const tbody = document.querySelector('#depth-by-table tbody');
    if (!tbody) return;

    tbody.innerHTML = tables.slice(0, 20).map(t => {
        // Color code by avg depth
        const depthColor = t.avg_depth >= 3 ? '#ef4444' : t.avg_depth >= 2 ? '#f59e0b' : '#22c55e';
        return `
            <tr>
                <td>${t.table_name}</td>
                <td>${fmt(t.ops_count)}</td>
                <td>${t.max_depth}</td>
                <td style="color: ${depthColor}; font-weight: 600;">${t.avg_depth.toFixed(2)}</td>
                <td>${t.avg_faults.toFixed(1)}</td>
                <td>${t.avg_latency_us.toFixed(0)}us</td>
            </tr>
        `;
    }).join('');
}

// Fill per-operation depth stats table
function fillDepthByOperation(operations) {
    const tbody = document.querySelector('#depth-by-operation tbody');
    if (!tbody) return;

    tbody.innerHTML = operations.map(op => {
        // Color code by avg depth
        const depthColor = op.avg_depth >= 3 ? '#ef4444' : op.avg_depth >= 2 ? '#f59e0b' : '#22c55e';
        const typeLabel = op.is_seek ? '<span style="color: #f59e0b;">seek</span>' : '<span style="color: #22c55e;">nav</span>';
        return `
            <tr>
                <td>${op.operation}</td>
                <td>${typeLabel}</td>
                <td>${fmt(op.ops_count)}</td>
                <td>${op.max_depth}</td>
                <td style="color: ${depthColor}; font-weight: 600;">${op.avg_depth.toFixed(2)}</td>
                <td>${op.avg_faults.toFixed(1)}</td>
            </tr>
        `;
    }).join('');
}

// Helper: format large numbers with K suffix
function fmtK(n) {
    if (n >= 1000000) return (n / 1000000).toFixed(1) + 'M';
    if (n >= 1000) return (n / 1000).toFixed(1) + 'K';
    return n.toString();
}

function initResourcesTxns() {
    const t = DATA.txn_data;
    // Check both has_data flag and actual transaction count
    if (!t || !t.has_data || !t.summary || t.summary.begin_count === 0) {
        document.getElementById('txn-no-data').style.display = 'block';
        document.getElementById('txn-content').style.display = 'none';
        return;
    }

    document.getElementById('txn-no-data').style.display = 'none';
    document.getElementById('txn-content').style.display = 'block';

    const s = t.summary;
    document.getElementById('txn-total').textContent = fmt(s.begin_count);
    document.getElementById('txn-rate').textContent = fmt(s.txn_rate_per_sec);
    document.getElementById('txn-ro').textContent = fmt(s.ro_count);
    document.getElementById('txn-rw').textContent = fmt(s.rw_count);
    document.getElementById('txn-commits').textContent = fmt(s.commit_count);
    document.getElementById('txn-aborts').textContent = fmt(s.abort_count);

    // Concurrency
    const c = t.concurrency;
    document.getElementById('txn-max-ro').textContent = c.max_concurrent_ro;
    document.getElementById('txn-max-rw').textContent = c.max_concurrent_rw;
    document.getElementById('txn-avg-ro').textContent = c.avg_concurrent_ro.toFixed(1);

    // Concurrency chart (uPlot interactive)
    if (c.concurrency_timeline.length) {
        createUPlotChart('txn-concurrency-chart', c.concurrency_timeline.map(p => p.concurrent_ro), {
            durationSecs: s.duration_secs,
            color: '#60a5fa',
            label: 'Concurrent RO'
        });
    }

    // Latency
    document.getElementById('txn-avg-latency').textContent = fmtLat(s.avg_commit_latency_us);
    document.getElementById('txn-p95-latency').textContent = fmtLat(s.p95_commit_latency_us);
    document.getElementById('txn-p99-latency').textContent = fmtLat(s.p99_commit_latency_us);
    document.getElementById('txn-max-latency').textContent = fmtLat(s.max_commit_latency_us);

    // RW Commit latency timeline (shows WHEN commits happen and their latency)
    if (t.rw_commit_timeline && t.rw_commit_timeline.length > 0) {
        createCommitTimelineChart('txn-latency-chart', t.rw_commit_timeline, {
            color: '#3b82f6'
        });
    }
}

function initResourcesThreads() {
    // Page faults by thread
    const faultsByThread = DATA.threads;
    if (faultsByThread && faultsByThread.length > 0) {
        const tbody = document.querySelector('#threads-faults-table tbody');
        faultsByThread.slice(0, 20).forEach(th => {
            const tr = document.createElement('tr');
            tr.innerHTML = `<td>${th.tid}</td><td>${fmt(th.faults)}</td><td>${th.percentage.toFixed(1)}%</td>`;
            tbody.appendChild(tr);
        });
    }

    // Transactions by thread
    const t = DATA.txn_data;
    if (t && t.has_data && t.thread_stats) {
        const tbody = document.querySelector('#threads-txn-table tbody');
        t.thread_stats.slice(0, 20).forEach(th => {
            const tr = document.createElement('tr');
            tr.innerHTML = `<td>${th.tid}</td><td>${fmt(th.total_txns)}</td><td>${fmt(th.ro_txns)}</td><td>${fmt(th.rw_txns)}</td><td>${fmt(th.commits)}</td>`;
            tbody.appendChild(tr);
        });
    }
}

// Init overview on load
initTab('overview');

// Resize charts after initial render (ensure proper sizing)
setTimeout(() => {
    uplotInstances.forEach(({ uplot, el, height }) => {
        const rect = el.getBoundingClientRect();
        if (rect.width > 0) {
            uplot.setSize({ width: rect.width, height: height });
        }
    });
}, 100);
"##;
