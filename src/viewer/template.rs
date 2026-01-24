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
            <button class="tab" data-tab="resources">Resources</button>
            <button class="export-btn" id="export-compact-btn" title="Download JSON for analysis">Export</button>
        </nav>

        <main>
            <!-- OVERVIEW TAB -->
            <section id="overview" class="panel active">
                <!-- Compact metrics row with grouped related data -->
                <div class="metrics-row">
                    <div class="metric">
                        <span class="metric-label">Duration</span>
                        <span class="metric-value" id="duration"></span>
                    </div>
                    <div class="metric" id="block-range-metric" style="display:none;">
                        <span class="metric-label">Blocks</span>
                        <span class="metric-value" id="block-range"></span>
                    </div>
                    <div class="metrics-group">
                        <div class="metric">
                            <span class="metric-label">Faults</span>
                            <span class="metric-value" id="total-faults"></span>
                        </div>
                        <div class="metric">
                            <span class="metric-label">Major</span>
                            <span class="metric-value major" id="major-faults"></span>
                        </div>
                        <div class="metric">
                            <span class="metric-label">Minor</span>
                            <span class="metric-value minor" id="minor-faults"></span>
                        </div>
                    </div>
                    <div class="metric">
                        <span class="metric-label">Rate</span>
                        <span class="metric-value" id="fault-rate"></span>
                        <span class="metric-unit">/s</span>
                    </div>
                    <div class="metric">
                        <span class="metric-label">I/O Ratio</span>
                        <span class="metric-value" id="major-ratio"></span>
                    </div>
                </div>

                <!-- Access Heatmap (full width, larger) -->
                <div class="card" style="margin-bottom: 16px;">
                    <div class="card-header">Access Heatmap <span class="axis-hint">(drag to zoom, dbl-click to reset)</span></div>
                    <div class="card-body heatmap-container" style="position: relative;">
                        <div id="heatmap-plotly" style="width: 100%; height: 100%;"></div>
                    </div>
                </div>

                <!-- Access Pattern - single card, full width -->
                <div class="card">
                    <div class="card-header">Access Pattern</div>
                    <div class="card-body">
                        <div class="access-pattern-row">
                            <div class="pattern-section">
                                <div class="section-title">Distribution <span class="help-icon" data-tooltip="How page faults are distributed across the database file. Sequential means consecutive pages (good for OS prefetch). Random means scattered access (typical for B+ tree traversal like trie lookups).">?</span></div>
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
                                <div class="section-title">Stride Patterns <span class="help-icon" data-tooltip="Common distances (in pages) between consecutive accesses. Large strides = jumping across the file (trie traversal). Small strides = sequential scanning.">?</span></div>
                                <div id="stride-list"></div>
                            </div>
                        </div>
                    </div>
                </div>

                <!-- Section separator -->
                <div class="section-separator">
                    <span class="separator-label">Table Analysis</span>
                </div>

                <!-- Tables section (merged from Tables tab) -->
                <div id="tables-content">
                    <div id="tables-no-data" class="no-data" style="display:none;">
                        No table data. Run with <code>--trace-cursors</code> for full attribution.
                    </div>

                    <!-- Sortable table -->
                    <div class="card full-width">
                        <div class="card-header">
                            Tables
                            <span class="card-hint">Click column headers to sort, click row to expand</span>
                            <span class="expand-controls">
                                <button class="expand-btn" id="expand-all-btn" title="Expand all rows">Expand All</button>
                                <button class="expand-btn" id="collapse-all-btn" title="Collapse all rows">Collapse All</button>
                            </span>
                        </div>
                        <div class="card-body compact-table-container" style="max-height: none; padding: 0;">
                            <table class="compact-table expandable-table sortable-table" id="unified-tables">
                                <thead>
                                    <tr>
                                        <th style="width:30px;"></th>
                                        <th data-sort="name">Table</th>
                                        <th data-sort="faults" class="sortable sorted-desc">Faults <span class="sort-icon">▼</span></th>
                                        <th data-sort="major_pct" class="sortable">Major %</th>
                                        <th data-sort="bl_ratio" class="sortable">B:L Ratio</th>
                                        <th data-sort="working_set" class="sortable">Working Set</th>
                                        <th data-sort="reuse" class="sortable">Reuse %</th>
                                        <th data-sort="io_time" class="sortable">I/O Time</th>
                                        <th data-sort="cpu" class="sortable">CPU %</th>
                                    </tr>
                                </thead>
                                <tbody></tbody>
                            </table>
                        </div>
                    </div>
                </div>
            </section>

            <!-- Hidden Tables tab for compatibility (data still loads here) -->
            <section id="tables" class="panel" style="display:none;"></section>

            <!-- RESOURCES TAB - Compact layout -->
            <section id="resources" class="panel">
                <div id="resources-content">
                    <!-- Combined Memory + Txn metrics in single row -->
                    <div class="metrics-row">
                        <div class="metrics-group" id="memory-metrics-group">
                            <div class="metric">
                                <span class="metric-label">Working Set</span>
                                <span class="metric-value" id="mem-working-set">-</span>
                            </div>
                            <div class="metric">
                                <span class="metric-label">Unique Pages</span>
                                <span class="metric-value" id="mem-unique-pages">-</span>
                            </div>
                            <div class="metric">
                                <span class="metric-label">Reuse</span>
                                <span class="metric-value" id="mem-reuse-ratio">-</span>
                            </div>
                        </div>
                        <div class="metrics-group" id="txn-metrics-group">
                            <div class="metric">
                                <span class="metric-label">Txns</span>
                                <span class="metric-value" id="txn-total">-</span>
                            </div>
                            <div class="metric">
                                <span class="metric-label">RO</span>
                                <span class="metric-value minor" id="txn-ro">-</span>
                            </div>
                            <div class="metric">
                                <span class="metric-label">RW</span>
                                <span class="metric-value major" id="txn-rw">-</span>
                            </div>
                            <div class="metric">
                                <span class="metric-label">Commits</span>
                                <span class="metric-value" id="txn-commits">-</span>
                            </div>
                        </div>
                        <div class="metrics-group" id="concurrency-metrics-group">
                            <div class="metric">
                                <span class="metric-label">Max RO</span>
                                <span class="metric-value" id="txn-max-ro">-</span>
                            </div>
                            <div class="metric">
                                <span class="metric-label">Max RW</span>
                                <span class="metric-value" id="txn-max-rw">-</span>
                            </div>
                            <div class="metric">
                                <span class="metric-label">Avg RO</span>
                                <span class="metric-value" id="txn-avg-ro">-</span>
                            </div>
                        </div>
                    </div>

                    <div id="memory-no-data" class="no-data" style="display:none;">
                        No working set data available.
                    </div>
                    <div id="txn-no-data" class="no-data" style="display:none;">
                        No transaction data available.
                    </div>

                    <!-- Commit Latency Chart -->
                    <div class="card" id="txn-content">
                        <div class="card-header">Commit Latency
                            <span class="latency-inline-stats">
                                <span class="lat-inline"><span class="lat-label">AVG</span> <span id="txn-avg-latency">-</span></span>
                                <span class="lat-inline"><span class="lat-label">P95</span> <span id="txn-p95-latency">-</span></span>
                                <span class="lat-inline major"><span class="lat-label">P99</span> <span id="txn-p99-latency">-</span></span>
                                <span class="lat-inline major"><span class="lat-label">MAX</span> <span id="txn-max-latency">-</span></span>
                            </span>
                        </div>
                        <div class="card-body">
                            <div class="uplot-container" id="txn-latency-chart" style="height: 200px;"></div>
                        </div>
                    </div>

                    <!-- Hidden fields for JS compatibility -->
                    <div style="display:none;">
                        <span id="txn-rate"></span>
                        <span id="txn-aborts"></span>
                        <span id="mem-avg-accesses"></span>
                        <span id="memory-summary-text"></span>
                        <div id="memory-content"></div>
                        <div id="access-count-chart"></div>
                    </div>

                    <!-- Thread Activity Swimlane -->
                    <div class="card" id="thread-swimlane-card" style="margin-top: 16px;">
                        <div class="card-header">Thread Activity
                            <span class="swimlane-legend">
                                <span class="legend-item"><span class="legend-swatch" style="background: #60a5fa;"></span> Minor</span>
                                <span class="legend-item"><span class="legend-swatch" style="background: #ef4444;"></span> Major</span>
                                <span class="legend-item"><span class="legend-swatch" style="background: rgba(251, 191, 36, 0.4); border: 1px solid #f59e0b;"></span> RW Commit</span>
                            </span>
                        </div>
                        <div class="card-body" style="padding: 0;">
                            <div id="thread-swimlane" style="width: 100%; height: 400px;"></div>
                        </div>
                    </div>
                    <div id="thread-swimlane-no-data" class="no-data" style="display:none;">
                        No thread activity data available.
                    </div>

                    <!-- Thread Table Attribution -->
                    <div class="card" id="thread-tables-card" style="margin-top: 16px; display: none;">
                        <div class="card-header">Thread Table Attribution
                            <span class="card-hint">Which tables each thread accesses</span>
                        </div>
                        <div class="card-body compact-table-container" style="padding: 0;">
                            <table class="compact-table" id="thread-tables">
                                <thead>
                                    <tr>
                                        <th>Thread</th>
                                        <th>Total Faults</th>
                                        <th>Major %</th>
                                        <th>Top Tables</th>
                                    </tr>
                                </thead>
                                <tbody></tbody>
                            </table>
                        </div>
                    </div>

                    <!-- Hidden tables for JS compatibility -->
                    <div style="display:none;">
                        <table id="threads-faults-table"><tbody></tbody></table>
                        <table id="threads-txn-table"><tbody></tbody></table>
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

/* Header/Tabs */
.tabs {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-bottom: 20px;
    background: rgba(18, 18, 26, 0.6);
    backdrop-filter: blur(12px);
    -webkit-backdrop-filter: blur(12px);
    padding: 4px;
    border-radius: 8px;
    border: 1px solid rgba(255, 255, 255, 0.06);
}

.tab {
    padding: 8px 16px;
    background: transparent;
    border: none;
    color: #71717a;
    cursor: pointer;
    border-radius: 6px;
    font-size: 13px;
    font-weight: 500;
    transition: all 0.15s;
}

.tab:hover { background: rgba(255, 255, 255, 0.05); color: #a1a1aa; }
.tab.active { background: rgba(59, 130, 246, 0.15); color: #60a5fa; }

.export-btn {
    margin-left: auto;
    padding: 8px 14px;
    background: rgba(59, 130, 246, 0.1);
    color: #60a5fa;
    border: 1px solid rgba(59, 130, 246, 0.2);
    border-radius: 6px;
    cursor: pointer;
    font-size: 12px;
    font-weight: 500;
    transition: all 0.15s;
}
.export-btn:hover { background: rgba(59, 130, 246, 0.2); border-color: rgba(59, 130, 246, 0.3); }

/* Panels */
.panel { display: none; }
.panel.active { display: block; }

/* Metrics row - compact inline design */
.metrics-row {
    display: flex;
    gap: 8px;
    margin-bottom: 16px;
    flex-wrap: wrap;
}

.metric {
    background: rgba(18, 18, 26, 0.6);
    padding: 10px 14px;
    border-radius: 6px;
    display: flex;
    align-items: center;
    gap: 8px;
    border: 1px solid rgba(255, 255, 255, 0.04);
}

.metric-value {
    font-size: 16px;
    font-weight: 600;
    color: #e4e4e7;
    font-variant-numeric: tabular-nums;
}
.metric-value.major { color: #f87171; }
.metric-value.minor { color: #34d399; }

.metric-label {
    font-size: 11px;
    color: #52525b;
    text-transform: uppercase;
    letter-spacing: 0.3px;
}

/* Grouped metrics for correlated data */
.metrics-group {
    display: flex;
    gap: 2px;
    background: rgba(18, 18, 26, 0.6);
    border-radius: 6px;
    border: 1px solid rgba(255, 255, 255, 0.04);
    overflow: hidden;
}

.metrics-group .metric {
    background: transparent;
    border: none;
    border-radius: 0;
    border-right: 1px solid rgba(255, 255, 255, 0.04);
}

.metrics-group .metric:last-child {
    border-right: none;
}

.metric-unit {
    font-size: 11px;
    color: #52525b;
    margin-left: -4px;
}

/* Access pattern row layout */
.access-pattern-row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 32px;
    align-items: start;
}

@media (max-width: 800px) {
    .access-pattern-row {
        grid-template-columns: 1fr;
        gap: 20px;
    }
}

/* Section separator */
.section-separator {
    display: flex;
    align-items: center;
    margin: 24px 0 16px 0;
    gap: 16px;
}

.section-separator::before,
.section-separator::after {
    content: '';
    flex: 1;
    height: 1px;
    background: linear-gradient(90deg, transparent, #2a2a3a, transparent);
}

.separator-label {
    font-size: 11px;
    font-weight: 600;
    color: #52525b;
    text-transform: uppercase;
    letter-spacing: 1px;
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

/* Sortable table headers */
.sortable-table th.sortable {
    cursor: pointer;
    user-select: none;
    transition: color 0.15s;
}

.sortable-table th.sortable:hover {
    color: #60a5fa;
}

.sortable-table th.sorted-asc,
.sortable-table th.sorted-desc {
    color: #60a5fa;
}

.sortable-table .sort-icon {
    font-size: 10px;
    margin-left: 4px;
    opacity: 0.7;
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

/* Expand/Collapse controls */
.expand-controls {
    display: flex;
    gap: 6px;
    margin-left: 12px;
}

.expand-btn {
    padding: 4px 10px;
    font-size: 11px;
    font-weight: 500;
    color: #a1a1aa;
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 4px;
    cursor: pointer;
    transition: all 0.15s;
    text-transform: none;
}

.expand-btn:hover {
    color: #e4e4e7;
    background: rgba(255, 255, 255, 0.1);
    border-color: rgba(255, 255, 255, 0.2);
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

/* Swimlane styles */
.swimlane-legend {
    display: flex;
    gap: 16px;
    font-size: 11px;
    font-weight: 400;
    text-transform: none;
    margin-left: auto;
}

.thread-swimlane-container {
    width: 100%;
    overflow-x: auto;
}

.thread-swimlane-row {
    display: flex;
    align-items: center;
    border-bottom: 1px solid #1e1e2a;
}

.thread-swimlane-row:last-child {
    border-bottom: none;
}

.thread-swimlane-label {
    width: 100px;
    min-width: 100px;
    padding: 8px 12px;
    font-size: 11px;
    color: #a1a1aa;
    background: #0a0a0f;
    border-right: 1px solid #1e1e2a;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.thread-swimlane-track {
    flex: 1;
    height: 40px;
    position: relative;
    background: #12121a;
}

.thread-swimlane-canvas {
    width: 100%;
    height: 100%;
    display: block;
}

.thread-swimlane-time-axis {
    display: flex;
    margin-left: 100px;
    padding: 4px 0;
    background: #0a0a0f;
    border-top: 1px solid #1e1e2a;
}

.thread-swimlane-time-axis span {
    flex: 1;
    text-align: center;
    font-size: 10px;
    color: #52525b;
}

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
    margin-bottom: 0;
}

.pattern-explanation,
.stride-explanation {
    margin-bottom: 12px;
}

.explanation-text {
    font-size: 12px;
    color: #71717a;
    line-height: 1.5;
}

.explanation-text strong {
    color: #a1a1aa;
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

/* Help icon with tooltip */
.help-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    font-size: 10px;
    font-weight: 600;
    color: #52525b;
    background: rgba(82, 82, 91, 0.2);
    border: 1px solid rgba(82, 82, 91, 0.3);
    border-radius: 50%;
    cursor: help;
    margin-left: 6px;
    vertical-align: middle;
    position: relative;
    transition: all 0.15s;
}

.help-icon:hover {
    color: #a1a1aa;
    background: rgba(82, 82, 91, 0.3);
    border-color: rgba(161, 161, 170, 0.4);
}

.help-icon::after {
    content: attr(data-tooltip);
    position: fixed;
    top: var(--tooltip-top, auto);
    left: var(--tooltip-left, auto);
    transform: var(--tooltip-transform, translateY(0));
    background: #1e1e2a;
    color: #d4d4d8;
    font-size: 12px;
    font-weight: 400;
    text-transform: none;
    letter-spacing: normal;
    line-height: 1.5;
    padding: 10px 14px;
    border-radius: 6px;
    border: 1px solid #3b82f6;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
    white-space: normal;
    width: 280px;
    max-width: 90vw;
    opacity: 0;
    visibility: hidden;
    transition: opacity 0.15s, visibility 0.15s;
    z-index: 1000;
    pointer-events: none;
}

.help-icon:hover::after {
    opacity: 1;
    visibility: visible;
}

/* Stride section */
.stride-section {
    margin-bottom: 0;
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

/* Latency inline stats in card header */
.latency-inline-stats {
    display: flex;
    gap: 12px;
    font-size: 11px;
    font-weight: 400;
    text-transform: none;
    color: #a1a1aa;
}

.lat-inline {
    display: flex;
    gap: 4px;
    align-items: center;
}

.lat-inline .lat-label {
    color: #52525b;
    font-size: 10px;
}

.lat-inline.major {
    color: #f87171;
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

// Plotly bar chart for commit latency timeline
function createCommitLatencyChart(container, commitData) {
    const el = typeof container === 'string' ? document.getElementById(container) : container;
    if (!el || !commitData.length) return null;

    // Extract timestamps (seconds) and latencies (ms)
    const timestamps = commitData.map(p => p.time_secs);
    const latencies = commitData.map(p => p.latency_ms);

    // Calculate p95 threshold for coloring outliers
    const sortedLatencies = [...latencies].sort((a, b) => a - b);
    const p95Index = Math.floor(sortedLatencies.length * 0.95);
    const p95Threshold = sortedLatencies[p95Index] || sortedLatencies[sortedLatencies.length - 1];

    // Color each bar based on whether it's an outlier
    const colors = latencies.map(v => v >= p95Threshold ? '#f87171' : '#3b82f6');

    const trace = {
        x: timestamps,
        y: latencies,
        type: 'bar',
        marker: {
            color: colors,
            line: { width: 0 }
        },
        hovertemplate: '<b>%{y:.1f}ms</b> at %{x:.1f}s<extra></extra>'
    };

    const layout = {
        paper_bgcolor: 'transparent',
        plot_bgcolor: 'transparent',
        margin: { t: 10, r: 20, b: 40, l: 55 },
        xaxis: {
            title: { text: '', font: { size: 10, color: '#52525b' } },
            tickfont: { size: 10, color: '#71717a' },
            gridcolor: '#1e1e2a',
            linecolor: '#1e1e2a',
            ticksuffix: 's'
        },
        yaxis: {
            title: { text: '', font: { size: 10, color: '#52525b' } },
            tickfont: { size: 10, color: '#71717a' },
            gridcolor: '#1e1e2a',
            linecolor: '#1e1e2a',
            ticksuffix: 'ms',
            rangemode: 'tozero'
        },
        bargap: 0.1,
        hovermode: 'closest'
    };

    const config = {
        responsive: true,
        displayModeBar: false
    };

    Plotly.newPlot(el, [trace], layout, config);
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
function initPlotlyHeatmap(container, data, timelineData, durationSecs) {
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
    const hoverTexts = [];  // Custom hover text with attribution
    for (let o = 0; o < offset_buckets; o++) {
        const row = [];
        const hoverRow = [];
        for (let t = 0; t < time_buckets; t++) {
            const idx = t * offset_buckets + o;
            const count = cells[idx] || 0;
            row.push(count);

            // Build hover text with attribution
            const timeRange = max_time_ms - min_time_ms;
            const offsetRange = max_offset_gb - min_offset_gb;
            const time = min_time_ms + (t + 0.5) * timeRange / time_buckets;
            const offset = min_offset_gb + (o + 0.5) * offsetRange / offset_buckets;
            const timeStr = time < 60000
                ? (time / 1000).toFixed(2) + 's'
                : (time / 60000).toFixed(2) + 'm';

            let hoverText = '<b>TIME:</b> ' + timeStr + '<br>' +
                           '<b>OFFSET:</b> ' + offset.toFixed(2) + ' GB<br>' +
                           '<b style="color:#60a5fa">' + count + ' faults</b>';

            // Add table attribution if available
            const tables = attrLookup[idx];
            if (tables && tables.length > 0) {
                hoverText += '<br><br><b>TOP TABLES:</b>';
                tables.slice(0, 5).forEach(([name, total, major]) => {
                    const majorPct = total > 0 ? Math.round(major / total * 100) : 0;
                    hoverText += '<br>' + name + ': ' + total + ' (' + majorPct + '% major)';
                });
            }

            hoverRow.push(hoverText);
        }
        z.push(row);
        hoverTexts.push(hoverRow);
    }

    // Generate axis values (numeric for proper alignment with timeline overlay)
    const timeRange = max_time_ms - min_time_ms;
    const offsetRange = max_offset_gb - min_offset_gb;

    const xValues = [];
    for (let t = 0; t < time_buckets; t++) {
        const time = min_time_ms + (t + 0.5) * timeRange / time_buckets;
        xValues.push(time / 1000);  // Convert to seconds
    }

    const yValues = [];
    for (let o = 0; o < offset_buckets; o++) {
        const offset = min_offset_gb + (o + 0.5) * offsetRange / offset_buckets;
        yValues.push(offset);
    }

    // Heatmap trace
    const heatmapTrace = {
        z: z,
        x: xValues,
        y: yValues,
        type: 'heatmap',
        colorscale: [
            [0, '#000000'],
            [0.1, '#1a1a3e'],
            [0.25, '#14147a'],
            [0.5, '#1478b4'],
            [0.75, '#3cfff0'],
            [1, '#ffff50']
        ],
        hoverinfo: 'text',
        text: hoverTexts,
        showscale: true,
        colorbar: {
            title: 'Faults',
            titleside: 'right',
            tickfont: { color: '#a1a1aa', size: 10 },
            titlefont: { color: '#a1a1aa', size: 11 }
        },
        zsmooth: false
    };

    const traces = [heatmapTrace];

    // Add fault timeline as area overlay at bottom of chart
    // Aggregate heatmap columns to get fault rate per time bucket (more accurate than separate timeline)
    if (time_buckets > 0 && offset_buckets > 0) {
        const timelineX = [];
        const timelineY = [];

        // Sum faults per time column from heatmap data
        const columnSums = [];
        for (let t = 0; t < time_buckets; t++) {
            let sum = 0;
            for (let o = 0; o < offset_buckets; o++) {
                sum += z[o][t];
            }
            columnSums.push(sum);
        }

        const maxSum = Math.max(...columnSums, 1);

        for (let t = 0; t < time_buckets; t++) {
            const time = min_time_ms + (t + 0.5) * timeRange / time_buckets;
            timelineX.push(time / 1000);  // Same X as heatmap
            // Scale to bottom 30% of chart
            const normalizedY = min_offset_gb + (columnSums[t] / maxSum) * offsetRange * 0.3;
            timelineY.push(normalizedY);
        }

        const timelineTrace = {
            x: timelineX,
            y: timelineY,
            type: 'scatter',
            mode: 'lines',
            name: 'Fault Rate',
            line: {
                color: 'rgba(251, 146, 60, 0.25)',
                width: 1
            },
            fill: 'tozeroy',
            fillcolor: 'rgba(251, 146, 60, 0.1)',
            hoverinfo: 'skip',
            yaxis: 'y'
        };

        traces.push(timelineTrace);
    }

    const layout = {
        paper_bgcolor: '#12121a',  // Match card background color
        plot_bgcolor: '#000000',
        font: { color: '#a1a1aa', family: '-apple-system, BlinkMacSystemFont, sans-serif' },
        margin: { l: 70, r: 80, t: 20, b: 50 },
        xaxis: {
            title: 'Time',
            titlefont: { size: 12 },
            tickfont: { size: 10 },
            tickangle: 0,
            nticks: 8,
            gridcolor: '#3f3f46',
            gridwidth: 1,
            linecolor: '#3f3f46',
            showgrid: true,
            layer: 'above traces',  // Grid lines on top
            tickformat: '.1f',
            ticksuffix: 's'
        },
        yaxis: {
            title: 'File Offset (GB)',
            titlefont: { size: 12 },
            tickfont: { size: 10 },
            nticks: 8,
            gridcolor: '#3f3f46',
            gridwidth: 1,
            linecolor: '#3f3f46',
            showgrid: true,
            layer: 'above traces',  // Grid lines on top
            tickformat: '.1f',
            ticksuffix: ' GB'
        },
        dragmode: 'zoom',  // Only zoom, no pan
        showlegend: false,
        hovermode: 'closest'
    };

    const config = {
        responsive: true,
        displayModeBar: true,
        modeBarButtonsToRemove: ['lasso2d', 'select2d', 'autoScale2d', 'pan2d', 'zoomIn2d', 'zoomOut2d'],  // Remove pan and zoom buttons
        displaylogo: false,
        doubleClick: 'reset',
        scrollZoom: false  // Disable scroll zoom to prevent accidental panning
    };

    Plotly.newPlot(container, traces, layout, config);
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
    // Build ultra-compact export optimized for LLM analysis (<300KB target)
    // Focuses on actionable insights, removes redundant/verbose data

    // Helper: round numbers to reduce JSON size
    const r2 = (n) => n != null ? Math.round(n * 100) / 100 : null;
    const r1 = (n) => n != null ? Math.round(n * 10) / 10 : null;
    const r0 = (n) => n != null ? Math.round(n) : null;

    // Helper: compact operation breakdown (top 3 only, minimal fields)
    const compactOps = (ops) => {
        if (!ops || !ops.length) return [];
        return ops.slice(0, 3).map(o => ({
            op: o.operation,
            faults: o.faults,
            major: o.major_faults,
            major_pct: o.faults > 0 ? r1(o.major_faults / o.faults * 100) : 0
        }));
    };

    // Helper: compact slow keys (top 5, truncated)
    const compactKeys = (keys) => {
        if (!keys || !keys.length) return [];
        return keys.slice(0, 5).map(k => ({
            key: k.key_hex?.substring(0, 24) || k.key_prefix?.substring(0, 24) || '?',
            slow: k.slow_count || k.slow_access_count,
            avg_ms: r2((k.avg_latency_us || 0) / 1000)
        }));
    };

    const compact = {
        _format: "mdbx-trace-v3",
        _generated: new Date().toISOString(),

        // === SUMMARY (key metrics for quick understanding) ===
        summary: {
            duration_mins: r2(DATA.summary.duration_secs / 60),
            blocks: DATA.summary.block_range ?
                `${DATA.summary.block_range.min_block}-${DATA.summary.block_range.max_block}` : null,
            db_size_gb: r1(DATA.summary.file_size_gb),
            // Faults
            faults_total: DATA.summary.page_faults,
            faults_major: DATA.summary.major_faults,
            faults_minor: DATA.summary.minor_faults,
            major_pct: r1(DATA.summary.major_fault_ratio),
            fault_rate: r0(DATA.summary.fault_rate_per_sec),
            // Access pattern
            random_pct: r1(DATA.patterns.random_ratio),
            sequential_pct: r1(DATA.patterns.sequential_ratio),
            // Working set
            unique_pages: DATA.summary.unique_pages,
            working_set_gb: r2(DATA.summary.unique_pages * 4096 / 1024 / 1024 / 1024),
        },

        // === TABLES (the core analysis data) ===
        tables: DATA.unified_tables.map(t => {
            const entry = {
                name: t.name,
                faults: t.faults,
                fault_pct: r1(t.fault_percentage),
                major_pct: t.faults > 0 ? r1(t.major_faults / t.faults * 100) : 0,
                // Working set for this table
                unique_pages: t.details?.unique_pages || null,
                reuse_pct: t.details?.reuse_ratio != null ? r1(t.details.reuse_ratio * 100) : null,
                // Operations breakdown (most important for optimization)
                ops: compactOps(t.details?.faults_by_op),
                // I/O time
                io_time_secs: r2((t.time_lost_ms || 0) / 1000),
                cpu_pct: r1((t.cpu_efficiency || 0) * 100),
            };
            // Add slow keys if present
            const keys = compactKeys(t.details?.hot_keys);
            if (keys.length > 0) entry.slow_keys = keys;
            return entry;
        }),

        // === THREADS (which threads cause which faults) ===
        threads: DATA.threads.slice(0, 10).map(t => ({
            tid: t.tid,
            faults: t.faults,
            major_pct: t.faults > 0 ? r1(t.major_faults / t.faults * 100) : 0,
            tables: (t.top_tables || []).slice(0, 5).map(tt => ({
                name: tt.table_name,
                faults: tt.faults,
                major_pct: r1(tt.major_pct)
            }))
        })),

        // === TRANSACTIONS (commit patterns) ===
        txns: DATA.txn_data?.has_data ? {
            total: DATA.txn_data.summary.begin_count,
            ro: DATA.txn_data.summary.ro_count,
            rw: DATA.txn_data.summary.rw_count,
            commits: DATA.txn_data.summary.commit_count,
            commit_latency_ms: {
                avg: r2(DATA.txn_data.summary.avg_commit_latency_us / 1000),
                p95: r2(DATA.txn_data.summary.p95_commit_latency_us / 1000),
                p99: r2(DATA.txn_data.summary.p99_commit_latency_us / 1000),
                max: r2(DATA.txn_data.summary.max_commit_latency_us / 1000)
            },
            max_concurrent_ro: DATA.txn_data.concurrency.max_concurrent_ro,
            avg_concurrent_ro: r1(DATA.txn_data.concurrency.avg_concurrent_ro)
        } : null,

        // === ACCESS PATTERNS (stride analysis) ===
        strides: (DATA.patterns.top_strides || []).slice(0, 5).map(s => ({
            type: s.pattern_type,
            pages: s.stride_pages,
            count: s.count,
            pct: r1(s.percentage)
        })),

        // === CURSOR OPERATIONS (if available) ===
        cursor_ops: DATA.cursor_data?.has_data ? {
            total: DATA.cursor_data.summary.total_ops,
            rate: r0(DATA.cursor_data.summary.op_rate_per_sec),
            seek_pct: r1(DATA.cursor_data.summary.seek_ratio * 100),
            latency_us: {
                avg: r1(DATA.cursor_data.summary.avg_latency_us),
                p95: r1(DATA.cursor_data.summary.p95_latency_us),
                p99: r1(DATA.cursor_data.summary.p99_latency_us)
            },
            by_op: (DATA.cursor_data.operations || []).slice(0, 8).map(o => ({
                op: o.name,
                count: o.count,
                pct: r1(o.percentage),
                avg_us: r1(o.avg_latency_us)
            }))
        } : null,

        // === SLOW KEYS (global top offenders) ===
        slow_keys: (DATA.cursor_data?.slow_keys || []).slice(0, 15).map(k => ({
            table: k.table,
            key: k.key_hex?.substring(0, 24) || '?',
            slow_count: k.slow_access_count,
            avg_ms: r2(k.avg_latency_us / 1000)
        })),

        // === WORKING SET (memory analysis) ===
        working_set: DATA.working_set?.has_data ? {
            total_unique_pages: DATA.working_set.total_unique_pages,
            total_accesses: DATA.working_set.total_accesses,
            reuse_pct: r1(DATA.working_set.reuse_ratio * 100),
            hot_pages: DATA.working_set.hot_page_analysis ? {
                for_80pct: DATA.working_set.hot_page_analysis.pages_for_80pct,
                for_90pct: DATA.working_set.hot_page_analysis.pages_for_90pct,
                pareto_ratio: r2(DATA.working_set.hot_page_analysis.pareto_ratio)
            } : null,
            per_table: (DATA.working_set.per_table || []).slice(0, 10).map(t => ({
                name: t.name,
                unique_pages: t.unique_pages,
                hot_pages: t.hot_pages,
                working_set_mb: r1(t.working_set_mb)
            }))
        } : null,

        // === ATTRIBUTION (fault source tracking) ===
        attribution: DATA.direct_fault_attribution?.has_data ? {
            direct: DATA.direct_fault_attribution.directly_attributed_count,
            fallback: DATA.direct_fault_attribution.timestamp_fallback_count,
            uncorrelated: DATA.direct_fault_attribution.uncorrelated_count,
            by_op: (DATA.direct_fault_attribution.faults_by_op_type || []).map(o => ({
                op: o.op_type,
                faults: o.total_faults,
                major: o.major_faults,
                pct: r1(o.percentage)
            }))
        } : null
    };

    // Remove null values to save space
    const removeNulls = (obj) => {
        if (Array.isArray(obj)) {
            return obj.map(removeNulls).filter(v => v != null);
        } else if (obj && typeof obj === 'object') {
            const cleaned = {};
            for (const [k, v] of Object.entries(obj)) {
                const cleanedV = removeNulls(v);
                if (cleanedV != null && !(Array.isArray(cleanedV) && cleanedV.length === 0)) {
                    cleaned[k] = cleanedV;
                }
            }
            return Object.keys(cleaned).length > 0 ? cleaned : null;
        }
        return obj;
    };

    const cleanedCompact = removeNulls(compact);
    const json = JSON.stringify(cleanedCompact, null, 2);

    // Log size for debugging
    console.log(`Export size: ${(json.length / 1024).toFixed(1)} KB`);

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

    if (name === 'overview') {
        initOverview();
        initTables();  // Tables are now part of overview
    }
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



    // Plotly Heatmap with built-in zoom/pan/tooltips and timeline overlay
    if (DATA.heatmap.data.length) {
        const container = document.getElementById('heatmap-plotly');
        initPlotlyHeatmap(container, DATA.heatmap, DATA.timeline, s.duration_secs);
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

// Global state for table sorting
let tablesSortColumn = 'faults';
let tablesSortDesc = true;

function initTables() {
    const unified = DATA.unified_tables;

    if (!unified || unified.length === 0) {
        document.getElementById('tables-no-data').style.display = 'block';
        document.getElementById('tables-content').style.display = 'none';
        return;
    }

    // Initial render
    renderTablesTable(unified);

    // Setup sortable column headers
    document.querySelectorAll('#unified-tables th.sortable, #unified-tables th[data-sort]').forEach(th => {
        th.style.cursor = 'pointer';
        th.addEventListener('click', () => {
            const sortKey = th.dataset.sort;
            if (!sortKey || sortKey === 'name') return; // Don't sort by name or expand column

            // Toggle direction if same column, else default to desc
            if (tablesSortColumn === sortKey) {
                tablesSortDesc = !tablesSortDesc;
            } else {
                tablesSortColumn = sortKey;
                tablesSortDesc = true;
            }

            // Update header styles
            document.querySelectorAll('#unified-tables th').forEach(h => {
                h.classList.remove('sorted-asc', 'sorted-desc');
                const icon = h.querySelector('.sort-icon');
                if (icon) icon.remove();
            });
            th.classList.add(tablesSortDesc ? 'sorted-desc' : 'sorted-asc');
            th.innerHTML = th.textContent.trim() + ` <span class="sort-icon">${tablesSortDesc ? '▼' : '▲'}</span>`;

            // Re-render table
            renderTablesTable(unified);
        });
    });
}

function renderTablesTable(unified) {
    const tbody = document.querySelector('#unified-tables tbody');
    tbody.innerHTML = '';

    // Prepare sortable data
    const tableData = unified.map((t, originalIdx) => {
        const treeTable = DATA.tree_traversal?.tables?.find(tt => tt.name === t.name);
        const wsTable = DATA.working_set?.per_table?.find(wt => wt.name === t.name);
        return {
            ...t,
            originalIdx,
            majorPct: t.faults > 0 ? t.major_faults / t.faults * 100 : 0,
            blRatio: treeTable ? treeTable.branch_leaf_ratio : -1,
            workingSetNum: wsTable ? wsTable.unique_pages : 0,
            reusePct: wsTable ? wsTable.reuse_ratio * 100 : 0,
            cpuPct: t.cpu_efficiency * 100
        };
    });

    // Sort
    tableData.sort((a, b) => {
        let aVal, bVal;
        switch (tablesSortColumn) {
            case 'faults': aVal = a.faults; bVal = b.faults; break;
            case 'major_pct': aVal = a.majorPct; bVal = b.majorPct; break;
            case 'bl_ratio': aVal = a.blRatio; bVal = b.blRatio; break;
            case 'working_set': aVal = a.workingSetNum; bVal = b.workingSetNum; break;
            case 'reuse': aVal = a.reusePct; bVal = b.reusePct; break;
            case 'io_time': aVal = a.time_lost_ms; bVal = b.time_lost_ms; break;
            case 'cpu': aVal = a.cpuPct; bVal = b.cpuPct; break;
            default: aVal = a.faults; bVal = b.faults;
        }
        return tablesSortDesc ? bVal - aVal : aVal - bVal;
    });

    // Render rows
    tableData.forEach((t, idx) => {
        // Main row
        const tr = document.createElement('tr');
        tr.className = 'table-row';
        tr.dataset.idx = t.originalIdx;

        const ioTime = t.time_lost_ms >= 1000
            ? (t.time_lost_ms / 1000).toFixed(1) + 's'
            : t.time_lost_ms.toFixed(0) + 'ms';

        const blRatioStr = t.blRatio >= 0 ? t.blRatio.toFixed(2) : '-';
        const workingSet = t.workingSetNum > 0 ? fmt(t.workingSetNum) : '-';
        const reusePctStr = t.workingSetNum > 0 ? t.reusePct.toFixed(1) + '%' : '-';
        const cpuClass = t.is_io_bound ? 'io-bound' : (t.cpu_efficiency > 0.8 ? 'cpu-bound' : '');

        tr.innerHTML = `
            <td><span class="expand-icon">▶</span></td>
            <td>${t.name}</td>
            <td>${fmt(t.faults)}</td>
            <td>${t.majorPct.toFixed(1)}%</td>
            <td>${blRatioStr}</td>
            <td>${workingSet}</td>
            <td>${reusePctStr}</td>
            <td class="io-time">${t.time_lost_ms > 0 ? ioTime : '-'}</td>
            <td class="${cpuClass}">${t.total_wall_time_ms > 0 ? t.cpuPct.toFixed(1) + '%' : '-'}</td>
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

        // Click handler to expand/collapse (allows multiple rows to be expanded)
        tr.addEventListener('click', () => {
            const isExpanded = tr.classList.contains('expanded');

            // Toggle this row only (don't collapse others)
            if (isExpanded) {
                tr.classList.remove('expanded');
                detailsTr.classList.add('hidden');
            } else {
                tr.classList.add('expanded');
                detailsTr.classList.remove('hidden');
            }
        });
    });

    // Expand All button
    document.getElementById('expand-all-btn')?.addEventListener('click', (e) => {
        e.stopPropagation();
        document.querySelectorAll('#unified-tables .table-row').forEach(r => {
            r.classList.add('expanded');
        });
        document.querySelectorAll('#unified-tables .details-row').forEach(r => {
            r.classList.remove('hidden');
        });
    });

    // Collapse All button
    document.getElementById('collapse-all-btn')?.addEventListener('click', (e) => {
        e.stopPropagation();
        document.querySelectorAll('#unified-tables .table-row').forEach(r => {
            r.classList.remove('expanded');
        });
        document.querySelectorAll('#unified-tables .details-row').forEach(r => {
            r.classList.add('hidden');
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
        createCommitLatencyChart('txn-latency-chart', t.rw_commit_timeline);
    }
}

function initResourcesThreads() {
    const threads = DATA.threads;
    const txnData = DATA.txn_data;

    // We need thread data with timelines
    if (!threads || threads.length === 0) {
        document.getElementById('thread-swimlane-card').style.display = 'none';
        document.getElementById('thread-swimlane-no-data').style.display = 'block';
        return;
    }

    document.getElementById('thread-swimlane-card').style.display = 'block';
    document.getElementById('thread-swimlane-no-data').style.display = 'none';

    const container = document.getElementById('thread-swimlane');

    // Get time range from summary
    const durationSecs = DATA.summary.duration_secs || 60;

    // Get RW commit times if available
    const rwCommits = (txnData && txnData.rw_commit_timeline) || [];

    // Sort threads by fault count, take top 8
    const sortedThreads = [...threads]
        .sort((a, b) => b.faults - a.faults)
        .slice(0, 8);

    const traces = [];

    // Determine bucket size from data (typically 50ms)
    const bucketMs = DATA.patterns?.burst?.bucket_ms || 50;
    // Downsample if we have too many points - target ~1000 points max per thread
    const maxPointsPerThread = 1000;

    // Add traces for each thread (minor faults as area)
    sortedThreads.forEach((thread, idx) => {
        const timeline = thread.timeline || [];
        if (timeline.length === 0) return;

        // Sort timeline by time
        const sorted = [...timeline].sort((a, b) => a.time_ms - b.time_ms);

        // Calculate step size for downsampling if needed
        const step = Math.max(1, Math.floor(sorted.length / maxPointsPerThread));

        // Build data arrays
        const x = [];
        const yMinor = [];
        const yMajor = [];

        for (let i = 0; i < sorted.length; i += step) {
            const curr = sorted[i];
            const currTimeMin = curr.time_ms / 60000;

            // Add zero boundary before this point if there's a gap
            if (x.length === 0) {
                // First point - add zero just before it
                x.push(currTimeMin - bucketMs / 60000);
                yMinor.push(0);
                yMajor.push(0);
            } else {
                const prevTimeMin = x[x.length - 1];
                const gapMin = currTimeMin - prevTimeMin;

                // If gap is significant (> 2 bucket widths), add zero boundaries
                if (gapMin > (bucketMs * 2) / 60000) {
                    // Zero right after previous data
                    x.push(prevTimeMin + bucketMs / 60000);
                    yMinor.push(0);
                    yMajor.push(0);
                    // Zero right before current data
                    x.push(currTimeMin - bucketMs / 60000);
                    yMinor.push(0);
                    yMajor.push(0);
                }
            }

            // Add the actual data point
            x.push(currTimeMin);
            yMinor.push(curr.faults - curr.major_faults);
            yMajor.push(curr.major_faults);
        }

        // Add zero after last point
        if (x.length > 0) {
            const lastTimeMin = x[x.length - 1];
            x.push(lastTimeMin + bucketMs / 60000);
            yMinor.push(0);
            yMajor.push(0);
        }



        // Minor faults (blue area)
        traces.push({
            x: x,
            y: yMinor,
            name: 'T' + thread.tid,
            type: 'scatter',
            mode: 'lines',
            fill: 'tozeroy',
            fillcolor: 'rgba(96, 165, 250, 0.7)',
            line: { color: 'rgba(96, 165, 250, 1)', width: 1 },
            yaxis: 'y' + (idx + 1),
            hovertemplate: 'T' + thread.tid + '<br>Minor: %{y}<br>Time: %{x:.2f}m<extra></extra>',
            showlegend: idx === 0
        });

        // Major faults (red area, stacked on top conceptually but shown as separate)
        traces.push({
            x: x,
            y: yMajor,
            name: 'Major',
            type: 'scatter',
            mode: 'lines',
            fill: 'tozeroy',
            fillcolor: 'rgba(239, 68, 68, 0.8)',
            line: { color: 'rgba(239, 68, 68, 1)', width: 1 },
            yaxis: 'y' + (idx + 1),
            hovertemplate: 'T' + thread.tid + '<br>Major: %{y}<br>Time: %{x:.2f}m<extra></extra>',
            showlegend: idx === 0
        });
    });

    // Add RW commit markers as vertical lines (shapes)
    const shapes = rwCommits.map(commit => ({
        type: 'line',
        x0: commit.time_secs / 60, // convert to minutes
        x1: commit.time_secs / 60,
        y0: 0,
        y1: 1,
        yref: 'paper',
        line: { color: 'rgba(251, 191, 36, 0.3)', width: 1 }
    }));

    // Build yaxis configs for each thread row
    const numThreads = sortedThreads.length;
    const rowHeight = 1 / numThreads;
    const yaxes = {};
    const annotations = [];

    sortedThreads.forEach((thread, idx) => {
        const domain = [1 - (idx + 1) * rowHeight + 0.02, 1 - idx * rowHeight - 0.02];
        const axisName = idx === 0 ? 'yaxis' : 'yaxis' + (idx + 1);
        yaxes[axisName] = {
            domain: domain,
            showticklabels: false,
            showgrid: false,
            zeroline: false,
            fixedrange: true
        };

        // Thread label annotation
        annotations.push({
            x: 0,
            y: (domain[0] + domain[1]) / 2,
            xref: 'paper',
            yref: 'paper',
            text: 'T' + thread.tid,
            showarrow: false,
            font: { size: 10, color: '#a1a1aa' },
            xanchor: 'right',
            xshift: -5
        });
    });

    const layout = {
        paper_bgcolor: 'transparent',
        plot_bgcolor: 'transparent',
        margin: { t: 10, r: 20, b: 40, l: 70 },
        xaxis: {
            title: { text: '', font: { size: 10, color: '#52525b' } },
            tickfont: { size: 10, color: '#71717a' },
            gridcolor: '#1e1e2a',
            linecolor: '#1e1e2a',
            ticksuffix: 'm',
            range: [0, durationSecs / 60],
            fixedrange: false
        },
        ...yaxes,
        shapes: shapes,
        annotations: annotations,
        hovermode: 'closest',
        hoverlabel: {
            bgcolor: '#27272a',
            bordercolor: '#3f3f46',
            font: { color: '#fafafa', size: 12 }
        },
        showlegend: false
    };

    const config = {
        responsive: true,
        displayModeBar: true,
        modeBarButtonsToRemove: ['lasso2d', 'select2d', 'autoScale2d'],
        displaylogo: false
    };

    Plotly.newPlot(container, traces, layout, config);

    // Populate thread table attribution table
    const tableCard = document.getElementById('thread-tables-card');
    const tableBody = document.querySelector('#thread-tables tbody');

    // Check if any thread has table data
    const hasTableData = sortedThreads.some(t => t.top_tables && t.top_tables.length > 0);

    if (hasTableData) {
        tableCard.style.display = 'block';
        tableBody.innerHTML = '';

        sortedThreads.forEach(thread => {
            const majorPct = thread.faults > 0
                ? (thread.major_faults / thread.faults * 100).toFixed(1)
                : '0.0';

            const topTablesHtml = (thread.top_tables || [])
                .map(t => {
                    const color = t.major_pct > 50 ? '#f87171' : '#a1a1aa';
                    return `<span style="color: ${color}; margin-right: 8px;">${t.table_name} (${t.major_pct.toFixed(0)}% major)</span>`;
                })
                .join('');

            const row = document.createElement('tr');
            row.innerHTML = `
                <td style="font-family: monospace;">T${thread.tid}</td>
                <td>${thread.faults.toLocaleString()}</td>
                <td style="color: ${parseFloat(majorPct) > 50 ? '#f87171' : '#a1a1aa'};">${majorPct}%</td>
                <td style="font-size: 11px;">${topTablesHtml || '<span style="color: #52525b;">No table data</span>'}</td>
            `;
            tableBody.appendChild(row);
        });
    }
}

// Init overview on load
initTab('overview');

// Help icon tooltip positioning
document.querySelectorAll('.help-icon').forEach(icon => {
    icon.addEventListener('mouseenter', function(e) {
        const tooltip = this.querySelector('::after');
        const rect = this.getBoundingClientRect();
        const tooltipWidth = 280;

        // Calculate position - prefer below and to the right
        let left = rect.left + rect.width / 2 - tooltipWidth / 2;
        let top = rect.bottom + 8;

        // Keep within viewport
        if (left < 10) left = 10;
        if (left + tooltipWidth > window.innerWidth - 10) {
            left = window.innerWidth - tooltipWidth - 10;
        }

        // If would go below viewport, show above instead
        if (top + 100 > window.innerHeight) {
            top = rect.top - 8;
            this.style.setProperty('--tooltip-top', (top) + 'px');
            this.style.setProperty('--tooltip-transform', 'translateY(-100%)');
        } else {
            this.style.setProperty('--tooltip-top', top + 'px');
            this.style.setProperty('--tooltip-transform', 'translateY(0)');
        }

        this.style.setProperty('--tooltip-left', left + 'px');
    });
});

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
