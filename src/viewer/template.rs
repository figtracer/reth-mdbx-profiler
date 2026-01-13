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
            <button class="tab" data-tab="btree">B+ Tree</button>
            <button class="tab" data-tab="transactions">MDBX Txns</button>
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

                <div id="tables-content">
                    <!-- Summary cards row -->
                    <div class="tables-summary-row">
                        <div class="tables-summary-card">
                            <div class="tables-summary-icon">&#x26A1;</div>
                            <div class="tables-summary-content">
                                <span class="tables-summary-value" id="tables-total-faults">0</span>
                                <span class="tables-summary-label">Total Faults</span>
                            </div>
                        </div>
                        <div class="tables-summary-card">
                            <div class="tables-summary-icon">&#x23F1;</div>
                            <div class="tables-summary-content">
                                <span class="tables-summary-value" id="tables-io-time">0ms</span>
                                <span class="tables-summary-label">I/O Time Lost</span>
                            </div>
                        </div>
                        <div class="tables-summary-card hottest">
                            <div class="tables-summary-icon">&#x1F525;</div>
                            <div class="tables-summary-content">
                                <span class="tables-summary-value" id="tables-hottest">-</span>
                                <span class="tables-summary-label">Hottest Table</span>
                            </div>
                        </div>
                        <div class="tables-summary-card">
                            <div class="tables-summary-icon">&#x1F4CA;</div>
                            <div class="tables-summary-content">
                                <span class="tables-summary-value" id="tables-count">0</span>
                                <span class="tables-summary-label">Tables Traced</span>
                            </div>
                        </div>
                    </div>

                    <!-- Attribution badge -->
                    <div class="attribution-header" id="tables-attribution-header">
                        <span class="card-badge direct-badge">Direct BPF Attribution</span>
                        <span class="attribution-summary" id="tables-attribution-summary"></span>
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

                    <!-- Table cards grid -->
                    <div class="table-cards-header">
                        <h3>I/O Impact by Table</h3>
                        <div class="severity-legend">
                            <span class="severity-dot critical"></span><span>Critical (&gt;30%)</span>
                            <span class="severity-dot high"></span><span>High (&gt;15%)</span>
                            <span class="severity-dot medium"></span><span>Medium (&gt;5%)</span>
                            <span class="severity-dot low"></span><span>Low</span>
                        </div>
                    </div>
                    <div id="table-cards-grid" class="table-cards-grid"></div>

                    <!-- Legacy table (hidden by default, toggle-able) -->
                    <div class="card full-width" id="legacy-table-card" style="display:none;">
                        <div class="card-header">
                            I/O Impact by Table (Table View)
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
                </div>
            </section>

            <!-- B+ TREE TAB -->
            <section id="btree" class="panel">
                <div id="btree-no-data" class="no-data" style="display:none;">
                    No page type data available. This feature requires the BPF page type detection to be enabled.
                </div>

                <div id="btree-content">
                    <!-- Page Type Distribution - Hero Section -->
                    <div class="btree-hero">
                        <div class="btree-hero-chart">
                            <div class="donut-container">
                                <canvas id="page-type-donut" width="220" height="220"></canvas>
                                <div class="donut-center">
                                    <span class="donut-total" id="page-type-total">0</span>
                                    <span class="donut-label">Total Faults</span>
                                </div>
                            </div>
                            <div class="donut-legend" id="page-type-legend"></div>
                        </div>
                        <div class="btree-hero-stats">
                            <div class="btree-stat-card btree-branch">
                                <div class="btree-stat-icon">&#x1F333;</div>
                                <div class="btree-stat-content">
                                    <span class="btree-stat-value" id="branch-faults">0</span>
                                    <span class="btree-stat-label">Branch Faults</span>
                                    <span class="btree-stat-hint">Tree traversal overhead</span>
                                </div>
                            </div>
                            <div class="btree-stat-card btree-leaf">
                                <div class="btree-stat-icon">&#x1F343;</div>
                                <div class="btree-stat-content">
                                    <span class="btree-stat-value" id="leaf-faults">0</span>
                                    <span class="btree-stat-label">Leaf Faults</span>
                                    <span class="btree-stat-hint">Actual data access</span>
                                </div>
                            </div>
                            <div class="btree-stat-card btree-ratio">
                                <div class="btree-stat-icon">&#x2696;</div>
                                <div class="btree-stat-content">
                                    <span class="btree-stat-value" id="traversal-ratio">0.0</span>
                                    <span class="btree-stat-label">Traversal Ratio</span>
                                    <span class="btree-stat-hint">Branch:Leaf (lower is better)</span>
                                </div>
                            </div>
                        </div>
                    </div>

                    <!-- Faults per Operation Histogram -->
                    <div class="two-col">
                        <div class="card">
                            <div class="card-header">
                                Faults per Operation
                                <span class="card-hint">How many pages are touched per DB operation</span>
                            </div>
                            <div class="card-body">
                                <div class="histogram-stats">
                                    <div class="hist-stat">
                                        <span class="hist-label">AVG</span>
                                        <span class="hist-value" id="hist-avg">0.0</span>
                                    </div>
                                    <div class="hist-stat">
                                        <span class="hist-label">P50</span>
                                        <span class="hist-value" id="hist-p50">0</span>
                                    </div>
                                    <div class="hist-stat">
                                        <span class="hist-label">P95</span>
                                        <span class="hist-value" id="hist-p95">0</span>
                                    </div>
                                    <div class="hist-stat">
                                        <span class="hist-label">P99</span>
                                        <span class="hist-value major" id="hist-p99">0</span>
                                    </div>
                                    <div class="hist-stat">
                                        <span class="hist-label">MAX</span>
                                        <span class="hist-value major" id="hist-max">0</span>
                                    </div>
                                </div>
                                <div class="histogram-chart" id="histogram-container">
                                    <canvas id="faults-histogram" height="180"></canvas>
                                </div>
                            </div>
                        </div>
                        <div class="card">
                            <div class="card-header">
                                Understanding B+ Tree I/O
                            </div>
                            <div class="card-body btree-explainer">
                                <div class="explainer-visual">
                                    <div class="tree-diagram">
                                        <div class="tree-level">
                                            <div class="tree-node branch-node">Root</div>
                                        </div>
                                        <div class="tree-level">
                                            <div class="tree-node branch-node">Branch</div>
                                            <div class="tree-node branch-node">Branch</div>
                                        </div>
                                        <div class="tree-level">
                                            <div class="tree-node leaf-node">Leaf</div>
                                            <div class="tree-node leaf-node">Leaf</div>
                                            <div class="tree-node leaf-node">Leaf</div>
                                            <div class="tree-node leaf-node">Leaf</div>
                                        </div>
                                    </div>
                                </div>
                                <div class="explainer-text">
                                    <p><strong>Branch pages</strong> (amber) are internal B+ tree nodes used for navigation. High branch fault counts indicate deep tree traversals.</p>
                                    <p><strong>Leaf pages</strong> (green) contain actual key-value data. These are the "productive" I/O operations.</p>
                                    <p><strong>Traversal ratio</strong> above 1.0 means more pages are spent navigating than accessing data - a sign of sparse or random access patterns.</p>
                                </div>
                            </div>
                        </div>
                    </div>

                    <!-- Per-Table Tree Depth -->
                    <div class="card full-width">
                        <div class="card-header">
                            Per-Table Page Type Breakdown
                            <span class="card-hint">Branch vs Leaf faults by table - identifies tables with deep tree traversal</span>
                        </div>
                        <div class="card-body" style="padding: 16px;">
                            <div id="table-tree-bars"></div>
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
                            <div class="card-header">RW Commit Latency (ms) <span class="axis-hint">(drag to zoom, dbl-click to reset)</span></div>
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
                                <div class="uplot-container" id="txn-latency-chart" style="height: 180px;">
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

/* ============================================
   IMPROVED TABLES TAB STYLES
   ============================================ */

/* Summary cards row */
.tables-summary-row {
    display: flex;
    gap: 16px;
    margin-bottom: 16px;
    flex-wrap: wrap;
}

.tables-summary-card {
    flex: 1;
    min-width: 180px;
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 20px 24px;
    background: #12121a;
    border-radius: 12px;
    border: 1px solid #1e1e2a;
}

.tables-summary-card.hottest {
    background: linear-gradient(135deg, #12121a 0%, #1a1210 100%);
    border-color: #f9731640;
}

.tables-summary-icon {
    font-size: 32px;
    opacity: 0.9;
}

.tables-summary-content {
    display: flex;
    flex-direction: column;
}

.tables-summary-value {
    font-size: 24px;
    font-weight: 700;
    color: #fff;
}

.tables-summary-label {
    font-size: 12px;
    color: #71717a;
    text-transform: uppercase;
    letter-spacing: 0.5px;
}

/* Table cards header */
.table-cards-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin: 24px 0 16px;
}

.table-cards-header h3 {
    font-size: 14px;
    font-weight: 600;
    color: #a1a1aa;
    text-transform: uppercase;
    letter-spacing: 0.5px;
}

.severity-legend {
    display: flex;
    gap: 16px;
    font-size: 12px;
    color: #71717a;
}

.severity-dot {
    display: inline-block;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    margin-right: 4px;
}

.severity-dot.critical { background: #ef4444; }
.severity-dot.high { background: #f97316; }
.severity-dot.medium { background: #eab308; }
.severity-dot.low { background: #22c55e; }

/* Table cards grid */
.table-cards-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(340px, 1fr));
    gap: 16px;
    margin-bottom: 24px;
}

.table-card {
    background: #12121a;
    border-radius: 12px;
    border: 1px solid #1e1e2a;
    overflow: hidden;
    transition: transform 0.15s, box-shadow 0.15s;
}

.table-card:hover {
    transform: translateY(-2px);
    box-shadow: 0 4px 20px rgba(0, 0, 0, 0.3);
}

.table-card-header {
    padding: 16px;
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    border-bottom: 1px solid #1e1e2a;
}

.table-card-header.critical { border-left: 4px solid #ef4444; }
.table-card-header.high { border-left: 4px solid #f97316; }
.table-card-header.medium { border-left: 4px solid #eab308; }
.table-card-header.low { border-left: 4px solid #22c55e; }

.table-card-name {
    font-size: 15px;
    font-weight: 600;
    color: #e4e4e7;
    word-break: break-word;
}

.table-card-badge {
    font-size: 11px;
    padding: 2px 8px;
    border-radius: 4px;
    font-weight: 500;
    white-space: nowrap;
}

.table-card-badge.critical { background: #ef444420; color: #ef4444; }
.table-card-badge.high { background: #f9731620; color: #f97316; }
.table-card-badge.medium { background: #eab30820; color: #eab308; }
.table-card-badge.low { background: #22c55e20; color: #22c55e; }

.table-card-metrics {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 12px;
    padding: 16px;
}

.table-card-metric {
    display: flex;
    flex-direction: column;
}

.table-card-metric-value {
    font-size: 18px;
    font-weight: 600;
    color: #fff;
}

.table-card-metric-value.major {
    color: #f87171;
}

.table-card-metric-label {
    font-size: 11px;
    color: #71717a;
    text-transform: uppercase;
}

.table-card-bar {
    height: 6px;
    background: #0a0a0f;
    margin: 0 16px;
}

.table-card-bar-fill {
    height: 100%;
    border-radius: 3px;
    transition: width 0.3s ease;
}

.table-card-bar-fill.critical { background: #ef4444; }
.table-card-bar-fill.high { background: #f97316; }
.table-card-bar-fill.medium { background: #eab308; }
.table-card-bar-fill.low { background: #22c55e; }

.table-card-footer {
    padding: 12px 16px;
    background: #0a0a0f;
    font-size: 12px;
    color: #71717a;
    display: flex;
    justify-content: space-between;
    align-items: center;
}

.table-card-op {
    color: #a1a1aa;
}

.table-card-link {
    color: #3b82f6;
    text-decoration: none;
    font-size: 11px;
}

.table-card-link:hover {
    text-decoration: underline;
}

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

/* Hero section with donut chart */
.btree-hero {
    display: flex;
    gap: 24px;
    margin-bottom: 16px;
    background: #12121a;
    border-radius: 12px;
    padding: 24px;
    border: 1px solid #1e1e2a;
}

.btree-hero-chart {
    display: flex;
    align-items: center;
    gap: 24px;
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

/* Stat cards */
.btree-hero-stats {
    display: flex;
    flex-direction: column;
    gap: 12px;
    flex: 1;
}

.btree-stat-card {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 16px 20px;
    border-radius: 10px;
    background: #0a0a0f;
    border-left: 4px solid;
}

.btree-stat-card.btree-branch { border-color: var(--color-branch); }
.btree-stat-card.btree-leaf { border-color: var(--color-leaf); }
.btree-stat-card.btree-ratio { border-color: #3b82f6; }

.btree-stat-icon {
    font-size: 28px;
    opacity: 0.9;
}

.btree-stat-content {
    display: flex;
    flex-direction: column;
}

.btree-stat-value {
    font-size: 24px;
    font-weight: 700;
    color: #fff;
}

.btree-stat-label {
    font-size: 13px;
    color: #a1a1aa;
    font-weight: 500;
}

.btree-stat-hint {
    font-size: 11px;
    color: #52525b;
}

/* Histogram stats row */
.histogram-stats {
    display: flex;
    gap: 16px;
    margin-bottom: 16px;
    flex-wrap: wrap;
}

.hist-stat {
    display: flex;
    flex-direction: column;
    align-items: center;
    min-width: 60px;
}

.hist-label {
    font-size: 10px;
    color: #71717a;
    text-transform: uppercase;
    letter-spacing: 0.5px;
}

.hist-value {
    font-size: 18px;
    font-weight: 600;
    color: #3b82f6;
}

.hist-value.major {
    color: #f87171;
}

/* B+ Tree explainer */
.btree-explainer {
    display: flex;
    flex-direction: column;
    gap: 16px;
}

.explainer-visual {
    display: flex;
    justify-content: center;
    padding: 16px;
}

.tree-diagram {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
}

.tree-level {
    display: flex;
    gap: 8px;
}

.tree-node {
    padding: 8px 16px;
    border-radius: 6px;
    font-size: 12px;
    font-weight: 500;
}

.tree-node.branch-node {
    background: var(--color-branch);
    color: #000;
}

.tree-node.leaf-node {
    background: var(--color-leaf);
    color: #000;
}

.explainer-text {
    font-size: 13px;
    color: #a1a1aa;
    line-height: 1.6;
}

.explainer-text p {
    margin-bottom: 8px;
}

.explainer-text strong {
    color: #e4e4e7;
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

// Horizontal bar chart for fault distribution (dynamically sized)
function drawFaultDistChart(canvas, labels, totalFaults, majorFaults) {
    const ctx = canvas.getContext('2d');
    const rect = canvas.getBoundingClientRect();
    if (rect.width === 0) return;

    // Dynamic height based on number of bars
    const barHeight = 22;
    const barGap = 12;
    const pad = { t: 12, r: 80, b: 12, l: 160 };
    const chartH = labels.length * (barHeight + barGap) - barGap;
    const totalH = chartH + pad.t + pad.b;

    // Set canvas size
    canvas.style.height = totalH + 'px';
    canvas.width = rect.width * 2;
    canvas.height = totalH * 2;
    ctx.scale(2, 2);

    const w = rect.width;
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
    const rect = canvas.getBoundingClientRect();
    if (rect.width === 0) return;

    // Dynamic height based on number of bars
    const barHeight = 22;
    const barGap = 12;
    const pad = { t: 12, r: 70, b: 12, l: 160 };
    const chartH = labels.length * (barHeight + barGap) - barGap;
    const totalH = chartH + pad.t + pad.b;

    // Set canvas size
    canvas.style.height = totalH + 'px';
    canvas.width = rect.width * 2;
    canvas.height = totalH * 2;
    ctx.scale(2, 2);

    const w = rect.width;
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
    else if (name === 'btree') initBTree();
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

function renderTableCards(unified, totalFaults) {
    const grid = document.getElementById('table-cards-grid');
    if (!grid) return;

    // Calculate severity for each table
    const getSeverity = (pct) => {
        if (pct > 30) return 'critical';
        if (pct > 15) return 'high';
        if (pct > 5) return 'medium';
        return 'low';
    };

    grid.innerHTML = unified.map(t => {
        const pct = totalFaults > 0 ? (t.faults / totalFaults) * 100 : 0;
        const severity = getSeverity(pct);

        const ioTime = t.time_lost_ms >= 1000
            ? (t.time_lost_ms / 1000).toFixed(1) + 's'
            : t.time_lost_ms.toFixed(0) + 'ms';

        // Get Reth source link if available
        const sourceLink = RETH_TABLE_SOURCES[t.name];
        const sourceLinkHtml = sourceLink
            ? `<a href="${sourceLink.github_url}" target="_blank" class="table-card-link">View in Reth</a>`
            : '';

        return `
            <div class="table-card">
                <div class="table-card-header ${severity}">
                    <div class="table-card-name">${t.name}</div>
                    <span class="table-card-badge ${severity}">${pct.toFixed(1)}%</span>
                </div>
                <div class="table-card-metrics">
                    <div class="table-card-metric">
                        <span class="table-card-metric-value">${fmt(t.faults)}</span>
                        <span class="table-card-metric-label">Faults</span>
                    </div>
                    <div class="table-card-metric">
                        <span class="table-card-metric-value major">${fmt(t.major_faults)}</span>
                        <span class="table-card-metric-label">Major</span>
                    </div>
                    <div class="table-card-metric">
                        <span class="table-card-metric-value">${t.slow_ops > 0 ? fmt(t.slow_ops) : '-'}</span>
                        <span class="table-card-metric-label">Slow Ops</span>
                    </div>
                    <div class="table-card-metric">
                        <span class="table-card-metric-value">${t.time_lost_ms > 0 ? ioTime : '-'}</span>
                        <span class="table-card-metric-label">I/O Time</span>
                    </div>
                </div>
                <div class="table-card-bar">
                    <div class="table-card-bar-fill ${severity}" style="width: ${Math.min(pct, 100)}%;"></div>
                </div>
                <div class="table-card-footer">
                    <span class="table-card-op">${t.top_operation || 'No operations traced'}</span>
                    ${sourceLinkHtml}
                </div>
            </div>
        `;
    }).join('');
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

    // Draw fault distribution and I/O time charts
    if (unified.length > 0) {
        document.getElementById('fault-dist-row').style.display = 'grid';

        // Fault distribution chart (top 8 by faults)
        const faultCanvas = document.getElementById('fault-dist-chart');
        const topByFaults = unified.slice(0, 8);
        drawFaultDistChart(faultCanvas,
            topByFaults.map(t => t.name),
            topByFaults.map(t => t.faults),
            topByFaults.map(t => t.major_faults)
        );

        // I/O time chart (top 8 by time lost, filtered to those with actual I/O time)
        const ioCanvas = document.getElementById('io-time-chart');
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

    // Render table cards
    renderTableCards(unified, totalFaults);

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

// ============================================
// B+ TREE TAB
// ============================================

function initBTree() {
    const pt = DATA.page_type_stats;
    const oh = DATA.operation_histogram;
    const tt = DATA.tree_traversal;

    // Check if we have data
    if (!pt || !pt.has_data) {
        document.getElementById('btree-no-data').style.display = 'block';
        document.getElementById('btree-content').style.display = 'none';
        return;
    }

    document.getElementById('btree-no-data').style.display = 'none';
    document.getElementById('btree-content').style.display = 'block';

    // Page type donut chart
    drawPageTypeDonut(pt);

    // Update hero stats
    const branchFaults = pt.by_type.find(t => t.page_type === 'Branch');
    const leafFaults = pt.by_type.find(t => t.page_type === 'Leaf');
    document.getElementById('branch-faults').textContent = fmt(branchFaults ? branchFaults.total_faults : 0);
    document.getElementById('leaf-faults').textContent = fmt(leafFaults ? leafFaults.total_faults : 0);
    document.getElementById('traversal-ratio').textContent = pt.traversal_to_data_ratio.toFixed(2);
    document.getElementById('page-type-total').textContent = fmt(pt.total_faults);

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

    // Operation histogram
    if (oh && oh.has_data) {
        document.getElementById('hist-avg').textContent = oh.avg_faults_per_op.toFixed(1);
        document.getElementById('hist-p50').textContent = oh.p50_faults;
        document.getElementById('hist-p95').textContent = oh.p95_faults;
        document.getElementById('hist-p99').textContent = oh.p99_faults;
        document.getElementById('hist-max').textContent = oh.max_faults_per_op;
        drawFaultsHistogram(oh);
    }

    // Per-table tree bars
    if (tt && tt.has_data && tt.tables.length) {
        drawTableTreeBars(tt);
    }
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

function drawFaultsHistogram(oh) {
    const canvas = document.getElementById('faults-histogram');
    const ctx = canvas.getContext('2d');
    const dpr = window.devicePixelRatio || 1;

    const rect = canvas.parentElement.getBoundingClientRect();
    const width = rect.width || 400;
    const height = 180;

    canvas.width = width * dpr;
    canvas.height = height * dpr;
    canvas.style.width = width + 'px';
    canvas.style.height = height + 'px';
    ctx.scale(dpr, dpr);

    const pad = { t: 10, r: 20, b: 40, l: 50 };
    const chartW = width - pad.l - pad.r;
    const chartH = height - pad.t - pad.b;

    // Background
    ctx.fillStyle = '#0a0a0f';
    ctx.fillRect(pad.l, pad.t, chartW, chartH);

    // Find max count
    const maxCount = Math.max(...oh.distribution.map(b => b.count), 1);

    // Draw bars
    const barWidth = chartW / oh.distribution.length - 10;
    const barGap = 10;

    oh.distribution.forEach((bucket, i) => {
        const barHeight = (bucket.count / maxCount) * chartH;
        const x = pad.l + i * (barWidth + barGap) + barGap / 2;
        const y = pad.t + chartH - barHeight;

        // Gradient fill
        const gradient = ctx.createLinearGradient(x, y, x, y + barHeight);
        gradient.addColorStop(0, '#3b82f6');
        gradient.addColorStop(1, '#1d4ed8');
        ctx.fillStyle = gradient;
        ctx.fillRect(x, y, barWidth, barHeight);

        // Label
        ctx.fillStyle = '#71717a';
        ctx.font = '11px sans-serif';
        ctx.textAlign = 'center';
        ctx.fillText(bucket.label, x + barWidth / 2, height - 12);

        // Count on top
        if (bucket.count > 0) {
            ctx.fillStyle = '#e4e4e7';
            ctx.fillText(fmt(bucket.count), x + barWidth / 2, y - 5);
        }
    });

    // Y-axis label
    ctx.fillStyle = '#71717a';
    ctx.font = '11px sans-serif';
    ctx.save();
    ctx.translate(15, height / 2);
    ctx.rotate(-Math.PI / 2);
    ctx.textAlign = 'center';
    ctx.fillText('Operations', 0, 0);
    ctx.restore();

    // X-axis label
    ctx.textAlign = 'center';
    ctx.fillText('Faults per operation', width / 2, height - 2);
}

function drawTableTreeBars(tt) {
    const container = document.getElementById('table-tree-bars');
    if (!container) return;

    // Find max total for scaling
    const maxTotal = Math.max(...tt.tables.map(t => t.total_faults), 1);

    container.innerHTML = tt.tables.slice(0, 15).map(table => {
        const branchPct = (table.branch_faults / table.total_faults) * 100;
        const leafPct = (table.leaf_faults / table.total_faults) * 100;
        const overflowPct = (table.overflow_faults / table.total_faults) * 100;
        const widthPct = (table.total_faults / maxTotal) * 100;

        return `
            <div class="table-tree-bar">
                <div class="table-tree-name" title="${table.name}">${table.name}</div>
                <div class="table-tree-bar-container" style="width: ${widthPct}%;">
                    <div class="table-tree-segment branch" style="width: ${branchPct}%;" title="Branch: ${fmt(table.branch_faults)}"></div>
                    <div class="table-tree-segment leaf" style="width: ${leafPct}%;" title="Leaf: ${fmt(table.leaf_faults)}"></div>
                    <div class="table-tree-segment overflow" style="width: ${overflowPct}%;" title="Overflow: ${fmt(table.overflow_faults)}"></div>
                </div>
                <div class="table-tree-stats">
                    <div class="table-tree-stat">
                        <span class="table-tree-stat-label">B:</span>
                        <span class="table-tree-stat-value branch">${fmt(table.branch_faults)}</span>
                    </div>
                    <div class="table-tree-stat">
                        <span class="table-tree-stat-label">L:</span>
                        <span class="table-tree-stat-value leaf">${fmt(table.leaf_faults)}</span>
                    </div>
                    <div class="table-tree-stat">
                        <span class="table-tree-stat-label">Ratio:</span>
                        <span class="table-tree-stat-value">${table.branch_leaf_ratio.toFixed(2)}</span>
                    </div>
                </div>
            </div>
        `;
    }).join('');
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
