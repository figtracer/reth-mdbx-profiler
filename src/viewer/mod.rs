//! Web-based trace viewer
//!
//! Generates a self-contained HTML file with interactive visualizations
//! for MDBX page fault traces and cursor operations.

mod template;

use crate::event::{dbi_to_table_name, is_pre_trace_cursor, CursorEvent, PageFaultEvent};
use crate::mdbx_metadata::{PageAttribution, RethTable};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

/// Result of correlating page faults with cursor operations
#[derive(Debug, Default)]
pub struct FaultCorrelation {
    /// Map from table name to (total_faults, major_faults) that were correlated
    pub correlated_faults: HashMap<String, (u64, u64)>,
    /// Number of faults that couldn't be correlated to any cursor operation
    pub uncorrelated_faults: u64,
    /// Number of uncorrelated major faults
    pub uncorrelated_major_faults: u64,
    /// Total faults processed
    pub total_faults: u64,
}

/// Correlate page faults with cursor operations by matching thread ID and timestamp windows.
///
/// A page fault is attributed to a cursor operation if:
/// 1. It occurred on the same thread (tid matches)
/// 2. The fault timestamp falls within the cursor operation's time window
///    (between cursor start and cursor start + latency)
///
/// This gives us accurate per-table fault attribution instead of proportional estimates.
pub fn correlate_faults_with_cursors(
    page_faults: &[&PageFaultEvent],
    cursor_events: &[CursorEvent],
) -> FaultCorrelation {
    let mut result = FaultCorrelation::default();
    result.total_faults = page_faults.len() as u64;

    if cursor_events.is_empty() {
        result.uncorrelated_faults = page_faults.len() as u64;
        result.uncorrelated_major_faults =
            page_faults.iter().filter(|e| e.is_major_fault()).count() as u64;
        return result;
    }

    // Build an index of cursor operations by thread ID for faster lookup
    // Each entry is (start_time, end_time, dbi, table_name)
    let mut cursor_windows_by_tid: HashMap<u32, Vec<(u64, u64, u32, String)>> = HashMap::new();

    for cursor in cursor_events {
        let start_time = cursor.timestamp_ns;
        let end_time = cursor.timestamp_ns + cursor.latency_ns;
        let table_name = if cursor.dbi < 100 {
            dbi_to_table_name(cursor.dbi).to_string()
        } else {
            format!("Unknown (pre-trace cursor)")
        };

        cursor_windows_by_tid
            .entry(cursor.tid)
            .or_default()
            .push((start_time, end_time, cursor.dbi, table_name));
    }

    // Sort each thread's cursor windows by start time for binary search
    for windows in cursor_windows_by_tid.values_mut() {
        windows.sort_by_key(|w| w.0);
    }

    // Now correlate each page fault
    for fault in page_faults {
        let fault_time = fault.timestamp_ns;
        let fault_tid = fault.tid;
        let is_major = fault.is_major_fault();

        // Look up cursor windows for this thread
        let mut correlated = false;
        if let Some(windows) = cursor_windows_by_tid.get(&fault_tid) {
            // Binary search to find potential matching windows
            // Find first window that could contain this fault (start_time <= fault_time)
            let search_result = windows.binary_search_by(|w| {
                if w.1 < fault_time {
                    std::cmp::Ordering::Less
                } else if w.0 > fault_time {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            });

            // Check windows around the search point
            let check_start = match search_result {
                Ok(idx) => idx.saturating_sub(5),
                Err(idx) => idx.saturating_sub(5),
            };
            let check_end = (check_start + 15).min(windows.len());

            for i in check_start..check_end {
                let (start, end, _dbi, ref table_name) = windows[i];
                if fault_time >= start && fault_time <= end {
                    // Found a match!
                    let entry = result
                        .correlated_faults
                        .entry(table_name.clone())
                        .or_insert((0, 0));
                    entry.0 += 1;
                    if is_major {
                        entry.1 += 1;
                    }
                    correlated = true;
                    break;
                }
            }
        }

        if !correlated {
            result.uncorrelated_faults += 1;
            if is_major {
                result.uncorrelated_major_faults += 1;
            }
        }
    }

    result
}

/// Data structure for the web viewer
#[derive(Debug, Serialize)]
pub struct ViewerData {
    /// Summary statistics
    pub summary: TraceSummary,
    /// Timeline data (sampled for performance)
    pub timeline: Vec<TimelinePoint>,
    /// Table breakdown
    pub tables: Vec<TableStats>,
    /// Thread distribution
    pub threads: Vec<ThreadStats>,

    /// Access pattern analysis
    pub patterns: PatternAnalysis,
    /// Prefetch analysis
    pub prefetch: PrefetchAnalysis,
    /// Heatmap data (2D grid)
    pub heatmap: HeatmapData,
    /// Cursor operation data
    pub cursor_data: CursorData,
    /// Warning about page fault attribution method
    pub page_fault_attribution_warning: Option<String>,
}

/// Cursor operation statistics
#[derive(Debug, Serialize, Default)]
pub struct CursorData {
    /// Whether cursor data is available
    pub has_data: bool,
    /// Summary statistics
    pub summary: CursorSummary,
    /// Operations by type
    pub operations: Vec<OperationStats>,
    /// Table access statistics
    pub table_stats: Vec<CursorTableStats>,
    /// Timeline of cursor operations
    pub timeline: Vec<CursorTimelinePoint>,
    /// Sample of recent operations for the log view
    pub recent_ops: Vec<CursorOpSample>,
    /// Slow operations (>100μs) grouped by table - likely page faults
    pub slow_ops_by_table: Vec<SlowOpsTableStats>,
    /// Slow keys - frequently accessed keys with high latency
    pub slow_keys: Vec<SlowKeyStats>,
    /// Count of operations from cursors opened before tracing started
    pub pre_trace_cursor_ops: u64,
    /// Warning message if many cursors were opened before tracing
    pub pre_trace_warning: Option<String>,
}

/// Statistics for slow operations (>100μs) per table
#[derive(Debug, Serialize)]
pub struct SlowOpsTableStats {
    pub table: String,
    pub dbi: u32,
    pub slow_op_count: u64,
    pub total_op_count: u64,
    pub slow_op_percentage: f64,
    pub avg_slow_latency_us: f64,
    pub max_latency_us: f64,
    pub total_slow_time_ms: f64,
    /// Breakdown by operation type
    pub by_operation: Vec<SlowOpBreakdown>,
}

#[derive(Debug, Serialize)]
pub struct SlowOpBreakdown {
    pub operation: String,
    pub count: u64,
    pub avg_latency_us: f64,
    pub max_latency_us: f64,
}

/// Statistics for keys that are frequently slow
#[derive(Debug, Serialize)]
pub struct SlowKeyStats {
    pub table: String,
    pub key_hex: String,
    pub key_prefix: String,
    pub slow_access_count: u64,
    pub total_access_count: u64,
    pub avg_latency_us: f64,
    pub max_latency_us: f64,
    pub operations: Vec<String>,
}

#[derive(Debug, Serialize, Default)]
pub struct CursorSummary {
    pub total_ops: u64,
    pub op_rate_per_sec: f64,
    pub avg_latency_us: f64,
    pub p50_latency_us: f64,
    pub p99_latency_us: f64,
    pub seek_count: u64,
    pub seek_ratio: f64,
    pub nav_count: u64,
    pub error_count: u64,
    pub duration_secs: f64,
    /// Number of direct mdbx_get() calls (not cursor-based)
    pub direct_get_count: u64,
    /// Percentage of operations that are direct gets
    pub direct_get_ratio: f64,
}

#[derive(Debug, Serialize)]
pub struct OperationStats {
    pub name: String,
    pub count: u64,
    pub percentage: f64,
    pub avg_latency_us: f64,
    pub is_seek: bool,
}

#[derive(Debug, Serialize)]
pub struct CursorTableStats {
    pub dbi: u32,
    pub name: String,
    pub ops: u64,
    pub percentage: f64,
    pub seeks: u64,
    pub navs: u64,
    pub avg_latency_us: f64,
}

#[derive(Debug, Serialize)]
pub struct CursorTimelinePoint {
    pub time_ms: u64,
    pub ops: u32,
    pub seeks: u32,
    pub avg_latency_us: f64,
}

#[derive(Debug, Serialize)]
pub struct CursorOpSample {
    pub timestamp_ms: u64,
    pub table: String,
    pub operation: String,
    pub key_hex: String,
    pub latency_us: f64,
    pub success: bool,
}

#[derive(Debug, Serialize)]
pub struct TraceSummary {
    pub duration_secs: f64,
    pub total_events: u64,
    pub page_faults: u64,
    pub major_faults: u64,
    pub minor_faults: u64,
    pub major_fault_ratio: f64,
    pub fault_rate_per_sec: f64,
    pub unique_pages: u64,
    pub file_size_gb: f64,
    pub min_offset: u64,
    pub max_offset: u64,
}

#[derive(Debug, Serialize)]
pub struct TimelinePoint {
    pub time_ms: u64,
    pub faults: u32,
    pub major_faults: u32,
    pub unique_pages: u32,
}

#[derive(Debug, Serialize)]
pub struct TableStats {
    pub name: String,
    pub category: String,
    pub faults: u64,
    pub major_faults: u64,
    pub percentage: f64,
    /// Whether this table had actual cursor operations
    pub has_cursor_ops: bool,
    /// Number of cursor operations on this table (0 if no cursor data)
    pub cursor_ops: u64,
    /// Whether faults are directly correlated (true) or proportionally estimated (false)
    pub faults_correlated: bool,
}

#[derive(Debug, Serialize)]
pub struct ThreadStats {
    pub tid: u32,
    pub faults: u64,
    pub percentage: f64,
}

#[derive(Debug, Serialize)]
pub struct PatternAnalysis {
    pub sequential_ratio: f64,
    pub random_ratio: f64,
    pub stride_distribution: Vec<StrideInfo>,
    pub burst_stats: BurstStats,
}

#[derive(Debug, Serialize)]
pub struct StrideInfo {
    pub stride_pages: i64,
    pub count: u64,
    pub pattern_type: String,
}

#[derive(Debug, Serialize)]
pub struct BurstStats {
    pub median_events: u32,
    pub p95_events: u32,
    pub max_events: u32,
    pub bucket_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct PrefetchAnalysis {
    pub prediction_hit_rate: f64,
    pub locality_score: f64,
    pub recommendation: String,
    pub prefetch_benefit_estimate: f64,
}

#[derive(Debug, Serialize)]
pub struct HeatmapData {
    pub time_buckets: u32,
    pub offset_buckets: u32,
    pub min_time_ms: u64,
    pub max_time_ms: u64,
    pub min_offset_gb: f64,
    pub max_offset_gb: f64,
    /// Flattened 2D array: [time][offset] -> count
    pub data: Vec<u32>,
    pub max_count: u32,
}

/// Generate viewer data from trace events
pub fn generate_viewer_data(
    events: &[PageFaultEvent],
    cursor_events: &[CursorEvent],
    attribution: Option<&PageAttribution>,
) -> ViewerData {
    let page_faults: Vec<_> = events.iter().filter(|e| e.event_type == 1).collect();

    // Generate cursor data
    let cursor_data = generate_cursor_data(cursor_events);

    // Build a set of tables that have cursor operations (by name)
    let tables_with_ops: std::collections::HashSet<String> = cursor_data
        .table_stats
        .iter()
        .filter(|t| t.ops > 0)
        .map(|t| t.name.clone())
        .collect();

    // Build a map of table name -> cursor ops count
    let cursor_ops_by_table: HashMap<String, u64> = cursor_data
        .table_stats
        .iter()
        .map(|t| (t.name.clone(), t.ops))
        .collect();

    if page_faults.is_empty() {
        return ViewerData {
            summary: TraceSummary {
                duration_secs: 0.0,
                total_events: events.len() as u64,
                page_faults: 0,
                major_faults: 0,
                minor_faults: 0,
                major_fault_ratio: 0.0,
                fault_rate_per_sec: 0.0,
                unique_pages: 0,
                file_size_gb: 0.0,
                min_offset: 0,
                max_offset: 0,
            },
            timeline: vec![],
            tables: vec![],
            threads: vec![],

            patterns: PatternAnalysis {
                sequential_ratio: 0.0,
                random_ratio: 0.0,
                stride_distribution: vec![],
                burst_stats: BurstStats {
                    median_events: 0,
                    p95_events: 0,
                    max_events: 0,
                    bucket_ms: 100,
                },
            },
            prefetch: PrefetchAnalysis {
                prediction_hit_rate: 0.0,
                locality_score: 0.0,
                recommendation: "No data".to_string(),
                prefetch_benefit_estimate: 0.0,
            },
            heatmap: HeatmapData {
                time_buckets: 0,
                offset_buckets: 0,
                min_time_ms: 0,
                max_time_ms: 0,
                min_offset_gb: 0.0,
                max_offset_gb: 0.0,
                data: vec![],
                max_count: 0,
            },
            cursor_data,
            page_fault_attribution_warning: None,
        };
    }

    // Compute summary
    let min_ts = page_faults.iter().map(|e| e.timestamp_ns).min().unwrap();
    let max_ts = page_faults.iter().map(|e| e.timestamp_ns).max().unwrap();
    let duration_ns = max_ts - min_ts;
    let duration_secs = duration_ns as f64 / 1e9;

    let major_faults = page_faults.iter().filter(|e| e.is_major_fault()).count() as u64;
    let minor_faults = page_faults.len() as u64 - major_faults;

    let mut unique_pages: std::collections::HashSet<u64> = std::collections::HashSet::new();
    for e in &page_faults {
        unique_pages.insert(e.page_number());
    }

    let min_offset = page_faults.iter().map(|e| e.file_offset).min().unwrap();
    let max_offset = page_faults.iter().map(|e| e.file_offset).max().unwrap();

    let summary = TraceSummary {
        duration_secs,
        total_events: events.len() as u64,
        page_faults: page_faults.len() as u64,
        major_faults,
        minor_faults,
        major_fault_ratio: if page_faults.is_empty() {
            0.0
        } else {
            major_faults as f64 / page_faults.len() as f64
        },
        fault_rate_per_sec: if duration_secs > 0.0 {
            page_faults.len() as f64 / duration_secs
        } else {
            0.0
        },
        unique_pages: unique_pages.len() as u64,
        file_size_gb: max_offset as f64 / 1e9,
        min_offset,
        max_offset,
    };

    // Generate timeline (bucket by 100ms intervals, sample if too many)
    let timeline = generate_timeline(&page_faults, min_ts, duration_ns);

    // Correlate page faults with cursor operations by timestamp + thread matching
    let correlation = correlate_faults_with_cursors(&page_faults, cursor_events);

    // Table breakdown - use correlation data for accurate attribution
    let (tables, page_fault_attribution_warning) = generate_table_stats_correlated(
        &page_faults,
        &correlation,
        &cursor_ops_by_table,
        attribution,
    );

    // Thread distribution
    let threads = generate_thread_stats(&page_faults);

    // Hot pages

    // Pattern analysis
    let patterns = analyze_patterns(&page_faults);

    // Prefetch analysis
    let prefetch = analyze_prefetch(&page_faults);

    // Heatmap
    let heatmap = generate_heatmap(&page_faults, min_ts, duration_ns, min_offset, max_offset);

    ViewerData {
        summary,
        timeline,
        tables,
        threads,

        patterns,
        prefetch,
        heatmap,
        cursor_data,
        page_fault_attribution_warning,
    }
}

/// Generate cursor operation statistics
fn generate_cursor_data(events: &[CursorEvent]) -> CursorData {
    if events.is_empty() {
        return CursorData::default();
    }

    let total = events.len() as u64;

    // Timing
    let min_ts = events.iter().map(|e| e.timestamp_ns).min().unwrap();
    let max_ts = events.iter().map(|e| e.timestamp_ns).max().unwrap();
    let duration_ns = max_ts - min_ts;
    let duration_secs = duration_ns as f64 / 1e9;

    // Collect latencies for percentile calculation
    let mut latencies: Vec<u64> = events.iter().map(|e| e.latency_ns).collect();
    latencies.sort();

    let p50_idx = (latencies.len() as f64 * 0.5) as usize;
    let p99_idx = (latencies.len() as f64 * 0.99) as usize;

    let total_latency: u64 = latencies.iter().sum();
    let avg_latency_us = (total_latency as f64 / total as f64) / 1000.0;
    let p50_latency_us = latencies.get(p50_idx).copied().unwrap_or(0) as f64 / 1000.0;
    let p99_latency_us = latencies
        .get(p99_idx.min(latencies.len() - 1))
        .copied()
        .unwrap_or(0) as f64
        / 1000.0;

    // Count by operation type
    let mut op_counts: HashMap<String, (u64, u64)> = HashMap::new(); // (count, total_latency)
    let mut seek_count = 0u64;
    let mut nav_count = 0u64;
    let mut error_count = 0u64;
    let mut pre_trace_cursor_ops = 0u64;
    let mut direct_get_count = 0u64;

    // Count by DBI/table - now includes direct_gets count
    let mut dbi_stats: HashMap<u32, (u64, u64, u64, u64, u64)> = HashMap::new(); // (ops, seeks, navs, total_latency, direct_gets)

    for event in events {
        // Track pre-trace cursor operations (only for cursor ops, not direct gets)
        if !event.is_direct_get() && is_pre_trace_cursor(event.dbi) {
            pre_trace_cursor_ops += 1;
        }

        // Handle direct gets specially
        if event.is_direct_get() {
            direct_get_count += 1;
            let entry = op_counts.entry("DIRECT_GET".to_string()).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += event.latency_ns;
            // Direct gets count as seeks (point lookups)
            seek_count += 1;
        } else {
            let op = event.cursor_op();
            let op_name = op.to_string();

            let entry = op_counts.entry(op_name).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += event.latency_ns;

            if op.is_seek() {
                seek_count += 1;
            }
            if op.is_navigation() {
                nav_count += 1;
            }
        }

        if !event.is_success() && !event.is_not_found() {
            error_count += 1;
        }

        // DBI stats
        let dbi_entry = dbi_stats.entry(event.dbi).or_insert((0, 0, 0, 0, 0));
        dbi_entry.0 += 1;
        if event.is_direct_get() {
            dbi_entry.4 += 1; // direct_gets
            dbi_entry.1 += 1; // count as seek too
        } else {
            let op = event.cursor_op();
            if op.is_seek() {
                dbi_entry.1 += 1;
            }
            if op.is_navigation() {
                dbi_entry.2 += 1;
            }
        }
        dbi_entry.3 += event.latency_ns;
    }

    // Build operation stats
    let mut operations: Vec<OperationStats> = op_counts
        .into_iter()
        .map(|(name, (count, total_lat))| {
            let is_seek = matches!(
                name.as_str(),
                "SET"
                    | "SET_KEY"
                    | "SET_RANGE"
                    | "SET_LOWERBOUND"
                    | "SET_UPPERBOUND"
                    | "GET_BOTH"
                    | "GET_BOTH_RANGE"
                    | "DIRECT_GET"
            );
            OperationStats {
                name,
                count,
                percentage: count as f64 / total as f64 * 100.0,
                avg_latency_us: (total_lat as f64 / count as f64) / 1000.0,
                is_seek,
            }
        })
        .collect();
    operations.sort_by(|a, b| b.count.cmp(&a.count));

    // Build table stats - group pre-trace cursors together
    // Tuple: (ops, seeks, navs, total_latency, direct_gets)
    let mut pre_trace_total: (u64, u64, u64, u64, u64) = (0, 0, 0, 0, 0);
    let mut known_dbi_stats: HashMap<u32, (u64, u64, u64, u64, u64)> = HashMap::new();

    for (dbi, stats) in dbi_stats {
        if is_pre_trace_cursor(dbi) {
            pre_trace_total.0 += stats.0;
            pre_trace_total.1 += stats.1;
            pre_trace_total.2 += stats.2;
            pre_trace_total.3 += stats.3;
            pre_trace_total.4 += stats.4;
        } else {
            known_dbi_stats.insert(dbi, stats);
        }
    }

    let mut table_stats: Vec<CursorTableStats> = known_dbi_stats
        .into_iter()
        .map(
            |(dbi, (ops, seeks, navs, total_lat, _direct_gets))| CursorTableStats {
                dbi,
                name: dbi_to_table_name(dbi).to_string(),
                ops,
                percentage: ops as f64 / total as f64 * 100.0,
                seeks,
                navs,
                avg_latency_us: (total_lat as f64 / ops as f64) / 1000.0,
            },
        )
        .collect();

    // Add pre-trace cursors as a single group if any exist
    if pre_trace_total.0 > 0 {
        table_stats.push(CursorTableStats {
            dbi: 0xFFFFFFFF,
            name: "Unknown (pre-trace cursors)".to_string(),
            ops: pre_trace_total.0,
            percentage: pre_trace_total.0 as f64 / total as f64 * 100.0,
            seeks: pre_trace_total.1,
            navs: pre_trace_total.2,
            avg_latency_us: (pre_trace_total.3 as f64 / pre_trace_total.0 as f64) / 1000.0,
        });
    }

    table_stats.sort_by(|a, b| b.ops.cmp(&a.ops));
    table_stats.truncate(20);

    // Generate timeline (bucket by 100ms)
    let bucket_ns = 100_000_000u64; // 100ms
    let num_buckets = ((duration_ns / bucket_ns) + 1) as usize;
    let mut timeline_buckets: Vec<(u32, u32, u64)> = vec![(0, 0, 0); num_buckets.min(1000)];

    for event in events {
        let bucket_idx = ((event.timestamp_ns - min_ts) / bucket_ns) as usize;
        if bucket_idx < timeline_buckets.len() {
            timeline_buckets[bucket_idx].0 += 1;
            if event.cursor_op().is_seek() {
                timeline_buckets[bucket_idx].1 += 1;
            }
            timeline_buckets[bucket_idx].2 += event.latency_ns;
        }
    }

    let timeline: Vec<CursorTimelinePoint> = timeline_buckets
        .into_iter()
        .enumerate()
        .filter(|(_, (ops, _, _))| *ops > 0)
        .map(|(i, (ops, seeks, total_lat))| CursorTimelinePoint {
            time_ms: (i as u64 * bucket_ns) / 1_000_000,
            ops,
            seeks,
            avg_latency_us: (total_lat as f64 / ops as f64) / 1000.0,
        })
        .collect();

    // Sample recent operations for log view
    let sample_size = 200.min(events.len());
    let step = events.len() / sample_size.max(1);
    let recent_ops: Vec<CursorOpSample> = events
        .iter()
        .step_by(step.max(1))
        .take(sample_size)
        .map(|e| {
            let table_name = if e.dbi < 100 {
                dbi_to_table_name(e.dbi).to_string()
            } else {
                format!("Cursor_{:x}", e.dbi)
            };
            let operation = if e.is_direct_get() {
                "DIRECT_GET".to_string()
            } else {
                e.cursor_op().to_string()
            };
            CursorOpSample {
                timestamp_ms: (e.timestamp_ns - min_ts) / 1_000_000,
                table: table_name,
                operation,
                key_hex: if e.key_size > 0 {
                    format!("0x{}", e.key_hex())
                } else {
                    String::new()
                },
                latency_us: e.latency_ns as f64 / 1000.0,
                success: e.is_success() || e.is_not_found(),
            }
        })
        .collect();

    // Analyze slow operations (>100μs) - likely page faults
    let slow_threshold_ns = 100_000u64; // 100μs

    // Group slow ops by table and operation
    // Key: (dbi, op_name), Value: (count, total_latency, max_latency)
    let mut slow_by_table_op: HashMap<(u32, String), (u64, u64, u64)> = HashMap::new();
    // Key: dbi, Value: (slow_count, total_count, total_slow_latency, max_latency)
    let mut slow_by_table: HashMap<u32, (u64, u64, u64, u64)> = HashMap::new();

    for event in events {
        let dbi = event.dbi;
        let latency = event.latency_ns;
        let op_name = if event.is_direct_get() {
            "DIRECT_GET".to_string()
        } else {
            event.cursor_op().to_string()
        };

        // Track total count per table
        let table_entry = slow_by_table.entry(dbi).or_insert((0, 0, 0, 0));
        table_entry.1 += 1;

        if latency >= slow_threshold_ns {
            // Slow operation
            table_entry.0 += 1;
            table_entry.2 += latency;
            table_entry.3 = table_entry.3.max(latency);

            let op_entry = slow_by_table_op.entry((dbi, op_name)).or_insert((0, 0, 0));
            op_entry.0 += 1;
            op_entry.1 += latency;
            op_entry.2 = op_entry.2.max(latency);
        }
    }

    // Build slow ops by table stats
    let mut slow_ops_by_table: Vec<SlowOpsTableStats> = slow_by_table
        .iter()
        .filter(|(_, (slow_count, _, _, _))| *slow_count > 0)
        .map(
            |(dbi, (slow_count, total_count, total_slow_latency, max_latency))| {
                let table_name = if *dbi < 100 {
                    dbi_to_table_name(*dbi).to_string()
                } else {
                    format!("Cursor_{:x}", dbi)
                };

                // Get breakdown by operation for this table
                let mut by_operation: Vec<SlowOpBreakdown> = slow_by_table_op
                    .iter()
                    .filter(|((d, _), _)| *d == *dbi)
                    .map(
                        |((_, op_name), (count, total_lat, max_lat))| SlowOpBreakdown {
                            operation: op_name.clone(),
                            count: *count,
                            avg_latency_us: (*total_lat as f64 / *count as f64) / 1000.0,
                            max_latency_us: *max_lat as f64 / 1000.0,
                        },
                    )
                    .collect();
                by_operation.sort_by(|a, b| b.count.cmp(&a.count));

                SlowOpsTableStats {
                    table: table_name,
                    dbi: *dbi,
                    slow_op_count: *slow_count,
                    total_op_count: *total_count,
                    slow_op_percentage: *slow_count as f64 / *total_count as f64 * 100.0,
                    avg_slow_latency_us: (*total_slow_latency as f64 / *slow_count as f64) / 1000.0,
                    max_latency_us: *max_latency as f64 / 1000.0,
                    total_slow_time_ms: *total_slow_latency as f64 / 1_000_000.0,
                    by_operation,
                }
            },
        )
        .collect();
    slow_ops_by_table.sort_by(|a, b| {
        b.total_slow_time_ms
            .partial_cmp(&a.total_slow_time_ms)
            .unwrap()
    });

    // Analyze slow keys - keys that are frequently accessed with high latency
    // Key: (dbi, key_hex), Value: (slow_count, total_count, total_latency, max_latency, operations)
    let mut key_stats: HashMap<(u32, String), (u64, u64, u64, u64, Vec<String>)> = HashMap::new();

    for event in events {
        if event.key_size == 0 {
            continue;
        }

        let dbi = event.dbi;
        let key_hex = event.key_hex();
        let latency = event.latency_ns;
        let op_name = if event.is_direct_get() {
            "DIRECT_GET".to_string()
        } else {
            event.cursor_op().to_string()
        };

        let entry = key_stats
            .entry((dbi, key_hex))
            .or_insert((0, 0, 0, 0, Vec::new()));
        entry.1 += 1; // total count

        if latency >= slow_threshold_ns {
            entry.0 += 1; // slow count
            entry.2 += latency; // total latency (only slow ops)
            entry.3 = entry.3.max(latency); // max latency
            if !entry.4.contains(&op_name) {
                entry.4.push(op_name);
            }
        }
    }

    // Build slow keys stats - only keys with multiple slow accesses
    let mut slow_keys: Vec<SlowKeyStats> = key_stats
        .into_iter()
        .filter(|(_, (slow_count, _, _, _, _))| *slow_count >= 2) // At least 2 slow accesses
        .map(
            |(
                (dbi, key_hex),
                (slow_count, total_count, total_latency, max_latency, operations),
            )| {
                let table_name = if dbi < 100 {
                    dbi_to_table_name(dbi).to_string()
                } else {
                    format!("Cursor_{:x}", dbi)
                };

                // Create a readable prefix (first 8 bytes or less)
                let key_prefix = if key_hex.len() > 16 {
                    format!("0x{}...", &key_hex[..16])
                } else {
                    format!("0x{}", key_hex)
                };

                SlowKeyStats {
                    table: table_name,
                    key_hex: format!("0x{}", key_hex),
                    key_prefix,
                    slow_access_count: slow_count,
                    total_access_count: total_count,
                    avg_latency_us: (total_latency as f64 / slow_count as f64) / 1000.0,
                    max_latency_us: max_latency as f64 / 1000.0,
                    operations,
                }
            },
        )
        .collect();
    slow_keys.sort_by(|a, b| b.slow_access_count.cmp(&a.slow_access_count));
    slow_keys.truncate(50); // Top 50 slow keys

    // Generate warning if significant portion of ops are from pre-trace cursors
    let pre_trace_ratio = pre_trace_cursor_ops as f64 / total as f64;
    let pre_trace_warning = if pre_trace_ratio > 0.1 {
        Some(format!(
            "Warning: {:.1}% of cursor operations ({}) are from cursors opened before tracing started. \
             These cannot be attributed to specific tables. To capture all cursors, start tracing \
             before the application opens its database, or restart the application after tracing begins.",
            pre_trace_ratio * 100.0,
            pre_trace_cursor_ops
        ))
    } else {
        None
    };

    CursorData {
        has_data: true,
        summary: CursorSummary {
            total_ops: total,
            op_rate_per_sec: if duration_secs > 0.0 {
                total as f64 / duration_secs
            } else {
                0.0
            },
            avg_latency_us,
            p50_latency_us,
            p99_latency_us,
            seek_count,
            seek_ratio: seek_count as f64 / total as f64 * 100.0,
            nav_count,
            error_count,
            duration_secs,
            direct_get_count,
            direct_get_ratio: direct_get_count as f64 / total as f64 * 100.0,
        },
        operations,
        table_stats,
        timeline,
        recent_ops,
        slow_ops_by_table,
        slow_keys,
        pre_trace_cursor_ops,
        pre_trace_warning,
    }
}

fn generate_timeline(
    events: &[&PageFaultEvent],
    min_ts: u64,
    duration_ns: u64,
) -> Vec<TimelinePoint> {
    const MAX_BUCKETS: u64 = 1000;
    let bucket_ns = (duration_ns / MAX_BUCKETS).max(1_000_000); // At least 1ms buckets
    let num_buckets = (duration_ns / bucket_ns + 1) as usize;

    let mut buckets: Vec<(u32, u32, std::collections::HashSet<u64>)> =
        vec![(0, 0, std::collections::HashSet::new()); num_buckets.min(MAX_BUCKETS as usize)];

    for e in events {
        let bucket_idx = ((e.timestamp_ns - min_ts) / bucket_ns) as usize;
        if bucket_idx < buckets.len() {
            buckets[bucket_idx].0 += 1;
            if e.is_major_fault() {
                buckets[bucket_idx].1 += 1;
            }
            buckets[bucket_idx].2.insert(e.page_number());
        }
    }

    buckets
        .into_iter()
        .enumerate()
        .map(|(i, (faults, major, pages))| TimelinePoint {
            time_ms: (i as u64 * bucket_ns) / 1_000_000,
            faults,
            major_faults: major,
            unique_pages: pages.len() as u32,
        })
        .collect()
}

/// Generate table stats using direct correlation between page faults and cursor operations.
///
/// This is the preferred method - it uses timestamp + thread matching to attribute
/// each page fault to the cursor operation that caused it.
fn generate_table_stats_correlated(
    events: &[&PageFaultEvent],
    correlation: &FaultCorrelation,
    cursor_ops_by_table: &HashMap<String, u64>,
    _attribution: Option<&PageAttribution>,
) -> (Vec<TableStats>, Option<String>) {
    let total_faults = events.len() as u64;

    // If no faults were correlated, return empty with warning
    if correlation.correlated_faults.is_empty() {
        let warning = if correlation.total_faults > 0 {
            Some(format!(
                "Could not correlate any page faults with cursor operations. \
                 {} faults occurred outside of cursor operation windows.",
                correlation.uncorrelated_faults
            ))
        } else {
            None
        };
        return (vec![], warning);
    }

    // Build stats from correlated faults
    let correlated_total: u64 = correlation.correlated_faults.values().map(|(f, _)| f).sum();

    let mut stats: Vec<_> = correlation
        .correlated_faults
        .iter()
        .map(|(table_name, (faults, major_faults))| {
            let table = RethTable::from_name(table_name);
            let ops = cursor_ops_by_table.get(table_name).copied().unwrap_or(0);
            TableStats {
                name: table_name.clone(),
                category: table.category().to_string(),
                faults: *faults,
                major_faults: *major_faults,
                percentage: *faults as f64 / total_faults as f64 * 100.0,
                has_cursor_ops: ops > 0,
                cursor_ops: ops,
                faults_correlated: true,
            }
        })
        .collect();

    stats.sort_by(|a, b| b.faults.cmp(&a.faults));

    // Generate informative message about correlation
    let correlation_rate = correlated_total as f64 / total_faults as f64 * 100.0;
    let warning = Some(format!(
        "Correlated {:.1}% of page faults ({} of {}) with cursor operations. \
         {} faults occurred outside cursor windows (background I/O, prefetch, or kernel activity).",
        correlation_rate, correlated_total, total_faults, correlation.uncorrelated_faults
    ));

    (stats, warning)
}

#[allow(dead_code)]
fn generate_table_stats(
    events: &[&PageFaultEvent],
    attribution: Option<&PageAttribution>,
    tables_with_ops: &std::collections::HashSet<String>,
    cursor_ops_by_table: &HashMap<String, u64>,
) -> (Vec<TableStats>, Option<String>) {
    // If we have mdbx_stat data, use proportion-based attribution
    // Since we can't map individual pages to tables, we distribute faults
    // proportionally based on each table's share of total pages
    if let Some(attr) = attribution {
        if let Some(mdbx_stats) = attr.get_mdbx_stats() {
            return generate_table_stats_from_mdbx(
                events,
                mdbx_stats,
                tables_with_ops,
                cursor_ops_by_table,
            );
        }
    }

    // Fallback: try to attribute based on page-level mapping
    let mut table_counts: HashMap<RethTable, (u64, u64)> = HashMap::new();
    let page_size = attribution.map(|a| a.page_size()).unwrap_or(4096);

    for e in events {
        let table = if let Some(attr) = attribution {
            attr.get_table_for_offset(e.file_offset)
                .unwrap_or(RethTable::Unknown(0))
        } else {
            // Use heuristic based on offset
            crate::mdbx_metadata::estimate_table_from_pattern(e.file_offset, page_size, 0, None)
        };

        let entry = table_counts.entry(table).or_insert((0, 0));
        entry.0 += 1;
        if e.is_major_fault() {
            entry.1 += 1;
        }
    }

    let total = events.len() as f64;
    let mut stats: Vec<_> = table_counts
        .into_iter()
        .map(|(table, (faults, major))| {
            let name = table.to_string();
            let ops = cursor_ops_by_table.get(&name).copied().unwrap_or(0);
            TableStats {
                name: name.clone(),
                category: table.category().to_string(),
                faults,
                major_faults: major,
                percentage: faults as f64 / total * 100.0,
                has_cursor_ops: tables_with_ops.contains(&name),
                cursor_ops: ops,
                faults_correlated: false,
            }
        })
        .collect();

    stats.sort_by(|a, b| b.faults.cmp(&a.faults));
    (stats, None)
}

/// Generate table stats using mdbx_stat proportions
///
/// When cursor operation data is available, only attribute faults to tables
/// that were actually accessed. This prevents false positives from large tables
/// (like TransactionHashNumbers) that weren't accessed during the trace.
fn generate_table_stats_from_mdbx(
    events: &[&PageFaultEvent],
    mdbx_stats: &[crate::mdbx_metadata::MdbxStatOutput],
    tables_with_ops: &std::collections::HashSet<String>,
    cursor_ops_by_table: &HashMap<String, u64>,
) -> (Vec<TableStats>, Option<String>) {
    let total_faults = events.len() as u64;
    let major_faults = events.iter().filter(|e| e.is_major_fault()).count() as u64;

    // Check if we have cursor operation data to filter by
    let has_cursor_data = !tables_with_ops.is_empty();

    // Calculate total pages - if we have cursor data, only count tables that were accessed
    let (total_pages, filtered_tables): (u64, Vec<_>) = if has_cursor_data {
        let filtered: Vec<_> = mdbx_stats
            .iter()
            .filter(|s| s.name != "@main" && tables_with_ops.contains(&s.name))
            .collect();
        let pages: u64 = filtered.iter().map(|s| s.total_pages).sum();
        (pages, filtered)
    } else {
        let filtered: Vec<_> = mdbx_stats
            .iter()
            .filter(|s| s.name != "@main" && s.total_pages > 0)
            .collect();
        let pages: u64 = filtered.iter().map(|s| s.total_pages).sum();
        (pages, filtered)
    };

    if total_pages == 0 {
        return (
            vec![TableStats {
                name: "Unknown".to_string(),
                category: "Unknown".to_string(),
                faults: total_faults,
                major_faults,
                percentage: 100.0,
                has_cursor_ops: false,
                cursor_ops: 0,
                faults_correlated: false,
            }],
            None,
        );
    }

    // Generate warning if we filtered out tables
    let warning = if has_cursor_data {
        let excluded_tables: Vec<_> = mdbx_stats
            .iter()
            .filter(|s| {
                s.name != "@main" && s.total_pages > 0 && !tables_with_ops.contains(&s.name)
            })
            .collect();

        if !excluded_tables.is_empty() {
            let excluded_pages: u64 = excluded_tables.iter().map(|s| s.total_pages).sum();
            let all_pages: u64 = mdbx_stats
                .iter()
                .filter(|s| s.name != "@main")
                .map(|s| s.total_pages)
                .sum();
            let excluded_pct = excluded_pages as f64 / all_pages as f64 * 100.0;

            // Get top 3 excluded tables by size
            let mut excluded_sorted = excluded_tables.clone();
            excluded_sorted.sort_by(|a, b| b.total_pages.cmp(&a.total_pages));
            let top_excluded: Vec<_> = excluded_sorted
                .iter()
                .take(3)
                .map(|s| {
                    format!(
                        "{} ({:.1}%)",
                        s.name,
                        s.total_pages as f64 / all_pages as f64 * 100.0
                    )
                })
                .collect();

            Some(format!(
                "Page faults are attributed only to tables with cursor operations. \
                 {} tables ({:.1}% of DB size) had no operations and were excluded: {}{}",
                excluded_tables.len(),
                excluded_pct,
                top_excluded.join(", "),
                if excluded_tables.len() > 3 {
                    ", ..."
                } else {
                    ""
                }
            ))
        } else {
            None
        }
    } else {
        Some(
            "No cursor operation data available. Page faults are attributed proportionally \
             by table size, which may not reflect actual access patterns."
                .to_string(),
        )
    };

    let mut stats: Vec<_> = filtered_tables
        .iter()
        .map(|s| {
            let proportion = s.total_pages as f64 / total_pages as f64;
            let estimated_faults = (total_faults as f64 * proportion).round() as u64;
            let estimated_major = (major_faults as f64 * proportion).round() as u64;
            let table = RethTable::from_name(&s.name);
            let ops = cursor_ops_by_table.get(&s.name).copied().unwrap_or(0);

            TableStats {
                name: s.name.clone(),
                category: table.category().to_string(),
                faults: estimated_faults,
                major_faults: estimated_major,
                percentage: proportion * 100.0,
                has_cursor_ops: tables_with_ops.contains(&s.name),
                cursor_ops: ops,
                faults_correlated: false, // These are proportional estimates, not correlated
            }
        })
        .collect();

    stats.sort_by(|a, b| b.faults.cmp(&a.faults));
    (stats, warning)
}

fn generate_thread_stats(events: &[&PageFaultEvent]) -> Vec<ThreadStats> {
    let mut thread_counts: HashMap<u32, u64> = HashMap::new();
    for e in events {
        *thread_counts.entry(e.tid).or_insert(0) += 1;
    }

    let total = events.len() as f64;
    let mut stats: Vec<_> = thread_counts
        .into_iter()
        .map(|(tid, faults)| ThreadStats {
            tid,
            faults,
            percentage: faults as f64 / total * 100.0,
        })
        .collect();

    stats.sort_by(|a, b| b.faults.cmp(&a.faults));
    stats.truncate(20); // Top 20 threads
    stats
}

fn analyze_patterns(events: &[&PageFaultEvent]) -> PatternAnalysis {
    if events.len() < 2 {
        return PatternAnalysis {
            sequential_ratio: 0.0,
            random_ratio: 0.0,
            stride_distribution: vec![],
            burst_stats: BurstStats {
                median_events: 0,
                p95_events: 0,
                max_events: 0,
                bucket_ms: 100,
            },
        };
    }

    // Stride analysis
    let mut stride_counts: HashMap<i64, u64> = HashMap::new();
    let mut sequential = 0u64;
    let mut random = 0u64;

    for window in events.windows(2) {
        let stride = window[1].page_number() as i64 - window[0].page_number() as i64;
        // Bucket to page granularity
        let bucketed = stride;
        *stride_counts.entry(bucketed).or_insert(0) += 1;

        if stride.abs() <= 4 {
            sequential += 1;
        } else {
            random += 1;
        }
    }

    let total = sequential + random;
    let sequential_ratio = if total > 0 {
        sequential as f64 / total as f64
    } else {
        0.0
    };

    // Get top strides
    let mut strides: Vec<_> = stride_counts.into_iter().collect();
    strides.sort_by(|a, b| b.1.cmp(&a.1));

    let stride_distribution: Vec<_> = strides
        .into_iter()
        .take(15)
        .map(|(stride, count)| {
            let pattern_type = match stride {
                0 => "same-page",
                1 => "sequential-forward",
                -1 => "sequential-backward",
                2..=4 => "near-sequential",
                -4..=-2 => "near-sequential-back",
                s if s > 100 => "random-jump",
                s if s < -100 => "random-jump-back",
                _ => "medium-jump",
            };
            StrideInfo {
                stride_pages: stride,
                count,
                pattern_type: pattern_type.to_string(),
            }
        })
        .collect();

    // Burst analysis
    let bucket_ms = 100u64;
    let min_ts = events.iter().map(|e| e.timestamp_ns).min().unwrap();
    let mut buckets: HashMap<u64, u32> = HashMap::new();

    for e in events {
        let bucket = (e.timestamp_ns - min_ts) / (bucket_ms * 1_000_000);
        *buckets.entry(bucket).or_insert(0) += 1;
    }

    let mut bucket_counts: Vec<u32> = buckets.values().copied().collect();
    bucket_counts.sort();

    let burst_stats = if bucket_counts.is_empty() {
        BurstStats {
            median_events: 0,
            p95_events: 0,
            max_events: 0,
            bucket_ms,
        }
    } else {
        BurstStats {
            median_events: bucket_counts[bucket_counts.len() / 2],
            p95_events: bucket_counts[(bucket_counts.len() as f64 * 0.95) as usize],
            max_events: *bucket_counts.last().unwrap(),
            bucket_ms,
        }
    };

    PatternAnalysis {
        sequential_ratio,
        random_ratio: 1.0 - sequential_ratio,
        stride_distribution,
        burst_stats,
    }
}

fn analyze_prefetch(events: &[&PageFaultEvent]) -> PrefetchAnalysis {
    if events.len() < 100 {
        return PrefetchAnalysis {
            prediction_hit_rate: 0.0,
            locality_score: 0.0,
            recommendation: "Not enough data for analysis".to_string(),
            prefetch_benefit_estimate: 0.0,
        };
    }

    // Stride-based prediction
    let window_size = 10;
    let lookahead = 5;
    let mut correct_predictions = 0;
    let mut total_predictions = 0;

    for i in window_size..(events.len() - lookahead) {
        let recent_strides: Vec<i64> = (0..window_size - 1)
            .map(|j| {
                events[i - window_size + j + 1].file_offset as i64
                    - events[i - window_size + j].file_offset as i64
            })
            .collect();

        let avg_stride: i64 = recent_strides.iter().sum::<i64>() / recent_strides.len() as i64;

        let current_offset = events[i].file_offset;
        let predictions: Vec<u64> = (1..=lookahead)
            .map(|j| (current_offset as i64 + avg_stride * j as i64) as u64)
            .collect();

        let actual_offsets: Vec<u64> = (1..=lookahead).map(|j| events[i + j].file_offset).collect();

        for pred in &predictions {
            total_predictions += 1;
            let pred_page = pred / 4096;
            for actual in &actual_offsets {
                let actual_page = actual / 4096;
                if (pred_page as i64 - actual_page as i64).abs() <= 1 {
                    correct_predictions += 1;
                    break;
                }
            }
        }
    }

    let hit_rate = if total_predictions > 0 {
        correct_predictions as f64 / total_predictions as f64 * 100.0
    } else {
        0.0
    };

    // Locality analysis
    let locality_window = 100;
    let mut locality_scores: Vec<f64> = Vec::new();

    for chunk in events.chunks(locality_window) {
        let unique_pages: std::collections::HashSet<u64> =
            chunk.iter().map(|e| e.page_number()).collect();
        let locality = unique_pages.len() as f64 / chunk.len() as f64;
        locality_scores.push(locality);
    }

    let avg_locality: f64 =
        locality_scores.iter().sum::<f64>() / locality_scores.len().max(1) as f64;

    let (recommendation, benefit) = if hit_rate > 30.0 {
        (
            "Good predictability - prefetching would significantly reduce page faults".to_string(),
            hit_rate * 0.8,
        )
    } else if hit_rate > 15.0 {
        (
            "Moderate predictability - prefetching may help for some access patterns".to_string(),
            hit_rate * 0.5,
        )
    } else {
        (
            "Poor predictability - consider larger page sizes, caching, or mlock()".to_string(),
            hit_rate * 0.2,
        )
    };

    PrefetchAnalysis {
        prediction_hit_rate: hit_rate,
        locality_score: 1.0 - avg_locality, // Invert so higher is better
        recommendation,
        prefetch_benefit_estimate: benefit,
    }
}

fn generate_heatmap(
    events: &[&PageFaultEvent],
    min_ts: u64,
    duration_ns: u64,
    min_offset: u64,
    max_offset: u64,
) -> HeatmapData {
    const TIME_BUCKETS: u32 = 100;
    const OFFSET_BUCKETS: u32 = 50;

    if events.is_empty() || duration_ns == 0 || max_offset == min_offset {
        return HeatmapData {
            time_buckets: 0,
            offset_buckets: 0,
            min_time_ms: 0,
            max_time_ms: 0,
            min_offset_gb: 0.0,
            max_offset_gb: 0.0,
            data: vec![],
            max_count: 0,
        };
    }

    let time_bucket_ns = duration_ns / TIME_BUCKETS as u64;
    let offset_bucket_size = (max_offset - min_offset) / OFFSET_BUCKETS as u64;

    let mut data = vec![0u32; (TIME_BUCKETS * OFFSET_BUCKETS) as usize];
    let mut max_count = 0u32;

    for e in events {
        let time_idx =
            (((e.timestamp_ns - min_ts) / time_bucket_ns.max(1)) as u32).min(TIME_BUCKETS - 1);
        let offset_idx = (((e.file_offset - min_offset) / offset_bucket_size.max(1)) as u32)
            .min(OFFSET_BUCKETS - 1);

        let idx = (time_idx * OFFSET_BUCKETS + offset_idx) as usize;
        data[idx] += 1;
        max_count = max_count.max(data[idx]);
    }

    HeatmapData {
        time_buckets: TIME_BUCKETS,
        offset_buckets: OFFSET_BUCKETS,
        min_time_ms: 0,
        max_time_ms: duration_ns / 1_000_000,
        min_offset_gb: min_offset as f64 / 1e9,
        max_offset_gb: max_offset as f64 / 1e9,
        data,
        max_count,
    }
}

/// Generate the HTML viewer file
pub fn generate_html(data: &ViewerData) -> String {
    template::generate_html(data)
}

/// Write the HTML viewer to a file
pub fn write_html(data: &ViewerData, path: impl AsRef<Path>) -> std::io::Result<()> {
    let html = generate_html(data);
    std::fs::write(path, html)
}
