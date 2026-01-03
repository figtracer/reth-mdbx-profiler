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
    <title>MDBX Page Fault Trace Viewer</title>
    <style>
{css}
    </style>
</head>
<body>
    <div id="app">
        <header>
            <h1>MDBX Page Fault Trace Analysis</h1>
            <div class="subtitle">Reth Database Profiler</div>
        </header>

        <nav class="tabs">
            <button class="tab active" data-tab="summary">Summary</button>
            <button class="tab" data-tab="timeline">Timeline</button>
            <button class="tab" data-tab="heatmap">Heatmap</button>
            <button class="tab" data-tab="tables">Tables</button>
            <button class="tab" data-tab="threads">Threads</button>
            <button class="tab" data-tab="hotpages">Hot Pages</button>
            <button class="tab" data-tab="patterns">Patterns</button>
            <button class="tab" data-tab="prefetch">Prefetch</button>
        </nav>

        <main>
            <section id="summary" class="panel active">
                <div class="summary-grid">
                    <div class="stat-card">
                        <div class="stat-label">Duration</div>
                        <div class="stat-value" id="duration"></div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-label">Total Page Faults</div>
                        <div class="stat-value" id="total-faults"></div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-label">Major Faults (Disk I/O)</div>
                        <div class="stat-value major" id="major-faults"></div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-label">Minor Faults (Cache)</div>
                        <div class="stat-value minor" id="minor-faults"></div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-label">Fault Rate</div>
                        <div class="stat-value" id="fault-rate"></div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-label">Unique Pages</div>
                        <div class="stat-value" id="unique-pages"></div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-label">Major Fault Ratio</div>
                        <div class="stat-value" id="major-ratio"></div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-label">File Range</div>
                        <div class="stat-value" id="file-range"></div>
                    </div>
                </div>

                <div class="summary-charts">
                    <div class="chart-container">
                        <h3>Fault Type Distribution</h3>
                        <canvas id="fault-type-chart"></canvas>
                    </div>
                    <div class="chart-container">
                        <h3>Access Pattern</h3>
                        <canvas id="access-pattern-chart"></canvas>
                    </div>
                </div>
            </section>

            <section id="timeline" class="panel">
                <h2>Fault Timeline</h2>
                <div class="chart-full">
                    <canvas id="timeline-chart"></canvas>
                </div>
                <div class="timeline-controls">
                    <label><input type="checkbox" id="show-major" checked> Show Major Faults</label>
                    <label><input type="checkbox" id="show-unique" checked> Show Unique Pages</label>
                </div>
            </section>

            <section id="heatmap" class="panel">
                <h2>Access Heatmap (Time vs File Offset)</h2>
                <div class="heatmap-container">
                    <canvas id="heatmap-canvas"></canvas>
                    <div class="heatmap-legend">
                        <span class="legend-label">Low</span>
                        <div class="legend-gradient"></div>
                        <span class="legend-label">High</span>
                    </div>
                </div>
                <div class="heatmap-info">
                    <span id="heatmap-time-range"></span>
                    <span id="heatmap-offset-range"></span>
                </div>
            </section>

            <section id="tables" class="panel">
                <h2>Table Breakdown</h2>
                <div class="table-charts">
                    <div class="chart-container">
                        <canvas id="tables-pie-chart"></canvas>
                    </div>
                    <div class="chart-container">
                        <canvas id="tables-bar-chart"></canvas>
                    </div>
                </div>
                <table class="data-table" id="tables-table">
                    <thead>
                        <tr>
                            <th>Table</th>
                            <th>Category</th>
                            <th>Faults</th>
                            <th>Major</th>
                            <th>%</th>
                        </tr>
                    </thead>
                    <tbody></tbody>
                </table>
            </section>

            <section id="threads" class="panel">
                <h2>Thread Distribution</h2>
                <div class="chart-container">
                    <canvas id="threads-chart"></canvas>
                </div>
                <table class="data-table" id="threads-table">
                    <thead>
                        <tr>
                            <th>Thread ID</th>
                            <th>Faults</th>
                            <th>%</th>
                        </tr>
                    </thead>
                    <tbody></tbody>
                </table>
            </section>

            <section id="hotpages" class="panel">
                <h2>Hot Pages (Most Accessed)</h2>
                <div class="filter-bar">
                    <input type="text" id="page-filter" placeholder="Filter by table name...">
                    <select id="page-sort">
                        <option value="accesses">Sort by Accesses</option>
                        <option value="major">Sort by Major Faults</option>
                        <option value="offset">Sort by Offset</option>
                    </select>
                </div>
                <table class="data-table" id="hotpages-table">
                    <thead>
                        <tr>
                            <th>Page #</th>
                            <th>Offset (GB)</th>
                            <th>Accesses</th>
                            <th>Major</th>
                            <th>Table</th>
                        </tr>
                    </thead>
                    <tbody></tbody>
                </table>
            </section>

            <section id="patterns" class="panel">
                <h2>Access Pattern Analysis</h2>
                <div class="pattern-summary">
                    <div class="stat-card">
                        <div class="stat-label">Sequential Ratio</div>
                        <div class="stat-value" id="seq-ratio"></div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-label">Random Ratio</div>
                        <div class="stat-value" id="rand-ratio"></div>
                    </div>
                </div>
                <h3>Stride Distribution</h3>
                <div class="chart-container">
                    <canvas id="stride-chart"></canvas>
                </div>
                <h3>Burst Analysis</h3>
                <div class="burst-stats">
                    <div class="stat-card">
                        <div class="stat-label">Median Events/Bucket</div>
                        <div class="stat-value" id="burst-median"></div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-label">P95 Events/Bucket</div>
                        <div class="stat-value" id="burst-p95"></div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-label">Max Events/Bucket</div>
                        <div class="stat-value" id="burst-max"></div>
                    </div>
                </div>
            </section>

            <section id="prefetch" class="panel">
                <h2>Prefetch Opportunity Analysis</h2>
                <div class="prefetch-gauge">
                    <div class="gauge-container">
                        <canvas id="prefetch-gauge"></canvas>
                        <div class="gauge-label">Prediction Hit Rate</div>
                    </div>
                    <div class="gauge-container">
                        <canvas id="locality-gauge"></canvas>
                        <div class="gauge-label">Locality Score</div>
                    </div>
                </div>
                <div class="recommendation-box" id="recommendation">
                </div>
                <div class="prefetch-details">
                    <h3>Details</h3>
                    <p><strong>Hit Rate:</strong> <span id="hit-rate"></span></p>
                    <p><strong>Locality Score:</strong> <span id="locality-score"></span></p>
                    <p><strong>Estimated Benefit:</strong> <span id="prefetch-benefit"></span></p>
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
* {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
}

body {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, sans-serif;
    background: #1a1a2e;
    color: #eee;
    min-height: 100vh;
}

#app {
    max-width: 1400px;
    margin: 0 auto;
    padding: 20px;
}

header {
    text-align: center;
    padding: 30px 0;
    border-bottom: 1px solid #333;
    margin-bottom: 20px;
}

header h1 {
    font-size: 2em;
    color: #00d4ff;
    margin-bottom: 5px;
}

.subtitle {
    color: #888;
    font-size: 0.9em;
}

.tabs {
    display: flex;
    gap: 5px;
    margin-bottom: 20px;
    flex-wrap: wrap;
}

.tab {
    padding: 10px 20px;
    background: #252542;
    border: none;
    color: #aaa;
    cursor: pointer;
    border-radius: 5px 5px 0 0;
    transition: all 0.2s;
}

.tab:hover {
    background: #303050;
    color: #fff;
}

.tab.active {
    background: #00d4ff;
    color: #1a1a2e;
    font-weight: bold;
}

.panel {
    display: none;
    background: #252542;
    border-radius: 10px;
    padding: 25px;
    animation: fadeIn 0.3s ease;
}

.panel.active {
    display: block;
}

@keyframes fadeIn {
    from { opacity: 0; transform: translateY(10px); }
    to { opacity: 1; transform: translateY(0); }
}

.summary-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: 15px;
    margin-bottom: 30px;
}

.stat-card {
    background: #1a1a2e;
    padding: 20px;
    border-radius: 8px;
    text-align: center;
}

.stat-label {
    font-size: 0.85em;
    color: #888;
    margin-bottom: 8px;
}

.stat-value {
    font-size: 1.5em;
    font-weight: bold;
    color: #00d4ff;
}

.stat-value.major {
    color: #ff6b6b;
}

.stat-value.minor {
    color: #51cf66;
}

.summary-charts {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: 20px;
}

.chart-container {
    background: #1a1a2e;
    padding: 20px;
    border-radius: 8px;
}

.chart-container h3 {
    margin-bottom: 15px;
    font-size: 1em;
    color: #aaa;
}

.chart-full {
    background: #1a1a2e;
    padding: 20px;
    border-radius: 8px;
    margin-bottom: 15px;
}

.chart-full canvas {
    width: 100% !important;
    height: 400px !important;
}

.timeline-controls {
    display: flex;
    gap: 20px;
    justify-content: center;
}

.timeline-controls label {
    display: flex;
    align-items: center;
    gap: 5px;
    cursor: pointer;
}

.heatmap-container {
    position: relative;
    background: #1a1a2e;
    padding: 20px;
    border-radius: 8px;
}

#heatmap-canvas {
    width: 100%;
    height: 400px;
    cursor: crosshair;
}

.heatmap-legend {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    margin-top: 15px;
}

.legend-gradient {
    width: 200px;
    height: 20px;
    background: linear-gradient(to right, #1a1a2e, #0066ff, #00d4ff, #ffff00, #ff6600, #ff0000);
    border-radius: 3px;
}

.legend-label {
    font-size: 0.8em;
    color: #888;
}

.heatmap-info {
    display: flex;
    justify-content: space-between;
    margin-top: 10px;
    font-size: 0.85em;
    color: #888;
}

.table-charts {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 20px;
    margin-bottom: 20px;
}

.data-table {
    width: 100%;
    border-collapse: collapse;
    background: #1a1a2e;
    border-radius: 8px;
    overflow: hidden;
}

.data-table th,
.data-table td {
    padding: 12px 15px;
    text-align: left;
    border-bottom: 1px solid #333;
}

.data-table th {
    background: #303050;
    font-weight: 600;
    color: #00d4ff;
    cursor: pointer;
}

.data-table th:hover {
    background: #404060;
}

.data-table tr:hover {
    background: #303050;
}

.data-table tbody tr:last-child td {
    border-bottom: none;
}

.filter-bar {
    display: flex;
    gap: 15px;
    margin-bottom: 15px;
}

.filter-bar input,
.filter-bar select {
    padding: 10px 15px;
    background: #1a1a2e;
    border: 1px solid #444;
    border-radius: 5px;
    color: #eee;
    font-size: 0.9em;
}

.filter-bar input {
    flex: 1;
}

.filter-bar input:focus,
.filter-bar select:focus {
    outline: none;
    border-color: #00d4ff;
}

.pattern-summary {
    display: flex;
    gap: 20px;
    margin-bottom: 25px;
}

.pattern-summary .stat-card {
    flex: 1;
}

.burst-stats {
    display: flex;
    gap: 15px;
    margin-top: 15px;
}

.burst-stats .stat-card {
    flex: 1;
}

.prefetch-gauge {
    display: flex;
    justify-content: center;
    gap: 50px;
    margin-bottom: 30px;
}

.gauge-container {
    text-align: center;
}

.gauge-container canvas {
    width: 200px;
    height: 200px;
}

.gauge-label {
    margin-top: 10px;
    color: #888;
}

.recommendation-box {
    background: #1a1a2e;
    padding: 20px;
    border-radius: 8px;
    border-left: 4px solid #00d4ff;
    margin-bottom: 20px;
}

.recommendation-box.good {
    border-left-color: #51cf66;
}

.recommendation-box.moderate {
    border-left-color: #ffd43b;
}

.recommendation-box.poor {
    border-left-color: #ff6b6b;
}

.prefetch-details {
    background: #1a1a2e;
    padding: 20px;
    border-radius: 8px;
}

.prefetch-details h3 {
    margin-bottom: 15px;
    color: #aaa;
}

.prefetch-details p {
    margin-bottom: 10px;
}

h2 {
    margin-bottom: 20px;
    color: #00d4ff;
}

h3 {
    margin: 20px 0 15px;
    color: #aaa;
    font-size: 1.1em;
}

@media (max-width: 768px) {
    .tabs {
        justify-content: center;
    }

    .tab {
        flex: 1;
        text-align: center;
        min-width: 80px;
    }

    .table-charts {
        grid-template-columns: 1fr;
    }

    .prefetch-gauge {
        flex-direction: column;
        align-items: center;
    }
}
"##;

const JAVASCRIPT: &str = r##"
// Simple chart library (no external dependencies)
class SimpleChart {
    constructor(canvas) {
        this.canvas = canvas;
        this.ctx = canvas.getContext('2d');
        this.resize();
        window.addEventListener('resize', () => this.resize());
    }

    resize() {
        const rect = this.canvas.getBoundingClientRect();
        this.canvas.width = rect.width * window.devicePixelRatio;
        this.canvas.height = rect.height * window.devicePixelRatio;
        this.ctx.scale(window.devicePixelRatio, window.devicePixelRatio);
        this.width = rect.width;
        this.height = rect.height;
    }

    clear() {
        this.ctx.clearRect(0, 0, this.width, this.height);
    }

    drawPie(data, colors) {
        this.clear();
        const cx = this.width / 2;
        const cy = this.height / 2;
        const radius = Math.min(cx, cy) - 20;

        const total = data.reduce((a, b) => a + b, 0);
        let startAngle = -Math.PI / 2;

        data.forEach((value, i) => {
            const sliceAngle = (value / total) * Math.PI * 2;

            this.ctx.beginPath();
            this.ctx.moveTo(cx, cy);
            this.ctx.arc(cx, cy, radius, startAngle, startAngle + sliceAngle);
            this.ctx.closePath();
            this.ctx.fillStyle = colors[i % colors.length];
            this.ctx.fill();

            startAngle += sliceAngle;
        });
    }

    drawBar(labels, data, color) {
        this.clear();
        const padding = { top: 20, right: 20, bottom: 60, left: 60 };
        const chartWidth = this.width - padding.left - padding.right;
        const chartHeight = this.height - padding.top - padding.bottom;
        const barWidth = chartWidth / data.length * 0.8;
        const barGap = chartWidth / data.length * 0.2;
        const maxValue = Math.max(...data);

        // Draw axes
        this.ctx.strokeStyle = '#444';
        this.ctx.beginPath();
        this.ctx.moveTo(padding.left, padding.top);
        this.ctx.lineTo(padding.left, this.height - padding.bottom);
        this.ctx.lineTo(this.width - padding.right, this.height - padding.bottom);
        this.ctx.stroke();

        // Draw bars
        data.forEach((value, i) => {
            const barHeight = (value / maxValue) * chartHeight;
            const x = padding.left + i * (barWidth + barGap) + barGap / 2;
            const y = this.height - padding.bottom - barHeight;

            this.ctx.fillStyle = color;
            this.ctx.fillRect(x, y, barWidth, barHeight);

            // Labels
            this.ctx.fillStyle = '#888';
            this.ctx.font = '10px sans-serif';
            this.ctx.textAlign = 'center';
            this.ctx.save();
            this.ctx.translate(x + barWidth / 2, this.height - padding.bottom + 10);
            this.ctx.rotate(Math.PI / 4);
            this.ctx.fillText(labels[i].substring(0, 15), 0, 0);
            this.ctx.restore();
        });
    }

    drawLine(data, options = {}) {
        this.clear();
        const padding = { top: 20, right: 20, bottom: 40, left: 60 };
        const chartWidth = this.width - padding.left - padding.right;
        const chartHeight = this.height - padding.top - padding.bottom;

        const datasets = Array.isArray(data[0]) ? data : [data];
        const colors = options.colors || ['#00d4ff', '#ff6b6b', '#51cf66'];

        let allValues = datasets.flat();
        const maxValue = Math.max(...allValues);
        const minValue = Math.min(...allValues);
        const range = maxValue - minValue || 1;

        // Draw grid
        this.ctx.strokeStyle = '#333';
        this.ctx.lineWidth = 0.5;
        for (let i = 0; i <= 5; i++) {
            const y = padding.top + (chartHeight / 5) * i;
            this.ctx.beginPath();
            this.ctx.moveTo(padding.left, y);
            this.ctx.lineTo(this.width - padding.right, y);
            this.ctx.stroke();

            const value = maxValue - (range / 5) * i;
            this.ctx.fillStyle = '#666';
            this.ctx.font = '10px sans-serif';
            this.ctx.textAlign = 'right';
            this.ctx.fillText(formatNumber(value), padding.left - 5, y + 3);
        }

        // Draw lines
        datasets.forEach((dataset, di) => {
            this.ctx.strokeStyle = colors[di % colors.length];
            this.ctx.lineWidth = 2;
            this.ctx.beginPath();

            dataset.forEach((value, i) => {
                const x = padding.left + (i / (dataset.length - 1)) * chartWidth;
                const y = padding.top + ((maxValue - value) / range) * chartHeight;

                if (i === 0) {
                    this.ctx.moveTo(x, y);
                } else {
                    this.ctx.lineTo(x, y);
                }
            });

            this.ctx.stroke();
        });
    }

    drawGauge(value, maxValue, color) {
        this.clear();
        const cx = this.width / 2;
        const cy = this.height / 2 + 20;
        const radius = Math.min(cx, cy) - 30;

        // Background arc
        this.ctx.beginPath();
        this.ctx.arc(cx, cy, radius, Math.PI, 0);
        this.ctx.strokeStyle = '#333';
        this.ctx.lineWidth = 20;
        this.ctx.stroke();

        // Value arc
        const angle = Math.PI + (value / maxValue) * Math.PI;
        this.ctx.beginPath();
        this.ctx.arc(cx, cy, radius, Math.PI, angle);
        this.ctx.strokeStyle = color;
        this.ctx.lineWidth = 20;
        this.ctx.stroke();

        // Value text
        this.ctx.fillStyle = '#fff';
        this.ctx.font = 'bold 24px sans-serif';
        this.ctx.textAlign = 'center';
        this.ctx.fillText(value.toFixed(1) + '%', cx, cy);
    }
}

// Heatmap renderer
class HeatmapRenderer {
    constructor(canvas) {
        this.canvas = canvas;
        this.ctx = canvas.getContext('2d');
    }

    render(heatmapData) {
        const { time_buckets, offset_buckets, data, max_count } = heatmapData;

        const rect = this.canvas.getBoundingClientRect();
        this.canvas.width = rect.width * window.devicePixelRatio;
        this.canvas.height = rect.height * window.devicePixelRatio;
        this.ctx.scale(window.devicePixelRatio, window.devicePixelRatio);

        const width = rect.width;
        const height = rect.height;
        const padding = { top: 30, right: 20, bottom: 50, left: 70 };

        const cellWidth = (width - padding.left - padding.right) / time_buckets;
        const cellHeight = (height - padding.top - padding.bottom) / offset_buckets;

        // Draw cells
        for (let t = 0; t < time_buckets; t++) {
            for (let o = 0; o < offset_buckets; o++) {
                const idx = t * offset_buckets + o;
                const count = data[idx];
                const intensity = max_count > 0 ? count / max_count : 0;

                const x = padding.left + t * cellWidth;
                const y = padding.top + (offset_buckets - 1 - o) * cellHeight;

                this.ctx.fillStyle = this.getHeatColor(intensity);
                this.ctx.fillRect(x, y, cellWidth + 1, cellHeight + 1);
            }
        }

        // Draw axes labels
        this.ctx.fillStyle = '#888';
        this.ctx.font = '11px sans-serif';
        this.ctx.textAlign = 'center';
        this.ctx.fillText('Time (seconds)', width / 2, height - 10);

        this.ctx.save();
        this.ctx.translate(15, height / 2);
        this.ctx.rotate(-Math.PI / 2);
        this.ctx.fillText('File Offset (GB)', 0, 0);
        this.ctx.restore();

        // Time axis ticks
        for (let i = 0; i <= 5; i++) {
            const x = padding.left + (i / 5) * (width - padding.left - padding.right);
            const time = (heatmapData.min_time_ms + (heatmapData.max_time_ms - heatmapData.min_time_ms) * i / 5) / 1000;
            this.ctx.fillText(time.toFixed(0) + 's', x, height - padding.bottom + 20);
        }

        // Offset axis ticks
        this.ctx.textAlign = 'right';
        for (let i = 0; i <= 5; i++) {
            const y = padding.top + (i / 5) * (height - padding.top - padding.bottom);
            const offset = heatmapData.max_offset_gb - (heatmapData.max_offset_gb - heatmapData.min_offset_gb) * i / 5;
            this.ctx.fillText(offset.toFixed(1), padding.left - 5, y + 4);
        }
    }

    getHeatColor(intensity) {
        if (intensity === 0) return '#1a1a2e';

        // Blue -> Cyan -> Yellow -> Orange -> Red
        const colors = [
            [0, 102, 255],    // Blue
            [0, 212, 255],    // Cyan
            [255, 255, 0],    // Yellow
            [255, 102, 0],    // Orange
            [255, 0, 0]       // Red
        ];

        const idx = intensity * (colors.length - 1);
        const i = Math.floor(idx);
        const t = idx - i;

        if (i >= colors.length - 1) {
            const c = colors[colors.length - 1];
            return `rgb(${c[0]}, ${c[1]}, ${c[2]})`;
        }

        const c1 = colors[i];
        const c2 = colors[i + 1];

        const r = Math.round(c1[0] + (c2[0] - c1[0]) * t);
        const g = Math.round(c1[1] + (c2[1] - c1[1]) * t);
        const b = Math.round(c1[2] + (c2[2] - c1[2]) * t);

        return `rgb(${r}, ${g}, ${b})`;
    }
}

// Utility functions
function formatNumber(num) {
    if (num >= 1000000) return (num / 1000000).toFixed(1) + 'M';
    if (num >= 1000) return (num / 1000).toFixed(1) + 'K';
    return num.toFixed(0);
}

function formatDuration(secs) {
    if (secs < 60) return secs.toFixed(1) + 's';
    if (secs < 3600) return (secs / 60).toFixed(1) + 'm';
    return (secs / 3600).toFixed(1) + 'h';
}

// Initialize the viewer
function init() {
    // Tab switching
    document.querySelectorAll('.tab').forEach(tab => {
        tab.addEventListener('click', () => {
            document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
            document.querySelectorAll('.panel').forEach(p => p.classList.remove('active'));
            tab.classList.add('active');
            document.getElementById(tab.dataset.tab).classList.add('active');
        });
    });

    // Summary
    document.getElementById('duration').textContent = formatDuration(DATA.summary.duration_secs);
    document.getElementById('total-faults').textContent = formatNumber(DATA.summary.page_faults);
    document.getElementById('major-faults').textContent = formatNumber(DATA.summary.major_faults);
    document.getElementById('minor-faults').textContent = formatNumber(DATA.summary.minor_faults);
    document.getElementById('fault-rate').textContent = formatNumber(DATA.summary.fault_rate_per_sec) + '/s';
    document.getElementById('unique-pages').textContent = formatNumber(DATA.summary.unique_pages);
    document.getElementById('major-ratio').textContent = (DATA.summary.major_fault_ratio * 100).toFixed(1) + '%';
    document.getElementById('file-range').textContent = DATA.summary.file_size_gb.toFixed(2) + ' GB';

    // Fault type pie chart
    const faultTypeChart = new SimpleChart(document.getElementById('fault-type-chart'));
    faultTypeChart.drawPie(
        [DATA.summary.major_faults, DATA.summary.minor_faults],
        ['#ff6b6b', '#51cf66']
    );

    // Access pattern pie chart
    const accessPatternChart = new SimpleChart(document.getElementById('access-pattern-chart'));
    accessPatternChart.drawPie(
        [DATA.patterns.sequential_ratio, DATA.patterns.random_ratio],
        ['#00d4ff', '#ffd43b']
    );

    // Timeline chart
    if (DATA.timeline.length > 0) {
        const timelineChart = new SimpleChart(document.getElementById('timeline-chart'));
        const faults = DATA.timeline.map(t => t.faults);
        const major = DATA.timeline.map(t => t.major_faults);
        timelineChart.drawLine([faults, major], { colors: ['#00d4ff', '#ff6b6b'] });
    }

    // Heatmap
    if (DATA.heatmap.data.length > 0) {
        const heatmap = new HeatmapRenderer(document.getElementById('heatmap-canvas'));
        heatmap.render(DATA.heatmap);

        document.getElementById('heatmap-time-range').textContent =
            `Time: ${(DATA.heatmap.min_time_ms / 1000).toFixed(1)}s - ${(DATA.heatmap.max_time_ms / 1000).toFixed(1)}s`;
        document.getElementById('heatmap-offset-range').textContent =
            `Offset: ${DATA.heatmap.min_offset_gb.toFixed(2)} - ${DATA.heatmap.max_offset_gb.toFixed(2)} GB`;
    }

    // Tables
    if (DATA.tables.length > 0) {
        const tablesPieChart = new SimpleChart(document.getElementById('tables-pie-chart'));
        const topTables = DATA.tables.slice(0, 8);
        const colors = ['#00d4ff', '#ff6b6b', '#51cf66', '#ffd43b', '#cc5de8', '#20c997', '#fd7e14', '#868e96'];
        tablesPieChart.drawPie(topTables.map(t => t.faults), colors);

        const tablesBarChart = new SimpleChart(document.getElementById('tables-bar-chart'));
        tablesBarChart.drawBar(
            topTables.map(t => t.name),
            topTables.map(t => t.faults),
            '#00d4ff'
        );

        const tbody = document.querySelector('#tables-table tbody');
        DATA.tables.forEach(t => {
            const row = document.createElement('tr');
            row.innerHTML = `
                <td>${t.name}</td>
                <td>${t.category}</td>
                <td>${formatNumber(t.faults)}</td>
                <td>${formatNumber(t.major_faults)}</td>
                <td>${t.percentage.toFixed(1)}%</td>
            `;
            tbody.appendChild(row);
        });
    }

    // Threads
    if (DATA.threads.length > 0) {
        const threadsChart = new SimpleChart(document.getElementById('threads-chart'));
        threadsChart.drawBar(
            DATA.threads.map(t => 'TID ' + t.tid),
            DATA.threads.map(t => t.faults),
            '#51cf66'
        );

        const tbody = document.querySelector('#threads-table tbody');
        DATA.threads.forEach(t => {
            const row = document.createElement('tr');
            row.innerHTML = `
                <td>${t.tid}</td>
                <td>${formatNumber(t.faults)}</td>
                <td>${t.percentage.toFixed(1)}%</td>
            `;
            tbody.appendChild(row);
        });
    }

    // Hot pages
    let hotPagesData = [...DATA.hot_pages];
    function renderHotPages() {
        const filter = document.getElementById('page-filter').value.toLowerCase();
        const sort = document.getElementById('page-sort').value;

        let filtered = hotPagesData.filter(p =>
            p.table.toLowerCase().includes(filter) ||
            p.page_number.toString().includes(filter)
        );

        if (sort === 'accesses') filtered.sort((a, b) => b.accesses - a.accesses);
        else if (sort === 'major') filtered.sort((a, b) => b.major_faults - a.major_faults);
        else if (sort === 'offset') filtered.sort((a, b) => a.page_number - b.page_number);

        const tbody = document.querySelector('#hotpages-table tbody');
        tbody.innerHTML = '';
        filtered.slice(0, 50).forEach(p => {
            const row = document.createElement('tr');
            row.innerHTML = `
                <td>${p.page_number}</td>
                <td>${p.offset_gb.toFixed(4)}</td>
                <td>${formatNumber(p.accesses)}</td>
                <td>${formatNumber(p.major_faults)}</td>
                <td>${p.table}</td>
            `;
            tbody.appendChild(row);
        });
    }

    document.getElementById('page-filter').addEventListener('input', renderHotPages);
    document.getElementById('page-sort').addEventListener('change', renderHotPages);
    renderHotPages();

    // Patterns
    document.getElementById('seq-ratio').textContent = (DATA.patterns.sequential_ratio * 100).toFixed(1) + '%';
    document.getElementById('rand-ratio').textContent = (DATA.patterns.random_ratio * 100).toFixed(1) + '%';

    if (DATA.patterns.stride_distribution.length > 0) {
        const strideChart = new SimpleChart(document.getElementById('stride-chart'));
        strideChart.drawBar(
            DATA.patterns.stride_distribution.map(s => s.stride_pages + ' pg'),
            DATA.patterns.stride_distribution.map(s => s.count),
            '#ffd43b'
        );
    }

    document.getElementById('burst-median').textContent = DATA.patterns.burst_stats.median_events;
    document.getElementById('burst-p95').textContent = DATA.patterns.burst_stats.p95_events;
    document.getElementById('burst-max').textContent = DATA.patterns.burst_stats.max_events;

    // Prefetch
    const prefetchGauge = new SimpleChart(document.getElementById('prefetch-gauge'));
    prefetchGauge.drawGauge(DATA.prefetch.prediction_hit_rate, 100, '#00d4ff');

    const localityGauge = new SimpleChart(document.getElementById('locality-gauge'));
    localityGauge.drawGauge(DATA.prefetch.locality_score * 100, 100, '#51cf66');

    const recBox = document.getElementById('recommendation');
    recBox.textContent = DATA.prefetch.recommendation;
    if (DATA.prefetch.prediction_hit_rate > 30) recBox.classList.add('good');
    else if (DATA.prefetch.prediction_hit_rate > 15) recBox.classList.add('moderate');
    else recBox.classList.add('poor');

    document.getElementById('hit-rate').textContent = DATA.prefetch.prediction_hit_rate.toFixed(1) + '%';
    document.getElementById('locality-score').textContent = (DATA.prefetch.locality_score * 100).toFixed(1) + '%';
    document.getElementById('prefetch-benefit').textContent = DATA.prefetch.prefetch_benefit_estimate.toFixed(1) + '% fault reduction';
}

// Run on load
init();
"##;
