//! Web-based trace viewer
//!
//! Generates a self-contained HTML file with interactive visualizations
//! for MDBX page fault traces and cursor operations.

mod template;

use crate::event::{dbi_to_table_name, is_pre_trace_cursor, CursorEvent, PageFaultEvent, TxnEvent};
use crate::mdbx_metadata::{PageAttribution, RethTable};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

/// DBI numbers for tables where WRITES indicate block processing.
/// We use writes to these tables to determine which blocks were actually traced/synced.
/// Based on reth's table definitions (verified against reth main branch 2025-01).
const BLOCK_WRITE_DBIS: &[u32] = &[
    2,  // CanonicalHeaders - Key: BlockNumber (u64) - definitive for canonicalized blocks
    6,  // BlockBodyIndices - Key: BlockNumber (u64)
    18, // AccountChangeSets - Key: BlockNumber (u64)
    19, // StorageChangeSets - Key: BlockNumberAddress (block_number || address)
];

/// Extract block number from a key if it's from a block-write table.
/// Returns None if the key is too short or the DBI doesn't match.
fn extract_block_from_key(dbi: u32, key_data: &[u8], key_size: u32) -> Option<u64> {
    // Check if this is a block-write table
    if !BLOCK_WRITE_DBIS.contains(&dbi) {
        return None;
    }

    // Need at least 8 bytes for block number
    if key_size < 8 || key_data.len() < 8 {
        return None;
    }

    // Block number is stored as big-endian u64 in first 8 bytes
    let block = u64::from_be_bytes([
        key_data[0],
        key_data[1],
        key_data[2],
        key_data[3],
        key_data[4],
        key_data[5],
        key_data[6],
        key_data[7],
    ]);

    // Sanity check: block numbers should be reasonable
    // Ethereum mainnet is around 21 million blocks as of 2025
    // Allow up to 50 million for future growth and testnets
    if block > 50_000_000 {
        return None;
    }

    Some(block)
}

/// Extract block range from cursor events by looking at WRITE operations to block-keyed tables.
/// This gives us the range of blocks that were actually processed/synced during the trace,
/// rather than the range of all historical blocks accessed (which would be much larger).
fn extract_block_range(cursor_events: &[CursorEvent]) -> Option<BlockRange> {
    let mut min_block: Option<u64> = None;
    let mut max_block: Option<u64> = None;

    for event in cursor_events {
        // Only consider write operations (cursor put, direct put)
        if !event.is_write_op() {
            continue;
        }

        if let Some(block) = extract_block_from_key(event.dbi, &event.key_data, event.key_size) {
            min_block = Some(min_block.map_or(block, |m| m.min(block)));
            max_block = Some(max_block.map_or(block, |m| m.max(block)));
        }
    }

    match (min_block, max_block) {
        (Some(min), Some(max)) => Some(BlockRange {
            min_block: min,
            max_block: max,
            block_count: max.saturating_sub(min) + 1,
        }),
        _ => None,
    }
}

/// Compute p50, p95, p99 percentiles from a sorted slice of latencies in nanoseconds.
/// Returns values converted to microseconds.
fn compute_percentiles_us(sorted_latencies: &[u64]) -> (f64, f64, f64) {
    if sorted_latencies.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let len = sorted_latencies.len();
    let p50_idx = (len as f64 * 0.5) as usize;
    let p95_idx = ((len as f64 * 0.95) as usize).min(len - 1);
    let p99_idx = ((len as f64 * 0.99) as usize).min(len - 1);

    let p50 = sorted_latencies.get(p50_idx).copied().unwrap_or(0) as f64 / 1000.0;
    let p95 = sorted_latencies.get(p95_idx).copied().unwrap_or(0) as f64 / 1000.0;
    let p99 = sorted_latencies.get(p99_idx).copied().unwrap_or(0) as f64 / 1000.0;

    (p50, p95, p99)
}

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
    /// Number of faults with direct BPF attribution (100% accurate)
    pub directly_attributed: u64,
    /// Number of faults correlated via timestamp matching (fallback)
    pub timestamp_correlated: u64,
}

/// Correlate page faults with cursor operations.
///
/// This function uses two methods, in order of preference:
///
/// 1. Direct BPF Attribution (100% accurate): Page faults that occurred during an
///    active MDBX operation have the operation's DBI embedded directly in the event
///    by the BPF probe.
///
/// 2. Timestamp + Thread ID Matching (fallback)**: For page faults without direct
///    attribution (e.g., from older traces), we fall back to matching the fault's
///    timestamp against cursor operation time windows on the same thread.
///
/// Direct attribution provides 100% accurate correlation - we know exactly which
/// MDBX operation caused each page fault. Timestamp matching is a statistical fallback.
pub fn correlate_faults_with_cursors(
    page_faults: &[&PageFaultEvent],
    cursor_events: &[CursorEvent],
) -> FaultCorrelation {
    let mut result = FaultCorrelation::default();
    result.total_faults = page_faults.len() as u64;

    // First pass: use direct BPF attribution for faults that have it
    let mut faults_needing_timestamp_correlation: Vec<&PageFaultEvent> = Vec::new();

    for fault in page_faults {
        let is_major = fault.is_major_fault();

        // Check if this fault has direct BPF attribution (active_dbi is set)
        if fault.has_active_op() {
            // Direct attribution - we know exactly which table caused this fault
            result.directly_attributed += 1;

            let table_name = if fault.active_dbi < 100 {
                dbi_to_table_name(fault.active_dbi).to_string()
            } else if is_pre_trace_cursor(fault.active_dbi) {
                "Unknown (pre-trace cursor)".to_string()
            } else {
                format!("Unknown (DBI {})", fault.active_dbi)
            };

            let entry = result.correlated_faults.entry(table_name).or_insert((0, 0));
            entry.0 += 1;
            if is_major {
                entry.1 += 1;
            }
        } else {
            // No direct attribution - need timestamp correlation
            faults_needing_timestamp_correlation.push(fault);
        }
    }

    // If all faults have direct attribution, we're done
    if faults_needing_timestamp_correlation.is_empty() {
        return result;
    }

    // Second pass: timestamp correlation for faults without direct attribution
    if cursor_events.is_empty() {
        // No cursor events to correlate against
        result.uncorrelated_faults = faults_needing_timestamp_correlation.len() as u64;
        result.uncorrelated_major_faults = faults_needing_timestamp_correlation
            .iter()
            .filter(|e| e.is_major_fault())
            .count() as u64;
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
            "Unknown (pre-trace cursor)".to_string()
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

    // Now correlate remaining page faults using timestamp matching
    for fault in faults_needing_timestamp_correlation {
        let fault_time = fault.timestamp_ns;
        let fault_tid = fault.tid;
        let is_major = fault.is_major_fault();

        // Look up cursor windows for this thread
        let mut correlated = false;
        if let Some(windows) = cursor_windows_by_tid.get(&fault_tid) {
            // Binary search to find potential matching windows
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
                    // Found a match via timestamp correlation
                    result.timestamp_correlated += 1;
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
    /// Table breakdown (legacy - kept for compatibility)
    pub tables: Vec<TableStats>,
    /// Unified table view with drill-down (new)
    pub unified_tables: Vec<UnifiedTableStats>,
    /// Thread distribution
    pub threads: Vec<ThreadStats>,

    /// Access pattern analysis
    pub patterns: PatternAnalysis,
    /// Heatmap data (2D grid)
    pub heatmap: HeatmapData,
    /// Cursor operation data
    pub cursor_data: CursorData,
    /// Transaction lifecycle data
    pub txn_data: TxnData,
    /// Warning about page fault attribution method
    pub page_fault_attribution_warning: Option<String>,
    /// Direct fault attribution data (from BPF active_ops tracking)
    pub direct_fault_attribution: DirectFaultAttribution,
}

/// Direct fault attribution data from BPF active_ops tracking.
#[derive(Debug, Serialize, Default)]
pub struct DirectFaultAttribution {
    /// Whether direct attribution data is available
    pub has_data: bool,
    /// Number of faults with direct attribution
    pub directly_attributed_count: u64,
    /// Number of faults using timestamp fallback
    pub timestamp_fallback_count: u64,
    /// Number of uncorrelated faults
    pub uncorrelated_count: u64,
    /// Faults by operation type (CURSOR_GET, CURSOR_PUT, etc.)
    pub faults_by_op_type: Vec<FaultsByOpType>,
    /// Faults by cursor operation (SET_RANGE, NEXT, etc.) - only for CURSOR_GET
    pub faults_by_cursor_op: Vec<FaultsByCursorOp>,
}

/// Fault counts grouped by operation type
#[derive(Debug, Serialize)]
pub struct FaultsByOpType {
    pub op_type: String,
    pub total_faults: u64,
    pub major_faults: u64,
    pub percentage: f64,
}

/// Fault counts grouped by cursor operation (for CURSOR_GET only)
#[derive(Debug, Serialize)]
pub struct FaultsByCursorOp {
    pub cursor_op: String,
    pub total_faults: u64,
    pub major_faults: u64,
    pub percentage: f64,
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
    pub p95_latency_us: f64,
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
    pub p50_latency_us: f64,
    pub p95_latency_us: f64,
    pub p99_latency_us: f64,
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

/// Transaction lifecycle data for parallelization analysis
#[derive(Debug, Serialize, Default)]
pub struct TxnData {
    /// Whether transaction data is available
    pub has_data: bool,
    /// Summary statistics
    pub summary: TxnSummary,
    /// Timeline of transactions (for Gantt chart visualization)
    pub timeline: Vec<TxnTimelineEntry>,
    /// Thread activity breakdown
    pub thread_stats: Vec<TxnThreadStats>,
    /// Concurrent transaction analysis
    pub concurrency: TxnConcurrencyStats,
    /// RW commit timeline - timestamp and latency for each commit (for timeline chart)
    pub rw_commit_timeline: Vec<RwCommitPoint>,
}

/// A single RW commit point for timeline visualization
#[derive(Debug, Serialize, Default, Clone)]
pub struct RwCommitPoint {
    /// Time relative to trace start (seconds)
    pub time_secs: f64,
    /// Commit latency in milliseconds
    pub latency_ms: f64,
}

/// Transaction summary statistics
#[derive(Debug, Serialize, Default)]
pub struct TxnSummary {
    pub total_events: u64,
    pub begin_count: u64,
    pub commit_count: u64,
    pub abort_count: u64,
    pub ro_count: u64,
    pub rw_count: u64,
    pub duration_secs: f64,
    pub txn_rate_per_sec: f64,
    pub avg_commit_latency_us: f64,
    pub p50_commit_latency_us: f64,
    pub p95_commit_latency_us: f64,
    pub p99_commit_latency_us: f64,
    pub max_commit_latency_us: f64,
}

/// A transaction's lifecycle for timeline visualization
#[derive(Debug, Serialize)]
pub struct TxnTimelineEntry {
    /// Thread ID
    pub tid: u32,
    /// Transaction pointer as hex string (to avoid JS precision loss with u64)
    pub txn_ptr: String,
    /// Start time relative to trace start (ms)
    pub start_ms: f64,
    /// End time relative to trace start (ms) - None if still open
    pub end_ms: Option<f64>,
    /// Duration in ms (if completed)
    pub duration_ms: Option<f64>,
    /// Transaction type: "RO" or "RW"
    pub txn_type: String,
    /// How the transaction ended: "commit", "abort", or "open"
    pub end_type: String,
    /// Commit latency in us (if committed)
    pub commit_latency_us: Option<f64>,
}

/// Per-thread transaction statistics
#[derive(Debug, Serialize)]
pub struct TxnThreadStats {
    pub tid: u32,
    pub total_txns: u64,
    pub ro_txns: u64,
    pub rw_txns: u64,
    pub commits: u64,
    pub aborts: u64,
    pub avg_commit_latency_us: f64,
    pub percentage: f64,
}

/// Concurrency analysis statistics
#[derive(Debug, Serialize, Default)]
pub struct TxnConcurrencyStats {
    /// Maximum number of concurrent RO transactions observed
    pub max_concurrent_ro: u32,
    /// Maximum number of concurrent RW transactions observed (should be 0 or 1 for MDBX)
    pub max_concurrent_rw: u32,
    /// Maximum total concurrent transactions
    pub max_concurrent_total: u32,
    /// Average concurrent RO transactions
    pub avg_concurrent_ro: f64,
    /// Timeline of concurrency levels
    pub concurrency_timeline: Vec<ConcurrencyPoint>,
}

/// A point in the concurrency timeline
#[derive(Debug, Serialize)]
pub struct ConcurrencyPoint {
    pub time_ms: u64,
    pub concurrent_ro: u32,
    pub concurrent_rw: u32,
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
    /// Block range extracted from cursor operation keys (if available)
    pub block_range: Option<BlockRange>,
}

/// Block range information extracted from trace
#[derive(Debug, Serialize, Clone)]
pub struct BlockRange {
    pub min_block: u64,
    pub max_block: u64,
    pub block_count: u64,
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

/// Unified table view combining faults, operations, and slow ops data
#[derive(Debug, Serialize)]
pub struct UnifiedTableStats {
    pub name: String,
    pub dbi: u32,
    // Fault data (physical layer)
    pub faults: u64,
    pub major_faults: u64,
    pub fault_percentage: f64,
    // Operation data (logical layer)
    pub total_ops: u64,
    pub slow_ops: u64,
    pub slow_ops_percentage: f64,
    pub time_lost_ms: f64,
    pub avg_latency_us: f64,
    pub max_latency_us: f64,
    // Top operation causing faults/slowness
    pub top_operation: String,
    // Drill-down details
    pub details: TableDrillDown,
}

/// Drill-down details for a table
#[derive(Debug, Serialize)]
pub struct TableDrillDown {
    /// Faults by operation type (CURSOR_GET, CURSOR_PUT, etc.)
    pub faults_by_op: Vec<OpFaultCount>,
    /// Faults by cursor operation (SET_RANGE, NEXT, etc.)
    pub faults_by_cursor_op: Vec<OpFaultCount>,
    /// Slow operations breakdown
    pub slow_ops_breakdown: Vec<SlowOpBreakdown>,
    /// Hot keys for this table
    pub hot_keys: Vec<TableHotKey>,
}

/// Fault count for an operation type
#[derive(Debug, Serialize)]
pub struct OpFaultCount {
    pub operation: String,
    pub faults: u64,
    pub major_faults: u64,
}

/// Hot key entry for a specific table
#[derive(Debug, Serialize)]
pub struct TableHotKey {
    pub key_hex: String,
    pub slow_count: u64,
    pub total_count: u64,
    pub avg_latency_us: f64,
    pub max_latency_us: f64,
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
    pub burst_stats: BurstStats,
    /// Top stride patterns for summary display
    pub top_strides: Vec<StrideInfo>,
}

#[derive(Debug, Serialize)]
pub struct StrideInfo {
    pub stride_pages: i64,
    pub count: u64,
    pub pattern_type: String,
    pub percentage: f64,
}

#[derive(Debug, Serialize)]
pub struct BurstStats {
    pub median_events: u32,
    pub p95_events: u32,
    pub max_events: u32,
    pub bucket_ms: u64,
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
    txn_events: &[TxnEvent],
    attribution: Option<&PageAttribution>,
) -> ViewerData {
    let page_faults: Vec<_> = events.iter().filter(|e| e.event_type == 1).collect();

    // Extract block range from cursor events
    let block_range = extract_block_range(cursor_events);

    // Generate cursor data
    let cursor_data = generate_cursor_data(cursor_events);

    // Generate transaction data
    let txn_data = generate_txn_data(txn_events);

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
                block_range: block_range.clone(),
            },
            timeline: vec![],
            tables: vec![],
            unified_tables: vec![],
            threads: vec![],

            patterns: PatternAnalysis {
                sequential_ratio: 0.0,
                random_ratio: 0.0,
                burst_stats: BurstStats {
                    median_events: 0,
                    p95_events: 0,
                    max_events: 0,
                    bucket_ms: 100,
                },
                top_strides: vec![],
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
            txn_data,
            page_fault_attribution_warning: None,
            direct_fault_attribution: DirectFaultAttribution::default(),
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
        block_range,
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

    // Heatmap
    let heatmap = generate_heatmap(&page_faults, min_ts, duration_ns, min_offset, max_offset);

    // Generate direct fault attribution data from page faults with active_op info
    let direct_fault_attribution = generate_direct_fault_attribution(&page_faults, &correlation);

    // Generate unified table stats combining faults + slow ops
    let unified_tables = generate_unified_table_stats(&page_faults, &correlation, &cursor_data);

    ViewerData {
        summary,
        timeline,
        tables,
        unified_tables,
        threads,

        patterns,
        heatmap,
        cursor_data,
        txn_data,
        page_fault_attribution_warning,
        direct_fault_attribution,
    }
}

/// Generate direct fault attribution data from page faults with active_op info.
/// This extracts per-operation-type and per-cursor-op fault counts from BPF-attributed faults.
fn generate_direct_fault_attribution(
    page_faults: &[&PageFaultEvent],
    correlation: &FaultCorrelation,
) -> DirectFaultAttribution {
    use crate::event::CursorOp;

    // Count faults by operation type
    // Key: op_type (3=CURSOR_GET, 4=CURSOR_PUT, etc.), Value: (total, major)
    let mut faults_by_op_type: HashMap<u32, (u64, u64)> = HashMap::new();

    // Count faults by cursor operation (only for CURSOR_GET, op_type=3)
    // Key: cursor_op, Value: (total, major)
    let mut faults_by_cursor_op: HashMap<u32, (u64, u64)> = HashMap::new();

    let mut directly_attributed = 0u64;

    for fault in page_faults {
        if fault.has_active_op() {
            directly_attributed += 1;
            let is_major = fault.is_major_fault();

            // Count by operation type
            let entry = faults_by_op_type
                .entry(fault.active_op_type)
                .or_insert((0, 0));
            entry.0 += 1;
            if is_major {
                entry.1 += 1;
            }

            // For CURSOR_GET operations, also count by cursor op
            if fault.active_op_type == 3 {
                // EVENT_CURSOR_GET
                let cursor_entry = faults_by_cursor_op
                    .entry(fault.active_cursor_op)
                    .or_insert((0, 0));
                cursor_entry.0 += 1;
                if is_major {
                    cursor_entry.1 += 1;
                }
            }
        }
    }

    // If no direct attribution data, return empty
    if directly_attributed == 0 {
        return DirectFaultAttribution::default();
    }

    // Convert to sorted vectors
    let _total_faults = page_faults.len() as u64;

    let op_type_name = |op: u32| -> &'static str {
        match op {
            3 => "CURSOR_GET",
            4 => "CURSOR_PUT",
            5 => "DIRECT_GET",
            6 => "CURSOR_DEL",
            10 => "DIRECT_PUT",
            11 => "DIRECT_DEL",
            _ => "UNKNOWN",
        }
    };

    let mut faults_by_op_type_vec: Vec<FaultsByOpType> = faults_by_op_type
        .into_iter()
        .map(|(op_type, (total, major))| FaultsByOpType {
            op_type: op_type_name(op_type).to_string(),
            total_faults: total,
            major_faults: major,
            percentage: total as f64 / directly_attributed as f64 * 100.0,
        })
        .collect();
    faults_by_op_type_vec.sort_by(|a, b| b.total_faults.cmp(&a.total_faults));

    let mut faults_by_cursor_op_vec: Vec<FaultsByCursorOp> = faults_by_cursor_op
        .into_iter()
        .map(|(cursor_op, (total, major))| {
            let op = CursorOp::from_raw(cursor_op);
            FaultsByCursorOp {
                cursor_op: op.name().to_string(),
                total_faults: total,
                major_faults: major,
                percentage: total as f64 / directly_attributed as f64 * 100.0,
            }
        })
        .collect();
    faults_by_cursor_op_vec.sort_by(|a, b| b.total_faults.cmp(&a.total_faults));

    DirectFaultAttribution {
        has_data: true,
        directly_attributed_count: correlation.directly_attributed,
        timestamp_fallback_count: correlation.timestamp_correlated,
        uncorrelated_count: correlation.uncorrelated_faults,
        faults_by_op_type: faults_by_op_type_vec,
        faults_by_cursor_op: faults_by_cursor_op_vec,
    }
}

/// Generate unified table stats combining fault data with slow ops data.
/// This creates a single view per table with all relevant metrics for the Tables tab.
fn generate_unified_table_stats(
    page_faults: &[&PageFaultEvent],
    correlation: &FaultCorrelation,
    cursor_data: &CursorData,
) -> Vec<UnifiedTableStats> {
    use crate::event::CursorOp;

    // Build a map of table name -> slow ops data
    let slow_ops_by_name: HashMap<String, &SlowOpsTableStats> = cursor_data
        .slow_ops_by_table
        .iter()
        .map(|s| (s.table.clone(), s))
        .collect();

    // Build a map of table name -> cursor table stats (for total ops)
    let cursor_stats_by_name: HashMap<String, &CursorTableStats> = cursor_data
        .table_stats
        .iter()
        .map(|s| (s.name.clone(), s))
        .collect();

    // Build a map of table name -> hot keys
    let mut hot_keys_by_table: HashMap<String, Vec<&SlowKeyStats>> = HashMap::new();
    for key in &cursor_data.slow_keys {
        hot_keys_by_table
            .entry(key.table.clone())
            .or_default()
            .push(key);
    }

    // Build per-table fault breakdown by operation type and cursor op
    // Key: (table_name, op_type) -> (faults, major_faults)
    let mut faults_by_table_op: HashMap<(String, u32), (u64, u64)> = HashMap::new();
    // Key: (table_name, cursor_op) -> (faults, major_faults)
    let mut faults_by_table_cursor_op: HashMap<(String, u32), (u64, u64)> = HashMap::new();

    for fault in page_faults {
        if fault.has_active_op() {
            let table_name = if fault.active_dbi < 100 {
                dbi_to_table_name(fault.active_dbi).to_string()
            } else {
                continue; // Skip unknown tables for unified view
            };
            let is_major = fault.is_major_fault();

            // Count by (table, op_type)
            let entry = faults_by_table_op
                .entry((table_name.clone(), fault.active_op_type))
                .or_insert((0, 0));
            entry.0 += 1;
            if is_major {
                entry.1 += 1;
            }

            // For CURSOR_GET, also count by cursor op
            if fault.active_op_type == 3 {
                let cursor_entry = faults_by_table_cursor_op
                    .entry((table_name, fault.active_cursor_op))
                    .or_insert((0, 0));
                cursor_entry.0 += 1;
                if is_major {
                    cursor_entry.1 += 1;
                }
            }
        }
    }

    let total_faults = page_faults.len() as u64;

    // Build unified stats from correlated_faults
    let mut unified: Vec<UnifiedTableStats> = correlation
        .correlated_faults
        .iter()
        .filter_map(|(table_name, (faults, major_faults))| {
            // Skip unknown/pre-trace tables
            if table_name.starts_with("Unknown") {
                return None;
            }

            // Get DBI for this table
            let dbi = table_name_to_dbi(table_name);

            // Get slow ops data
            let slow_data = slow_ops_by_name.get(table_name);
            let cursor_stats = cursor_stats_by_name.get(table_name);

            // Build faults by operation type for this table
            let op_type_names = [
                (3, "CURSOR_GET"),
                (4, "CURSOR_PUT"),
                (5, "DIRECT_GET"),
                (6, "CURSOR_DEL"),
                (10, "DIRECT_PUT"),
                (11, "DIRECT_DEL"),
            ];
            let mut faults_by_op: Vec<OpFaultCount> = op_type_names
                .iter()
                .filter_map(|(op_type, name)| {
                    faults_by_table_op
                        .get(&(table_name.clone(), *op_type))
                        .map(|(f, m)| OpFaultCount {
                            operation: name.to_string(),
                            faults: *f,
                            major_faults: *m,
                        })
                })
                .filter(|o| o.faults > 0)
                .collect();
            faults_by_op.sort_by(|a, b| b.faults.cmp(&a.faults));

            // Build faults by cursor operation for this table
            let mut faults_by_cursor_op: Vec<OpFaultCount> = faults_by_table_cursor_op
                .iter()
                .filter(|((t, _), _)| t == table_name)
                .map(|((_, cursor_op), (f, m))| {
                    let op = CursorOp::from_raw(*cursor_op);
                    OpFaultCount {
                        operation: op.name().to_string(),
                        faults: *f,
                        major_faults: *m,
                    }
                })
                .filter(|o| o.faults > 0)
                .collect();
            faults_by_cursor_op.sort_by(|a, b| b.faults.cmp(&a.faults));

            // Get slow ops breakdown
            let slow_ops_breakdown: Vec<SlowOpBreakdown> = slow_data
                .map(|s| s.by_operation.clone())
                .unwrap_or_default();

            // Get hot keys for this table
            let hot_keys: Vec<TableHotKey> = hot_keys_by_table
                .get(table_name)
                .map(|keys| {
                    keys.iter()
                        .take(5)
                        .map(|k| TableHotKey {
                            key_hex: k.key_hex.clone(),
                            slow_count: k.slow_access_count,
                            total_count: k.total_access_count,
                            avg_latency_us: k.avg_latency_us,
                            max_latency_us: k.max_latency_us,
                        })
                        .collect()
                })
                .unwrap_or_default();

            // Determine top operation (most faults or most slow)
            let top_operation = faults_by_op
                .first()
                .map(|o| o.operation.clone())
                .or_else(|| slow_ops_breakdown.first().map(|o| o.operation.clone()))
                .unwrap_or_default();

            Some(UnifiedTableStats {
                name: table_name.clone(),
                dbi,
                faults: *faults,
                major_faults: *major_faults,
                fault_percentage: if total_faults > 0 {
                    *faults as f64 / total_faults as f64 * 100.0
                } else {
                    0.0
                },
                total_ops: cursor_stats.map(|s| s.ops).unwrap_or(0),
                slow_ops: slow_data.map(|s| s.slow_op_count).unwrap_or(0),
                slow_ops_percentage: slow_data.map(|s| s.slow_op_percentage).unwrap_or(0.0),
                time_lost_ms: slow_data.map(|s| s.total_slow_time_ms).unwrap_or(0.0),
                avg_latency_us: slow_data.map(|s| s.avg_slow_latency_us).unwrap_or(0.0),
                max_latency_us: slow_data.map(|s| s.max_latency_us).unwrap_or(0.0),
                top_operation,
                details: TableDrillDown {
                    faults_by_op,
                    faults_by_cursor_op,
                    slow_ops_breakdown,
                    hot_keys,
                },
            })
        })
        .collect();

    // Sort by time lost (most impactful first), then by faults
    unified.sort_by(|a, b| {
        b.time_lost_ms
            .partial_cmp(&a.time_lost_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.faults.cmp(&a.faults))
    });

    unified
}

/// Convert table name back to DBI
fn table_name_to_dbi(name: &str) -> u32 {
    match name {
        "CanonicalHeaders" => 2,
        "HeaderTerminalDifficulties" => 3,
        "HeaderNumbers" => 4,
        "Headers" => 5,
        "BlockBodyIndices" => 6,
        "BlockOmmers" => 7,
        "BlockWithdrawals" => 8,
        "Transactions" => 9,
        "TransactionHashNumbers" => 10,
        "TransactionBlocks" => 11,
        "Receipts" => 12,
        "Bytecodes" => 13,
        "PlainAccountState" => 14,
        "PlainStorageState" => 15,
        "AccountChangeSets" => 16,
        "StorageChangeSets" => 17,
        "AccountsHistory" => 18,
        "StoragesHistory" => 19,
        "HashedAccounts" => 20,
        "HashedStorages" => 21,
        "AccountsTrie" => 22,
        "StoragesTrie" => 23,
        "TransactionSenders" => 24,
        "StageCheckpoints" => 25,
        "StageCheckpointProgresses" => 26,
        "PruneCheckpoints" => 27,
        "AccountsTrieChangeSets" => 28,
        "StoragesTrieChangeSets" => 29,
        _ => 0,
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

    let total_latency: u64 = latencies.iter().sum();
    let avg_latency_us = (total_latency as f64 / total as f64) / 1000.0;
    let (p50_latency_us, p95_latency_us, p99_latency_us) = compute_percentiles_us(&latencies);

    // Count by operation type
    let mut op_counts: HashMap<String, (u64, u64)> = HashMap::new(); // (count, total_latency)
    let mut seek_count = 0u64;
    let mut nav_count = 0u64;
    let mut error_count = 0u64;
    let mut pre_trace_cursor_ops = 0u64;
    let mut direct_get_count = 0u64;

    // Count by DBI/table - collect latencies for per-table percentiles
    // Key: dbi, Value: (ops, seeks, navs, latencies_vec, direct_gets)
    let mut dbi_stats: HashMap<u32, (u64, u64, u64, Vec<u64>, u64)> = HashMap::new();

    for event in events {
        // Track pre-trace cursor operations (only for cursor ops, not direct ops)
        if !event.is_direct_op() && is_pre_trace_cursor(event.dbi) {
            pre_trace_cursor_ops += 1;
        }

        // Handle direct operations specially
        if event.is_direct_op() {
            let op_name = if event.is_direct_get() {
                direct_get_count += 1;
                seek_count += 1; // Direct gets count as seeks
                "DIRECT_GET"
            } else if event.is_direct_put() {
                "DIRECT_PUT"
            } else {
                "DIRECT_DEL"
            };
            let entry = op_counts.entry(op_name.to_string()).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += event.latency_ns;
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

        // DBI stats - collect latencies for per-table percentiles
        let dbi_entry = dbi_stats
            .entry(event.dbi)
            .or_insert((0, 0, 0, Vec::new(), 0));
        dbi_entry.0 += 1;
        dbi_entry.3.push(event.latency_ns);
        if event.is_direct_op() {
            if event.is_direct_get() {
                dbi_entry.4 += 1; // direct_gets
                dbi_entry.1 += 1; // count as seek too
            }
            // Direct puts/dels don't affect seek/nav counts
        } else {
            let op = event.cursor_op();
            if op.is_seek() {
                dbi_entry.1 += 1;
            }
            if op.is_navigation() {
                dbi_entry.2 += 1;
            }
        }
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
    // Tuple: (ops, seeks, navs, latencies, direct_gets)
    let mut pre_trace_latencies: Vec<u64> = Vec::new();
    let mut pre_trace_total: (u64, u64, u64, u64) = (0, 0, 0, 0); // (ops, seeks, navs, direct_gets)
    let mut known_dbi_stats: HashMap<u32, (u64, u64, u64, Vec<u64>, u64)> = HashMap::new();

    for (dbi, stats) in dbi_stats {
        if is_pre_trace_cursor(dbi) {
            pre_trace_total.0 += stats.0;
            pre_trace_total.1 += stats.1;
            pre_trace_total.2 += stats.2;
            pre_trace_latencies.extend(stats.3);
            pre_trace_total.3 += stats.4;
        } else {
            known_dbi_stats.insert(dbi, stats);
        }
    }

    let mut table_stats: Vec<CursorTableStats> = known_dbi_stats
        .into_iter()
        .map(|(dbi, (ops, seeks, navs, mut latencies, _direct_gets))| {
            latencies.sort();
            let total_lat: u64 = latencies.iter().sum();
            let (p50, p95, p99) = compute_percentiles_us(&latencies);
            CursorTableStats {
                dbi,
                name: dbi_to_table_name(dbi).to_string(),
                ops,
                percentage: ops as f64 / total as f64 * 100.0,
                seeks,
                navs,
                avg_latency_us: (total_lat as f64 / ops as f64) / 1000.0,
                p50_latency_us: p50,
                p95_latency_us: p95,
                p99_latency_us: p99,
            }
        })
        .collect();

    // Add pre-trace cursors as a single group if any exist
    if pre_trace_total.0 > 0 {
        pre_trace_latencies.sort();
        let pre_trace_total_lat: u64 = pre_trace_latencies.iter().sum();
        let (p50, p95, p99) = compute_percentiles_us(&pre_trace_latencies);
        table_stats.push(CursorTableStats {
            dbi: 0xFFFFFFFF,
            name: "Unknown (pre-trace cursors)".to_string(),
            ops: pre_trace_total.0,
            percentage: pre_trace_total.0 as f64 / total as f64 * 100.0,
            seeks: pre_trace_total.1,
            navs: pre_trace_total.2,
            avg_latency_us: (pre_trace_total_lat as f64 / pre_trace_total.0 as f64) / 1000.0,
            p50_latency_us: p50,
            p95_latency_us: p95,
            p99_latency_us: p99,
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
            } else if e.is_direct_put() {
                "DIRECT_PUT".to_string()
            } else if e.is_direct_del() {
                "DIRECT_DEL".to_string()
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
        } else if event.is_direct_put() {
            "DIRECT_PUT".to_string()
        } else if event.is_direct_del() {
            "DIRECT_DEL".to_string()
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
        } else if event.is_direct_put() {
            "DIRECT_PUT".to_string()
        } else if event.is_direct_del() {
            "DIRECT_DEL".to_string()
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
            p95_latency_us,
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

/// Generate transaction lifecycle data for visualization
fn generate_txn_data(events: &[TxnEvent]) -> TxnData {
    if events.is_empty() {
        return TxnData::default();
    }

    let total = events.len() as u64;

    // Timing
    let min_ts = events.iter().map(|e| e.timestamp_ns).min().unwrap();
    let max_ts = events.iter().map(|e| e.timestamp_ns).max().unwrap();
    let duration_ns = max_ts - min_ts;
    let duration_secs = duration_ns as f64 / 1e9;

    // Count events and collect commit latencies
    let mut begin_count = 0u64;
    let mut commit_count = 0u64;
    let mut abort_count = 0u64;
    let mut ro_count = 0u64;
    let mut rw_count = 0u64;
    let mut commit_latencies: Vec<u64> = Vec::new(); // All commits (for stats)
    let mut rw_commit_timeline: Vec<RwCommitPoint> = Vec::new(); // RW commits with timestamps

    // Track active transactions: (txn_ptr, tid) -> (start_time, is_ro)
    // Key by both ptr and tid since MDBX reuses transaction pointers
    let mut active_txns: HashMap<(u64, u32), (u64, bool)> = HashMap::new();

    // Build timeline entries
    let mut timeline_entries: Vec<TxnTimelineEntry> = Vec::new();

    // Track per-thread stats: tid -> (total, ro, rw, commits, aborts, total_commit_latency)
    let mut thread_stats_map: HashMap<u32, (u64, u64, u64, u64, u64, u64)> = HashMap::new();

    // Track concurrency over time
    let mut concurrency_events: Vec<(u64, i32, bool)> = Vec::new(); // (timestamp, delta, is_ro)

    for event in events {
        let thread_entry = thread_stats_map
            .entry(event.tid)
            .or_insert((0, 0, 0, 0, 0, 0));

        match event.event_type {
            7 => {
                // TXN_BEGIN
                begin_count += 1;
                let is_ro = event.is_read_only();
                if is_ro {
                    ro_count += 1;
                    thread_entry.1 += 1;
                } else {
                    rw_count += 1;
                    thread_entry.2 += 1;
                }
                thread_entry.0 += 1;

                // Track active transaction by (ptr, tid) pair
                active_txns.insert((event.txn_ptr, event.tid), (event.timestamp_ns, is_ro));

                // Track concurrency
                concurrency_events.push((event.timestamp_ns, 1, is_ro));
            }
            8 => {
                // TXN_COMMIT
                commit_count += 1;
                thread_entry.3 += 1;
                commit_latencies.push(event.latency_ns);
                thread_entry.5 += event.latency_ns;

                // Create timeline entry if we have the begin (keyed by ptr + tid)
                if let Some((start_ts, is_ro)) = active_txns.remove(&(event.txn_ptr, event.tid)) {
                    let start_ms = (start_ts - min_ts) as f64 / 1_000_000.0;
                    let end_ms = (event.timestamp_ns - min_ts) as f64 / 1_000_000.0;

                    // Collect RW commit latencies (RO commits are just cleanup, ~microseconds)
                    if !is_ro {
                        rw_commit_timeline.push(RwCommitPoint {
                            time_secs: (event.timestamp_ns - min_ts) as f64 / 1e9,
                            latency_ms: event.latency_ns as f64 / 1_000_000.0,
                        });
                    }

                    timeline_entries.push(TxnTimelineEntry {
                        tid: event.tid,
                        txn_ptr: format!("0x{:x}", event.txn_ptr),
                        start_ms,
                        end_ms: Some(end_ms),
                        duration_ms: Some(end_ms - start_ms),
                        txn_type: if is_ro {
                            "RO".to_string()
                        } else {
                            "RW".to_string()
                        },
                        end_type: "commit".to_string(),
                        commit_latency_us: Some(event.latency_ns as f64 / 1000.0),
                    });

                    // Track concurrency
                    concurrency_events.push((event.timestamp_ns, -1, is_ro));
                }
            }
            9 => {
                // TXN_ABORT
                abort_count += 1;
                thread_entry.4 += 1;

                // Create timeline entry if we have the begin (keyed by ptr + tid)
                if let Some((start_ts, is_ro)) = active_txns.remove(&(event.txn_ptr, event.tid)) {
                    let start_ms = (start_ts - min_ts) as f64 / 1_000_000.0;
                    let end_ms = (event.timestamp_ns - min_ts) as f64 / 1_000_000.0;
                    timeline_entries.push(TxnTimelineEntry {
                        tid: event.tid,
                        txn_ptr: format!("0x{:x}", event.txn_ptr),
                        start_ms,
                        end_ms: Some(end_ms),
                        duration_ms: Some(end_ms - start_ms),
                        txn_type: if is_ro {
                            "RO".to_string()
                        } else {
                            "RW".to_string()
                        },
                        end_type: "abort".to_string(),
                        commit_latency_us: None,
                    });

                    // Track concurrency
                    concurrency_events.push((event.timestamp_ns, -1, is_ro));
                }
            }
            _ => {}
        }
    }

    // Add entries for transactions still open at end of trace
    for ((txn_ptr, tid), (start_ts, is_ro)) in active_txns {
        let start_ms = (start_ts - min_ts) as f64 / 1_000_000.0;
        timeline_entries.push(TxnTimelineEntry {
            tid,
            txn_ptr: format!("0x{:x}", txn_ptr),
            start_ms,
            end_ms: None,
            duration_ms: None,
            txn_type: if is_ro {
                "RO".to_string()
            } else {
                "RW".to_string()
            },
            end_type: "open".to_string(),
            commit_latency_us: None,
        });
    }

    // Sort timeline by start time
    timeline_entries.sort_by(|a, b| a.start_ms.partial_cmp(&b.start_ms).unwrap());

    // Limit timeline entries for performance, but always keep ALL RW transactions
    if timeline_entries.len() > 1000 {
        // Separate RW and RO transactions - RW are rare and important
        let (rw_entries, ro_entries): (Vec<_>, Vec<_>) = timeline_entries
            .into_iter()
            .partition(|e| e.txn_type == "RW");

        // Keep all RW transactions, sample RO transactions
        let ro_budget = 1000usize.saturating_sub(rw_entries.len());
        let sampled_ro: Vec<_> = if ro_entries.len() > ro_budget && ro_budget > 0 {
            let step = ro_entries.len() / ro_budget;
            ro_entries
                .into_iter()
                .step_by(step)
                .take(ro_budget)
                .collect()
        } else {
            ro_entries
        };

        // Merge and re-sort by start time
        timeline_entries = rw_entries;
        timeline_entries.extend(sampled_ro);
        timeline_entries.sort_by(|a, b| a.start_ms.partial_cmp(&b.start_ms).unwrap());
    }

    // Calculate commit latency stats
    commit_latencies.sort();
    let avg_commit_latency_us = if !commit_latencies.is_empty() {
        (commit_latencies.iter().sum::<u64>() as f64 / commit_latencies.len() as f64) / 1000.0
    } else {
        0.0
    };
    let (p50_commit_latency_us, p95_commit_latency_us, p99_commit_latency_us) =
        compute_percentiles_us(&commit_latencies);
    let max_commit_latency_us = commit_latencies.last().copied().unwrap_or(0) as f64 / 1000.0;

    // Build thread stats
    let mut thread_stats: Vec<TxnThreadStats> = thread_stats_map
        .into_iter()
        .map(
            |(tid, (total_txns, ro_txns, rw_txns, commits, aborts, total_lat))| TxnThreadStats {
                tid,
                total_txns,
                ro_txns,
                rw_txns,
                commits,
                aborts,
                avg_commit_latency_us: if commits > 0 {
                    (total_lat as f64 / commits as f64) / 1000.0
                } else {
                    0.0
                },
                percentage: total_txns as f64 / begin_count as f64 * 100.0,
            },
        )
        .collect();
    thread_stats.sort_by(|a, b| b.total_txns.cmp(&a.total_txns));
    thread_stats.truncate(20);

    // Calculate concurrency stats
    concurrency_events.sort_by_key(|(ts, _, _)| *ts);

    let mut current_ro = 0i32;
    let mut current_rw = 0i32;
    let mut max_concurrent_ro = 0u32;
    let mut max_concurrent_rw = 0u32;
    let mut max_concurrent_total = 0u32;
    let mut concurrency_samples: Vec<(u64, u32, u32)> = Vec::new();
    let mut total_ro_time = 0u64;
    let mut last_ts = min_ts;

    for (ts, delta, is_ro) in &concurrency_events {
        // Accumulate time at current level
        if current_ro > 0 {
            total_ro_time += (ts - last_ts) * current_ro as u64;
        }
        last_ts = *ts;

        if *is_ro {
            current_ro += delta;
        } else {
            current_rw += delta;
        }

        max_concurrent_ro = max_concurrent_ro.max(current_ro.max(0) as u32);
        max_concurrent_rw = max_concurrent_rw.max(current_rw.max(0) as u32);
        max_concurrent_total = max_concurrent_total.max((current_ro + current_rw).max(0) as u32);

        concurrency_samples.push((*ts, current_ro.max(0) as u32, current_rw.max(0) as u32));
    }

    let avg_concurrent_ro = if duration_ns > 0 {
        total_ro_time as f64 / duration_ns as f64
    } else {
        0.0
    };

    // Sample concurrency timeline for visualization (bucket by 100ms)
    let bucket_ns = 100_000_000u64; // 100ms
    let num_buckets = ((duration_ns / bucket_ns) + 1) as usize;
    let mut concurrency_timeline: Vec<ConcurrencyPoint> = Vec::with_capacity(num_buckets.min(1000));

    if !concurrency_samples.is_empty() {
        let mut bucket_idx = 0usize;
        let mut sample_idx = 0usize;
        let mut last_ro = 0u32;
        let mut last_rw = 0u32;

        while bucket_idx < num_buckets.min(1000) {
            let bucket_start = min_ts + (bucket_idx as u64 * bucket_ns);

            // Find the last sample before or at this bucket
            while sample_idx < concurrency_samples.len()
                && concurrency_samples[sample_idx].0 <= bucket_start
            {
                last_ro = concurrency_samples[sample_idx].1;
                last_rw = concurrency_samples[sample_idx].2;
                sample_idx += 1;
            }

            concurrency_timeline.push(ConcurrencyPoint {
                time_ms: (bucket_idx as u64 * bucket_ns) / 1_000_000,
                concurrent_ro: last_ro,
                concurrent_rw: last_rw,
            });

            bucket_idx += 1;
        }
    }

    TxnData {
        has_data: true,
        summary: TxnSummary {
            total_events: total,
            begin_count,
            commit_count,
            abort_count,
            ro_count,
            rw_count,
            duration_secs,
            txn_rate_per_sec: if duration_secs > 0.0 {
                begin_count as f64 / duration_secs
            } else {
                0.0
            },
            avg_commit_latency_us,
            p50_commit_latency_us,
            p95_commit_latency_us,
            p99_commit_latency_us,
            max_commit_latency_us,
        },
        timeline: timeline_entries,
        thread_stats,
        concurrency: TxnConcurrencyStats {
            max_concurrent_ro,
            max_concurrent_rw,
            max_concurrent_total,
            avg_concurrent_ro,
            concurrency_timeline,
        },
        rw_commit_timeline,
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
/// This uses two attribution methods:
/// 1. Direct BPF attribution (100% accurate) - faults that have active_dbi set
/// 2. Timestamp + thread matching (fallback) - for older traces without direct attribution
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

    // Generate informative message about correlation method used
    let correlation_rate = correlated_total as f64 / total_faults as f64 * 100.0;

    let warning = if correlation.directly_attributed > 0 {
        // New BPF-based direct attribution
        let direct_pct = correlation.directly_attributed as f64 / total_faults as f64 * 100.0;
        if correlation.timestamp_correlated == 0 {
            // All faults have direct attribution - best case
            Some(format!(
                "Direct BPF attribution: {:.1}% of page faults ({} of {}) attributed with 100% accuracy. \
                 {} faults occurred outside MDBX operations (background I/O, prefetch, or kernel activity).",
                direct_pct,
                correlation.directly_attributed,
                total_faults,
                correlation.uncorrelated_faults
            ))
        } else {
            // Mix of direct and timestamp correlation
            Some(format!(
                "Correlated {:.1}% of page faults ({} of {}): {} via direct BPF attribution (100% accurate), \
                 {} via timestamp matching. {} faults occurred outside cursor windows.",
                correlation_rate,
                correlated_total,
                total_faults,
                correlation.directly_attributed,
                correlation.timestamp_correlated,
                correlation.uncorrelated_faults
            ))
        }
    } else {
        // Fallback to timestamp matching only (older traces)
        Some(format!(
            "Timestamp correlation: {:.1}% of page faults ({} of {}) correlated with cursor operations. \
             {} faults occurred outside cursor windows (background I/O, prefetch, or kernel activity). \
             Note: Upgrade your trace for 100% accurate direct BPF attribution.",
            correlation_rate, correlated_total, total_faults, correlation.uncorrelated_faults
        ))
    };

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
            burst_stats: BurstStats {
                median_events: 0,
                p95_events: 0,
                max_events: 0,
                bucket_ms: 100,
            },
            top_strides: vec![],
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

    // Get top strides for summary display
    let mut strides: Vec<_> = stride_counts.into_iter().collect();
    strides.sort_by(|a, b| b.1.cmp(&a.1));

    let top_strides: Vec<_> = strides
        .into_iter()
        .take(5) // Only top 5 for summary
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
                percentage: count as f64 / total as f64 * 100.0,
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
        burst_stats,
        top_strides,
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

// ============================================================================
// Compact Export Format (for LLM analysis)
// ============================================================================

/// Compact export format optimized for LLM analysis.
/// Contains key metrics and insights without raw data arrays.
#[derive(Debug, Serialize)]
pub struct CompactExport {
    /// Overall trace summary
    pub trace: TraceSummaryCompact,
    /// Page fault analysis
    pub page_faults: PageFaultAnalysis,
    /// Per-table breakdown (sorted by impact)
    pub tables: Vec<TableAnalysis>,
    /// Cursor operation analysis
    pub cursor_ops: Option<CursorAnalysis>,
    /// Transaction analysis
    pub transactions: Option<TxnAnalysis>,
    /// Top slow operations (potential optimization targets)
    pub slow_operations: Vec<SlowOpSummary>,
    /// Key insights and recommendations
    pub insights: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TraceSummaryCompact {
    pub duration_secs: f64,
    pub total_events: u64,
    pub file_size_gb: f64,
    pub file_offset_range_gb: (f64, f64),
    /// Block range extracted from cursor operation keys (if available)
    pub block_range: Option<BlockRange>,
}

#[derive(Debug, Serialize)]
pub struct PageFaultAnalysis {
    pub total: u64,
    pub major: u64,
    pub minor: u64,
    pub major_ratio: f64,
    pub rate_per_sec: f64,
    pub unique_pages: u64,
    /// Sequential vs random access ratio
    pub sequential_ratio: f64,
    pub random_ratio: f64,
    /// Burst characteristics
    pub burst_median: u32,
    pub burst_p95: u32,
    pub burst_max: u32,
}

#[derive(Debug, Serialize)]
pub struct TableAnalysis {
    pub name: String,
    pub category: String,
    pub faults: u64,
    pub major_faults: u64,
    pub fault_percentage: f64,
    pub cursor_ops: u64,
    /// Faults per cursor op (higher = more cache misses)
    pub faults_per_op: Option<f64>,
    /// Whether fault attribution is direct correlation or estimated
    pub correlation_method: String,
}

#[derive(Debug, Serialize)]
pub struct CursorAnalysis {
    pub total_ops: u64,
    pub rate_per_sec: f64,
    pub seek_count: u64,
    pub seek_ratio: f64,
    pub nav_count: u64,
    pub direct_get_count: u64,
    pub error_count: u64,
    pub latency_avg_us: f64,
    pub latency_p50_us: f64,
    pub latency_p95_us: f64,
    pub latency_p99_us: f64,
    /// Ops by type (sorted by count)
    pub by_operation: Vec<OpBreakdown>,
    /// Top tables by ops
    pub top_tables: Vec<TableOpBreakdown>,
}

#[derive(Debug, Serialize)]
pub struct OpBreakdown {
    pub name: String,
    pub count: u64,
    pub percentage: f64,
    pub avg_latency_us: f64,
}

#[derive(Debug, Serialize)]
pub struct TableOpBreakdown {
    pub name: String,
    pub ops: u64,
    pub percentage: f64,
    pub avg_latency_us: f64,
    pub seeks: u64,
    pub navs: u64,
}

#[derive(Debug, Serialize)]
pub struct TxnAnalysis {
    pub total_txns: u64,
    pub rate_per_sec: f64,
    pub ro_count: u64,
    pub rw_count: u64,
    pub ro_ratio: f64,
    pub commit_count: u64,
    pub abort_count: u64,
    pub commit_latency_avg_us: f64,
    pub commit_latency_p50_us: f64,
    pub commit_latency_p95_us: f64,
    pub commit_latency_p99_us: f64,
    pub commit_latency_max_us: f64,
    /// Thread breakdown (top threads by activity)
    pub top_threads: Vec<ThreadTxnBreakdown>,
}

#[derive(Debug, Serialize)]
pub struct ThreadTxnBreakdown {
    pub tid: u32,
    pub total: u64,
    pub ro: u64,
    pub rw: u64,
    pub commits: u64,
    pub aborts: u64,
    pub avg_commit_latency_us: f64,
}

#[derive(Debug, Serialize)]
pub struct SlowOpSummary {
    pub table: String,
    pub operation: String,
    pub count: u64,
    pub avg_latency_us: f64,
    pub max_latency_us: f64,
}

/// Generate compact export from viewer data
pub fn generate_compact_export(data: &ViewerData) -> CompactExport {
    let mut insights = Vec::new();

    // Generate insights based on data
    if data.summary.major_fault_ratio > 0.1 {
        insights.push(format!(
            "High major fault ratio ({:.1}%) indicates significant disk I/O. Consider increasing system RAM or optimizing access patterns.",
            data.summary.major_fault_ratio * 100.0
        ));
    }

    if data.patterns.random_ratio > 0.7 {
        insights.push(format!(
            "High random access ratio ({:.1}%) suggests poor locality. Tables may benefit from different indexing or access order.",
            data.patterns.random_ratio * 100.0
        ));
    }

    if let Some(ref cursor) = data.cursor_data.has_data.then_some(&data.cursor_data) {
        if cursor.summary.seek_ratio > 0.8 {
            insights.push(format!(
                "Seek-heavy workload ({:.1}% seeks). Each seek traverses B+ tree. Consider batching or caching.",
                cursor.summary.seek_ratio * 100.0
            ));
        }
        if cursor.summary.error_count > 0 {
            let error_rate = cursor.summary.error_count as f64 / cursor.summary.total_ops as f64;
            if error_rate > 0.01 {
                insights.push(format!(
                    "Notable error rate ({:.2}%). Most are MDBX_NOTFOUND which may be normal for existence checks.",
                    error_rate * 100.0
                ));
            }
        }
    }

    if data.txn_data.has_data {
        let rw_ratio =
            data.txn_data.summary.rw_count as f64 / data.txn_data.summary.begin_count.max(1) as f64;
        if rw_ratio > 0.1 {
            insights.push(format!(
                "Higher than typical RW transaction ratio ({:.1}%). Reth usually has <1% RW transactions.",
                rw_ratio * 100.0
            ));
        }
        if data.txn_data.summary.avg_commit_latency_us > 200_000.0 {
            insights.push(format!(
                "High average commit latency ({:.1}ms). May indicate I/O bottleneck or large write batches.",
                data.txn_data.summary.avg_commit_latency_us / 1000.0
            ));
        }
    }

    // Find tables with high faults-per-op ratio
    let mut high_fault_tables: Vec<_> = data
        .tables
        .iter()
        .filter(|t| t.has_cursor_ops && t.cursor_ops > 100)
        .filter_map(|t| {
            let faults_per_op = t.faults as f64 / t.cursor_ops as f64;
            if faults_per_op > 0.5 {
                Some((t.name.clone(), faults_per_op))
            } else {
                None
            }
        })
        .collect();
    high_fault_tables.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    for (table, ratio) in high_fault_tables.iter().take(3) {
        insights.push(format!(
            "Table '{}' has high fault-per-op ratio ({:.2}). Consider prefetching or access pattern optimization.",
            table, ratio
        ));
    }

    // Build table analysis
    let tables: Vec<TableAnalysis> = data
        .tables
        .iter()
        .filter(|t| t.faults > 0 || t.cursor_ops > 0)
        .map(|t| {
            let faults_per_op = if t.cursor_ops > 0 {
                Some(t.faults as f64 / t.cursor_ops as f64)
            } else {
                None
            };
            TableAnalysis {
                name: t.name.clone(),
                category: t.category.clone(),
                faults: t.faults,
                major_faults: t.major_faults,
                fault_percentage: t.percentage,
                cursor_ops: t.cursor_ops,
                faults_per_op,
                correlation_method: if t.faults_correlated {
                    "direct".to_string()
                } else {
                    "estimated".to_string()
                },
            }
        })
        .collect();

    // Build cursor analysis
    let cursor_ops = if data.cursor_data.has_data {
        let by_operation: Vec<OpBreakdown> = data
            .cursor_data
            .operations
            .iter()
            .filter(|o| o.count > 0)
            .map(|o| OpBreakdown {
                name: o.name.clone(),
                count: o.count,
                percentage: o.percentage,
                avg_latency_us: o.avg_latency_us,
            })
            .collect();

        let top_tables: Vec<TableOpBreakdown> = data
            .cursor_data
            .table_stats
            .iter()
            .filter(|t| t.ops > 0)
            .take(15)
            .map(|t| TableOpBreakdown {
                name: t.name.clone(),
                ops: t.ops,
                percentage: t.percentage,
                avg_latency_us: t.avg_latency_us,
                seeks: t.seeks,
                navs: t.navs,
            })
            .collect();

        Some(CursorAnalysis {
            total_ops: data.cursor_data.summary.total_ops,
            rate_per_sec: data.cursor_data.summary.op_rate_per_sec,
            seek_count: data.cursor_data.summary.seek_count,
            seek_ratio: data.cursor_data.summary.seek_ratio,
            nav_count: data.cursor_data.summary.nav_count,
            direct_get_count: data.cursor_data.summary.direct_get_count,
            error_count: data.cursor_data.summary.error_count,
            latency_avg_us: data.cursor_data.summary.avg_latency_us,
            latency_p50_us: data.cursor_data.summary.p50_latency_us,
            latency_p95_us: data.cursor_data.summary.p95_latency_us,
            latency_p99_us: data.cursor_data.summary.p99_latency_us,
            by_operation,
            top_tables,
        })
    } else {
        None
    };

    // Build transaction analysis
    let transactions = if data.txn_data.has_data {
        let top_threads: Vec<ThreadTxnBreakdown> = data
            .txn_data
            .thread_stats
            .iter()
            .take(10)
            .map(|t| ThreadTxnBreakdown {
                tid: t.tid,
                total: t.total_txns,
                ro: t.ro_txns,
                rw: t.rw_txns,
                commits: t.commits,
                aborts: t.aborts,
                avg_commit_latency_us: t.avg_commit_latency_us,
            })
            .collect();

        Some(TxnAnalysis {
            total_txns: data.txn_data.summary.begin_count,
            rate_per_sec: data.txn_data.summary.txn_rate_per_sec,
            ro_count: data.txn_data.summary.ro_count,
            rw_count: data.txn_data.summary.rw_count,
            ro_ratio: data.txn_data.summary.ro_count as f64
                / data.txn_data.summary.begin_count.max(1) as f64,
            commit_count: data.txn_data.summary.commit_count,
            abort_count: data.txn_data.summary.abort_count,
            commit_latency_avg_us: data.txn_data.summary.avg_commit_latency_us,
            commit_latency_p50_us: data.txn_data.summary.p50_commit_latency_us,
            commit_latency_p95_us: data.txn_data.summary.p95_commit_latency_us,
            commit_latency_p99_us: data.txn_data.summary.p99_commit_latency_us,
            commit_latency_max_us: data.txn_data.summary.max_commit_latency_us,
            top_threads,
        })
    } else {
        None
    };

    // Build slow operations summary
    let slow_operations: Vec<SlowOpSummary> = data
        .cursor_data
        .slow_ops_by_table
        .iter()
        .take(10)
        .map(|s| {
            // Get top operation from by_operation breakdown
            let top_op = s
                .by_operation
                .first()
                .map(|op| op.operation.clone())
                .unwrap_or_default();
            SlowOpSummary {
                table: s.table.clone(),
                operation: top_op,
                count: s.slow_op_count,
                avg_latency_us: s.avg_slow_latency_us,
                max_latency_us: s.max_latency_us,
            }
        })
        .collect();

    CompactExport {
        trace: TraceSummaryCompact {
            duration_secs: data.summary.duration_secs,
            total_events: data.summary.total_events,
            file_size_gb: data.summary.file_size_gb,
            file_offset_range_gb: (
                data.summary.min_offset as f64 / 1e9,
                data.summary.max_offset as f64 / 1e9,
            ),
            block_range: data.summary.block_range.clone(),
        },
        page_faults: PageFaultAnalysis {
            total: data.summary.page_faults,
            major: data.summary.major_faults,
            minor: data.summary.minor_faults,
            major_ratio: data.summary.major_fault_ratio,
            rate_per_sec: data.summary.fault_rate_per_sec,
            unique_pages: data.summary.unique_pages,
            sequential_ratio: data.patterns.sequential_ratio,
            random_ratio: data.patterns.random_ratio,
            burst_median: data.patterns.burst_stats.median_events,
            burst_p95: data.patterns.burst_stats.p95_events,
            burst_max: data.patterns.burst_stats.max_events,
        },
        tables,
        cursor_ops,
        transactions,
        slow_operations,
        insights,
    }
}

/// Write compact export to JSON file
pub fn write_compact_export(data: &ViewerData, path: impl AsRef<Path>) -> std::io::Result<()> {
    let export = generate_compact_export(data);
    let json = serde_json::to_string_pretty(&export)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(path, json)
}
