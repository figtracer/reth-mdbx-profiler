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
            <button class="tab active" data-tab="physical">Physical</button>
            <button class="tab" data-tab="tables">Tables</button>
            <button class="tab" data-tab="transactions">Transactions</button>
            <button class="export-btn" id="export-compact-btn" title="Download JSON for analysis">Export</button>
        </nav>

        <main>
            <!-- PHYSICAL TAB -->
            <section id="physical" class="panel active">
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
                        <div class="card-header">Top Threads by Page Faults</div>
                        <div class="card-body">
                            <div class="threads-section">
                                <div id="threads-summary"></div>
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

                <!-- Attribution summary -->
                <div class="attribution-header" id="tables-attribution-header">
                    <span class="card-badge direct-badge">Direct BPF Attribution</span>
                    <span class="attribution-summary" id="tables-attribution-summary"></span>
                </div>

                <!-- Fault distribution bar chart -->
                <div class="card full-width" id="fault-dist-card" style="display:none;">
                    <div class="card-header">
                        Fault Distribution by Table
                        <span class="chart-legend">
                            <span class="legend-item"><span class="legend-swatch minor-swatch"></span>Minor</span>
                            <span class="legend-item"><span class="legend-swatch major-swatch"></span>Major</span>
                        </span>
                    </div>
                    <div class="card-body" style="padding: 8px 16px;">
                        <canvas id="fault-dist-chart"></canvas>
                    </div>
                </div>

                <!-- Unified table with expandable rows -->
                <div class="card full-width">
                    <div class="card-header">
                        I/O Impact by Table
                        <span class="card-hint">Click row to expand details</span>
                    </div>
                    <div class="card-body compact-table-container" style="max-height: none; padding: 0;">
                        <table class="compact-table expandable-table" id="unified-tables">
                            <thead>
                                <tr>
                                    <th style="width:30px;"></th>
                                    <th>Table</th>
                                    <th>Faults</th>
                                    <th>Major</th>
                                    <th>Slow Ops</th>
                                    <th>I/O Time</th>
                                    <th>Top Operation</th>
                                </tr>
                            </thead>
                            <tbody></tbody>
                        </table>
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
                        <div class="card-body compact-table-container" style="max-height: 300px; padding: 0;">
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
    grid-template-columns: repeat(3, 1fr);
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

// Horizontal bar chart for fault distribution
function drawFaultDistChart(canvas, labels, totalFaults, majorFaults) {
    const ctx = canvas.getContext('2d');
    const rect = canvas.getBoundingClientRect();
    if (rect.width === 0) return;

    canvas.width = rect.width * 2;
    canvas.height = rect.height * 2;
    ctx.scale(2, 2);

    const w = rect.width;
    const h = rect.height;
    const pad = { t: 8, r: 80, b: 24, l: 140 };
    const chartW = w - pad.l - pad.r;
    const chartH = h - pad.t - pad.b;

    const max = Math.max(...totalFaults) * 1.1 || 1;
    const barH = Math.min(18, (chartH / labels.length) * 0.7);
    const gap = (chartH / labels.length) - barH;

    ctx.clearRect(0, 0, w, h);

    labels.forEach((label, i) => {
        const total = totalFaults[i];
        const major = majorFaults[i];

        const totalBarW = (total / max) * chartW;
        const minorBarW = ((total - major) / max) * chartW;
        const y = pad.t + i * (barH + gap) + gap / 2;

        // Draw minor faults (blue) first, then major (red) stacked after
        ctx.fillStyle = '#3b82f6';
        ctx.fillRect(pad.l, y, minorBarW, barH);

        if (major > 0) {
            ctx.fillStyle = '#f87171';
            ctx.fillRect(pad.l + minorBarW, y, totalBarW - minorBarW, barH);
        }

        // Label (left)
        ctx.fillStyle = '#a1a1aa';
        ctx.font = '11px sans-serif';
        ctx.textAlign = 'right';
        const labelText = label.length > 18 ? label.substring(0, 16) + '...' : label;
        ctx.fillText(labelText, pad.l - 8, y + barH / 2 + 4);

        // Value (right of bar)
        ctx.fillStyle = '#71717a';
        ctx.textAlign = 'left';
        const fmtVal = n => n >= 1e6 ? (n/1e6).toFixed(1)+'M' : n >= 1e3 ? (n/1e3).toFixed(1)+'K' : n.toFixed(0);
        ctx.fillText(fmtVal(total), pad.l + totalBarW + 8, y + barH / 2 + 4);
    });

    // Legend at bottom right
    ctx.font = '10px sans-serif';
    const legendY = h - 10;

    ctx.fillStyle = '#3b82f6';
    ctx.fillRect(w - 140, legendY - 8, 10, 10);
    ctx.fillStyle = '#71717a';
    ctx.textAlign = 'left';
    ctx.fillText('Minor', w - 126, legendY);

    ctx.fillStyle = '#f87171';
    ctx.fillRect(w - 70, legendY - 8, 10, 10);
    ctx.fillStyle = '#71717a';
    ctx.fillText('Major', w - 56, legendY);
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

// Export - generates compact JSON optimized for analysis
document.getElementById('export-compact-btn').addEventListener('click', () => {
    // Build compact export (removes large arrays, keeps key metrics)
    const compact = {
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
        tables: DATA.unified_tables.map(t => ({
            name: t.name,
            faults: t.faults,
            major_faults: t.major_faults,
            fault_pct: t.fault_percentage,
            slow_ops: t.slow_ops,
            time_lost_ms: t.time_lost_ms,
            top_op: t.top_operation,
            faults_by_op: t.details.faults_by_op
        })),
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
            slow_keys: DATA.cursor_data.slow_keys.slice(0, 20)
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

    if (name === 'physical') initPhysical();
    else if (name === 'tables') initTables();
    else if (name === 'transactions') initTxns();
}

function initPhysical() {
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

function initTables() {
    const unified = DATA.unified_tables;
    const dfa = DATA.direct_fault_attribution;

    if (!unified || unified.length === 0) {
        document.getElementById('tables-no-data').style.display = 'block';
        document.getElementById('tables-attribution-header').style.display = 'none';
        return;
    }

    // Attribution summary
    if (dfa && dfa.has_data) {
        const total = dfa.directly_attributed_count + dfa.timestamp_fallback_count + dfa.uncorrelated_count;
        const directPct = (dfa.directly_attributed_count / total * 100).toFixed(1);
        document.getElementById('tables-attribution-summary').innerHTML =
            `<strong>${fmt(dfa.directly_attributed_count)}</strong> directly attributed (${directPct}%) · ` +
            `${fmt(dfa.uncorrelated_count)} uncorrelated`;
    }

    // Draw fault distribution bar chart
    if (unified.length > 0) {
        document.getElementById('fault-dist-card').style.display = 'block';
        const chartCanvas = document.getElementById('fault-dist-chart');
        const topTables = unified.slice(0, 8); // Top 8 tables by faults
        const labels = topTables.map(t => t.name);
        const faults = topTables.map(t => t.faults);
        const majorFaults = topTables.map(t => t.major_faults);
        drawFaultDistChart(chartCanvas, labels, faults, majorFaults);
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

        tr.innerHTML = `
            <td><span class="expand-icon">▶</span></td>
            <td>${t.name}</td>
            <td>${fmt(t.faults)}</td>
            <td class="major">${fmt(t.major_faults)}</td>
            <td>${t.slow_ops > 0 ? fmt(t.slow_ops) + ' (' + t.slow_ops_percentage.toFixed(1) + '%)' : '-'}</td>
            <td class="io-time">${t.time_lost_ms > 0 ? ioTime : '-'}</td>
            <td>${t.top_operation || '-'}</td>
        `;
        tbody.appendChild(tr);

        // Details row (hidden by default)
        const detailsTr = document.createElement('tr');
        detailsTr.className = 'details-row hidden';
        detailsTr.dataset.idx = idx;

        let detailsHtml = '<td colspan="7"><div class="details-content">';

        // Faults by Operation Type
        detailsHtml += '<div class="details-section"><div class="details-section-title">Faults by Operation</div><div class="details-list">';
        if (t.details.faults_by_op && t.details.faults_by_op.length > 0) {
            t.details.faults_by_op.forEach(op => {
                detailsHtml += `<div class="details-item">
                    <span class="details-item-name">${op.operation}</span>
                    <span class="details-item-value">${fmt(op.faults)} <span class="major">(${fmt(op.major_faults)} major)</span></span>
                </div>`;
            });
        } else {
            detailsHtml += '<div class="details-item"><span class="details-item-name" style="color:#52525b;">No fault data</span></div>';
        }
        detailsHtml += '</div></div>';

        // Faults by Cursor Operation (for CURSOR_GET)
        detailsHtml += '<div class="details-section"><div class="details-section-title">By Cursor Op (GET)</div><div class="details-list">';
        if (t.details.faults_by_cursor_op && t.details.faults_by_cursor_op.length > 0) {
            t.details.faults_by_cursor_op.slice(0, 5).forEach(op => {
                detailsHtml += `<div class="details-item">
                    <span class="details-item-name">${op.operation}</span>
                    <span class="details-item-value">${fmt(op.faults)}</span>
                </div>`;
            });
        } else {
            detailsHtml += '<div class="details-item"><span class="details-item-name" style="color:#52525b;">No GET operations</span></div>';
        }
        detailsHtml += '</div></div>';

        // Hot Keys
        detailsHtml += '<div class="details-section"><div class="details-section-title">Hot Keys</div><div class="details-list">';
        if (t.details.hot_keys && t.details.hot_keys.length > 0) {
            t.details.hot_keys.slice(0, 4).forEach(key => {
                const keyShort = key.key_hex.length > 20 ? key.key_hex.substring(0, 20) + '...' : key.key_hex;
                const avgMs = (key.avg_latency_us / 1000).toFixed(1);
                detailsHtml += `<div class="details-item">
                    <span class="details-item-name" style="font-family:monospace;font-size:11px;">${keyShort}</span>
                    <span class="details-item-value">${fmt(key.slow_count)} slow, ${avgMs}ms avg</span>
                </div>`;
            });
        } else {
            detailsHtml += '<div class="details-item"><span class="details-item-name" style="color:#52525b;">No hot keys</span></div>';
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
initTab('physical');

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
