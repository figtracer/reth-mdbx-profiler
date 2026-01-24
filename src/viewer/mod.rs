//! Web-based trace viewer
//!
//! Generates a self-contained HTML file with interactive visualizations
//! for MDBX page fault traces and cursor operations.

mod template;

use serde::Serialize;
use std::path::Path;

/// Data structure for the web viewer
#[derive(Debug, Serialize)]
pub struct ViewerData {
    /// Summary statistics
    pub summary: TraceSummary,
    /// Timeline data (sampled for performance)
    pub timeline: Vec<TimelinePoint>,
    /// Table breakdown (legacy - kept for compatibility)
    pub tables: Vec<TableStats>,
    /// Unified table view with drill-dow
    pub unified_tables: Vec<UnifiedTableStats>,
    /// Thread distribution
    pub threads: Vec<ThreadStats>,

    /// Access pattern analysis
    pub patterns: PatternAnalysis,
    /// Heatmap data (2D grid)
    pub heatmap: HeatmapData,
    /// Cursor operation data
    pub cursor_data: CursorData,
    /// Cursor lifecycle data (open/close tracking)
    pub cursor_lifecycle: CursorLifecycleData,
    /// Transaction lifecycle data
    pub txn_data: TxnData,
    /// Warning about page fault attribution method
    pub page_fault_attribution_warning: Option<String>,
    /// Direct fault attribution data (from BPF active_ops tracking)
    pub direct_fault_attribution: DirectFaultAttribution,
    /// Page type distribution (Branch, Leaf, Overflow, Meta)
    pub page_type_stats: PageTypeStats,
    /// Histogram of faults per operation
    pub operation_histogram: OperationFaultHistogram,
    /// B+ tree traversal visualization data
    pub tree_traversal: TreeTraversalViz,
    /// Comprehensive B+ tree visualization data including per-block analysis,
    /// operation-to-page-type breakdown, and tree depth estimates
    pub btree_viz: BTreeVisualization,
    /// Working set analysis for memory requirement estimation
    pub working_set: WorkingSetAnalysis,
    /// CPU profiling summary
    pub cpu_profile: CpuProfileSummary,
}

// ============================================================================
// Working Set Analysis Types
// ============================================================================

/// Working set analysis for understanding memory requirements
#[derive(Debug, Serialize, Default)]
pub struct WorkingSetAnalysis {
    /// Whether working set data is available
    pub has_data: bool,
    /// Total unique pages accessed during trace
    pub total_unique_pages: u64,
    /// Total page accesses (including repeats)
    pub total_accesses: u64,
    /// Ratio of accesses to previously-seen pages (0.0-1.0)
    pub reuse_ratio: f64,
    /// Average accesses per unique page
    pub avg_accesses_per_page: f64,
    /// Cache hit rate simulation at various cache sizes
    pub cache_simulation: Vec<CacheSimulationPoint>,
    /// Access count distribution (how many pages have 1x, 2x, 3-5x, etc. accesses)
    pub access_count_distribution: Vec<AccessCountBucket>,
    /// Per-table working set statistics
    pub per_table: Vec<TableWorkingSet>,
    /// Time-windowed working set sizes
    pub time_windowed: Vec<TimeWindowedWSS>,
    /// Hot page analysis (Pareto distribution)
    pub hot_page_analysis: HotPageAnalysis,
    /// Summary text for quick understanding
    pub summary_text: String,
}

/// Cache simulation result at a specific cache size
#[derive(Debug, Serialize, Clone)]
pub struct CacheSimulationPoint {
    /// Cache size in GB
    pub cache_size_gb: f64,
    /// Cache size in pages
    pub cache_size_pages: u64,
    /// Estimated hit rate (0.0-1.0)
    pub hit_rate: f64,
    /// Estimated major faults avoided per second
    pub faults_avoided_per_sec: f64,
}

/// Access count distribution bucket (how many pages have N accesses)
#[derive(Debug, Serialize, Clone)]
pub struct AccessCountBucket {
    /// Bucket label (e.g., "1x", "2x", "3-5x")
    pub label: String,
    /// Number of pages in this bucket
    pub page_count: u64,
    /// Percentage of sampled pages
    pub percentage: f64,
}

/// Per-table working set statistics
#[derive(Debug, Serialize, Clone)]
pub struct TableWorkingSet {
    /// Table name
    pub name: String,
    /// DBI index
    pub dbi: u32,
    /// Unique pages accessed
    pub unique_pages: u64,
    /// Total accesses to this table
    pub total_accesses: u64,
    /// Reuse ratio for this table
    pub reuse_ratio: f64,
    /// Number of hot pages (accounting for 80% of accesses)
    pub hot_pages: u64,
    /// Hot page ratio (hot_pages / unique_pages)
    pub hot_page_ratio: f64,
    /// Estimated size of hot set in MB
    pub hot_set_mb: f64,
    /// Estimated total table working set in MB
    pub working_set_mb: f64,
}

/// Time-windowed working set size
#[derive(Debug, Serialize, Clone)]
pub struct TimeWindowedWSS {
    /// Window size in seconds
    pub window_secs: u64,
    /// Average working set size (unique pages) in each window
    pub avg_wss_pages: u64,
    /// Maximum working set size seen in any window
    pub max_wss_pages: u64,
    /// Minimum working set size seen in any window
    pub min_wss_pages: u64,
    /// Working set size in MB (avg)
    pub avg_wss_mb: f64,
}

/// Hot page analysis (Pareto distribution)
#[derive(Debug, Serialize, Default)]
pub struct HotPageAnalysis {
    /// Pages accounting for 50% of accesses
    pub pages_for_50pct: u64,
    /// Pages accounting for 80% of accesses
    pub pages_for_80pct: u64,
    /// Pages accounting for 90% of accesses
    pub pages_for_90pct: u64,
    /// Pages accounting for 95% of accesses
    pub pages_for_95pct: u64,
    /// Ratio: pages_for_80pct / total_unique_pages (lower = more skewed)
    pub pareto_ratio: f64,
    /// Distribution curve points for visualization
    pub distribution_curve: Vec<ParetoPoint>,
}

/// Point on the Pareto distribution curve
#[derive(Debug, Serialize, Clone)]
pub struct ParetoPoint {
    /// Percentage of pages (sorted by access count, hottest first)
    pub pages_pct: f64,
    /// Percentage of accesses covered
    pub accesses_pct: f64,
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

/// Page type distribution statistics - shows breakdown of faults by MDBX page type
/// (Branch pages for B+ tree traversal, Leaf pages for actual data, etc.)
#[derive(Debug, Serialize, Default)]
pub struct PageTypeStats {
    /// Whether page type data is available (requires BPF page type detection)
    pub has_data: bool,
    /// Total faults analyzed
    pub total_faults: u64,
    /// Breakdown by page type
    pub by_type: Vec<PageTypeFaultCount>,
    /// Ratio of traversal (branch/meta) to data (leaf/overflow) faults
    /// Higher values indicate more tree traversal overhead
    pub traversal_to_data_ratio: f64,
}

/// Fault count for a specific page type
#[derive(Debug, Serialize)]
pub struct PageTypeFaultCount {
    pub page_type: String,
    pub total_faults: u64,
    pub major_faults: u64,
    pub percentage: f64,
}

/// Histogram of page faults per operation
/// Shows how many faults occur during typical operations
#[derive(Debug, Serialize, Default)]
pub struct OperationFaultHistogram {
    /// Whether histogram data is available
    pub has_data: bool,
    /// Distribution buckets [0, 1-2, 3-5, 6-10, 11-20, 21+]
    pub distribution: Vec<HistogramBucket>,
    /// Average faults per operation
    pub avg_faults_per_op: f64,
    /// Maximum faults seen in any single operation
    pub max_faults_per_op: u32,
    /// P50 faults per operation
    pub p50_faults: u32,
    /// P95 faults per operation
    pub p95_faults: u32,
    /// P99 faults per operation
    pub p99_faults: u32,
}

/// Histogram bucket for fault distribution
#[derive(Debug, Serialize)]
pub struct HistogramBucket {
    pub label: String,
    pub count: u64,
    pub percentage: f64,
}

/// B+ tree traversal visualization data - shows how operations traverse the tree
#[derive(Debug, Serialize, Default)]
pub struct TreeTraversalViz {
    /// Whether tree traversal data is available
    pub has_data: bool,
    /// Per-table tree statistics
    pub tables: Vec<TableTreeStats>,
}

/// Per-block analysis showing page faults and I/O time for each Ethereum block.
/// This helps identify which blocks caused the most tree traversal overhead.
#[derive(Debug, Serialize, Default, Clone)]
pub struct BlockAnalysis {
    /// Ethereum block number
    pub block_number: u64,
    /// Faults on branch pages (B+ tree traversal)
    pub branch_faults: u32,
    /// Faults on leaf pages (data access)
    pub leaf_faults: u32,
    /// Faults on overflow pages (large values)
    pub overflow_faults: u32,
    /// Total major faults (disk I/O)
    pub major_faults: u32,
    /// Total I/O time in microseconds (estimated from major faults)
    pub io_time_us: u64,
    /// Tables touched while processing this block
    pub tables_touched: Vec<String>,
    /// Total faults for this block
    pub total_faults: u32,
}

/// Per-batch analysis showing page faults for each RW transaction batch.
/// Reth batches multiple blocks into single MDBX transactions, so this gives
/// more accurate attribution than per-block analysis.
#[derive(Debug, Serialize, Default, Clone)]
pub struct BatchAnalysis {
    /// Batch index (0-based, chronological order)
    pub batch_index: u32,
    /// First block number in this batch
    pub first_block: u64,
    /// Last block number in this batch
    pub last_block: u64,
    /// Number of blocks in this batch
    pub block_count: u32,
    /// Faults on branch pages (B+ tree traversal)
    pub branch_faults: u32,
    /// Faults on leaf pages (data access)
    pub leaf_faults: u32,
    /// Faults on overflow pages (large values)
    pub overflow_faults: u32,
    /// Total major faults (disk I/O)
    pub major_faults: u32,
    /// Total I/O time in microseconds (estimated from major faults)
    pub io_time_us: u64,
    /// Tables touched while processing this batch
    pub tables_touched: Vec<String>,
    /// Total faults for this batch
    pub total_faults: u32,
    /// Batch start timestamp (ns from trace start)
    pub start_time_ns: u64,
    /// Batch end timestamp (ns from trace start)
    pub end_time_ns: u64,
    /// Commit latency in microseconds
    pub commit_latency_us: u64,
}

/// Operation-to-page-type breakdown showing which cursor operations
/// cause which types of page faults (branch vs leaf).
#[derive(Debug, Serialize, Clone)]
pub struct OperationPageTypeBreakdown {
    /// Cursor operation name (SET_RANGE, NEXT, etc.)
    pub cursor_op: String,
    /// Faults on branch pages
    pub branch_faults: u32,
    /// Faults on leaf pages
    pub leaf_faults: u32,
    /// Faults on overflow pages
    pub overflow_faults: u32,
    /// Total operations of this type
    pub total_ops: u32,
    /// Average faults per operation
    pub avg_faults_per_op: f64,
    /// Major faults (disk I/O)
    pub major_faults: u32,
    /// Percentage of total faults this operation causes
    pub fault_percentage: f64,
}

/// Aggregated B+ tree visualization data for the enhanced Page Types tab
#[derive(Debug, Serialize, Default)]
pub struct BTreeVisualization {
    /// Whether B+ tree visualization data is available
    pub has_data: bool,
    /// Per-batch analysis (sorted by batch_index) - more accurate than block analysis
    pub batch_analysis: Vec<BatchAnalysis>,
    /// Per-block analysis (sorted by total_faults descending) - kept for compatibility
    pub block_analysis: Vec<BlockAnalysis>,
    /// Operation-to-page-type breakdown
    pub operation_page_types: Vec<OperationPageTypeBreakdown>,
    /// Block range covered
    pub block_range: Option<BlockRange>,
    /// Tree depth estimates per table (table_name -> estimated_depth)
    pub tree_depth_estimates: Vec<TreeDepthEstimate>,
    /// Overall traversal efficiency score (0-100, higher is better)
    pub traversal_efficiency_score: f64,
    /// Attribution statistics - how much data is accurately attributed
    pub attribution_stats: AttributionStats,
}

/// Statistics about how accurately we could attribute page faults to blocks/batches
#[derive(Debug, Serialize, Default, Clone)]
pub struct AttributionStats {
    /// Total page faults in the trace
    pub total_faults: u64,
    /// Faults attributed to specific batches (between RW commits)
    pub batch_attributed_faults: u64,
    /// Faults attributed to specific blocks (between block writes)
    pub block_attributed_faults: u64,
    /// Faults that couldn't be attributed (outside known windows)
    pub unattributed_faults: u64,
    /// Percentage of faults with batch attribution (0-100)
    pub batch_attribution_pct: f64,
    /// Percentage of faults with block attribution (0-100)
    pub block_attribution_pct: f64,
    /// Number of RW commits detected
    pub rw_commits_detected: u32,
    /// Number of blocks with write data
    pub blocks_with_writes: u32,
}

/// Tree traversal statistics for a specific table
#[derive(Debug, Serialize)]
pub struct TableTreeStats {
    pub name: String,
    pub dbi: u32,
    /// Total faults on this table
    pub total_faults: u64,
    /// Faults on branch pages (B+ tree traversal)
    pub branch_faults: u64,
    /// Faults on leaf pages (actual data)
    pub leaf_faults: u64,
    /// Faults on overflow pages (large values)
    pub overflow_faults: u64,
    /// Branch to leaf ratio (higher = deeper traversal overhead)
    pub branch_leaf_ratio: f64,
}

/// Estimated tree depth for a table based on branch:leaf ratio
#[derive(Debug, Serialize, Clone)]
pub struct TreeDepthEstimate {
    /// Table name
    pub table_name: String,
    /// Estimated tree depth (1 = flat, 4+ = deep)
    pub estimated_depth: f64,
    /// Branch to leaf ratio used for estimation
    pub branch_leaf_ratio: f64,
    /// Confidence level (based on sample size)
    pub confidence: String,
    /// Total faults used for estimation
    pub sample_size: u64,
}

/// Link to reth source code for a table
#[derive(Debug, Serialize, Clone)]
pub struct TableSourceLink {
    /// GitHub URL to the table definition
    pub github_url: String,
    /// Documentation URL
    pub docs_url: String,
    /// Brief description of the table
    pub description: String,
    /// Key type (e.g., "B256", "BlockNumber")
    pub key_type: String,
    /// Value type (e.g., "Account", "StorageValue")
    pub value_type: String,
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

/// Cursor lifecycle tracking data (open/close events)
#[derive(Debug, Serialize, Default)]
pub struct CursorLifecycleData {
    /// Whether lifecycle data is available
    pub has_data: bool,
    /// Total cursor opens tracked
    pub total_opens: u64,
    /// Total cursor closes tracked
    pub total_closes: u64,
    /// Cursors still open at end of trace (opened but not closed)
    pub still_open: u64,
    /// Average cursor lifetime in microseconds (for cursors that were closed)
    pub avg_lifetime_us: f64,
    /// Median cursor lifetime in microseconds
    pub p50_lifetime_us: f64,
    /// 95th percentile cursor lifetime in microseconds
    pub p95_lifetime_us: f64,
    /// 99th percentile cursor lifetime in microseconds
    pub p99_lifetime_us: f64,
    /// Per-table cursor lifecycle statistics
    pub by_table: Vec<CursorLifecycleTableStats>,
}

/// Per-table cursor lifecycle statistics
#[derive(Debug, Serialize)]
pub struct CursorLifecycleTableStats {
    /// Table name
    pub table: String,
    /// Database index
    pub dbi: u32,
    /// Number of cursor opens for this table
    pub opens: u64,
    /// Number of cursor closes for this table
    pub closes: u64,
    /// Average cursor lifetime in microseconds (for this table)
    pub avg_lifetime_us: f64,
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

#[derive(Debug, Clone, Serialize)]
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
    /// B+ tree depth statistics from per-operation tracking
    pub tree_depth_stats: TreeDepthStats,
}

/// Statistics about B+ tree traversal depth during operations
#[derive(Debug, Default, Serialize)]
pub struct TreeDepthStats {
    /// Total operations with depth tracking data
    pub ops_with_depth_data: u64,
    /// Maximum tree depth observed across all operations
    pub max_depth_observed: u32,
    /// Average tree depth across operations that had faults
    pub avg_depth: f64,
    /// Distribution of max depths: depth -> count
    pub depth_distribution: Vec<(u32, u64)>,
    /// Operations by depth bucket for histogram
    pub depth_histogram: Vec<DepthBucket>,
    /// Per-table depth statistics
    pub by_table: Vec<TableDepthStats>,
    /// Per-operation type depth statistics
    pub by_operation: Vec<OperationDepthStats>,
}

/// Bucket for depth histogram
#[derive(Debug, Serialize)]
pub struct DepthBucket {
    pub depth: u32,
    pub count: u64,
    pub percentage: f64,
    pub avg_faults: f64,
    pub avg_latency_us: f64,
}

/// Per-table depth statistics
#[derive(Debug, Serialize)]
pub struct TableDepthStats {
    pub table_name: String,
    pub dbi: u32,
    pub ops_count: u64,
    pub max_depth: u32,
    pub avg_depth: f64,
    pub avg_faults: f64,
    pub avg_latency_us: f64,
    /// Distribution: how many ops at each depth level
    pub depth_distribution: Vec<(u32, u64)>,
}

/// Per-operation type depth statistics
#[derive(Debug, Serialize)]
pub struct OperationDepthStats {
    pub operation: String,
    pub ops_count: u64,
    pub max_depth: u32,
    pub avg_depth: f64,
    pub avg_faults: f64,
    pub avg_latency_us: f64,
    pub is_seek: bool,
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

/// CPU profiling summary - shows time spent on CPU vs waiting for I/O
#[derive(Debug, Serialize, Default)]
pub struct CpuProfileSummary {
    /// Whether CPU profiling data is available
    pub has_data: bool,
    /// Total wall clock time across all operations (ms)
    pub total_wall_time_ms: f64,
    /// Estimated CPU time (wall time - fault handler time) (ms)
    pub total_cpu_time_ms: f64,
    /// Time spent waiting in page fault handlers (I/O) (ms)
    pub total_io_wait_ms: f64,
    /// Overall CPU efficiency (0.0-1.0, higher = more CPU bound)
    pub cpu_efficiency: f64,
    /// Bottleneck classification
    pub bottleneck: String,
    /// Top I/O bound tables (by wall time with low CPU efficiency)
    pub top_io_bound_tables: Vec<CpuTableEntry>,
    /// Top CPU bound tables (by wall time with high CPU efficiency)
    pub top_cpu_bound_tables: Vec<CpuTableEntry>,
}

/// Entry for CPU profile table rankings
#[derive(Debug, Serialize)]
pub struct CpuTableEntry {
    pub name: String,
    pub wall_time_ms: f64,
    pub cpu_time_ms: f64,
    pub cpu_efficiency: f64,
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
    // CPU profiling data
    /// Total wall time for all operations (ms)
    pub total_wall_time_ms: f64,
    /// Estimated CPU time (wall time - fault latency) (ms)
    pub total_cpu_time_ms: f64,
    /// CPU efficiency: cpu_time / wall_time (0.0-1.0, higher = more CPU bound)
    pub cpu_efficiency: f64,
    /// Whether this table is I/O bound (cpu_efficiency < 0.5)
    pub is_io_bound: bool,
    // Top operation causing faults/slowness
    pub top_operation: String,
    // Drill-down details
    pub details: TableDrillDown,
    /// Faults on branch pages (B+ tree traversal)
    pub branch_faults: u64,
    /// Faults on leaf pages (actual data)
    pub leaf_faults: u64,
    /// Faults on overflow pages (large values)
    pub overflow_faults: u64,
    /// Severity level for visual color coding (critical, high, medium, low)
    pub severity: String,
    /// Link to reth source code
    pub reth_source: Option<TableSourceLink>,
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
    pub major_faults: u64,
    pub percentage: f64,
    /// Per-thread timeline: fault counts per time bucket
    pub timeline: Vec<ThreadTimelinePoint>,
    /// Per-thread table breakdown: which tables this thread accessed
    pub top_tables: Vec<ThreadTableStats>,
}

#[derive(Debug, Serialize)]
pub struct ThreadTableStats {
    pub table_name: String,
    pub faults: u64,
    pub major_faults: u64,
    pub major_pct: f64,
}

#[derive(Debug, Serialize)]
pub struct ThreadTimelinePoint {
    pub time_ms: u64,
    pub faults: u32,
    pub major_faults: u32,
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

/// Attribution data for a single heatmap cell
#[derive(Debug, Serialize, Clone)]
pub struct HeatmapCellAttribution {
    /// Cell index (time_idx * offset_buckets + offset_idx)
    pub cell: u32,
    /// Top tables by fault count: (table_name, fault_count, major_count)
    pub tables: Vec<(String, u32, u32)>,
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
    /// Sparse attribution data for cells with faults (only top tables per cell)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cell_attribution: Vec<HeatmapCellAttribution>,
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
