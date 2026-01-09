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
        <nav class="tabs">
            <button class="tab active" data-tab="summary">Summary</button>
            <button class="tab" data-tab="timeline">Timeline</button>
            <button class="tab" data-tab="heatmap">Heatmap</button>
            <button class="tab" data-tab="tables">Tables</button>
            <button class="tab" data-tab="threads">Threads</button>
            <button class="tab" data-tab="patterns">Patterns</button>
            <button class="tab" data-tab="cursors">Cursor Ops</button>
            <button class="tab" data-tab="transactions">Transactions</button>
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
                <div id="fault-attribution-warning" class="attribution-warning" style="display: none;"></div>
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
                <h3>Top Stride Patterns</h3>
                <div class="stride-summary" id="stride-summary"></div>
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

            <section id="cursors" class="panel">
                <h2>MDBX Cursor Operations</h2>
                <div id="cursor-no-data" style="display:none; text-align:center; padding:40px; color:#888;">
                    No cursor operation data available. Run trace with --trace-cursors flag.
                </div>
                <div id="cursor-content">
                    <div class="summary-grid">
                        <div class="stat-card">
                            <div class="stat-label">Total Operations</div>
                            <div class="stat-value" id="cursor-total-ops"></div>
                        </div>
                        <div class="stat-card">
                            <div class="stat-label">Op Rate</div>
                            <div class="stat-value" id="cursor-op-rate"></div>
                        </div>
                        <div class="stat-card">
                            <div class="stat-label">Avg Latency</div>
                            <div class="stat-value" id="cursor-avg-latency"></div>
                        </div>
                        <div class="stat-card">
                            <div class="stat-label">P99 Latency</div>
                            <div class="stat-value major" id="cursor-p99-latency"></div>
                        </div>
                        <div class="stat-card">
                            <div class="stat-label">Seeks</div>
                            <div class="stat-value" id="cursor-seeks"></div>
                        </div>
                        <div class="stat-card">
                            <div class="stat-label">Seek Ratio</div>
                            <div class="stat-value" id="cursor-seek-ratio"></div>
                        </div>
                        <div class="stat-card">
                            <div class="stat-label">Navigation</div>
                            <div class="stat-value minor" id="cursor-navs"></div>
                        </div>
                        <div class="stat-card">
                            <div class="stat-label">Errors</div>
                            <div class="stat-value" id="cursor-errors"></div>
                        </div>
                    </div>

                    <div class="table-charts">
                        <div class="chart-container">
                            <h3>Operations by Type</h3>
                            <canvas id="cursor-ops-chart"></canvas>
                        </div>
                        <div class="chart-container">
                            <h3>Operations by Table</h3>
                            <canvas id="cursor-tables-chart"></canvas>
                        </div>
                    </div>

                    <h3>Cursor Timeline</h3>
                    <div class="chart-full">
                        <canvas id="cursor-timeline-chart"></canvas>
                    </div>

                    <h3>Table Access Details</h3>
                    <table class="data-table" id="cursor-tables-table">
                        <thead>
                            <tr>
                                <th>Table</th>
                                <th>DBI</th>
                                <th>Operations</th>
                                <th>Seeks</th>
                                <th>Nav</th>
                                <th>Avg Latency</th>
                                <th>%</th>
                            </tr>
                        </thead>
                        <tbody></tbody>
                    </table>

                    <h3>Slow Operations by Table (>100μs - Likely Page Faults)</h3>
                    <div id="slow-ops-section">
                        <table class="data-table" id="slow-ops-table">
                            <thead>
                                <tr>
                                    <th>Table</th>
                                    <th>Slow Ops</th>
                                    <th>Total Ops</th>
                                    <th>Slow %</th>
                                    <th>Avg Slow Latency</th>
                                    <th>Max Latency</th>
                                    <th>Total Slow Time</th>
                                    <th>Top Slow Operations</th>
                                </tr>
                            </thead>
                            <tbody></tbody>
                        </table>
                    </div>

                    <h3>Slow Keys (Frequently Slow Accesses)</h3>
                    <div id="slow-keys-section">
                        <table class="data-table" id="slow-keys-table">
                            <thead>
                                <tr>
                                    <th>Table</th>
                                    <th>Key</th>
                                    <th>Slow Accesses</th>
                                    <th>Total Accesses</th>
                                    <th>Avg Latency</th>
                                    <th>Max Latency</th>
                                    <th>Operations</th>
                                </tr>
                            </thead>
                            <tbody></tbody>
                        </table>
                    </div>

                    <h3>Operation Log (Sample)</h3>
                    <div class="cursor-log-container" style="max-height:400px; overflow-y:auto; background:#0a0a0f; border-radius:8px; border:1px solid #252530;">
                        <table class="data-table" id="cursor-log-table">
                            <thead>
                                <tr>
                                    <th>Time (ms)</th>
                                    <th>Table</th>
                                    <th>Operation</th>
                                    <th>Key</th>
                                    <th>Latency</th>
                                    <th>Status</th>
                                </tr>
                            </thead>
                            <tbody></tbody>
                        </table>
                    </div>
                </div>
            </section>

            <section id="transactions" class="panel">
                <h2>Transaction Lifecycle Analysis</h2>
                <div id="txn-no-data" style="display:none; text-align:center; padding:40px; color:#888;">
                    No transaction data available. Run trace with --trace-cursors flag.
                </div>
                <div id="txn-content">
                    <div class="summary-grid">
                        <div class="stat-card">
                            <div class="stat-label">Total Transactions</div>
                            <div class="stat-value" id="txn-total"></div>
                        </div>
                        <div class="stat-card">
                            <div class="stat-label">Txn Rate</div>
                            <div class="stat-value" id="txn-rate"></div>
                        </div>
                        <div class="stat-card">
                            <div class="stat-label">Read-Only</div>
                            <div class="stat-value minor" id="txn-ro"></div>
                        </div>
                        <div class="stat-card">
                            <div class="stat-label">Read-Write</div>
                            <div class="stat-value major" id="txn-rw"></div>
                        </div>
                        <div class="stat-card">
                            <div class="stat-label">Commits</div>
                            <div class="stat-value" id="txn-commits"></div>
                        </div>
                        <div class="stat-card">
                            <div class="stat-label">Aborts</div>
                            <div class="stat-value" id="txn-aborts"></div>
                        </div>
                        <div class="stat-card">
                            <div class="stat-label">Avg Commit Latency</div>
                            <div class="stat-value" id="txn-avg-latency"></div>
                        </div>
                        <div class="stat-card">
                            <div class="stat-label">P99 Commit Latency</div>
                            <div class="stat-value major" id="txn-p99-latency"></div>
                        </div>
                    </div>

                    <h3>Concurrency Analysis</h3>
                    <div class="summary-grid" style="grid-template-columns: repeat(4, 1fr);">
                        <div class="stat-card">
                            <div class="stat-label">Max Concurrent RO</div>
                            <div class="stat-value" id="txn-max-ro"></div>
                        </div>
                        <div class="stat-card">
                            <div class="stat-label">Max Concurrent RW</div>
                            <div class="stat-value" id="txn-max-rw"></div>
                        </div>
                        <div class="stat-card">
                            <div class="stat-label">Max Total Concurrent</div>
                            <div class="stat-value" id="txn-max-total"></div>
                        </div>
                        <div class="stat-card">
                            <div class="stat-label">Avg Concurrent RO</div>
                            <div class="stat-value" id="txn-avg-ro"></div>
                        </div>
                    </div>

                    <h3>Concurrency Timeline</h3>
                    <div class="chart-full">
                        <canvas id="txn-concurrency-chart"></canvas>
                    </div>

                    <h3>Transaction Timeline (Gantt View)</h3>
                    <div id="txn-gantt-container" class="chart-full" style="min-height: 400px; max-height: 800px; overflow-y: auto;">
                        <canvas id="txn-gantt-chart"></canvas>
                    </div>

                    <h3>Thread Transaction Distribution</h3>
                    <table class="data-table" id="txn-threads-table">
                        <thead>
                            <tr>
                                <th>Thread ID</th>
                                <th>Total Txns</th>
                                <th>RO</th>
                                <th>RW</th>
                                <th>Commits</th>
                                <th>Aborts</th>
                                <th>Avg Commit Latency</th>
                                <th>%</th>
                            </tr>
                        </thead>
                        <tbody></tbody>
                    </table>

                    <h3>Transaction Log (Sample)</h3>
                    <div class="cursor-log-container" style="max-height:400px; overflow-y:auto; background:#0a0a0f; border-radius:8px; border:1px solid #252530;">
                        <table class="data-table" id="txn-log-table">
                            <thead>
                                <tr>
                                    <th>Time (ms)</th>
                                    <th>Thread ID</th>
                                    <th>Event</th>
                                    <th>Type</th>
                                    <th>Txn Ptr</th>
                                    <th>Latency</th>
                                    <th>Status</th>
                                </tr>
                            </thead>
                            <tbody></tbody>
                        </table>
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
* {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
}

body {
    font-family: "Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Oxygen, Ubuntu, sans-serif;
    background: #050508;
    color: #e4e4e7;
    min-height: 100vh;
}

#app {
    max-width: 1600px;
    margin: 0 auto;
    padding: 20px 30px;
}

.tabs {
    display: flex;
    gap: 8px;
    margin-bottom: 25px;
    flex-wrap: wrap;
    background: #0a0a0f;
    padding: 8px;
    border-radius: 12px;
    border: 1px solid #252530;
}

.tab {
    padding: 12px 24px;
    background: transparent;
    border: none;
    color: #a1a1aa;
    cursor: pointer;
    border-radius: 8px;
    transition: all 0.2s ease;
    font-size: 0.9em;
    font-weight: 500;
}

.tab:hover {
    background: #16161f;
    color: #e4e4e7;
}

.tab.active {
    background: linear-gradient(135deg, #6366f1 0%, #4f46e5 100%);
    color: #fff;
    font-weight: 600;
    box-shadow: 0 4px 15px rgba(99, 102, 241, 0.3);
}

.panel {
    display: none;
    background: #0e0e14;
    border-radius: 16px;
    padding: 30px;
    animation: fadeIn 0.3s ease;
    border: 1px solid #252530;
    box-shadow: 0 10px 40px rgba(0, 0, 0, 0.3);
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
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 16px;
    margin-bottom: 35px;
}

.stat-card {
    background: #0a0a0f;
    padding: 24px 20px;
    border-radius: 12px;
    text-align: center;
    border: 1px solid #252530;
    transition: transform 0.2s ease, box-shadow 0.2s ease;
}

.stat-card:hover {
    transform: translateY(-2px);
    box-shadow: 0 8px 25px rgba(0, 0, 0, 0.3);
    border-color: #2a2a38;
}

.stat-label {
    font-size: 0.8em;
    color: #a1a1aa;
    margin-bottom: 10px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    font-weight: 500;
}

.stat-value {
    font-size: 1.6em;
    font-weight: 700;
    color: #6366f1;
    text-shadow: 0 0 20px rgba(99, 102, 241, 0.3);
}

.stat-value.major {
    color: #f87171;
    text-shadow: 0 0 20px rgba(248, 113, 113, 0.3);
}

.stat-value.minor {
    color: #34d399;
    text-shadow: 0 0 20px rgba(52, 211, 153, 0.3);
}

.summary-charts {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(350px, 1fr));
    gap: 24px;
}

.chart-container {
    background: #0a0a0f;
    padding: 24px;
    border-radius: 12px;
    border: 1px solid #252530;
    min-height: 280px;
}

.chart-container canvas {
    width: 100% !important;
    height: 220px !important;
}

.chart-container h3 {
    margin-bottom: 20px;
    font-size: 1em;
    color: #a1a1aa;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
}

.chart-full {
    background: #0a0a0f;
    padding: 24px;
    border-radius: 12px;
    margin-bottom: 20px;
    border: 1px solid #252530;
}

.chart-full canvas {
    width: 100% !important;
    height: 400px !important;
}

.timeline-controls {
    display: flex;
    gap: 30px;
    justify-content: center;
    padding: 15px;
    background: #0a0a0f;
    border-radius: 8px;
}

.timeline-controls label {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    color: #a1a1aa;
    font-size: 0.9em;
    transition: color 0.2s;
}

.timeline-controls label:hover {
    color: #e4e4e7;
}

.timeline-controls input[type="checkbox"] {
    width: 18px;
    height: 18px;
    accent-color: #6366f1;
}

.heatmap-container {
    position: relative;
    background: #0a0a0f;
    padding: 24px;
    border-radius: 12px;
    border: 1px solid #252530;
}

#heatmap-canvas {
    width: 100%;
    height: 450px;
    cursor: crosshair;
    border-radius: 8px;
}

.heatmap-legend {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 15px;
    margin-top: 20px;
    padding: 15px;
    background: #050508;
    border-radius: 8px;
}

.legend-gradient {
    width: 250px;
    height: 24px;
    background: linear-gradient(to right, #12121a, #3b82f6, #6366f1, #8b5cf6, #a78bfa, #c4b5fd);
    border-radius: 4px;
    border: 1px solid #252530;
}

.legend-label {
    font-size: 0.85em;
    color: #a1a1aa;
    font-weight: 500;
}

.heatmap-info {
    display: flex;
    justify-content: space-between;
    margin-top: 15px;
    font-size: 0.9em;
    color: #a1a1aa;
    padding: 0 10px;
}

.table-charts {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 24px;
    margin-bottom: 24px;
}

.attribution-warning {
    background: rgba(34, 197, 94, 0.1);
    border: 1px solid rgba(34, 197, 94, 0.3);
    border-radius: 8px;
    padding: 12px 16px;
    margin-bottom: 16px;
    color: #22c55e;
    font-size: 14px;
    line-height: 1.5;
}

.attribution-warning.warning {
    background: rgba(251, 191, 36, 0.1);
    border-color: rgba(251, 191, 36, 0.3);
    color: #fbbf24;
}

.data-table {
    width: 100%;
    border-collapse: collapse;
    background: #0a0a0f;
    border-radius: 12px;
    overflow: hidden;
    border: 1px solid #252530;
}

.data-table th,
.data-table td {
    padding: 14px 18px;
    text-align: left;
    border-bottom: 1px solid #252530;
}

.data-table th {
    background: rgba(99, 102, 241, 0.1);
    font-weight: 600;
    color: #6366f1;
    cursor: pointer;
    text-transform: uppercase;
    font-size: 0.8em;
    letter-spacing: 0.5px;
}

.data-table th:hover {
    background: rgba(99, 102, 241, 0.2);
}

.data-table tr {
    transition: background 0.2s;
}

.data-table tr:hover {
    background: #16161f;
}

.data-table tbody tr:last-child td {
    border-bottom: none;
}

.data-table td {
    font-variant-numeric: tabular-nums;
}

.filter-bar {
    display: flex;
    gap: 15px;
    margin-bottom: 20px;
}

.filter-bar input,
.filter-bar select {
    padding: 12px 18px;
    background: #0a0a0f;
    border: 1px solid #252530;
    border-radius: 8px;
    color: #e4e4e7;
    font-size: 0.9em;
    transition: border-color 0.2s, box-shadow 0.2s;
}

.filter-bar input {
    flex: 1;
}

.filter-bar input:focus,
.filter-bar select:focus {
    outline: none;
    border-color: #6366f1;
    box-shadow: 0 0 15px rgba(99, 102, 241, 0.2);
}

.filter-bar select {
    cursor: pointer;
}

.pattern-summary {
    display: flex;
    gap: 24px;
    margin-bottom: 30px;
}

.pattern-summary .stat-card {
    flex: 1;
}

.burst-stats {
    display: flex;
    gap: 20px;
    margin-top: 20px;
}

.burst-stats .stat-card {
    flex: 1;
}

.stride-summary {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: 12px;
    margin-bottom: 24px;
}

.stride-item {
    background: #0a0a0f;
    padding: 16px;
    border-radius: 8px;
    border: 1px solid #252530;
    display: flex;
    justify-content: space-between;
    align-items: center;
}

.stride-item .stride-label {
    font-size: 0.9em;
    color: #a1a1aa;
}

.stride-item .stride-value {
    font-size: 1.1em;
    font-weight: 600;
    color: #6366f1;
}

.stride-item .stride-pct {
    font-size: 0.85em;
    color: #888;
    margin-left: 8px;
}

h2 {
    margin-bottom: 25px;
    color: #6366f1;
    font-size: 1.4em;
    font-weight: 600;
}

h3 {
    margin: 25px 0 20px;
    color: #a1a1aa;
    font-size: 1.1em;
    font-weight: 600;
}

/* Scrollbar styling */
::-webkit-scrollbar {
    width: 8px;
    height: 8px;
}

::-webkit-scrollbar-track {
    background: #0a0a0f;
    border-radius: 4px;
}

::-webkit-scrollbar-thumb {
    background: rgba(99, 102, 241, 0.3);
    border-radius: 4px;
}

::-webkit-scrollbar-thumb:hover {
    background: rgba(99, 102, 241, 0.5);
}

@media (max-width: 768px) {
    .tabs {
        justify-content: center;
    }

    .tab {
        flex: 1;
        text-align: center;
        min-width: 80px;
        padding: 10px 12px;
        font-size: 0.8em;
    }

    .table-charts {
        grid-template-columns: 1fr;
    }

    .prefetch-gauge {
        flex-direction: column;
        align-items: center;
        gap: 40px;
    }

    .summary-grid {
        grid-template-columns: repeat(2, 1fr);
    }

    .pattern-summary,
    .burst-stats {
        flex-direction: column;
    }
}
"##;

const JAVASCRIPT: &str = r##"
// Simple chart library (no external dependencies)
class SimpleChart {
    constructor(canvas) {
        this.canvas = canvas;
        this.ctx = canvas.getContext('2d');
        this.width = 0;
        this.height = 0;
        this.lastDrawFn = null;
        this.lastDrawArgs = null;
        window.addEventListener('resize', () => this.redraw());
    }

    resize() {
        const rect = this.canvas.getBoundingClientRect();
        if (rect.width === 0 || rect.height === 0) return false;

        this.canvas.width = rect.width * window.devicePixelRatio;
        this.canvas.height = rect.height * window.devicePixelRatio;
        this.ctx.scale(window.devicePixelRatio, window.devicePixelRatio);
        this.width = rect.width;
        this.height = rect.height;
        return true;
    }

    redraw() {
        if (this.lastDrawFn && this.lastDrawArgs) {
            this.lastDrawFn.apply(this, this.lastDrawArgs);
        }
    }

    clear() {
        this.ctx.clearRect(0, 0, this.width, this.height);
    }

    drawPie(data, colors, labels = []) {
        this.lastDrawFn = this.drawPie;
        this.lastDrawArgs = [data, colors, labels];

        if (!this.resize()) return;
        this.clear();

        const cx = this.width * 0.35;
        const cy = this.height / 2;
        const radius = Math.max(10, Math.min(this.width * 0.3, this.height / 2) - 20);

        const total = data.reduce((a, b) => a + b, 0);
        if (total === 0 || radius <= 0) return;
        let startAngle = -Math.PI / 2;

        // Draw slices with gap
        data.forEach((value, i) => {
            if (value === 0) return;
            const sliceAngle = (value / total) * Math.PI * 2;
            const midAngle = startAngle + sliceAngle / 2;

            // Slight offset for 3D effect
            const offsetX = Math.cos(midAngle) * 3;
            const offsetY = Math.sin(midAngle) * 3;

            this.ctx.beginPath();
            this.ctx.moveTo(cx + offsetX, cy + offsetY);
            this.ctx.arc(cx + offsetX, cy + offsetY, radius, startAngle, startAngle + sliceAngle);
            this.ctx.closePath();

            // Gradient fill
            const gradient = this.ctx.createRadialGradient(cx + offsetX, cy + offsetY, 0, cx + offsetX, cy + offsetY, radius);
            gradient.addColorStop(0, this.lightenColor(colors[i % colors.length], 20));
            gradient.addColorStop(1, colors[i % colors.length]);
            this.ctx.fillStyle = gradient;
            this.ctx.fill();

            // Border
            this.ctx.strokeStyle = 'rgba(0,0,0,0.3)';
            this.ctx.lineWidth = 1;
            this.ctx.stroke();

            startAngle += sliceAngle;
        });

        // Draw legend on the right
        if (labels.length > 0) {
            const legendX = this.width * 0.65;
            let legendY = (this.height - labels.length * 25) / 2;

            this.ctx.font = '12px -apple-system, BlinkMacSystemFont, sans-serif';
            labels.forEach((label, i) => {
                if (data[i] === 0) return;

                // Color box
                this.ctx.fillStyle = colors[i % colors.length];
                this.ctx.fillRect(legendX, legendY, 14, 14);
                this.ctx.strokeStyle = 'rgba(255,255,255,0.2)';
                this.ctx.strokeRect(legendX, legendY, 14, 14);

                // Label text
                this.ctx.fillStyle = '#ccc';
                this.ctx.textAlign = 'left';
                const pct = ((data[i] / total) * 100).toFixed(1);
                this.ctx.fillText(`${label} (${pct}%)`, legendX + 22, legendY + 11);

                legendY += 25;
            });
        }
    }

    lightenColor(color, percent) {
        const num = parseInt(color.replace('#', ''), 16);
        const amt = Math.round(2.55 * percent);
        const R = Math.min(255, (num >> 16) + amt);
        const G = Math.min(255, ((num >> 8) & 0x00FF) + amt);
        const B = Math.min(255, (num & 0x0000FF) + amt);
        return `rgb(${R},${G},${B})`;
    }

    drawBar(labels, data, color, options = {}) {
        this.lastDrawFn = this.drawBar;
        this.lastDrawArgs = [labels, data, color, options];

        if (!this.resize()) return;
        this.clear();
        if (!data.length) return;

        const padding = { top: 30, right: 30, bottom: 70, left: 70 };
        const chartWidth = Math.max(1, this.width - padding.left - padding.right);
        const chartHeight = Math.max(1, this.height - padding.top - padding.bottom);
        const barWidth = (chartWidth / data.length) * 0.7;
        const barGap = (chartWidth / data.length) * 0.3;
        const maxValue = Math.max(...data) * 1.1;

        // Title
        if (options.title) {
            this.ctx.fillStyle = '#aaa';
            this.ctx.font = 'bold 14px -apple-system, BlinkMacSystemFont, sans-serif';
            this.ctx.textAlign = 'center';
            this.ctx.fillText(options.title, this.width / 2, 20);
        }

        // Draw grid lines
        this.ctx.strokeStyle = 'rgba(255,255,255,0.1)';
        this.ctx.lineWidth = 1;
        for (let i = 0; i <= 5; i++) {
            const y = padding.top + (chartHeight / 5) * i;
            this.ctx.beginPath();
            this.ctx.moveTo(padding.left, y);
            this.ctx.lineTo(this.width - padding.right, y);
            this.ctx.stroke();

            // Y-axis labels
            const value = maxValue - (maxValue / 5) * i;
            this.ctx.fillStyle = '#888';
            this.ctx.font = '11px -apple-system, BlinkMacSystemFont, sans-serif';
            this.ctx.textAlign = 'right';
            this.ctx.fillText(formatNumber(value), padding.left - 10, y + 4);
        }

        // Draw bars with gradient
        data.forEach((value, i) => {
            const barHeight = (value / maxValue) * chartHeight;
            const x = padding.left + i * (barWidth + barGap) + barGap / 2;
            const y = this.height - padding.bottom - barHeight;

            // Bar gradient
            const gradient = this.ctx.createLinearGradient(x, y, x, this.height - padding.bottom);
            gradient.addColorStop(0, this.lightenColor(color, 30));
            gradient.addColorStop(1, color);

            // Bar shadow
            this.ctx.fillStyle = 'rgba(0,0,0,0.3)';
            this.ctx.fillRect(x + 3, y + 3, barWidth, barHeight);

            // Bar
            this.ctx.fillStyle = gradient;
            this.ctx.fillRect(x, y, barWidth, barHeight);

            // Bar border
            this.ctx.strokeStyle = 'rgba(255,255,255,0.1)';
            this.ctx.strokeRect(x, y, barWidth, barHeight);

            // Value on top
            if (barHeight > 20) {
                this.ctx.fillStyle = '#fff';
                this.ctx.font = 'bold 10px -apple-system, BlinkMacSystemFont, sans-serif';
                this.ctx.textAlign = 'center';
                this.ctx.fillText(formatNumber(value), x + barWidth / 2, y - 5);
            }

            // Labels
            this.ctx.fillStyle = '#888';
            this.ctx.font = '10px -apple-system, BlinkMacSystemFont, sans-serif';
            this.ctx.textAlign = 'right';
            this.ctx.save();
            this.ctx.translate(x + barWidth / 2, this.height - padding.bottom + 8);
            this.ctx.rotate(Math.PI / 4);
            this.ctx.fillText(labels[i].substring(0, 20), 0, 0);
            this.ctx.restore();
        });
    }

    drawLine(data, options = {}) {
        this.lastDrawFn = this.drawLine;
        this.lastDrawArgs = [data, options];

        if (!this.resize()) return;
        this.clear();

        const padding = { top: 40, right: 30, bottom: 50, left: 70 };
        const chartWidth = Math.max(1, this.width - padding.left - padding.right);
        const chartHeight = Math.max(1, this.height - padding.top - padding.bottom);

        const datasets = Array.isArray(data[0]) ? data : [data];
        const colors = options.colors || ['#6366f1', '#f87171', '#34d399'];
        const labels = options.labels || [];

        let allValues = datasets.flat();
        if (allValues.length === 0) return;

        const maxValue = Math.max(...allValues) * 1.1;
        const minValue = Math.min(0, Math.min(...allValues));
        const range = maxValue - minValue || 1;

        // Title
        if (options.title) {
            this.ctx.fillStyle = '#aaa';
            this.ctx.font = 'bold 14px -apple-system, BlinkMacSystemFont, sans-serif';
            this.ctx.textAlign = 'center';
            this.ctx.fillText(options.title, this.width / 2, 20);
        }

        // Draw grid
        this.ctx.strokeStyle = 'rgba(255,255,255,0.1)';
        this.ctx.lineWidth = 1;
        for (let i = 0; i <= 5; i++) {
            const y = padding.top + (chartHeight / 5) * i;
            this.ctx.beginPath();
            this.ctx.moveTo(padding.left, y);
            this.ctx.lineTo(this.width - padding.right, y);
            this.ctx.stroke();

            const value = maxValue - (range / 5) * i;
            this.ctx.fillStyle = '#888';
            this.ctx.font = '11px -apple-system, BlinkMacSystemFont, sans-serif';
            this.ctx.textAlign = 'right';
            this.ctx.fillText(formatNumber(value), padding.left - 10, y + 4);
        }

        // Draw area fill and lines
        datasets.forEach((dataset, di) => {
            if (dataset.length === 0) return;

            const color = colors[di % colors.length];

            // Area fill
            this.ctx.beginPath();
            this.ctx.moveTo(padding.left, this.height - padding.bottom);

            dataset.forEach((value, i) => {
                const x = padding.left + (i / Math.max(1, dataset.length - 1)) * chartWidth;
                const y = padding.top + ((maxValue - value) / range) * chartHeight;
                this.ctx.lineTo(x, y);
            });

            this.ctx.lineTo(padding.left + chartWidth, this.height - padding.bottom);
            this.ctx.closePath();

            const gradient = this.ctx.createLinearGradient(0, padding.top, 0, this.height - padding.bottom);
            gradient.addColorStop(0, color + '40');
            gradient.addColorStop(1, color + '05');
            this.ctx.fillStyle = gradient;
            this.ctx.fill();

            // Line
            this.ctx.strokeStyle = color;
            this.ctx.lineWidth = 2.5;
            this.ctx.lineCap = 'round';
            this.ctx.lineJoin = 'round';
            this.ctx.beginPath();

            dataset.forEach((value, i) => {
                const x = padding.left + (i / Math.max(1, dataset.length - 1)) * chartWidth;
                const y = padding.top + ((maxValue - value) / range) * chartHeight;

                if (i === 0) {
                    this.ctx.moveTo(x, y);
                } else {
                    this.ctx.lineTo(x, y);
                }
            });

            this.ctx.stroke();
        });

        // Legend
        if (labels.length > 0) {
            const legendX = padding.left;
            const legendY = this.height - 20;

            this.ctx.font = '11px -apple-system, BlinkMacSystemFont, sans-serif';
            let offsetX = 0;

            labels.forEach((label, i) => {
                const color = colors[i % colors.length];

                // Line sample
                this.ctx.strokeStyle = color;
                this.ctx.lineWidth = 3;
                this.ctx.beginPath();
                this.ctx.moveTo(legendX + offsetX, legendY);
                this.ctx.lineTo(legendX + offsetX + 20, legendY);
                this.ctx.stroke();

                // Label
                this.ctx.fillStyle = '#aaa';
                this.ctx.textAlign = 'left';
                this.ctx.fillText(label, legendX + offsetX + 25, legendY + 4);

                offsetX += this.ctx.measureText(label).width + 50;
            });
        }
    }

    drawGauge(value, maxValue, color) {
        this.lastDrawFn = this.drawGauge;
        this.lastDrawArgs = [value, maxValue, color];

        if (!this.resize()) return;
        this.clear();

        const cx = this.width / 2;
        const cy = this.height / 2 + 20;
        const radius = Math.max(10, Math.min(cx, cy) - 40);
        if (radius <= 0 || maxValue === 0) return;

        // Background arc with gradient
        this.ctx.beginPath();
        this.ctx.arc(cx, cy, radius, Math.PI, 0);
        this.ctx.strokeStyle = '#2a2a4a';
        this.ctx.lineWidth = 25;
        this.ctx.lineCap = 'round';
        this.ctx.stroke();

        // Value arc with gradient
        const angle = Math.PI + (Math.min(value, maxValue) / maxValue) * Math.PI;

        // Create arc gradient
        const gradient = this.ctx.createLinearGradient(cx - radius, cy, cx + radius, cy);
        gradient.addColorStop(0, this.lightenColor(color, -20));
        gradient.addColorStop(0.5, color);
        gradient.addColorStop(1, this.lightenColor(color, 20));

        this.ctx.beginPath();
        this.ctx.arc(cx, cy, radius, Math.PI, angle);
        this.ctx.strokeStyle = gradient;
        this.ctx.lineWidth = 25;
        this.ctx.lineCap = 'round';
        this.ctx.stroke();

        // Glow effect
        this.ctx.shadowColor = color;
        this.ctx.shadowBlur = 15;
        this.ctx.beginPath();
        this.ctx.arc(cx, cy, radius, Math.PI, angle);
        this.ctx.strokeStyle = color;
        this.ctx.lineWidth = 3;
        this.ctx.stroke();
        this.ctx.shadowBlur = 0;

        // Value text
        this.ctx.fillStyle = '#fff';
        this.ctx.font = 'bold 32px -apple-system, BlinkMacSystemFont, sans-serif';
        this.ctx.textAlign = 'center';
        this.ctx.fillText(value.toFixed(1) + '%', cx, cy + 10);

        // Min/Max labels
        this.ctx.fillStyle = '#666';
        this.ctx.font = '12px -apple-system, BlinkMacSystemFont, sans-serif';
        this.ctx.fillText('0', cx - radius - 5, cy + 25);
        this.ctx.fillText('100', cx + radius + 5, cy + 25);
    }
}

// Heatmap renderer
class HeatmapRenderer {
    constructor(canvas) {
        this.canvas = canvas;
        this.ctx = canvas.getContext('2d');
        this.data = null;
        window.addEventListener('resize', () => this.redraw());
    }

    redraw() {
        if (this.data) this.render(this.data);
    }

    render(heatmapData) {
        this.data = heatmapData;
        const { time_buckets, offset_buckets, data, max_count } = heatmapData;

        const rect = this.canvas.getBoundingClientRect();
        if (rect.width === 0 || rect.height === 0) return;

        this.canvas.width = rect.width * window.devicePixelRatio;
        this.canvas.height = rect.height * window.devicePixelRatio;
        this.ctx.scale(window.devicePixelRatio, window.devicePixelRatio);

        const width = rect.width;
        const height = rect.height;
        const padding = { top: 40, right: 30, bottom: 60, left: 80 };

        const cellWidth = (width - padding.left - padding.right) / time_buckets;
        const cellHeight = (height - padding.top - padding.bottom) / offset_buckets;

        // Background
        this.ctx.fillStyle = '#0a0a0f';
        this.ctx.fillRect(0, 0, width, height);

        // Draw cells with smooth interpolation
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

        // Draw border around heatmap
        this.ctx.strokeStyle = 'rgba(255,255,255,0.2)';
        this.ctx.lineWidth = 1;
        this.ctx.strokeRect(padding.left, padding.top, width - padding.left - padding.right, height - padding.top - padding.bottom);

        // Draw axes labels
        this.ctx.fillStyle = '#aaa';
        this.ctx.font = '12px -apple-system, BlinkMacSystemFont, sans-serif';
        this.ctx.textAlign = 'center';
        this.ctx.fillText('Time (seconds)', width / 2, height - 15);

        this.ctx.save();
        this.ctx.translate(20, height / 2);
        this.ctx.rotate(-Math.PI / 2);
        this.ctx.fillText('File Offset (GB)', 0, 0);
        this.ctx.restore();

        // Time axis ticks
        this.ctx.fillStyle = '#888';
        this.ctx.font = '11px -apple-system, BlinkMacSystemFont, sans-serif';
        for (let i = 0; i <= 5; i++) {
            const x = padding.left + (i / 5) * (width - padding.left - padding.right);
            const time = (heatmapData.min_time_ms + (heatmapData.max_time_ms - heatmapData.min_time_ms) * i / 5) / 1000;

            // Tick mark
            this.ctx.strokeStyle = 'rgba(255,255,255,0.3)';
            this.ctx.beginPath();
            this.ctx.moveTo(x, height - padding.bottom);
            this.ctx.lineTo(x, height - padding.bottom + 5);
            this.ctx.stroke();

            this.ctx.fillText(time.toFixed(1) + 's', x, height - padding.bottom + 20);
        }

        // Offset axis ticks
        this.ctx.textAlign = 'right';
        for (let i = 0; i <= 5; i++) {
            const y = padding.top + (i / 5) * (height - padding.top - padding.bottom);
            const offset = heatmapData.max_offset_gb - (heatmapData.max_offset_gb - heatmapData.min_offset_gb) * i / 5;

            // Tick mark
            this.ctx.strokeStyle = 'rgba(255,255,255,0.3)';
            this.ctx.beginPath();
            this.ctx.moveTo(padding.left - 5, y);
            this.ctx.lineTo(padding.left, y);
            this.ctx.stroke();

            this.ctx.fillText(offset.toFixed(1), padding.left - 10, y + 4);
        }
    }

    getHeatColor(intensity) {
        if (intensity === 0) return '#0a0a0f';
        const colors = [
            [18, 18, 26],     // Dark (#12121a)
            [59, 130, 246],   // Blue (#3b82f6)
            [99, 102, 241],   // Indigo (#6366f1)
            [139, 92, 246],   // Violet (#8b5cf6)
            [167, 139, 250],  // Light purple (#a78bfa)
            [196, 181, 253],  // Lavender (#c4b5fd)
            [248, 113, 113],  // Red for hot spots (#f87171)
            [251, 191, 36]    // Yellow for hottest (#fbbf24)
        ];

        const idx = Math.pow(intensity, 0.7) * (colors.length - 1); // Gamma correction for better visibility
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

// Chart instances (created lazily when tabs become visible)
const charts = {};
const tabInitialized = {};

// Initialize a specific tab's charts
function initTab(tabName) {
    if (tabInitialized[tabName]) {
        // Just redraw existing charts
        Object.values(charts).forEach(chart => {
            if (chart && chart.redraw) chart.redraw();
        });
        return;
    }

    tabInitialized[tabName] = true;

    switch(tabName) {
        case 'summary':
            initSummary();
            break;
        case 'timeline':
            initTimeline();
            break;
        case 'heatmap':
            initHeatmap();
            break;
        case 'tables':
            initTables();
            break;
        case 'threads':
            initThreads();
            break;

        case 'patterns':
            initPatterns();
            break;
        case 'cursors':
            initCursors();
            break;
        case 'transactions':
            initTransactions();
            break;
    }
}

function initSummary() {
    // Summary stats
    document.getElementById('duration').textContent = formatDuration(DATA.summary.duration_secs);
    document.getElementById('total-faults').textContent = formatNumber(DATA.summary.page_faults);
    document.getElementById('major-faults').textContent = formatNumber(DATA.summary.major_faults);
    document.getElementById('minor-faults').textContent = formatNumber(DATA.summary.minor_faults);
    document.getElementById('fault-rate').textContent = formatNumber(DATA.summary.fault_rate_per_sec) + '/s';
    document.getElementById('unique-pages').textContent = formatNumber(DATA.summary.unique_pages);
    document.getElementById('major-ratio').textContent = (DATA.summary.major_fault_ratio * 100).toFixed(1) + '%';
    document.getElementById('file-range').textContent = DATA.summary.file_size_gb.toFixed(2) + ' GB';

    // Fault type pie chart
    charts.faultType = new SimpleChart(document.getElementById('fault-type-chart'));
    charts.faultType.drawPie(
        [DATA.summary.major_faults, DATA.summary.minor_faults],
        ['#f87171', '#34d399'],
        ['Major (Disk I/O)', 'Minor (Cache)']
    );

    // Access pattern pie chart
    charts.accessPattern = new SimpleChart(document.getElementById('access-pattern-chart'));
    charts.accessPattern.drawPie(
        [DATA.patterns.sequential_ratio, DATA.patterns.random_ratio],
        ['#6366f1', '#fbbf24'],
        ['Sequential', 'Random']
    );
}

function initTimeline() {
    if (DATA.timeline.length > 0) {
        charts.timeline = new SimpleChart(document.getElementById('timeline-chart'));
        const faults = DATA.timeline.map(t => t.faults);
        const major = DATA.timeline.map(t => t.major_faults);
        charts.timeline.drawLine([faults, major], {
            colors: ['#6366f1', '#f87171'],
            labels: ['Total Faults', 'Major Faults'],
            title: 'Page Faults Over Time'
        });

        // Timeline controls
        const showMajor = document.getElementById('show-major');
        const showUnique = document.getElementById('show-unique');

        function updateTimeline() {
            const datasets = [];
            const colors = [];
            const labels = [];

            datasets.push(DATA.timeline.map(t => t.faults));
            colors.push('#6366f1');
            labels.push('Total Faults');

            if (showMajor.checked) {
                datasets.push(DATA.timeline.map(t => t.major_faults));
                colors.push('#f87171');
                labels.push('Major Faults');
            }

            if (showUnique.checked) {
                datasets.push(DATA.timeline.map(t => t.unique_pages));
                colors.push('#34d399');
                labels.push('Unique Pages');
            }

            charts.timeline.drawLine(datasets, { colors, labels, title: 'Page Faults Over Time' });
        }

        showMajor.addEventListener('change', updateTimeline);
        showUnique.addEventListener('change', updateTimeline);
    }
}

function initHeatmap() {
    if (DATA.heatmap.data.length > 0) {
        charts.heatmap = new HeatmapRenderer(document.getElementById('heatmap-canvas'));
        charts.heatmap.render(DATA.heatmap);

        document.getElementById('heatmap-time-range').textContent =
            `Time: ${(DATA.heatmap.min_time_ms / 1000).toFixed(1)}s - ${(DATA.heatmap.max_time_ms / 1000).toFixed(1)}s`;
        document.getElementById('heatmap-offset-range').textContent =
            `Offset: ${DATA.heatmap.min_offset_gb.toFixed(2)} GB - ${DATA.heatmap.max_offset_gb.toFixed(2)} GB`;
    }
}

function initTables() {
    // Show attribution warning if present
    if (DATA.page_fault_attribution_warning) {
        const warningDiv = document.getElementById('fault-attribution-warning');
        warningDiv.textContent = DATA.page_fault_attribution_warning;
        warningDiv.style.display = 'block';
        // Use warning style if correlation rate is low
        if (DATA.page_fault_attribution_warning.includes('Could not correlate')) {
            warningDiv.classList.add('warning');
        }
    }

    if (DATA.tables.length > 0) {
        const topTables = DATA.tables.slice(0, 10);
        const colors = ['#8b5cf6', '#6366f1', '#3b82f6', '#0ea5e9', '#06b6d4', '#14b8a6', '#10b981', '#84cc16', '#eab308', '#f59e0b'];

        charts.tablesPie = new SimpleChart(document.getElementById('tables-pie-chart'));
        charts.tablesPie.drawPie(
            topTables.map(t => t.faults),
            colors,
            topTables.map(t => t.name)
        );

        charts.tablesBar = new SimpleChart(document.getElementById('tables-bar-chart'));
        charts.tablesBar.drawBar(
            topTables.map(t => t.name),
            topTables.map(t => t.faults),
            '#6366f1',
            { title: 'Faults by Table' }
        );

        const tbody = document.querySelector('#tables-table tbody');
        DATA.tables.forEach((t, i) => {
            const row = document.createElement('tr');
            row.innerHTML = `
                <td><span style="display:inline-block;width:10px;height:10px;background:${colors[i % colors.length]};margin-right:8px;border-radius:2px;"></span>${t.name}</td>
                <td>${t.category}</td>
                <td>${formatNumber(t.faults)}</td>
                <td>${formatNumber(t.major_faults)}</td>
                <td>${t.percentage.toFixed(1)}%</td>
            `;
            tbody.appendChild(row);
        });
    }
}

function initThreads() {
    if (DATA.threads.length > 0) {
        charts.threads = new SimpleChart(document.getElementById('threads-chart'));
        charts.threads.drawBar(
            DATA.threads.map(t => 'TID ' + t.tid),
            DATA.threads.map(t => t.faults),
            '#34d399',
            { title: 'Faults by Thread' }
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
}

function initPatterns() {
    document.getElementById('seq-ratio').textContent = (DATA.patterns.sequential_ratio * 100).toFixed(1) + '%';
    document.getElementById('rand-ratio').textContent = (DATA.patterns.random_ratio * 100).toFixed(1) + '%';

    // Display top stride patterns as summary cards
    const strideSummary = document.getElementById('stride-summary');
    if (DATA.patterns.top_strides && DATA.patterns.top_strides.length > 0) {
        DATA.patterns.top_strides.forEach(s => {
            const item = document.createElement('div');
            item.className = 'stride-item';
            item.innerHTML = `
                <span class="stride-label">${s.pattern_type} (${s.stride_pages} pg)</span>
                <span><span class="stride-value">${formatNumber(s.count)}</span><span class="stride-pct">${s.percentage.toFixed(1)}%</span></span>
            `;
            strideSummary.appendChild(item);
        });
    } else {
        strideSummary.innerHTML = '<div style="color:#888;padding:20px;text-align:center;">No stride data available</div>';
    }

    document.getElementById('burst-median').textContent = formatNumber(DATA.patterns.burst_stats.median_events);
    document.getElementById('burst-p95').textContent = formatNumber(DATA.patterns.burst_stats.p95_events);
    document.getElementById('burst-max').textContent = formatNumber(DATA.patterns.burst_stats.max_events);
}

function initCursors() {
    const cursorData = DATA.cursor_data;

    if (!cursorData || !cursorData.has_data) {
        document.getElementById('cursor-no-data').style.display = 'block';
        document.getElementById('cursor-content').style.display = 'none';
        return;
    }

    document.getElementById('cursor-no-data').style.display = 'none';
    document.getElementById('cursor-content').style.display = 'block';

    // Summary stats
    document.getElementById('cursor-total-ops').textContent = formatNumber(cursorData.summary.total_ops);
    document.getElementById('cursor-op-rate').textContent = formatNumber(cursorData.summary.op_rate_per_sec) + '/s';
    document.getElementById('cursor-avg-latency').textContent = cursorData.summary.avg_latency_us.toFixed(1) + ' μs';
    document.getElementById('cursor-p99-latency').textContent = cursorData.summary.p99_latency_us.toFixed(1) + ' μs';
    document.getElementById('cursor-seeks').textContent = formatNumber(cursorData.summary.seek_count);
    document.getElementById('cursor-seek-ratio').textContent = cursorData.summary.seek_ratio.toFixed(1) + '%';
    document.getElementById('cursor-navs').textContent = formatNumber(cursorData.summary.nav_count);
    document.getElementById('cursor-errors').textContent = formatNumber(cursorData.summary.error_count);

    // Operations chart
    if (cursorData.operations.length > 0) {
        const topOps = cursorData.operations.slice(0, 10);
        const colors = topOps.map(op => op.is_seek ? '#f59e0b' : '#34d399');

        charts.cursorOps = new SimpleChart(document.getElementById('cursor-ops-chart'));
        charts.cursorOps.drawBar(
            topOps.map(o => o.name),
            topOps.map(o => o.count),
            '#6366f1',
            { title: '' }
        );
    }

    // Tables chart
    if (cursorData.table_stats.length > 0) {
        const topTables = cursorData.table_stats.slice(0, 10);
        const colors = ['#8b5cf6', '#6366f1', '#3b82f6', '#0ea5e9', '#06b6d4', '#14b8a6', '#10b981', '#84cc16', '#eab308', '#f59e0b'];

        charts.cursorTables = new SimpleChart(document.getElementById('cursor-tables-chart'));
        charts.cursorTables.drawPie(
            topTables.map(t => t.ops),
            colors,
            topTables.map(t => t.name)
        );
    }

    // Timeline chart
    if (cursorData.timeline.length > 0) {
        charts.cursorTimeline = new SimpleChart(document.getElementById('cursor-timeline-chart'));
        charts.cursorTimeline.drawLine(
            [cursorData.timeline.map(t => t.ops), cursorData.timeline.map(t => t.seeks)],
            {
                colors: ['#6366f1', '#f59e0b'],
                labels: ['Total Ops', 'Seeks'],
                title: 'Cursor Operations Over Time'
            }
        );
    }

    // Tables table
    const tablesBody = document.querySelector('#cursor-tables-table tbody');
    cursorData.table_stats.forEach(t => {
        const row = document.createElement('tr');
        row.innerHTML = `
            <td>${t.name}</td>
            <td>${t.dbi}</td>
            <td>${formatNumber(t.ops)}</td>
            <td>${formatNumber(t.seeks)}</td>
            <td>${formatNumber(t.navs)}</td>
            <td>${t.avg_latency_us.toFixed(1)} μs</td>
            <td>${t.percentage.toFixed(1)}%</td>
        `;
        tablesBody.appendChild(row);
    });

    // Slow operations by table
    const slowOpsBody = document.querySelector('#slow-ops-table tbody');
    if (cursorData.slow_ops_by_table && cursorData.slow_ops_by_table.length > 0) {
        cursorData.slow_ops_by_table.forEach(t => {
            const row = document.createElement('tr');
            // Format top operations breakdown
            const topOps = t.by_operation.slice(0, 3).map(op =>
                `${op.operation}: ${op.count} (avg ${op.avg_latency_us.toFixed(0)}μs)`
            ).join(', ');

            // Color code by severity (percentage of slow ops)
            const sevColor = t.slow_op_percentage > 5 ? '#f87171' :
                            t.slow_op_percentage > 1 ? '#fbbf24' : '#34d399';

            row.innerHTML = `
                <td><strong>${t.table}</strong></td>
                <td style="color: ${sevColor}">${formatNumber(t.slow_op_count)}</td>
                <td>${formatNumber(t.total_op_count)}</td>
                <td style="color: ${sevColor}">${t.slow_op_percentage.toFixed(2)}%</td>
                <td>${t.avg_slow_latency_us.toFixed(1)} μs</td>
                <td style="color: #f87171">${t.max_latency_us.toFixed(1)} μs</td>
                <td>${t.total_slow_time_ms.toFixed(1)} ms</td>
                <td style="font-size: 0.85em">${topOps}</td>
            `;
            slowOpsBody.appendChild(row);
        });
    } else {
        const row = document.createElement('tr');
        row.innerHTML = '<td colspan="8" style="text-align:center;color:#888;">No slow operations detected</td>';
        slowOpsBody.appendChild(row);
    }

    // Slow keys table
    const slowKeysBody = document.querySelector('#slow-keys-table tbody');
    if (cursorData.slow_keys && cursorData.slow_keys.length > 0) {
        cursorData.slow_keys.forEach(k => {
            const row = document.createElement('tr');
            const opsDisplay = k.operations.join(', ');

            row.innerHTML = `
                <td>${k.table}</td>
                <td style="font-family: monospace; font-size: 0.85em;" title="${k.key_hex}">${k.key_prefix}</td>
                <td style="color: #f87171">${k.slow_access_count}</td>
                <td>${k.total_access_count}</td>
                <td>${k.avg_latency_us.toFixed(1)} μs</td>
                <td style="color: #f87171">${k.max_latency_us.toFixed(1)} μs</td>
                <td style="font-size: 0.85em">${opsDisplay}</td>
            `;
            slowKeysBody.appendChild(row);
        });
    } else {
        const row = document.createElement('tr');
        row.innerHTML = '<td colspan="7" style="text-align:center;color:#888;">No frequently slow keys detected</td>';
        slowKeysBody.appendChild(row);
    }

    // Log table
    const logBody = document.querySelector('#cursor-log-table tbody');
    cursorData.recent_ops.forEach(op => {
        const row = document.createElement('tr');
        const statusColor = op.success ? '#34d399' : '#f87171';
        const keyDisplay = op.key_hex.length > 40 ? op.key_hex.substring(0, 40) + '...' : op.key_hex;
        row.innerHTML = `
            <td>${op.timestamp_ms}</td>
            <td>${op.table}</td>
            <td style="color: ${op.operation.includes('SET') || op.operation.includes('GET_BOTH') ? '#f59e0b' : '#34d399'}">${op.operation}</td>
            <td style="font-family: monospace; font-size: 0.85em;">${keyDisplay}</td>
            <td>${op.latency_us.toFixed(1)} μs</td>
            <td style="color: ${statusColor}">${op.success ? 'OK' : 'ERR'}</td>
        `;
        logBody.appendChild(row);
    });
}

function initTransactions() {
    const txnData = DATA.txn_data;

    if (!txnData || !txnData.has_data) {
        document.getElementById('txn-no-data').style.display = 'block';
        document.getElementById('txn-content').style.display = 'none';
        return;
    }

    document.getElementById('txn-no-data').style.display = 'none';
    document.getElementById('txn-content').style.display = 'block';

    // Summary stats
    document.getElementById('txn-total').textContent = formatNumber(txnData.summary.begin_count);
    document.getElementById('txn-rate').textContent = formatNumber(txnData.summary.txn_rate_per_sec) + '/s';
    document.getElementById('txn-ro').textContent = formatNumber(txnData.summary.ro_count);
    document.getElementById('txn-rw').textContent = formatNumber(txnData.summary.rw_count);
    document.getElementById('txn-commits').textContent = formatNumber(txnData.summary.commit_count);
    document.getElementById('txn-aborts').textContent = formatNumber(txnData.summary.abort_count);
    document.getElementById('txn-avg-latency').textContent = txnData.summary.avg_commit_latency_us.toFixed(1) + ' μs';
    document.getElementById('txn-p99-latency').textContent = txnData.summary.p99_commit_latency_us.toFixed(1) + ' μs';

    // Concurrency stats
    document.getElementById('txn-max-ro').textContent = txnData.concurrency.max_concurrent_ro;
    document.getElementById('txn-max-rw').textContent = txnData.concurrency.max_concurrent_rw;
    document.getElementById('txn-max-total').textContent = txnData.concurrency.max_concurrent_total;
    document.getElementById('txn-avg-ro').textContent = txnData.concurrency.avg_concurrent_ro.toFixed(2);

    // Concurrency timeline chart
    if (txnData.concurrency.concurrency_timeline && txnData.concurrency.concurrency_timeline.length > 0) {
        charts.txnConcurrency = new SimpleChart(document.getElementById('txn-concurrency-chart'));
        charts.txnConcurrency.drawLine(
            [
                txnData.concurrency.concurrency_timeline.map(t => t.concurrent_ro),
                txnData.concurrency.concurrency_timeline.map(t => t.concurrent_rw)
            ],
            {
                colors: ['#34d399', '#f87171'],
                labels: ['Concurrent RO', 'Concurrent RW'],
                title: 'Transaction Concurrency Over Time'
            }
        );
    }

    // Interactive Gantt chart for transaction timeline
    if (txnData.timeline && txnData.timeline.length > 0) {
        const ganttContainer = document.getElementById('txn-gantt-chart').parentElement;
        const ganttCanvas = document.getElementById('txn-gantt-chart');

        // Get unique threads and sort
        const threads = [...new Set(txnData.timeline.map(t => t.tid))].sort((a, b) => a - b);
        const threadMap = new Map(threads.map((tid, i) => [tid, i]));

        // Much larger dimensions for better visibility
        const rowHeight = 50;
        const padding = { top: 60, right: 40, bottom: 60, left: 100 };
        const minHeight = 500;
        const chartHeight = Math.max(minHeight, threads.length * rowHeight + padding.top + padding.bottom);

        // Set container to be scrollable
        ganttContainer.style.height = Math.min(700, chartHeight) + 'px';
        ganttContainer.style.overflow = 'auto';
        ganttContainer.style.position = 'relative';

        // Find time range
        const dataMinTime = Math.min(...txnData.timeline.map(t => t.start_ms));
        const dataMaxTime = Math.max(...txnData.timeline.map(t => t.end_ms || t.start_ms + 100));
        const dataTimeRange = dataMaxTime - dataMinTime || 1;

        // Interactive state
        let viewMinTime = dataMinTime;
        let viewMaxTime = dataMaxTime;
        let isDragging = false;
        let dragStartX = 0;
        let dragStartMinTime = 0;
        let hoveredTxn = null;

        // Create tooltip element
        const tooltip = document.createElement('div');
        tooltip.style.cssText = `
            position: fixed;
            background: rgba(20, 20, 30, 0.95);
            border: 1px solid rgba(255,255,255,0.2);
            border-radius: 8px;
            padding: 12px 16px;
            font-size: 13px;
            color: #fff;
            pointer-events: none;
            z-index: 10000;
            display: none;
            max-width: 350px;
            box-shadow: 0 4px 20px rgba(0,0,0,0.5);
        `;
        document.body.appendChild(tooltip);

        // Calculate transaction rectangles for hit testing
        function calcTxnRects() {
            const width = ganttCanvas.width / window.devicePixelRatio;
            const chartWidth = width - padding.left - padding.right;
            const viewRange = viewMaxTime - viewMinTime || 1;

            return txnData.timeline.map(txn => {
                const threadIdx = threadMap.get(txn.tid);
                if (threadIdx === undefined) return null;

                const y = padding.top + threadIdx * rowHeight + 8;
                const x = padding.left + ((txn.start_ms - viewMinTime) / viewRange) * chartWidth;
                const endMs = txn.end_ms || (txn.start_ms + 100);
                const barWidth = Math.max(4, ((endMs - txn.start_ms) / viewRange) * chartWidth);
                const barHeight = rowHeight - 16;

                return { txn, x, y, width: barWidth, height: barHeight };
            }).filter(r => r !== null);
        }

        function render() {
            const rect = ganttContainer.getBoundingClientRect();
            const width = rect.width;

            ganttCanvas.style.width = width + 'px';
            ganttCanvas.style.height = chartHeight + 'px';
            ganttCanvas.width = width * window.devicePixelRatio;
            ganttCanvas.height = chartHeight * window.devicePixelRatio;

            const ctx = ganttCanvas.getContext('2d');
            ctx.scale(window.devicePixelRatio, window.devicePixelRatio);

            const chartWidth = width - padding.left - padding.right;
            const viewRange = viewMaxTime - viewMinTime || 1;

            // Background
            ctx.fillStyle = '#0a0a0f';
            ctx.fillRect(0, 0, width, chartHeight);

            // Draw alternating row backgrounds
            threads.forEach((tid, i) => {
                const y = padding.top + i * rowHeight;
                ctx.fillStyle = i % 2 === 0 ? 'rgba(255,255,255,0.02)' : 'rgba(0,0,0,0.1)';
                ctx.fillRect(padding.left, y, chartWidth, rowHeight);
            });

            // Draw grid
            ctx.strokeStyle = 'rgba(255,255,255,0.08)';
            ctx.lineWidth = 1;

            // Horizontal grid lines
            threads.forEach((tid, i) => {
                const y = padding.top + i * rowHeight + rowHeight;
                ctx.beginPath();
                ctx.moveTo(padding.left, y);
                ctx.lineTo(width - padding.right, y);
                ctx.stroke();

                // Thread label with larger font
                ctx.fillStyle = '#aaa';
                ctx.font = '13px -apple-system, BlinkMacSystemFont, sans-serif';
                ctx.textAlign = 'right';
                ctx.fillText('TID ' + tid, padding.left - 15, padding.top + i * rowHeight + rowHeight / 2 + 5);
            });

            // Vertical time grid with more lines
            const numGridLines = 10;
            for (let i = 0; i <= numGridLines; i++) {
                const x = padding.left + (i / numGridLines) * chartWidth;
                ctx.strokeStyle = 'rgba(255,255,255,0.08)';
                ctx.beginPath();
                ctx.moveTo(x, padding.top);
                ctx.lineTo(x, chartHeight - padding.bottom);
                ctx.stroke();

                // Time label
                const time = viewMinTime + (viewRange * i / numGridLines);
                ctx.fillStyle = '#888';
                ctx.font = '12px -apple-system, BlinkMacSystemFont, sans-serif';
                ctx.textAlign = 'center';
                ctx.fillText((time / 1000).toFixed(3) + 's', x, chartHeight - padding.bottom + 25);
            }

            // Draw transactions as bars
            const txnRects = calcTxnRects();
            txnRects.forEach(({ txn, x, y, width: barWidth, height: barHeight }) => {
                // Skip if outside visible area
                if (x + barWidth < padding.left || x > width - padding.right) return;

                // Color based on type and status
                let color, borderColor;
                if (txn.txn_type === 'RW') {
                    color = txn.end_type === 'commit' ? '#ef4444' :
                            txn.end_type === 'abort' ? '#7c2d12' : '#dc2626';
                    borderColor = txn.end_type === 'commit' ? '#f87171' : '#991b1b';
                } else {
                    color = txn.end_type === 'commit' ? '#22c55e' :
                            txn.end_type === 'abort' ? '#14532d' : '#16a34a';
                    borderColor = txn.end_type === 'commit' ? '#4ade80' : '#15803d';
                }

                // Highlight hovered transaction
                const isHovered = hoveredTxn && hoveredTxn.txn_ptr === txn.txn_ptr;
                if (isHovered) {
                    ctx.shadowColor = color;
                    ctx.shadowBlur = 15;
                }

                // Draw bar with rounded corners
                ctx.fillStyle = isHovered ? borderColor : color;
                ctx.beginPath();
                const radius = 4;
                ctx.roundRect(x, y, barWidth, barHeight, radius);
                ctx.fill();

                ctx.shadowBlur = 0;

                // Border
                ctx.strokeStyle = isHovered ? '#fff' : 'rgba(255,255,255,0.3)';
                ctx.lineWidth = isHovered ? 2 : 1;
                ctx.stroke();

                // Draw type label if bar is wide enough
                if (barWidth > 40) {
                    ctx.fillStyle = '#fff';
                    ctx.font = 'bold 11px -apple-system, BlinkMacSystemFont, sans-serif';
                    ctx.textAlign = 'center';
                    const label = txn.txn_type + (txn.end_type ? ' ' + txn.end_type[0].toUpperCase() : '');
                    ctx.fillText(label, x + barWidth / 2, y + barHeight / 2 + 4);
                }
            });

            // Legend at top
            ctx.font = '12px -apple-system, BlinkMacSystemFont, sans-serif';
            ctx.textAlign = 'left';
            const legendY = 30;
            let legendX = padding.left;

            const legendItems = [
                { color: '#22c55e', label: 'RO Commit' },
                { color: '#ef4444', label: 'RW Commit' },
                { color: '#14532d', label: 'RO Abort' },
                { color: '#7c2d12', label: 'RW Abort' },
                { color: '#16a34a', label: 'RO Open' },
                { color: '#dc2626', label: 'RW Open' }
            ];

            legendItems.forEach(item => {
                ctx.fillStyle = item.color;
                ctx.beginPath();
                ctx.roundRect(legendX, legendY - 10, 16, 16, 3);
                ctx.fill();
                ctx.strokeStyle = 'rgba(255,255,255,0.3)';
                ctx.stroke();

                ctx.fillStyle = '#ccc';
                ctx.fillText(item.label, legendX + 22, legendY + 2);
                legendX += ctx.measureText(item.label).width + 45;
            });

            // Zoom controls hint
            ctx.fillStyle = '#666';
            ctx.font = '11px -apple-system, BlinkMacSystemFont, sans-serif';
            ctx.textAlign = 'right';
            ctx.fillText('Scroll to zoom | Drag to pan | Hover for details', width - padding.right, 25);
        }

        // Mouse event handlers
        ganttCanvas.addEventListener('wheel', (e) => {
            e.preventDefault();
            const rect = ganttCanvas.getBoundingClientRect();
            const mouseX = e.clientX - rect.left;
            const chartWidth = rect.width - padding.left - padding.right;
            const mouseRatio = (mouseX - padding.left) / chartWidth;

            const viewRange = viewMaxTime - viewMinTime;
            const zoomFactor = e.deltaY > 0 ? 1.15 : 0.87;
            const newRange = Math.min(dataTimeRange * 2, Math.max(viewRange * zoomFactor, 10));

            const mouseTime = viewMinTime + viewRange * mouseRatio;
            viewMinTime = mouseTime - newRange * mouseRatio;
            viewMaxTime = mouseTime + newRange * (1 - mouseRatio);

            // Clamp to data bounds with some padding
            const dataPadding = dataTimeRange * 0.1;
            viewMinTime = Math.max(dataMinTime - dataPadding, viewMinTime);
            viewMaxTime = Math.min(dataMaxTime + dataPadding, viewMaxTime);

            render();
        }, { passive: false });

        ganttCanvas.addEventListener('mousedown', (e) => {
            isDragging = true;
            dragStartX = e.clientX;
            dragStartMinTime = viewMinTime;
            ganttCanvas.style.cursor = 'grabbing';
        });

        window.addEventListener('mousemove', (e) => {
            if (isDragging) {
                const rect = ganttCanvas.getBoundingClientRect();
                const chartWidth = rect.width - padding.left - padding.right;
                const dx = e.clientX - dragStartX;
                const viewRange = viewMaxTime - viewMinTime;
                const timeDelta = -(dx / chartWidth) * viewRange;

                viewMinTime = dragStartMinTime + timeDelta;
                viewMaxTime = viewMinTime + viewRange;

                // Clamp
                const dataPadding = dataTimeRange * 0.1;
                if (viewMinTime < dataMinTime - dataPadding) {
                    viewMinTime = dataMinTime - dataPadding;
                    viewMaxTime = viewMinTime + viewRange;
                }
                if (viewMaxTime > dataMaxTime + dataPadding) {
                    viewMaxTime = dataMaxTime + dataPadding;
                    viewMinTime = viewMaxTime - viewRange;
                }

                render();
            } else {
                // Hit test for hover
                const rect = ganttCanvas.getBoundingClientRect();
                const mouseX = e.clientX - rect.left;
                const mouseY = e.clientY - rect.top + ganttContainer.scrollTop;

                const txnRects = calcTxnRects();
                let found = null;
                for (const r of txnRects) {
                    if (mouseX >= r.x && mouseX <= r.x + r.width &&
                        mouseY >= r.y && mouseY <= r.y + r.height) {
                        found = r.txn;
                        break;
                    }
                }

                if (found !== hoveredTxn) {
                    hoveredTxn = found;
                    render();
                }

                if (hoveredTxn) {
                    ganttCanvas.style.cursor = 'pointer';
                    const txn = hoveredTxn;
                    const duration = txn.end_ms ? (txn.end_ms - txn.start_ms).toFixed(2) : 'ongoing';
                    tooltip.innerHTML = `
                        <div style="font-weight:bold;margin-bottom:8px;color:${txn.txn_type === 'RW' ? '#f87171' : '#34d399'}">
                            ${txn.txn_type} Transaction
                        </div>
                        <div style="margin-bottom:4px;"><span style="color:#888">Thread:</span> TID ${txn.tid}</div>
                        <div style="margin-bottom:4px;"><span style="color:#888">Ptr:</span> <code style="background:#222;padding:2px 6px;border-radius:3px;font-size:11px">${txn.txn_ptr}</code></div>
                        <div style="margin-bottom:4px;"><span style="color:#888">Start:</span> ${(txn.start_ms / 1000).toFixed(4)}s</div>
                        <div style="margin-bottom:4px;"><span style="color:#888">Duration:</span> ${duration}ms</div>
                        <div style="margin-bottom:4px;"><span style="color:#888">Status:</span>
                            <span style="color:${txn.end_type === 'commit' ? '#22c55e' : txn.end_type === 'abort' ? '#ef4444' : '#fbbf24'}">
                                ${txn.end_type || 'open'}
                            </span>
                        </div>
                        ${txn.latency_us ? `<div><span style="color:#888">Commit latency:</span> ${txn.latency_us.toFixed(1)} μs</div>` : ''}
                    `;
                    tooltip.style.display = 'block';
                    tooltip.style.left = (e.clientX + 15) + 'px';
                    tooltip.style.top = (e.clientY + 15) + 'px';
                } else {
                    ganttCanvas.style.cursor = 'grab';
                    tooltip.style.display = 'none';
                }
            }
        });

        window.addEventListener('mouseup', () => {
            if (isDragging) {
                isDragging = false;
                ganttCanvas.style.cursor = hoveredTxn ? 'pointer' : 'grab';
            }
        });

        ganttCanvas.addEventListener('mouseleave', () => {
            tooltip.style.display = 'none';
            if (hoveredTxn) {
                hoveredTxn = null;
                render();
            }
        });

        // Double click to reset zoom
        ganttCanvas.addEventListener('dblclick', () => {
            viewMinTime = dataMinTime;
            viewMaxTime = dataMaxTime;
            render();
        });

        // Initial render
        render();

        // Re-render on window resize
        window.addEventListener('resize', () => {
            render();
        });
    }

    // Thread stats table
    const threadBody = document.querySelector('#txn-threads-table tbody');
    if (txnData.thread_stats) {
        txnData.thread_stats.forEach(t => {
            const row = document.createElement('tr');
            row.innerHTML = `
                <td>${t.tid}</td>
                <td>${formatNumber(t.total_txns)}</td>
                <td style="color: #34d399">${formatNumber(t.ro_txns)}</td>
                <td style="color: #f87171">${formatNumber(t.rw_txns)}</td>
                <td>${formatNumber(t.commits)}</td>
                <td>${formatNumber(t.aborts)}</td>
                <td>${t.avg_commit_latency_us.toFixed(1)} μs</td>
                <td>${t.percentage.toFixed(1)}%</td>
            `;
            threadBody.appendChild(row);
        });
    }

    // Transaction log table
    const logBody = document.querySelector('#txn-log-table tbody');
    if (txnData.recent_txns) {
        txnData.recent_txns.forEach(txn => {
            const row = document.createElement('tr');
            const eventColor = txn.event_type === 'BEGIN' ? '#6366f1' :
                              txn.event_type === 'COMMIT' ? '#22c55e' : '#f87171';
            const typeColor = txn.txn_type === 'RO' ? '#34d399' : '#f87171';
            const statusColor = txn.success ? '#34d399' : '#f87171';
            const latencyStr = txn.latency_us ? txn.latency_us.toFixed(1) + ' μs' : '-';

            row.innerHTML = `
                <td>${txn.timestamp_ms}</td>
                <td>${txn.tid}</td>
                <td style="color: ${eventColor}">${txn.event_type}</td>
                <td style="color: ${typeColor}">${txn.txn_type}</td>
                <td style="font-family: monospace;">${txn.txn_ptr_short}</td>
                <td>${latencyStr}</td>
                <td style="color: ${statusColor}">${txn.success ? 'OK' : 'ERR'}</td>
            `;
            logBody.appendChild(row);
        });
    }
}

// Initialize the viewer
function init() {
    // Tab switching with lazy initialization
    document.querySelectorAll('.tab').forEach(tab => {
        tab.addEventListener('click', () => {
            document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
            document.querySelectorAll('.panel').forEach(p => p.classList.remove('active'));
            tab.classList.add('active');
            const panel = document.getElementById(tab.dataset.tab);
            panel.classList.add('active');

            // Initialize tab charts after panel is visible
            requestAnimationFrame(() => {
                initTab(tab.dataset.tab);
            });
        });
    });

    // Initialize summary tab (it's visible by default)
    initTab('summary');
}

// Run on load
document.addEventListener('DOMContentLoaded', init);
"##;
