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
                        <div class="card-header">Fault Timeline</div>
                        <div class="card-body">
                            <canvas id="timeline-chart"></canvas>
                        </div>
                    </div>
                    <div class="card">
                        <div class="card-header">Access Heatmap</div>
                        <div class="card-body">
                            <canvas id="heatmap-canvas"></canvas>
                        </div>
                    </div>
                </div>

                <!-- Tables and Threads side by side -->
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
                            <div class="pattern-bars">
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
                            <div class="threads-summary" id="threads-summary"></div>
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
                            <div class="card-body">
                                <canvas id="cursor-ops-chart"></canvas>
                            </div>
                        </div>
                        <div class="card">
                            <div class="card-header">Operations by Table</div>
                            <div class="card-body">
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
                        <div class="card-body compact-table-container" style="max-height: 300px;">
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
                                <canvas id="txn-concurrency-chart"></canvas>
                            </div>
                        </div>
                        <div class="card">
                            <div class="card-header">Commit Latency</div>
                            <div class="card-body">
                                <div class="latency-stats">
                                    <div class="lat-stat">
                                        <span class="lat-label">Avg</span>
                                        <span class="lat-value" id="txn-avg-latency"></span>
                                    </div>
                                    <div class="lat-stat">
                                        <span class="lat-label">p50</span>
                                        <span class="lat-value" id="txn-p50-latency"></span>
                                    </div>
                                    <div class="lat-stat">
                                        <span class="lat-label">p95</span>
                                        <span class="lat-value" id="txn-p95-latency"></span>
                                    </div>
                                    <div class="lat-stat">
                                        <span class="lat-label">p99</span>
                                        <span class="lat-value major" id="txn-p99-latency"></span>
                                    </div>
                                    <div class="lat-stat">
                                        <span class="lat-label">Max</span>
                                        <span class="lat-value major" id="txn-max-latency"></span>
                                    </div>
                                </div>
                            </div>
                        </div>
                    </div>

                    <div class="card full-width">
                        <div class="card-header">Thread Distribution</div>
                        <div class="card-body compact-table-container" style="max-height: 250px;">
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
    background: #0a0a0f;
    color: #e4e4e7;
    font-size: 13px;
    line-height: 1.4;
}

#app {
    max-width: 1400px;
    margin: 0 auto;
    padding: 12px 16px;
}

/* Tabs */
.tabs {
    display: flex;
    gap: 4px;
    margin-bottom: 12px;
    background: #12121a;
    padding: 4px;
    border-radius: 8px;
}

.tab {
    padding: 8px 16px;
    background: transparent;
    border: none;
    color: #71717a;
    cursor: pointer;
    border-radius: 6px;
    font-size: 12px;
    font-weight: 500;
    transition: all 0.15s;
}

.tab:hover { background: #1a1a24; color: #a1a1aa; }
.tab.active { background: #6366f1; color: #fff; }

.export-btn {
    margin-left: auto;
    padding: 8px 14px;
    background: #059669;
    color: #fff;
    border: none;
    border-radius: 6px;
    cursor: pointer;
    font-size: 11px;
    font-weight: 600;
}
.export-btn:hover { background: #10b981; }

/* Panels */
.panel { display: none; }
.panel.active { display: block; }

/* Metrics row */
.metrics-row {
    display: flex;
    gap: 8px;
    margin-bottom: 12px;
    flex-wrap: wrap;
}

.metric {
    background: #12121a;
    padding: 10px 14px;
    border-radius: 6px;
    text-align: center;
    min-width: 90px;
    flex: 1;
}

.metric-value {
    display: block;
    font-size: 16px;
    font-weight: 700;
    color: #6366f1;
}
.metric-value.major { color: #f87171; }
.metric-value.minor { color: #34d399; }

.metric-label {
    display: block;
    font-size: 10px;
    color: #71717a;
    text-transform: uppercase;
    margin-top: 2px;
}

/* Two column layout */
.two-col {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
    margin-bottom: 12px;
}

@media (max-width: 900px) {
    .two-col { grid-template-columns: 1fr; }
}

/* Cards */
.card {
    background: #12121a;
    border-radius: 8px;
    border: 1px solid #1e1e2a;
    overflow: hidden;
}

.card.full-width { grid-column: 1 / -1; }

.card.highlight {
    border-color: #f87171;
    box-shadow: 0 0 20px rgba(248, 113, 113, 0.1);
}

.card-header {
    padding: 10px 14px;
    background: #0a0a0f;
    font-size: 11px;
    font-weight: 600;
    color: #a1a1aa;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    display: flex;
    justify-content: space-between;
    align-items: center;
}

.card-badge {
    font-size: 10px;
    padding: 2px 6px;
    border-radius: 4px;
    background: #22c55e20;
    color: #22c55e;
    font-weight: 500;
}

.card-body {
    padding: 12px;
}

.card-body canvas {
    width: 100% !important;
    height: 180px !important;
}

/* Compact tables */
.compact-table-container {
    overflow-x: auto;
    overflow-y: auto;
    max-height: 400px;
}

.compact-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 11px;
}

.compact-table th,
.compact-table td {
    padding: 6px 8px;
    text-align: left;
    border-bottom: 1px solid #1e1e2a;
    white-space: nowrap;
}

.compact-table th {
    background: #0a0a0f;
    font-weight: 600;
    color: #6366f1;
    position: sticky;
    top: 0;
    z-index: 1;
}

.compact-table tr:hover { background: #1a1a24; }

.compact-table td { font-variant-numeric: tabular-nums; }

/* Pattern bars */
.pattern-bars { margin-bottom: 16px; }

.pattern-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
}

.pattern-label {
    width: 70px;
    font-size: 11px;
    color: #71717a;
}

.bar-container {
    flex: 1;
    height: 8px;
    background: #1e1e2a;
    border-radius: 4px;
    overflow: hidden;
}

.bar {
    height: 100%;
    border-radius: 4px;
    transition: width 0.3s;
}

.bar.sequential { background: #6366f1; }
.bar.random { background: #fbbf24; }

.pattern-value {
    width: 45px;
    text-align: right;
    font-size: 11px;
    font-weight: 600;
    color: #a1a1aa;
}

/* Threads summary */
.threads-summary {
    font-size: 11px;
    color: #71717a;
}

.threads-summary .thread-item {
    display: flex;
    justify-content: space-between;
    padding: 4px 0;
    border-bottom: 1px solid #1e1e2a;
}

/* Concurrency stats */
.concurrency-stats, .latency-stats {
    display: flex;
    gap: 16px;
    margin-bottom: 12px;
    flex-wrap: wrap;
}

.conc-stat, .lat-stat {
    text-align: center;
}

.conc-label, .lat-label {
    display: block;
    font-size: 10px;
    color: #71717a;
    text-transform: uppercase;
}

.conc-value, .lat-value {
    display: block;
    font-size: 18px;
    font-weight: 700;
    color: #6366f1;
}

.lat-value.major { color: #f87171; }

/* No data state */
.no-data {
    text-align: center;
    padding: 40px;
    color: #71717a;
}

.no-data code {
    background: #1e1e2a;
    padding: 2px 6px;
    border-radius: 4px;
    font-family: monospace;
}

/* Scrollbar */
::-webkit-scrollbar { width: 6px; height: 6px; }
::-webkit-scrollbar-track { background: #0a0a0f; }
::-webkit-scrollbar-thumb { background: #2a2a3a; border-radius: 3px; }
::-webkit-scrollbar-thumb:hover { background: #3a3a4a; }
"##;

const JAVASCRIPT: &str = r##"
// Minimal chart library
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

    drawLine(data, color = '#6366f1') {
        if (!this.resize() || !data.length) return;
        this.clear();

        const pad = { t: 10, r: 10, b: 20, l: 40 };
        const w = this.w - pad.l - pad.r;
        const h = this.h - pad.t - pad.b;
        const max = Math.max(...data) * 1.1 || 1;

        // Grid
        this.ctx.strokeStyle = '#1e1e2a';
        this.ctx.lineWidth = 1;
        for (let i = 0; i <= 4; i++) {
            const y = pad.t + (h / 4) * i;
            this.ctx.beginPath();
            this.ctx.moveTo(pad.l, y);
            this.ctx.lineTo(this.w - pad.r, y);
            this.ctx.stroke();
        }

        // Area
        this.ctx.beginPath();
        this.ctx.moveTo(pad.l, this.h - pad.b);
        data.forEach((v, i) => {
            const x = pad.l + (i / (data.length - 1)) * w;
            const y = pad.t + ((max - v) / max) * h;
            this.ctx.lineTo(x, y);
        });
        this.ctx.lineTo(pad.l + w, this.h - pad.b);
        this.ctx.closePath();
        this.ctx.fillStyle = color + '20';
        this.ctx.fill();

        // Line
        this.ctx.beginPath();
        data.forEach((v, i) => {
            const x = pad.l + (i / (data.length - 1)) * w;
            const y = pad.t + ((max - v) / max) * h;
            i === 0 ? this.ctx.moveTo(x, y) : this.ctx.lineTo(x, y);
        });
        this.ctx.strokeStyle = color;
        this.ctx.lineWidth = 1.5;
        this.ctx.stroke();
    }

    drawBar(labels, values, color = '#6366f1') {
        if (!this.resize() || !values.length) return;
        this.clear();

        const pad = { t: 10, r: 10, b: 50, l: 10 };
        const w = this.w - pad.l - pad.r;
        const h = this.h - pad.t - pad.b;
        const max = Math.max(...values) * 1.1 || 1;
        const barW = (w / values.length) * 0.7;
        const gap = (w / values.length) * 0.3;

        values.forEach((v, i) => {
            const barH = (v / max) * h;
            const x = pad.l + i * (barW + gap) + gap / 2;
            const y = pad.t + h - barH;

            this.ctx.fillStyle = color;
            this.ctx.fillRect(x, y, barW, barH);

            // Label
            this.ctx.fillStyle = '#71717a';
            this.ctx.font = '9px sans-serif';
            this.ctx.textAlign = 'center';
            this.ctx.save();
            this.ctx.translate(x + barW / 2, this.h - pad.b + 8);
            this.ctx.rotate(Math.PI / 4);
            this.ctx.fillText(labels[i].substring(0, 12), 0, 0);
            this.ctx.restore();
        });
    }

    drawHeatmap(data) {
        if (!this.resize()) return;
        const { time_buckets, offset_buckets, data: cells, max_count } = data;

        const pad = { t: 10, r: 10, b: 30, l: 40 };
        const w = this.w - pad.l - pad.r;
        const h = this.h - pad.t - pad.b;
        const cellW = w / time_buckets;
        const cellH = h / offset_buckets;

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
    }

    heatColor(i) {
        if (i === 0) return '#0a0a0f';
        const r = Math.min(255, Math.floor(i * 400));
        const g = Math.min(255, Math.floor(i * 150));
        const b = Math.min(255, 100 + Math.floor(i * 155));
        return `rgb(${r},${g},${b})`;
    }
}

// Utilities
const fmt = n => n >= 1e6 ? (n/1e6).toFixed(1)+'M' : n >= 1e3 ? (n/1e3).toFixed(1)+'K' : n.toFixed(0);
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

    // Block range
    if (s.block_range) {
        document.getElementById('block-range-metric').style.display = 'block';
        document.getElementById('block-range').textContent =
            fmt(s.block_range.min_block) + ' - ' + fmt(s.block_range.max_block);
    }

    // Timeline
    if (DATA.timeline.length) {
        new Chart(document.getElementById('timeline-chart'))
            .drawLine(DATA.timeline.map(t => t.faults));
    }

    // Heatmap
    if (DATA.heatmap.data.length) {
        new Chart(document.getElementById('heatmap-canvas')).drawHeatmap(DATA.heatmap);
    }

    // Tables
    const tbody = document.querySelector('#tables-table tbody');
    DATA.tables.slice(0, 15).forEach(t => {
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

    // Threads summary
    const threadsDiv = document.getElementById('threads-summary');
    threadsDiv.innerHTML = '<div style="margin-bottom:8px;color:#a1a1aa;font-weight:600;">Top Threads</div>';
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

    // Operations chart
    const ops = c.operations.slice(0, 10);
    new Chart(document.getElementById('cursor-ops-chart'))
        .drawBar(ops.map(o => o.name), ops.map(o => o.count));

    // Tables chart
    const tables = c.table_stats.slice(0, 10);
    new Chart(document.getElementById('cursor-tables-chart'))
        .drawBar(tables.map(t => t.name), tables.map(t => t.ops), '#34d399');

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
        tr.innerHTML = `<td>${t.table}</td><td>${fmt(t.slow_op_count)}</td><td>${fmt(t.total_op_count)}</td><td style="color:${t.slow_op_percentage > 20 ? '#f87171' : '#a1a1aa'}">${t.slow_op_percentage.toFixed(1)}%</td><td>${fmtLat(t.avg_slow_latency_us)}</td><td>${fmtLat(t.max_latency_us)}</td><td>${(t.total_slow_time_ms/1000).toFixed(1)}s</td><td style="font-size:10px;color:#71717a">${topOps}</td>`;
        slowTbody.appendChild(tr);
    });

    // Slow keys
    const keysTbody = document.querySelector('#slow-keys-table tbody');
    c.slow_keys.slice(0, 20).forEach(k => {
        const tr = document.createElement('tr');
        tr.innerHTML = `<td>${k.table}</td><td style="font-family:monospace;font-size:10px">${k.key_prefix}</td><td>${fmt(k.slow_access_count)}</td><td>${fmt(k.total_access_count)}</td><td>${fmtLat(k.avg_latency_us)}</td><td>${fmtLat(k.max_latency_us)}</td>`;
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

    // Concurrency chart
    if (c.concurrency_timeline.length) {
        new Chart(document.getElementById('txn-concurrency-chart'))
            .drawLine(c.concurrency_timeline.map(p => p.concurrent_ro));
    }

    // Latency
    document.getElementById('txn-avg-latency').textContent = fmtLat(s.avg_commit_latency_us);
    document.getElementById('txn-p50-latency').textContent = fmtLat(s.p50_commit_latency_us);
    document.getElementById('txn-p95-latency').textContent = fmtLat(s.p95_commit_latency_us);
    document.getElementById('txn-p99-latency').textContent = fmtLat(s.p99_commit_latency_us);
    document.getElementById('txn-max-latency').textContent = fmtLat(s.max_commit_latency_us);

    // Threads
    const tbody = document.querySelector('#txn-threads-table tbody');
    t.thread_stats.slice(0, 15).forEach(th => {
        const tr = document.createElement('tr');
        tr.innerHTML = `<td>${th.tid}</td><td>${fmt(th.total_txns)}</td><td>${fmt(th.ro_txns)}</td><td>${fmt(th.rw_txns)}</td><td>${fmt(th.commits)}</td><td>${fmt(th.aborts)}</td>`;
        tbody.appendChild(tr);
    });
}

// Init overview on load
initTab('overview');
"##;
