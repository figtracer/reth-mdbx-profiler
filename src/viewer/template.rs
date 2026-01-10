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
    <style>
{css}
    </style>
</head>
<body>
    <div id="app">
        <nav class="tabs">
            <button class="tab active" data-tab="overview">Overview</button>
            <button class="tab" data-tab="cursors">Cursor Ops</button>
            <button class="tab" data-tab="transactions">MDBX Txns</button>
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

                <!-- Two column layout: Timeline + Heatmap -->
                <div class="two-col">
                    <div class="card">
                        <div class="card-header">Fault Timeline <span class="axis-hint">(drag to zoom, dbl-click to reset)</span></div>
                        <div class="card-body uplot-container" id="timeline-chart">
                        </div>
                    </div>
                    <div class="card">
                        <div class="card-header">Access Heatmap <span class="axis-hint">(hover for details)</span></div>
                        <div class="card-body chart-container" style="position: relative;">
                            <canvas id="heatmap-canvas"></canvas>
                            <div id="heatmap-tooltip" class="chart-tooltip" style="display: none;"></div>
                        </div>
                    </div>
                </div>

                <!-- Tables and Patterns side by side -->
                <div class="two-col">
                    <div class="card">
                        <div class="card-header">
                            Page Faults by Table
                            <span class="card-badge" id="correlation-badge"></span>
                        </div>
                        <div class="card-body compact-table-container">
                            <table class="compact-table" id="tables-table">
                                <thead>
                                    <tr>
                                        <th>Table</th>
                                        <th>Faults</th>
                                        <th>Major</th>
                                        <th>%</th>
                                    </tr>
                                </thead>
                                <tbody></tbody>
                            </table>
                        </div>
                    </div>
                    <div class="card">
                        <div class="card-header">Access Patterns</div>
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
                            <div class="stride-section" id="stride-section">
                                <div class="section-title">Top Stride Patterns</div>
                                <div id="stride-list"></div>
                            </div>
                            <div class="threads-section">
                                <div class="section-title">Top Threads</div>
                                <div id="threads-summary"></div>
                            </div>
                        </div>
                    </div>
                </div>
            </section>

            <!-- CURSOR OPS TAB -->
            <section id="cursors" class="panel">
                <div id="cursor-no-data" class="no-data">
                    No cursor data. Run with <code>--trace-cursors</code>
                </div>
                <div id="cursor-content">
                    <!-- Cursor metrics -->
                    <div class="metrics-row">
                        <div class="metric">
                            <span class="metric-value" id="cursor-total-ops"></span>
                            <span class="metric-label">Total Ops</span>
                        </div>
                        <div class="metric">
                            <span class="metric-value" id="cursor-op-rate"></span>
                            <span class="metric-label">Ops/sec</span>
                        </div>
                        <div class="metric">
                            <span class="metric-value" id="cursor-avg-latency"></span>
                            <span class="metric-label">Avg Latency</span>
                        </div>
                        <div class="metric">
                            <span class="metric-value" id="cursor-p50-latency"></span>
                            <span class="metric-label">p50</span>
                        </div>
                        <div class="metric">
                            <span class="metric-value major" id="cursor-p95-latency"></span>
                            <span class="metric-label">p95</span>
                        </div>
                        <div class="metric">
                            <span class="metric-value major" id="cursor-p99-latency"></span>
                            <span class="metric-label">p99</span>
                        </div>
                        <div class="metric">
                            <span class="metric-value" id="cursor-seeks"></span>
                            <span class="metric-label">Seeks</span>
                        </div>
                        <div class="metric">
                            <span class="metric-value minor" id="cursor-navs"></span>
                            <span class="metric-label">Navigation</span>
                        </div>
                    </div>

                    <!-- Two columns: Ops by type + Ops by table -->
                    <div class="two-col">
                        <div class="card">
                            <div class="card-header">Operations by Type</div>
                            <div class="card-body chart-container">
                                <canvas id="cursor-ops-chart"></canvas>
                            </div>
                        </div>
                        <div class="card">
                            <div class="card-header">Operations by Table</div>
                            <div class="card-body chart-container">
                                <canvas id="cursor-tables-chart"></canvas>
                            </div>
                        </div>
                    </div>

                    <!-- Table details -->
                    <div class="card full-width">
                        <div class="card-header">Table Access Details</div>
                        <div class="card-body compact-table-container">
                            <table class="compact-table" id="cursor-tables-table">
                                <thead>
                                    <tr>
                                        <th>Table</th>
                                        <th>Ops</th>
                                        <th>Seeks</th>
                                        <th>Nav</th>
                                        <th>Avg</th>
                                        <th>p50</th>
                                        <th>p95</th>
                                        <th>p99</th>
                                    </tr>
                                </thead>
                                <tbody></tbody>
                            </table>
                        </div>
                    </div>

                    <!-- Slow operations - THE KEY INSIGHT -->
                    <div class="card full-width highlight">
                        <div class="card-header">Slow Operations (&gt;100μs) - Likely Page Faults</div>
                        <div class="card-body compact-table-container">
                            <table class="compact-table" id="slow-ops-table">
                                <thead>
                                    <tr>
                                        <th>Table</th>
                                        <th>Slow</th>
                                        <th>Total</th>
                                        <th>%</th>
                                        <th>Avg Slow</th>
                                        <th>Max</th>
                                        <th>Time Lost</th>
                                        <th>Top Operations</th>
                                    </tr>
                                </thead>
                                <tbody></tbody>
                            </table>
                        </div>
                    </div>

                    <!-- Slow keys -->
                    <div class="card full-width">
                        <div class="card-header">Hot Keys (Frequently Slow)</div>
                        <div class="card-body compact-table-container" style="max-height: 350px;">
                            <table class="compact-table" id="slow-keys-table">
                                <thead>
                                    <tr>
                                        <th>Table</th>
                                        <th>Key</th>
                                        <th>Slow</th>
                                        <th>Total</th>
                                        <th>Avg</th>
                                        <th>Max</th>
                                    </tr>
                                </thead>
                                <tbody></tbody>
                            </table>
                        </div>
                    </div>
                </div>
            </section>

            <!-- MDBX TRANSACTIONS TAB -->
            <section id="transactions" class="panel">
                <div id="txn-no-data" class="no-data">
                    No transaction data. Run with <code>--trace-cursors</code>
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
                            <div class="card-header">RW Commit Latency Timeline <span class="axis-hint">(drag to zoom, dbl-click to reset)</span></div>
                            <div class="card-body">
                                <div class="latency-stats" style="margin-bottom: 16px;">
                                    <div class="lat-stat">
                                        <span class="lat-label">AVG</span>
                                        <span class="lat-value" id="txn-avg-latency"></span>
                                    </div>
                                    <div class="lat-stat">
                                        <span class="lat-label">P50</span>
                                        <span class="lat-value" id="txn-p50-latency"></span>
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
                                <div class="uplot-container" id="txn-latency-chart" style="height: 140px;">
                                </div>
                            </div>
                        </div>
                    </div>

                    <div class="card full-width">
                        <div class="card-header">Thread Distribution</div>
                        <div class="card-body compact-table-container" style="max-height: 300px;">
                            <table class="compact-table" id="txn-threads-table">
                                <thead>
                                    <tr>
                                        <th>Thread</th>
                                        <th>Total</th>
                                        <th>RO</th>
                                        <th>RW</th>
                                        <th>Commits</th>
                                        <th>Aborts</th>
                                    </tr>
                                </thead>
                                <tbody></tbody>
                            </table>
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

.card.highlight {
    border-color: #f87171;
    box-shadow: 0 0 24px rgba(248, 113, 113, 0.15);
}

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

.compact-table {
    width: 100%;
    border-collapse: separate;
    border-spacing: 0;
    font-size: 13px;
}

.compact-table th,
.compact-table td {
    padding: 10px 12px;
    text-align: left;
    border-bottom: 1px solid #1e1e2a;
    white-space: nowrap;
}

.compact-table thead {
    position: sticky;
    top: 0;
    z-index: 10;
}

.compact-table th {
    background: #0f0f17;
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

/* Centered latency card */
.latency-card-body {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 200px;
}

.latency-stats.centered {
    justify-content: center;
    gap: 32px;
}

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
"##;

const JAVASCRIPT: &str = r##"
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
                size: 55,
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
                size: 55,
                values: (u, vals) => vals.map(v => {
                    if (v < 1) return v.toFixed(2) + 'ms';
                    if (v < 1000) return v.toFixed(0) + 'ms';
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

    return uplot;
}

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
        if (i === 0) return '#0a0a12';  // Dark background for empty cells
        if (i < 0.01) return '#0f172a'; // Very low activity - dark blue-gray
        // Blue gradient: dark blue -> light blue -> cyan (matching blob-exex scheme)
        // #1e3a5f (dark) -> #3b82f6 (blue) -> #60a5fa (light blue) -> #22d3ee (cyan for hot)
        if (i < 0.25) {
            // Dark blue to blue
            const t = i / 0.25;
            return `rgb(${Math.floor(30 + t * 29)}, ${Math.floor(58 + t * 72)}, ${Math.floor(95 + t * 151)})`;
        } else if (i < 0.5) {
            // Blue to light blue
            const t = (i - 0.25) / 0.25;
            return `rgb(${Math.floor(59 + t * 37)}, ${Math.floor(130 + t * 35)}, ${Math.floor(246 - t * 10)})`;
        } else if (i < 0.75) {
            // Light blue to cyan
            const t = (i - 0.5) / 0.25;
            return `rgb(${Math.floor(96 - t * 62)}, ${Math.floor(165 + t * 46)}, ${Math.floor(236 + t * 2)})`;
        } else {
            // Cyan to bright cyan/white for hotspots
            const t = (i - 0.75) / 0.25;
            return `rgb(${Math.floor(34 + t * 100)}, ${Math.floor(211 + t * 44)}, ${Math.floor(238 + t * 17)})`;
        }
    }

    fmtAxisVal(n) {
        if (n >= 1e6) return (n/1e6).toFixed(1) + 'M';
        if (n >= 1e3) return (n/1e3).toFixed(1) + 'K';
        return n.toFixed(0);
    }
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
    });
});

// Export
document.getElementById('export-compact-btn').addEventListener('click', () => {
    const json = JSON.stringify(DATA, null, 2);
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
    else if (name === 'cursors') initCursors();
    else if (name === 'transactions') initTxns();
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

    // Heatmap with interactivity
    if (DATA.heatmap.data.length) {
        const heatmapChart = new Chart(document.getElementById('heatmap-canvas'));
        heatmapChart.drawHeatmap(DATA.heatmap);

        // Add hover tooltip
        const canvas = document.getElementById('heatmap-canvas');
        const tooltip = document.getElementById('heatmap-tooltip');
        const hm = DATA.heatmap;

        canvas.addEventListener('mousemove', (e) => {
            const rect = canvas.getBoundingClientRect();
            const x = e.clientX - rect.left;
            const y = e.clientY - rect.top;

            // Calculate which cell we're hovering over
            const pad = { t: 20, r: 20, b: 40, l: 60 };
            const w = rect.width - pad.l - pad.r;
            const h = rect.height - pad.t - pad.b;

            if (x < pad.l || x > rect.width - pad.r || y < pad.t || y > rect.height - pad.b) {
                tooltip.style.display = 'none';
                return;
            }

            const relX = (x - pad.l) / w;
            const relY = (y - pad.t) / h;

            const timeIdx = Math.floor(relX * hm.time_buckets);
            const offsetIdx = hm.offset_buckets - 1 - Math.floor(relY * hm.offset_buckets);

            if (timeIdx >= 0 && timeIdx < hm.time_buckets && offsetIdx >= 0 && offsetIdx < hm.offset_buckets) {
                const cellIdx = timeIdx * hm.offset_buckets + offsetIdx;
                const count = hm.data[cellIdx] || 0;

                // Calculate actual values
                const timeRange = hm.max_time_ms - hm.min_time_ms;
                const timeMs = hm.min_time_ms + (timeIdx / hm.time_buckets) * timeRange;
                const offsetRange = hm.max_offset_gb - hm.min_offset_gb;
                const offsetGb = hm.min_offset_gb + (offsetIdx / hm.offset_buckets) * offsetRange;

                const timeStr = timeMs < 60000 ? (timeMs/1000).toFixed(1) + 's' : (timeMs/60000).toFixed(1) + 'm';
                const intensity = hm.max_count > 0 ? (count / hm.max_count * 100).toFixed(0) : 0;

                tooltip.innerHTML = `
                    <div class="chart-tooltip-label">Time: ${timeStr}</div>
                    <div class="chart-tooltip-label">Offset: ${offsetGb.toFixed(1)} GB</div>
                    <div class="chart-tooltip-value">${count} faults</div>
                    <div class="chart-tooltip-label">${intensity}% intensity</div>
                `;

                tooltip.style.display = 'block';
                tooltip.style.left = Math.min(x + 10, rect.width - 120) + 'px';
                tooltip.style.top = Math.min(y + 10, rect.height - 80) + 'px';
            } else {
                tooltip.style.display = 'none';
            }
        });

        canvas.addEventListener('mouseleave', () => {
            tooltip.style.display = 'none';
        });
    }

    // Tables
    const tbody = document.querySelector('#tables-table tbody');
    DATA.tables.forEach(t => {
        const tr = document.createElement('tr');
        tr.innerHTML = `<td>${t.name}</td><td>${fmt(t.faults)}</td><td>${fmt(t.major_faults)}</td><td>${t.percentage.toFixed(1)}%</td>`;
        tbody.appendChild(tr);
    });

    // Correlation badge
    if (DATA.page_fault_attribution_warning) {
        document.getElementById('correlation-badge').textContent = 'Correlated';
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

    // Threads summary
    const threadsDiv = document.getElementById('threads-summary');
    DATA.threads.slice(0, 5).forEach(t => {
        const div = document.createElement('div');
        div.className = 'thread-item';
        div.innerHTML = `<span>TID ${t.tid}</span><span>${fmt(t.faults)} (${t.percentage.toFixed(1)}%)</span>`;
        threadsDiv.appendChild(div);
    });
}

function initCursors() {
    const c = DATA.cursor_data;
    if (!c.has_data) {
        document.getElementById('cursor-no-data').style.display = 'block';
        document.getElementById('cursor-content').style.display = 'none';
        return;
    }

    document.getElementById('cursor-no-data').style.display = 'none';
    document.getElementById('cursor-content').style.display = 'block';

    const s = c.summary;
    document.getElementById('cursor-total-ops').textContent = fmt(s.total_ops);
    document.getElementById('cursor-op-rate').textContent = fmt(s.op_rate_per_sec);
    document.getElementById('cursor-avg-latency').textContent = fmtLat(s.avg_latency_us);
    document.getElementById('cursor-p50-latency').textContent = fmtLat(s.p50_latency_us);
    document.getElementById('cursor-p95-latency').textContent = fmtLat(s.p95_latency_us);
    document.getElementById('cursor-p99-latency').textContent = fmtLat(s.p99_latency_us);
    document.getElementById('cursor-seeks').textContent = fmt(s.seek_count);
    document.getElementById('cursor-navs').textContent = fmt(s.nav_count);

    // Operations chart - horizontal bars (blue scheme)
    const ops = c.operations.slice(0, 8);
    new Chart(document.getElementById('cursor-ops-chart'))
        .drawHorizontalBar(ops.map(o => o.name), ops.map(o => o.count), '#3b82f6');

    // Tables chart - horizontal bars (light blue)
    const tables = c.table_stats.slice(0, 8);
    new Chart(document.getElementById('cursor-tables-chart'))
        .drawHorizontalBar(tables.map(t => t.name), tables.map(t => t.ops), '#60a5fa');

    // Table details
    const tbody = document.querySelector('#cursor-tables-table tbody');
    c.table_stats.forEach(t => {
        const tr = document.createElement('tr');
        tr.innerHTML = `<td>${t.name}</td><td>${fmt(t.ops)}</td><td>${fmt(t.seeks)}</td><td>${fmt(t.navs)}</td><td>${fmtLat(t.avg_latency_us)}</td><td>${fmtLat(t.p50_latency_us)}</td><td>${fmtLat(t.p95_latency_us)}</td><td>${fmtLat(t.p99_latency_us)}</td>`;
        tbody.appendChild(tr);
    });

    // Slow ops
    const slowTbody = document.querySelector('#slow-ops-table tbody');
    c.slow_ops_by_table.forEach(t => {
        const topOps = t.by_operation.slice(0, 2).map(o => `${o.operation}: ${fmt(o.count)}`).join(', ');
        const tr = document.createElement('tr');
        tr.innerHTML = `<td>${t.table}</td><td>${fmt(t.slow_op_count)}</td><td>${fmt(t.total_op_count)}</td><td style="color:${t.slow_op_percentage > 20 ? '#f87171' : '#a1a1aa'}">${t.slow_op_percentage.toFixed(1)}%</td><td>${fmtLat(t.avg_slow_latency_us)}</td><td>${fmtLat(t.max_latency_us)}</td><td>${(t.total_slow_time_ms/1000).toFixed(1)}s</td><td style="font-size:11px;color:#71717a">${topOps}</td>`;
        slowTbody.appendChild(tr);
    });

    // Slow keys
    const keysTbody = document.querySelector('#slow-keys-table tbody');
    c.slow_keys.slice(0, 25).forEach(k => {
        const tr = document.createElement('tr');
        tr.innerHTML = `<td>${k.table}</td><td style="font-family:monospace;font-size:11px">${k.key_prefix}</td><td>${fmt(k.slow_access_count)}</td><td>${fmt(k.total_access_count)}</td><td>${fmtLat(k.avg_latency_us)}</td><td>${fmtLat(k.max_latency_us)}</td>`;
        keysTbody.appendChild(tr);
    });
}

function initTxns() {
    const t = DATA.txn_data;
    if (!t.has_data) {
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
    document.getElementById('txn-p50-latency').textContent = fmtLat(s.p50_commit_latency_us);
    document.getElementById('txn-p95-latency').textContent = fmtLat(s.p95_commit_latency_us);
    document.getElementById('txn-p99-latency').textContent = fmtLat(s.p99_commit_latency_us);
    document.getElementById('txn-max-latency').textContent = fmtLat(s.max_commit_latency_us);

    // RW Commit latency timeline (shows WHEN commits happen and their latency)
    if (t.rw_commit_timeline && t.rw_commit_timeline.length > 0) {
        createCommitTimelineChart('txn-latency-chart', t.rw_commit_timeline, {
            color: '#3b82f6'
        });
    }

    // Threads
    const tbody = document.querySelector('#txn-threads-table tbody');
    t.thread_stats.slice(0, 20).forEach(th => {
        const tr = document.createElement('tr');
        tr.innerHTML = `<td>${th.tid}</td><td>${fmt(th.total_txns)}</td><td>${fmt(th.ro_txns)}</td><td>${fmt(th.rw_txns)}</td><td>${fmt(th.commits)}</td><td>${fmt(th.aborts)}</td>`;
        tbody.appendChild(tr);
    });
}

// Init overview on load
initTab('overview');
"##;
