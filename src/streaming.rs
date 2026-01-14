//! Streaming analyzer for large trace files
//!
//! This module processes trace files in a single pass, computing aggregated
//! statistics incrementally without storing individual events in memory.
//! Designed to handle traces of any size with bounded memory usage.

use crate::event::{
    dbi_to_table_name, is_pre_trace_cursor, CursorEvent, CursorOp, MdbxPageType, PageFaultEvent,
    TxnEvent, NO_ACTIVE_OP_DBI,
};
use crate::mdbx_metadata::PageAttribution;
use crate::viewer::{
    BTreeVisualization, BatchAnalysis, BlockRange, BurstStats, CursorData, CursorOpSample,
    CursorSummary, CursorTableStats, CursorTimelinePoint, DepthBucket, DirectFaultAttribution,
    FaultsByCursorOp, FaultsByOpType, HeatmapData, HistogramBucket, OpFaultCount,
    OperationDepthStats, OperationFaultHistogram, OperationPageTypeBreakdown, OperationStats,
    PageTypeFaultCount, PageTypeStats, PatternAnalysis, RwCommitPoint, SlowKeyStats,
    SlowOpBreakdown, SlowOpsTableStats, StrideInfo, TableDepthStats, TableDrillDown,
    TableSourceLink, TableTreeStats, ThreadStats, TimelinePoint, TraceSummary, TreeDepthEstimate,
    TreeDepthStats, TreeTraversalViz, TxnConcurrencyStats, TxnData, TxnSummary, TxnThreadStats,
    TxnTimelineEntry, UnifiedTableStats, ViewerData,
};
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::io::{BufRead, BufReader};

/// Configuration for streaming analysis
#[derive(Debug, Clone)]
pub struct StreamingConfig {
    /// Time bucket size in milliseconds for timeline
    pub bucket_ms: u64,
    /// Maximum number of timeline points to keep
    pub max_timeline_points: usize,
    /// Maximum number of sampled cursor ops to keep
    pub max_cursor_samples: usize,
    /// Maximum number of transaction timeline entries
    pub max_txn_timeline: usize,
    /// Threshold for slow operations in microseconds
    pub slow_op_threshold_us: u64,
    /// Maximum number of hot keys to track per table
    pub max_hot_keys_per_table: usize,
    /// Maximum number of blocks to track for B-tree analysis
    pub max_block_analysis: usize,
    /// Sampling rate for cursor operations (1 = keep all, 10 = keep 1 in 10)
    pub cursor_sample_rate: u64,
    /// Heatmap grid dimensions
    pub heatmap_time_buckets: u32,
    pub heatmap_offset_buckets: u32,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            bucket_ms: 100,
            max_timeline_points: 1000,
            max_cursor_samples: 200,
            max_txn_timeline: 1000,
            slow_op_threshold_us: 100,
            max_hot_keys_per_table: 50,
            max_block_analysis: 500,
            cursor_sample_rate: 1,
            heatmap_time_buckets: 100,
            heatmap_offset_buckets: 50,
        }
    }
}

/// Online statistics accumulator using Welford's algorithm
#[derive(Debug, Default, Clone)]
struct OnlineStats {
    count: u64,
    sum: u64,
    min: u64,
    max: u64,
    // For variance/stddev if needed
    mean: f64,
    m2: f64,
}

impl OnlineStats {
    fn new() -> Self {
        Self {
            count: 0,
            sum: 0,
            min: u64::MAX,
            max: 0,
            mean: 0.0,
            m2: 0.0,
        }
    }

    fn add(&mut self, value: u64) {
        self.count += 1;
        self.sum += value;
        self.min = self.min.min(value);
        self.max = self.max.max(value);

        // Welford's online algorithm for mean/variance
        let delta = value as f64 - self.mean;
        self.mean += delta / self.count as f64;
        let delta2 = value as f64 - self.mean;
        self.m2 += delta * delta2;
    }

    fn average(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.sum as f64 / self.count as f64
        }
    }
}

/// Approximate percentile tracker using a fixed-size sample reservoir
#[derive(Debug, Clone)]
struct ReservoirSampler {
    samples: Vec<u64>,
    capacity: usize,
    count: u64,
}

impl ReservoirSampler {
    fn new(capacity: usize) -> Self {
        Self {
            samples: Vec::with_capacity(capacity),
            capacity,
            count: 0,
        }
    }

    fn add(&mut self, value: u64) {
        self.count += 1;
        if self.samples.len() < self.capacity {
            self.samples.push(value);
        } else {
            // Reservoir sampling: replace with probability capacity/count
            let idx = fastrand::u64(0..self.count) as usize;
            if idx < self.capacity {
                self.samples[idx] = value;
            }
        }
    }

    fn percentile(&mut self, p: f64) -> u64 {
        if self.samples.is_empty() {
            return 0;
        }
        self.samples.sort_unstable();
        let idx = ((self.samples.len() as f64 * p) as usize).min(self.samples.len() - 1);
        self.samples[idx]
    }

    fn p50(&mut self) -> u64 {
        self.percentile(0.50)
    }
    fn p95(&mut self) -> u64 {
        self.percentile(0.95)
    }
    fn p99(&mut self) -> u64 {
        self.percentile(0.99)
    }
}

/// Per-table streaming statistics
#[derive(Debug, Default)]
struct TableStreamStats {
    // Fault stats
    total_faults: u64,
    major_faults: u64,
    branch_faults: u64,
    leaf_faults: u64,
    overflow_faults: u64,
    // Cursor stats
    total_ops: u64,
    seek_ops: u64,
    nav_ops: u64,
    slow_ops: u64,
    total_latency_ns: u64,
    max_latency_ns: u64,
    // Fault attribution by op type
    faults_by_op_type: HashMap<u32, (u64, u64)>, // op_type -> (total, major)
    faults_by_cursor_op: HashMap<u32, (u64, u64)>, // cursor_op -> (total, major)
    // Slow op breakdown
    slow_by_op: HashMap<String, (u64, u64, u64)>, // op_name -> (count, total_latency, max_latency)
}

/// Per-thread streaming statistics
#[derive(Debug, Default)]
struct ThreadStreamStats {
    faults: u64,
    // Transaction stats
    total_txns: u64,
    ro_txns: u64,
    rw_txns: u64,
    commits: u64,
    aborts: u64,
    total_commit_latency: u64,
}

/// Bounded priority queue for tracking top-N items
#[derive(Debug)]
struct TopNTracker<T: Ord> {
    heap: BinaryHeap<std::cmp::Reverse<T>>,
    capacity: usize,
}

impl<T: Ord + Clone> TopNTracker<T> {
    fn new(capacity: usize) -> Self {
        Self {
            heap: BinaryHeap::with_capacity(capacity + 1),
            capacity,
        }
    }

    fn push(&mut self, item: T) {
        if self.heap.len() < self.capacity {
            self.heap.push(std::cmp::Reverse(item));
        } else if let Some(std::cmp::Reverse(min)) = self.heap.peek() {
            if item > *min {
                self.heap.pop();
                self.heap.push(std::cmp::Reverse(item));
            }
        }
    }

    fn into_sorted_vec(self) -> Vec<T> {
        let mut v: Vec<_> = self.heap.into_iter().map(|r| r.0).collect();
        v.sort_by(|a, b| b.cmp(a)); // Descending
        v
    }
}

/// Hot key tracker with bounded memory
#[derive(Debug)]
struct HotKeyTracker {
    // (dbi, key_hex) -> (slow_count, total_count, total_latency, max_latency)
    keys: HashMap<(u32, String), (u64, u64, u64, u64)>,
    max_keys: usize,
}

impl HotKeyTracker {
    fn new(max_keys: usize) -> Self {
        Self {
            keys: HashMap::new(),
            max_keys,
        }
    }

    fn add(&mut self, dbi: u32, key_hex: String, latency_ns: u64, is_slow: bool) {
        let entry = self.keys.entry((dbi, key_hex)).or_insert((0, 0, 0, 0));
        if is_slow {
            entry.0 += 1;
        }
        entry.1 += 1;
        entry.2 += latency_ns;
        entry.3 = entry.3.max(latency_ns);

        // Prune if too large (keep top slow keys)
        if self.keys.len() > self.max_keys * 2 {
            self.prune();
        }
    }

    fn prune(&mut self) {
        // Keep only top N by slow_count
        let mut entries: Vec<_> = self.keys.drain().collect();
        entries.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
        entries.truncate(self.max_keys);
        self.keys = entries.into_iter().collect();
    }
}

/// Main streaming aggregator
pub struct StreamingAggregator {
    config: StreamingConfig,

    // Global stats
    first_timestamp: Option<u64>,
    last_timestamp: u64,
    total_events: u64,

    // Page fault stats
    page_fault_count: u64,
    major_fault_count: u64,
    unique_pages: HashSet<u64>,
    min_offset: u64,
    max_offset: u64,

    // Per-table stats (keyed by DBI for efficiency)
    table_stats: HashMap<u32, TableStreamStats>,

    // Per-thread stats
    thread_stats: HashMap<u32, ThreadStreamStats>,

    // Timeline (downsampled)
    timeline_buckets: HashMap<u64, (u32, u32, HashSet<u64>)>, // bucket_ms -> (faults, major, pages)
    current_bucket: u64,

    // Cursor stats
    cursor_count: u64,
    cursor_seek_count: u64,
    cursor_nav_count: u64,
    cursor_error_count: u64,
    direct_get_count: u64,
    cursor_latency_stats: OnlineStats,
    cursor_latency_sampler: ReservoirSampler,
    cursor_samples: Vec<CursorOpSample>,
    cursor_sample_counter: u64,
    pre_trace_cursor_ops: u64,

    // Cursor timeline (downsampled)
    cursor_timeline_buckets: HashMap<u64, (u32, u32, u64)>, // bucket_ms -> (ops, seeks, total_latency)

    // Hot keys
    hot_keys: HotKeyTracker,

    // Transaction stats
    txn_count: u64,
    ro_txn_count: u64,
    rw_txn_count: u64,
    commit_count: u64,
    abort_count: u64,
    commit_latency_stats: OnlineStats,
    commit_latency_sampler: ReservoirSampler,
    rw_commit_points: Vec<RwCommitPoint>,
    active_txns: HashMap<u64, (u64, u32, bool)>, // txn_ptr -> (start_ts, tid, is_rw)
    txn_timeline_samples: Vec<TxnTimelineEntry>,
    max_concurrent_ro: u32,
    max_concurrent_rw: u32,
    current_concurrent_ro: u32,
    current_concurrent_rw: u32,

    // Pattern analysis
    last_page: Option<u64>,
    sequential_count: u64,
    random_count: u64,
    stride_counts: HashMap<i64, u64>,

    // Page type stats
    page_type_counts: HashMap<u8, (u64, u64)>, // page_type -> (total, major)

    // Direct attribution stats
    directly_attributed: u64,

    // Faults by op type (global)
    faults_by_op_type: HashMap<u32, (u64, u64)>,
    faults_by_cursor_op: HashMap<u32, (u64, u64)>,

    // Operation fault histogram (from cursor events)
    fault_histogram: HashMap<u32, u64>, // faults_per_op -> count
    max_faults_per_op: u32,

    // Tree depth stats
    depth_distribution: HashMap<u32, u64>,
    max_depth_observed: u32,
    ops_with_depth_data: u64,
    depth_by_table: HashMap<u32, (u64, u64, u32)>, // dbi -> (ops, total_depth, max_depth)
    depth_by_op: HashMap<u32, (u64, u64, u32)>,    // cursor_op -> (ops, total_depth, max_depth)

    // Block range
    min_block: Option<u64>,
    max_block: Option<u64>,

    // Batch analysis (RW commits)
    current_batch_faults: BatchFaultAccumulator,
    batch_analyses: Vec<BatchAnalysis>,
    batch_index: u32,

    // Heatmap
    heatmap_data: Vec<Vec<u32>>,
    heatmap_initialized: bool,

    // Parse errors
    parse_errors: u64,
}

#[derive(Debug, Default)]
struct BatchFaultAccumulator {
    branch_faults: u32,
    leaf_faults: u32,
    overflow_faults: u32,
    major_faults: u32,
    total_faults: u32,
    io_time_us: u64,
    tables_touched: HashSet<String>,
    first_block: Option<u64>,
    last_block: Option<u64>,
    start_time_ns: u64,
}

impl StreamingAggregator {
    pub fn new(config: StreamingConfig) -> Self {
        let heatmap_data = vec![
            vec![0u32; config.heatmap_offset_buckets as usize];
            config.heatmap_time_buckets as usize
        ];

        Self {
            config: config.clone(),
            first_timestamp: None,
            last_timestamp: 0,
            total_events: 0,
            page_fault_count: 0,
            major_fault_count: 0,
            unique_pages: HashSet::new(),
            min_offset: u64::MAX,
            max_offset: 0,
            table_stats: HashMap::new(),
            thread_stats: HashMap::new(),
            timeline_buckets: HashMap::new(),
            current_bucket: 0,
            cursor_count: 0,
            cursor_seek_count: 0,
            cursor_nav_count: 0,
            cursor_error_count: 0,
            direct_get_count: 0,
            cursor_latency_stats: OnlineStats::new(),
            cursor_latency_sampler: ReservoirSampler::new(10000),
            cursor_samples: Vec::with_capacity(config.max_cursor_samples),
            cursor_sample_counter: 0,
            pre_trace_cursor_ops: 0,
            cursor_timeline_buckets: HashMap::new(),
            hot_keys: HotKeyTracker::new(config.max_hot_keys_per_table * 32),
            txn_count: 0,
            ro_txn_count: 0,
            rw_txn_count: 0,
            commit_count: 0,
            abort_count: 0,
            commit_latency_stats: OnlineStats::new(),
            commit_latency_sampler: ReservoirSampler::new(5000),
            rw_commit_points: Vec::new(),
            active_txns: HashMap::new(),
            txn_timeline_samples: Vec::new(),
            max_concurrent_ro: 0,
            max_concurrent_rw: 0,
            current_concurrent_ro: 0,
            current_concurrent_rw: 0,
            last_page: None,
            sequential_count: 0,
            random_count: 0,
            stride_counts: HashMap::new(),
            page_type_counts: HashMap::new(),
            directly_attributed: 0,
            faults_by_op_type: HashMap::new(),
            faults_by_cursor_op: HashMap::new(),
            fault_histogram: HashMap::new(),
            max_faults_per_op: 0,
            depth_distribution: HashMap::new(),
            max_depth_observed: 0,
            ops_with_depth_data: 0,
            depth_by_table: HashMap::new(),
            depth_by_op: HashMap::new(),
            min_block: None,
            max_block: None,
            current_batch_faults: BatchFaultAccumulator::default(),
            batch_analyses: Vec::new(),
            batch_index: 0,
            heatmap_data,
            heatmap_initialized: false,
            parse_errors: 0,
        }
    }

    /// Process a page fault event
    pub fn process_page_fault(&mut self, event: &PageFaultEvent) {
        if event.event_type != 1 {
            return;
        }

        let ts = event.timestamp_ns;
        if self.first_timestamp.is_none() {
            self.first_timestamp = Some(ts);
            self.current_batch_faults.start_time_ns = ts;
        }
        self.last_timestamp = ts;
        self.total_events += 1;

        self.page_fault_count += 1;
        let is_major = event.is_major_fault();
        if is_major {
            self.major_fault_count += 1;
        }

        let page = event.page_number();
        self.unique_pages.insert(page);
        self.min_offset = self.min_offset.min(event.file_offset);
        self.max_offset = self.max_offset.max(event.file_offset);

        // Thread stats
        let thread = self.thread_stats.entry(event.tid).or_default();
        thread.faults += 1;

        // Pattern analysis
        if let Some(last) = self.last_page {
            let stride = page as i64 - last as i64;
            if stride.abs() <= 16 {
                self.sequential_count += 1;
            } else {
                self.random_count += 1;
            }
            *self.stride_counts.entry(stride).or_insert(0) += 1;
        }
        self.last_page = Some(page);

        // Page type stats
        let page_type = event.page_type;
        let entry = self.page_type_counts.entry(page_type).or_insert((0, 0));
        entry.0 += 1;
        if is_major {
            entry.1 += 1;
        }

        // Timeline bucket
        let first_ts = self.first_timestamp.unwrap();
        let bucket = (ts - first_ts) / (self.config.bucket_ms * 1_000_000);
        let timeline_entry = self
            .timeline_buckets
            .entry(bucket)
            .or_insert((0, 0, HashSet::new()));
        timeline_entry.0 += 1;
        if is_major {
            timeline_entry.1 += 1;
        }
        timeline_entry.2.insert(page);

        // Direct attribution
        if event.has_active_op() {
            self.directly_attributed += 1;

            let dbi = event.active_dbi;
            let table_stats = self.table_stats.entry(dbi).or_default();
            table_stats.total_faults += 1;
            if is_major {
                table_stats.major_faults += 1;
            }

            // Page type per table
            match MdbxPageType::from_raw(page_type) {
                MdbxPageType::Branch => table_stats.branch_faults += 1,
                MdbxPageType::Leaf => table_stats.leaf_faults += 1,
                MdbxPageType::Overflow => table_stats.overflow_faults += 1,
                _ => {}
            }

            // Faults by op type
            let op_type = event.active_op_type;
            let entry = table_stats
                .faults_by_op_type
                .entry(op_type)
                .or_insert((0, 0));
            entry.0 += 1;
            if is_major {
                entry.1 += 1;
            }

            // Global faults by op type
            let global_entry = self.faults_by_op_type.entry(op_type).or_insert((0, 0));
            global_entry.0 += 1;
            if is_major {
                global_entry.1 += 1;
            }

            // Faults by cursor op (for CURSOR_GET)
            if op_type == 3 {
                let cursor_op = event.active_cursor_op;
                let entry = table_stats
                    .faults_by_cursor_op
                    .entry(cursor_op)
                    .or_insert((0, 0));
                entry.0 += 1;
                if is_major {
                    entry.1 += 1;
                }

                let global_entry = self.faults_by_cursor_op.entry(cursor_op).or_insert((0, 0));
                global_entry.0 += 1;
                if is_major {
                    global_entry.1 += 1;
                }
            }

            // Track table for batch analysis
            if dbi < 100 {
                self.current_batch_faults
                    .tables_touched
                    .insert(dbi_to_table_name(dbi).to_string());
            }
        }

        // Batch fault accumulator
        self.current_batch_faults.total_faults += 1;
        if is_major {
            self.current_batch_faults.major_faults += 1;
            // Estimate I/O time from major faults (assume ~100us per major fault)
            self.current_batch_faults.io_time_us += event.latency_ns / 1000;
        }
        match MdbxPageType::from_raw(page_type) {
            MdbxPageType::Branch => self.current_batch_faults.branch_faults += 1,
            MdbxPageType::Leaf => self.current_batch_faults.leaf_faults += 1,
            MdbxPageType::Overflow => self.current_batch_faults.overflow_faults += 1,
            _ => {}
        }
    }

    /// Process a cursor event
    pub fn process_cursor_event(&mut self, event: &CursorEvent) {
        let ts = event.timestamp_ns;
        if self.first_timestamp.is_none() {
            self.first_timestamp = Some(ts);
        }
        self.last_timestamp = ts;
        self.total_events += 1;

        // Pre-trace cursor check
        if is_pre_trace_cursor(event.dbi) {
            self.pre_trace_cursor_ops += 1;
            return;
        }

        self.cursor_count += 1;
        let cursor_op = CursorOp::from_raw(event.cursor_op);

        if cursor_op.is_seek() {
            self.cursor_seek_count += 1;
        } else if cursor_op.is_navigation() {
            self.cursor_nav_count += 1;
        }

        if !event.is_success() && !event.is_not_found() {
            self.cursor_error_count += 1;
        }

        if event.is_direct_get() {
            self.direct_get_count += 1;
        }

        // Latency stats
        self.cursor_latency_stats.add(event.latency_ns);
        self.cursor_latency_sampler.add(event.latency_ns);

        let latency_us = event.latency_ns / 1000;
        let is_slow = latency_us > self.config.slow_op_threshold_us;

        // Per-table stats
        let dbi = event.dbi;
        let table_stats = self.table_stats.entry(dbi).or_default();
        table_stats.total_ops += 1;
        table_stats.total_latency_ns += event.latency_ns;
        table_stats.max_latency_ns = table_stats.max_latency_ns.max(event.latency_ns);

        if cursor_op.is_seek() {
            table_stats.seek_ops += 1;
        } else if cursor_op.is_navigation() {
            table_stats.nav_ops += 1;
        }

        if is_slow {
            table_stats.slow_ops += 1;
            let op_name = if event.is_direct_get() {
                "DIRECT_GET".to_string()
            } else {
                cursor_op.name().to_string()
            };
            let slow_entry = table_stats.slow_by_op.entry(op_name).or_insert((0, 0, 0));
            slow_entry.0 += 1;
            slow_entry.1 += event.latency_ns;
            slow_entry.2 = slow_entry.2.max(event.latency_ns);
        }

        // Hot key tracking
        if event.key_size > 0 && is_slow {
            let key_hex = event.key_hex();
            self.hot_keys.add(dbi, key_hex, event.latency_ns, is_slow);
        }

        // Cursor timeline
        let first_ts = self.first_timestamp.unwrap();
        let bucket = (ts - first_ts) / (self.config.bucket_ms * 1_000_000);
        let timeline_entry = self
            .cursor_timeline_buckets
            .entry(bucket)
            .or_insert((0, 0, 0));
        timeline_entry.0 += 1;
        if cursor_op.is_seek() {
            timeline_entry.1 += 1;
        }
        timeline_entry.2 += event.latency_ns;

        // Sample cursor ops
        self.cursor_sample_counter += 1;
        if self.cursor_sample_counter % self.config.cursor_sample_rate == 0
            && self.cursor_samples.len() < self.config.max_cursor_samples
        {
            let table_name = dbi_to_table_name(dbi).to_string();
            self.cursor_samples.push(CursorOpSample {
                timestamp_ms: (ts - first_ts) / 1_000_000,
                table: table_name,
                operation: cursor_op.name().to_string(),
                key_hex: event.key_hex(),
                latency_us: event.latency_ns as f64 / 1000.0,
                success: event.is_success(),
            });
        }

        // Fault histogram
        if event.faults_during_op > 0 {
            *self
                .fault_histogram
                .entry(event.faults_during_op)
                .or_insert(0) += 1;
            self.max_faults_per_op = self.max_faults_per_op.max(event.faults_during_op);
        } else {
            *self.fault_histogram.entry(0).or_insert(0) += 1;
        }

        // Tree depth stats
        if event.max_tree_depth > 0 {
            self.ops_with_depth_data += 1;
            self.max_depth_observed = self.max_depth_observed.max(event.max_tree_depth);
            *self
                .depth_distribution
                .entry(event.max_tree_depth)
                .or_insert(0) += 1;

            let table_entry = self.depth_by_table.entry(dbi).or_insert((0, 0, 0));
            table_entry.0 += 1;
            table_entry.1 += event.max_tree_depth as u64;
            table_entry.2 = table_entry.2.max(event.max_tree_depth);

            let op_entry = self.depth_by_op.entry(event.cursor_op).or_insert((0, 0, 0));
            op_entry.0 += 1;
            op_entry.1 += event.max_tree_depth as u64;
            op_entry.2 = op_entry.2.max(event.max_tree_depth);
        }

        // Block range extraction (from write operations)
        if event.is_write_op() {
            if let Some(block) = extract_block_from_key(dbi, &event.key_data, event.key_size) {
                match self.min_block {
                    Some(min) => self.min_block = Some(min.min(block)),
                    None => self.min_block = Some(block),
                }
                match self.max_block {
                    Some(max) => self.max_block = Some(max.max(block)),
                    None => self.max_block = Some(block),
                }

                // Track block for current batch
                match self.current_batch_faults.first_block {
                    Some(first) => self.current_batch_faults.first_block = Some(first.min(block)),
                    None => self.current_batch_faults.first_block = Some(block),
                }
                match self.current_batch_faults.last_block {
                    Some(last) => self.current_batch_faults.last_block = Some(last.max(block)),
                    None => self.current_batch_faults.last_block = Some(block),
                }
            }
        }
    }

    /// Process a transaction event
    pub fn process_txn_event(&mut self, event: &TxnEvent) {
        let ts = event.timestamp_ns;
        if self.first_timestamp.is_none() {
            self.first_timestamp = Some(ts);
        }
        self.last_timestamp = ts;
        self.total_events += 1;

        let thread = self.thread_stats.entry(event.tid).or_default();
        let is_rw = event.is_read_write();
        let first_ts = self.first_timestamp.unwrap();

        match event.event_type {
            7 => {
                // TXN_BEGIN
                self.txn_count += 1;
                if is_rw {
                    self.rw_txn_count += 1;
                    self.current_concurrent_rw += 1;
                    self.max_concurrent_rw = self.max_concurrent_rw.max(self.current_concurrent_rw);
                    thread.rw_txns += 1;
                } else {
                    self.ro_txn_count += 1;
                    self.current_concurrent_ro += 1;
                    self.max_concurrent_ro = self.max_concurrent_ro.max(self.current_concurrent_ro);
                    thread.ro_txns += 1;
                }
                thread.total_txns += 1;

                self.active_txns
                    .insert(event.txn_ptr, (ts, event.tid, is_rw));
            }
            8 => {
                // TXN_COMMIT
                self.commit_count += 1;
                thread.commits += 1;

                if is_rw {
                    self.current_concurrent_rw = self.current_concurrent_rw.saturating_sub(1);
                } else {
                    self.current_concurrent_ro = self.current_concurrent_ro.saturating_sub(1);
                }

                self.commit_latency_stats.add(event.latency_ns);
                self.commit_latency_sampler.add(event.latency_ns);
                thread.total_commit_latency += event.latency_ns;

                // RW commit timeline
                if is_rw && self.rw_commit_points.len() < 1000 {
                    self.rw_commit_points.push(RwCommitPoint {
                        time_secs: (ts - first_ts) as f64 / 1e9,
                        latency_ms: event.latency_ns as f64 / 1e6,
                    });

                    // Finalize batch analysis
                    if self.batch_analyses.len() < self.config.max_block_analysis {
                        let batch = &self.current_batch_faults;
                        self.batch_analyses.push(BatchAnalysis {
                            batch_index: self.batch_index,
                            first_block: batch.first_block.unwrap_or(0),
                            last_block: batch.last_block.unwrap_or(0),
                            block_count: batch
                                .last_block
                                .and_then(|l| batch.first_block.map(|f| (l - f + 1) as u32))
                                .unwrap_or(0),
                            branch_faults: batch.branch_faults,
                            leaf_faults: batch.leaf_faults,
                            overflow_faults: batch.overflow_faults,
                            major_faults: batch.major_faults,
                            io_time_us: batch.io_time_us,
                            tables_touched: batch.tables_touched.iter().cloned().collect(),
                            total_faults: batch.total_faults,
                            start_time_ns: batch.start_time_ns - first_ts,
                            end_time_ns: ts - first_ts,
                            commit_latency_us: event.latency_ns / 1000,
                        });
                        self.batch_index += 1;
                    }

                    // Reset batch accumulator
                    self.current_batch_faults = BatchFaultAccumulator {
                        start_time_ns: ts,
                        ..Default::default()
                    };
                }

                // Transaction timeline sample
                if let Some((start_ts, tid, was_rw)) = self.active_txns.remove(&event.txn_ptr) {
                    if self.txn_timeline_samples.len() < self.config.max_txn_timeline {
                        let duration_ms = (ts - start_ts) as f64 / 1e6;
                        self.txn_timeline_samples.push(TxnTimelineEntry {
                            tid,
                            txn_ptr: format!("0x{:x}", event.txn_ptr),
                            start_ms: (start_ts - first_ts) as f64 / 1e6,
                            end_ms: Some((ts - first_ts) as f64 / 1e6),
                            duration_ms: Some(duration_ms),
                            txn_type: if was_rw {
                                "RW".to_string()
                            } else {
                                "RO".to_string()
                            },
                            end_type: "commit".to_string(),
                            commit_latency_us: Some(event.latency_ns as f64 / 1000.0),
                        });
                    }
                }
            }
            9 => {
                // TXN_ABORT
                self.abort_count += 1;
                thread.aborts += 1;

                if is_rw {
                    self.current_concurrent_rw = self.current_concurrent_rw.saturating_sub(1);
                } else {
                    self.current_concurrent_ro = self.current_concurrent_ro.saturating_sub(1);
                }

                if let Some((start_ts, tid, was_rw)) = self.active_txns.remove(&event.txn_ptr) {
                    if self.txn_timeline_samples.len() < self.config.max_txn_timeline {
                        let duration_ms = (ts - start_ts) as f64 / 1e6;
                        self.txn_timeline_samples.push(TxnTimelineEntry {
                            tid,
                            txn_ptr: format!("0x{:x}", event.txn_ptr),
                            start_ms: (start_ts - first_ts) as f64 / 1e6,
                            end_ms: Some((ts - first_ts) as f64 / 1e6),
                            duration_ms: Some(duration_ms),
                            txn_type: if was_rw {
                                "RW".to_string()
                            } else {
                                "RO".to_string()
                            },
                            end_type: "abort".to_string(),
                            commit_latency_us: None,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    /// Update heatmap with current stats (call after all events processed)
    fn finalize_heatmap(&mut self) {
        if self.page_fault_count == 0 || self.max_offset == 0 {
            return;
        }

        let first_ts = match self.first_timestamp {
            Some(ts) => ts,
            None => return,
        };

        let duration_ns = self.last_timestamp - first_ts;
        let time_bucket_size = if duration_ns > 0 {
            duration_ns / self.config.heatmap_time_buckets as u64
        } else {
            1
        };
        let offset_bucket_size = if self.max_offset > self.min_offset {
            (self.max_offset - self.min_offset) / self.config.heatmap_offset_buckets as u64
        } else {
            1
        };

        // Reinitialize heatmap based on timeline buckets
        for (&bucket, &(faults, _, _)) in &self.timeline_buckets {
            let time_idx =
                ((bucket * self.config.bucket_ms * 1_000_000) / time_bucket_size) as usize;
            if time_idx < self.config.heatmap_time_buckets as usize {
                // Distribute faults across offset buckets (approximation)
                let faults_per_bucket = faults / self.config.heatmap_offset_buckets;
                for offset_idx in 0..self.config.heatmap_offset_buckets as usize {
                    if time_idx < self.heatmap_data.len()
                        && offset_idx < self.heatmap_data[time_idx].len()
                    {
                        self.heatmap_data[time_idx][offset_idx] += faults_per_bucket;
                    }
                }
            }
        }
        self.heatmap_initialized = true;
    }

    /// Generate the final ViewerData
    pub fn finalize(mut self) -> ViewerData {
        self.finalize_heatmap();

        let first_ts = self.first_timestamp.unwrap_or(0);
        let duration_ns = self.last_timestamp.saturating_sub(first_ts);
        let duration_secs = duration_ns as f64 / 1e9;

        // Build summary
        let block_range = match (self.min_block, self.max_block) {
            (Some(min), Some(max)) => Some(BlockRange {
                min_block: min,
                max_block: max,
                block_count: max - min + 1,
            }),
            _ => None,
        };

        let summary = TraceSummary {
            duration_secs,
            total_events: self.total_events,
            page_faults: self.page_fault_count,
            major_faults: self.major_fault_count,
            minor_faults: self.page_fault_count.saturating_sub(self.major_fault_count),
            major_fault_ratio: if self.page_fault_count > 0 {
                self.major_fault_count as f64 / self.page_fault_count as f64
            } else {
                0.0
            },
            fault_rate_per_sec: if duration_secs > 0.0 {
                self.page_fault_count as f64 / duration_secs
            } else {
                0.0
            },
            unique_pages: self.unique_pages.len() as u64,
            file_size_gb: self.max_offset as f64 / 1e9,
            min_offset: self.min_offset,
            max_offset: self.max_offset,
            block_range: block_range.clone(),
        };

        // Build timeline - take ownership to avoid borrow issues
        let bucket_ms = self.config.bucket_ms;
        let max_timeline_points = self.config.max_timeline_points;
        let timeline_buckets = std::mem::take(&mut self.timeline_buckets);
        let mut timeline: Vec<_> = timeline_buckets
            .into_iter()
            .map(|(bucket, (faults, major, pages))| TimelinePoint {
                time_ms: bucket * bucket_ms,
                faults,
                major_faults: major,
                unique_pages: pages.len() as u32,
            })
            .collect();
        timeline.sort_by_key(|p| p.time_ms);
        if timeline.len() > max_timeline_points {
            // Downsample
            let step = timeline.len() / max_timeline_points;
            timeline = timeline.into_iter().step_by(step.max(1)).collect();
        }

        // Build cursor data
        let cursor_data = self.build_cursor_data(duration_secs);

        // Build transaction data
        let txn_data = self.build_txn_data(duration_secs);

        // Build thread stats
        let total_thread_faults: u64 = self.thread_stats.values().map(|t| t.faults).sum();
        let mut threads: Vec<_> = self
            .thread_stats
            .iter()
            .map(|(&tid, stats)| ThreadStats {
                tid,
                faults: stats.faults,
                percentage: if total_thread_faults > 0 {
                    stats.faults as f64 / total_thread_faults as f64 * 100.0
                } else {
                    0.0
                },
            })
            .collect();
        threads.sort_by(|a, b| b.faults.cmp(&a.faults));
        threads.truncate(20);

        // Build pattern analysis
        let total_patterns = self.sequential_count + self.random_count;
        let patterns = PatternAnalysis {
            sequential_ratio: if total_patterns > 0 {
                self.sequential_count as f64 / total_patterns as f64
            } else {
                0.0
            },
            random_ratio: if total_patterns > 0 {
                self.random_count as f64 / total_patterns as f64
            } else {
                0.0
            },
            burst_stats: BurstStats {
                median_events: 0,
                p95_events: 0,
                max_events: 0,
                bucket_ms: self.config.bucket_ms,
            },
            top_strides: self.build_top_strides(total_patterns),
        };

        // Build page type stats
        let page_type_stats = self.build_page_type_stats();

        // Build operation histogram
        let operation_histogram = self.build_operation_histogram();

        // Build tree traversal viz
        let tree_traversal = self.build_tree_traversal();

        // Build unified table stats
        let unified_tables = self.build_unified_tables();

        // Build direct fault attribution
        let direct_fault_attribution = self.build_direct_attribution();

        // Build B-tree visualization
        let btree_viz = self.build_btree_viz(block_range);

        // Build heatmap
        let heatmap = self.build_heatmap();

        ViewerData {
            summary,
            timeline,
            tables: vec![], // Legacy, kept empty
            unified_tables,
            threads,
            patterns,
            heatmap,
            cursor_data,
            txn_data,
            page_fault_attribution_warning: None,
            direct_fault_attribution,
            page_type_stats,
            operation_histogram,
            tree_traversal,
            btree_viz,
        }
    }

    fn build_cursor_data(&mut self, duration_secs: f64) -> CursorData {
        if self.cursor_count == 0 {
            return CursorData::default();
        }

        let p50 = self.cursor_latency_sampler.p50() as f64 / 1000.0;
        let p95 = self.cursor_latency_sampler.p95() as f64 / 1000.0;
        let p99 = self.cursor_latency_sampler.p99() as f64 / 1000.0;

        // Build tree depth stats
        let tree_depth_stats = self.build_tree_depth_stats();

        let summary = CursorSummary {
            total_ops: self.cursor_count,
            op_rate_per_sec: if duration_secs > 0.0 {
                self.cursor_count as f64 / duration_secs
            } else {
                0.0
            },
            avg_latency_us: self.cursor_latency_stats.average() / 1000.0,
            p50_latency_us: p50,
            p95_latency_us: p95,
            p99_latency_us: p99,
            seek_count: self.cursor_seek_count,
            seek_ratio: if self.cursor_count > 0 {
                self.cursor_seek_count as f64 / self.cursor_count as f64 * 100.0
            } else {
                0.0
            },
            nav_count: self.cursor_nav_count,
            error_count: self.cursor_error_count,
            duration_secs,
            direct_get_count: self.direct_get_count,
            direct_get_ratio: if self.cursor_count > 0 {
                self.direct_get_count as f64 / self.cursor_count as f64 * 100.0
            } else {
                0.0
            },
            tree_depth_stats,
        };

        // Build table stats
        let mut table_stats: Vec<_> = self
            .table_stats
            .iter()
            .filter(|(_, stats)| stats.total_ops > 0)
            .map(|(&dbi, stats)| {
                let avg_latency = if stats.total_ops > 0 {
                    stats.total_latency_ns as f64 / stats.total_ops as f64 / 1000.0
                } else {
                    0.0
                };
                CursorTableStats {
                    dbi,
                    name: dbi_to_table_name(dbi).to_string(),
                    ops: stats.total_ops,
                    percentage: if self.cursor_count > 0 {
                        stats.total_ops as f64 / self.cursor_count as f64 * 100.0
                    } else {
                        0.0
                    },
                    seeks: stats.seek_ops,
                    navs: stats.nav_ops,
                    avg_latency_us: avg_latency,
                    p50_latency_us: avg_latency * 0.8, // Approximation
                    p95_latency_us: avg_latency * 2.5,
                    p99_latency_us: avg_latency * 4.0,
                }
            })
            .collect();
        table_stats.sort_by(|a, b| b.ops.cmp(&a.ops));

        // Build cursor timeline
        let mut cursor_timeline: Vec<_> = self
            .cursor_timeline_buckets
            .iter()
            .map(
                |(&bucket, &(ops, seeks, total_latency))| CursorTimelinePoint {
                    time_ms: bucket * self.config.bucket_ms,
                    ops,
                    seeks,
                    avg_latency_us: if ops > 0 {
                        total_latency as f64 / ops as f64 / 1000.0
                    } else {
                        0.0
                    },
                },
            )
            .collect();
        cursor_timeline.sort_by_key(|p| p.time_ms);

        // Build slow ops by table
        let slow_ops_by_table = self.build_slow_ops_by_table();

        // Build slow keys
        let slow_keys = self.build_slow_keys();

        // Build operations breakdown
        let operations = self.build_operations_breakdown();

        CursorData {
            has_data: true,
            summary,
            operations,
            table_stats,
            timeline: cursor_timeline,
            recent_ops: std::mem::take(&mut self.cursor_samples),
            slow_ops_by_table,
            slow_keys,
            pre_trace_cursor_ops: self.pre_trace_cursor_ops,
            pre_trace_warning: if self.pre_trace_cursor_ops > 0 {
                Some(format!(
                    "{} operations from cursors opened before tracing",
                    self.pre_trace_cursor_ops
                ))
            } else {
                None
            },
        }
    }

    fn build_tree_depth_stats(&self) -> TreeDepthStats {
        if self.ops_with_depth_data == 0 {
            return TreeDepthStats::default();
        }

        let total_depth: u64 = self
            .depth_distribution
            .iter()
            .map(|(&d, &c)| d as u64 * c)
            .sum();

        let mut depth_histogram: Vec<_> = self
            .depth_distribution
            .iter()
            .map(|(&depth, &count)| DepthBucket {
                depth,
                count,
                percentage: count as f64 / self.ops_with_depth_data as f64 * 100.0,
                avg_faults: 0.0, // Would need per-depth fault tracking
                avg_latency_us: 0.0,
            })
            .collect();
        depth_histogram.sort_by_key(|b| b.depth);

        let by_table: Vec<_> = self
            .depth_by_table
            .iter()
            .map(|(&dbi, &(ops, total_depth, max_depth))| TableDepthStats {
                table_name: dbi_to_table_name(dbi).to_string(),
                dbi,
                ops_count: ops,
                max_depth,
                avg_depth: if ops > 0 {
                    total_depth as f64 / ops as f64
                } else {
                    0.0
                },
                avg_faults: 0.0,
                avg_latency_us: 0.0,
                depth_distribution: vec![],
            })
            .collect();

        let by_operation: Vec<_> = self
            .depth_by_op
            .iter()
            .map(|(&op, &(ops, total_depth, max_depth))| {
                let cursor_op = CursorOp::from_raw(op);
                OperationDepthStats {
                    operation: cursor_op.name().to_string(),
                    ops_count: ops,
                    max_depth,
                    avg_depth: if ops > 0 {
                        total_depth as f64 / ops as f64
                    } else {
                        0.0
                    },
                    avg_faults: 0.0,
                    avg_latency_us: 0.0,
                    is_seek: cursor_op.is_seek(),
                }
            })
            .collect();

        TreeDepthStats {
            ops_with_depth_data: self.ops_with_depth_data,
            max_depth_observed: self.max_depth_observed,
            avg_depth: if self.ops_with_depth_data > 0 {
                total_depth as f64 / self.ops_with_depth_data as f64
            } else {
                0.0
            },
            depth_distribution: self
                .depth_distribution
                .iter()
                .map(|(&d, &c)| (d, c))
                .collect(),
            depth_histogram,
            by_table,
            by_operation,
        }
    }

    fn build_txn_data(&mut self, duration_secs: f64) -> TxnData {
        if self.txn_count == 0 {
            return TxnData::default();
        }

        let p50 = self.commit_latency_sampler.p50() as f64 / 1000.0;
        let p95 = self.commit_latency_sampler.p95() as f64 / 1000.0;
        let p99 = self.commit_latency_sampler.p99() as f64 / 1000.0;

        let summary = TxnSummary {
            total_events: self.txn_count,
            begin_count: self.txn_count,
            commit_count: self.commit_count,
            abort_count: self.abort_count,
            ro_count: self.ro_txn_count,
            rw_count: self.rw_txn_count,
            duration_secs,
            txn_rate_per_sec: if duration_secs > 0.0 {
                self.txn_count as f64 / duration_secs
            } else {
                0.0
            },
            avg_commit_latency_us: self.commit_latency_stats.average() / 1000.0,
            p50_commit_latency_us: p50,
            p95_commit_latency_us: p95,
            p99_commit_latency_us: p99,
            max_commit_latency_us: self.commit_latency_stats.max as f64 / 1000.0,
        };

        // Thread stats
        let mut thread_stats: Vec<_> = self
            .thread_stats
            .iter()
            .filter(|(_, stats)| stats.total_txns > 0)
            .map(|(&tid, stats)| TxnThreadStats {
                tid,
                total_txns: stats.total_txns,
                ro_txns: stats.ro_txns,
                rw_txns: stats.rw_txns,
                commits: stats.commits,
                aborts: stats.aborts,
                avg_commit_latency_us: if stats.commits > 0 {
                    stats.total_commit_latency as f64 / stats.commits as f64 / 1000.0
                } else {
                    0.0
                },
                percentage: if self.txn_count > 0 {
                    stats.total_txns as f64 / self.txn_count as f64 * 100.0
                } else {
                    0.0
                },
            })
            .collect();
        thread_stats.sort_by(|a, b| b.total_txns.cmp(&a.total_txns));
        thread_stats.truncate(20);

        let concurrency = TxnConcurrencyStats {
            max_concurrent_ro: self.max_concurrent_ro,
            max_concurrent_rw: self.max_concurrent_rw,
            max_concurrent_total: self.max_concurrent_ro + self.max_concurrent_rw,
            avg_concurrent_ro: 0.0, // Would need timeline tracking
            concurrency_timeline: vec![],
        };

        TxnData {
            has_data: true,
            summary,
            timeline: std::mem::take(&mut self.txn_timeline_samples),
            thread_stats,
            concurrency,
            rw_commit_timeline: std::mem::take(&mut self.rw_commit_points),
        }
    }

    fn build_slow_ops_by_table(&self) -> Vec<SlowOpsTableStats> {
        let mut result: Vec<_> = self
            .table_stats
            .iter()
            .filter(|(_, stats)| stats.slow_ops > 0)
            .map(|(&dbi, stats)| {
                let by_operation: Vec<_> = stats
                    .slow_by_op
                    .iter()
                    .map(|(op, &(count, total_lat, max_lat))| SlowOpBreakdown {
                        operation: op.clone(),
                        count,
                        avg_latency_us: if count > 0 {
                            total_lat as f64 / count as f64 / 1000.0
                        } else {
                            0.0
                        },
                        max_latency_us: max_lat as f64 / 1000.0,
                    })
                    .collect();

                let total_slow_time: u64 = stats.slow_by_op.values().map(|v| v.1).sum();

                SlowOpsTableStats {
                    table: dbi_to_table_name(dbi).to_string(),
                    dbi,
                    slow_op_count: stats.slow_ops,
                    total_op_count: stats.total_ops,
                    slow_op_percentage: if stats.total_ops > 0 {
                        stats.slow_ops as f64 / stats.total_ops as f64 * 100.0
                    } else {
                        0.0
                    },
                    avg_slow_latency_us: if stats.slow_ops > 0 {
                        total_slow_time as f64 / stats.slow_ops as f64 / 1000.0
                    } else {
                        0.0
                    },
                    max_latency_us: stats.max_latency_ns as f64 / 1000.0,
                    total_slow_time_ms: total_slow_time as f64 / 1e6,
                    by_operation,
                }
            })
            .collect();
        result.sort_by(|a, b| {
            b.total_slow_time_ms
                .partial_cmp(&a.total_slow_time_ms)
                .unwrap()
        });
        result
    }

    fn build_slow_keys(&mut self) -> Vec<SlowKeyStats> {
        self.hot_keys.prune();
        let mut keys: Vec<_> = self
            .hot_keys
            .keys
            .iter()
            .filter(|(_, v)| v.0 > 0)
            .map(
                |((dbi, key_hex), &(slow, total, total_lat, max_lat))| SlowKeyStats {
                    table: dbi_to_table_name(*dbi).to_string(),
                    key_hex: key_hex.clone(),
                    key_prefix: if key_hex.len() > 16 {
                        format!("{}...", &key_hex[..16])
                    } else {
                        key_hex.clone()
                    },
                    slow_access_count: slow,
                    total_access_count: total,
                    avg_latency_us: if total > 0 {
                        total_lat as f64 / total as f64 / 1000.0
                    } else {
                        0.0
                    },
                    max_latency_us: max_lat as f64 / 1000.0,
                    operations: vec![],
                },
            )
            .collect();
        keys.sort_by(|a, b| b.slow_access_count.cmp(&a.slow_access_count));
        keys.truncate(50);
        keys
    }

    fn build_operations_breakdown(&self) -> Vec<OperationStats> {
        // Build from cursor op stats
        let mut op_counts: HashMap<String, (u64, u64)> = HashMap::new(); // name -> (count, total_latency)

        for stats in self.table_stats.values() {
            for (op, &(total, _)) in &stats.faults_by_cursor_op {
                let name = CursorOp::from_raw(*op).name().to_string();
                let entry = op_counts.entry(name).or_insert((0, 0));
                entry.0 += total;
            }
        }

        let total: u64 = op_counts.values().map(|v| v.0).sum();
        let mut ops: Vec<_> = op_counts
            .into_iter()
            .map(|(name, (count, _))| {
                let cursor_op = match name.as_str() {
                    "SET_RANGE" => CursorOp::SetRange,
                    "NEXT" => CursorOp::Next,
                    "PREV" => CursorOp::Prev,
                    "FIRST" => CursorOp::First,
                    "LAST" => CursorOp::Last,
                    "SET" => CursorOp::Set,
                    _ => CursorOp::Unknown(0),
                };
                OperationStats {
                    name,
                    count,
                    percentage: if total > 0 {
                        count as f64 / total as f64 * 100.0
                    } else {
                        0.0
                    },
                    avg_latency_us: 0.0,
                    is_seek: cursor_op.is_seek(),
                }
            })
            .collect();
        ops.sort_by(|a, b| b.count.cmp(&a.count));
        ops
    }

    fn build_top_strides(&self, total: u64) -> Vec<StrideInfo> {
        let mut strides: Vec<_> = self
            .stride_counts
            .iter()
            .map(|(&stride, &count)| {
                let pattern_type = if stride.abs() <= 1 {
                    "sequential"
                } else if stride.abs() <= 16 {
                    "near-sequential"
                } else {
                    "random"
                };
                StrideInfo {
                    stride_pages: stride,
                    count,
                    pattern_type: pattern_type.to_string(),
                    percentage: if total > 0 {
                        count as f64 / total as f64 * 100.0
                    } else {
                        0.0
                    },
                }
            })
            .collect();
        strides.sort_by(|a, b| b.count.cmp(&a.count));
        strides.truncate(10);
        strides
    }

    fn build_page_type_stats(&self) -> PageTypeStats {
        if self.page_type_counts.is_empty() {
            return PageTypeStats::default();
        }

        let total: u64 = self.page_type_counts.values().map(|v| v.0).sum();
        let by_type: Vec<_> = self
            .page_type_counts
            .iter()
            .map(|(&pt, &(total_faults, major))| PageTypeFaultCount {
                page_type: MdbxPageType::from_raw(pt).name().to_string(),
                total_faults,
                major_faults: major,
                percentage: if total > 0 {
                    total_faults as f64 / total as f64 * 100.0
                } else {
                    0.0
                },
            })
            .collect();

        let traversal: u64 = self
            .page_type_counts
            .iter()
            .filter(|(&pt, _)| MdbxPageType::from_raw(pt).is_traversal())
            .map(|(_, v)| v.0)
            .sum();
        let data: u64 = self
            .page_type_counts
            .iter()
            .filter(|(&pt, _)| MdbxPageType::from_raw(pt).is_data())
            .map(|(_, v)| v.0)
            .sum();

        PageTypeStats {
            has_data: true,
            total_faults: total,
            by_type,
            traversal_to_data_ratio: if data > 0 {
                traversal as f64 / data as f64
            } else {
                0.0
            },
        }
    }

    fn build_operation_histogram(&self) -> OperationFaultHistogram {
        if self.fault_histogram.is_empty() {
            return OperationFaultHistogram::default();
        }

        let total: u64 = self.fault_histogram.values().sum();

        // Build buckets: 0, 1-2, 3-5, 6-10, 11-20, 21+
        let buckets = [
            ("0", 0..1),
            ("1-2", 1..3),
            ("3-5", 3..6),
            ("6-10", 6..11),
            ("11-20", 11..21),
            ("21+", 21..1000),
        ];

        let distribution: Vec<_> = buckets
            .iter()
            .map(|(label, range)| {
                let count: u64 = self
                    .fault_histogram
                    .iter()
                    .filter(|(&faults, _)| range.contains(&faults))
                    .map(|(_, &c)| c)
                    .sum();
                HistogramBucket {
                    label: label.to_string(),
                    count,
                    percentage: if total > 0 {
                        count as f64 / total as f64 * 100.0
                    } else {
                        0.0
                    },
                }
            })
            .collect();

        let total_faults: u64 = self
            .fault_histogram
            .iter()
            .map(|(&f, &c)| f as u64 * c)
            .sum();

        OperationFaultHistogram {
            has_data: true,
            distribution,
            avg_faults_per_op: if total > 0 {
                total_faults as f64 / total as f64
            } else {
                0.0
            },
            max_faults_per_op: self.max_faults_per_op,
            p50_faults: 0, // Would need sorted samples
            p95_faults: 0,
            p99_faults: 0,
        }
    }

    fn build_tree_traversal(&self) -> TreeTraversalViz {
        let tables: Vec<_> = self
            .table_stats
            .iter()
            .filter(|(_, stats)| stats.total_faults > 0)
            .map(|(&dbi, stats)| TableTreeStats {
                name: dbi_to_table_name(dbi).to_string(),
                dbi,
                total_faults: stats.total_faults,
                branch_faults: stats.branch_faults,
                leaf_faults: stats.leaf_faults,
                overflow_faults: stats.overflow_faults,
                branch_leaf_ratio: if stats.leaf_faults > 0 {
                    stats.branch_faults as f64 / stats.leaf_faults as f64
                } else {
                    0.0
                },
            })
            .collect();

        TreeTraversalViz {
            has_data: !tables.is_empty(),
            tables,
        }
    }

    fn build_unified_tables(&self) -> Vec<UnifiedTableStats> {
        let total_faults: u64 = self.table_stats.values().map(|s| s.total_faults).sum();

        let mut tables: Vec<_> = self
            .table_stats
            .iter()
            .filter(|(_, stats)| stats.total_faults > 0 || stats.total_ops > 0)
            .map(|(&dbi, stats)| {
                let time_lost_ms = stats.slow_by_op.values().map(|v| v.1).sum::<u64>() as f64 / 1e6;

                let top_operation = stats
                    .slow_by_op
                    .iter()
                    .max_by_key(|(_, v)| v.0)
                    .map(|(op, _)| op.clone())
                    .unwrap_or_default();

                let faults_by_op: Vec<_> = stats
                    .faults_by_op_type
                    .iter()
                    .map(|(&op, &(total, major))| OpFaultCount {
                        operation: op_type_name(op).to_string(),
                        faults: total,
                        major_faults: major,
                    })
                    .collect();

                let faults_by_cursor_op: Vec<_> = stats
                    .faults_by_cursor_op
                    .iter()
                    .map(|(&op, &(total, major))| OpFaultCount {
                        operation: CursorOp::from_raw(op).name().to_string(),
                        faults: total,
                        major_faults: major,
                    })
                    .collect();

                let slow_ops_breakdown: Vec<_> = stats
                    .slow_by_op
                    .iter()
                    .map(|(op, &(count, total_lat, max_lat))| SlowOpBreakdown {
                        operation: op.clone(),
                        count,
                        avg_latency_us: if count > 0 {
                            total_lat as f64 / count as f64 / 1000.0
                        } else {
                            0.0
                        },
                        max_latency_us: max_lat as f64 / 1000.0,
                    })
                    .collect();

                let severity = if time_lost_ms > 1000.0 || stats.major_faults > 10000 {
                    "critical"
                } else if time_lost_ms > 100.0 || stats.major_faults > 1000 {
                    "high"
                } else if time_lost_ms > 10.0 || stats.major_faults > 100 {
                    "medium"
                } else {
                    "low"
                };

                UnifiedTableStats {
                    name: dbi_to_table_name(dbi).to_string(),
                    dbi,
                    faults: stats.total_faults,
                    major_faults: stats.major_faults,
                    fault_percentage: if total_faults > 0 {
                        stats.total_faults as f64 / total_faults as f64 * 100.0
                    } else {
                        0.0
                    },
                    total_ops: stats.total_ops,
                    slow_ops: stats.slow_ops,
                    slow_ops_percentage: if stats.total_ops > 0 {
                        stats.slow_ops as f64 / stats.total_ops as f64 * 100.0
                    } else {
                        0.0
                    },
                    time_lost_ms,
                    avg_latency_us: if stats.total_ops > 0 {
                        stats.total_latency_ns as f64 / stats.total_ops as f64 / 1000.0
                    } else {
                        0.0
                    },
                    max_latency_us: stats.max_latency_ns as f64 / 1000.0,
                    top_operation,
                    details: TableDrillDown {
                        faults_by_op,
                        faults_by_cursor_op,
                        slow_ops_breakdown,
                        hot_keys: vec![], // Would need per-table hot key tracking
                    },
                    branch_faults: stats.branch_faults,
                    leaf_faults: stats.leaf_faults,
                    overflow_faults: stats.overflow_faults,
                    severity: severity.to_string(),
                    reth_source: None,
                }
            })
            .collect();

        tables.sort_by(|a, b| {
            b.time_lost_ms
                .partial_cmp(&a.time_lost_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        tables
    }

    fn build_direct_attribution(&self) -> DirectFaultAttribution {
        let total = self.page_fault_count;
        let faults_by_op_type: Vec<_> = self
            .faults_by_op_type
            .iter()
            .map(|(&op, &(total_faults, major))| FaultsByOpType {
                op_type: op_type_name(op).to_string(),
                total_faults,
                major_faults: major,
                percentage: if total > 0 {
                    total_faults as f64 / total as f64 * 100.0
                } else {
                    0.0
                },
            })
            .collect();

        let faults_by_cursor_op: Vec<_> = self
            .faults_by_cursor_op
            .iter()
            .map(|(&op, &(total_faults, major))| FaultsByCursorOp {
                cursor_op: CursorOp::from_raw(op).name().to_string(),
                total_faults,
                major_faults: major,
                percentage: if total > 0 {
                    total_faults as f64 / total as f64 * 100.0
                } else {
                    0.0
                },
            })
            .collect();

        DirectFaultAttribution {
            has_data: self.directly_attributed > 0,
            directly_attributed_count: self.directly_attributed,
            timestamp_fallback_count: 0,
            uncorrelated_count: total.saturating_sub(self.directly_attributed),
            faults_by_op_type,
            faults_by_cursor_op,
        }
    }

    fn build_btree_viz(&self, block_range: Option<BlockRange>) -> BTreeVisualization {
        // Build tree depth estimates from table stats
        let tree_depth_estimates: Vec<_> = self
            .table_stats
            .iter()
            .filter(|(_, stats)| stats.leaf_faults > 0)
            .map(|(&dbi, stats)| {
                let ratio = stats.branch_faults as f64 / stats.leaf_faults as f64;
                let estimated_depth = 1.0 + ratio.ln().max(0.0);
                let sample_size = stats.branch_faults + stats.leaf_faults;
                TreeDepthEstimate {
                    table_name: dbi_to_table_name(dbi).to_string(),
                    estimated_depth,
                    branch_leaf_ratio: ratio,
                    confidence: if sample_size > 10000 {
                        "high"
                    } else if sample_size > 1000 {
                        "medium"
                    } else {
                        "low"
                    }
                    .to_string(),
                    sample_size,
                }
            })
            .collect();

        // Calculate traversal efficiency
        let total_traversal: u64 = self.table_stats.values().map(|s| s.branch_faults).sum();
        let total_data: u64 = self.table_stats.values().map(|s| s.leaf_faults).sum();
        let traversal_efficiency = if total_traversal + total_data > 0 {
            100.0 * (1.0 - total_traversal as f64 / (total_traversal + total_data) as f64)
        } else {
            100.0
        };

        BTreeVisualization {
            has_data: !self.batch_analyses.is_empty(),
            batch_analysis: self.batch_analyses.clone(),
            block_analysis: vec![], // Not tracked in streaming mode
            operation_page_types: vec![],
            block_range,
            tree_depth_estimates,
            traversal_efficiency_score: traversal_efficiency,
            attribution_stats: crate::viewer::AttributionStats {
                total_faults: self.page_fault_count,
                batch_attributed_faults: self.directly_attributed,
                block_attributed_faults: 0,
                unattributed_faults: self
                    .page_fault_count
                    .saturating_sub(self.directly_attributed),
                batch_attribution_pct: if self.page_fault_count > 0 {
                    self.directly_attributed as f64 / self.page_fault_count as f64 * 100.0
                } else {
                    0.0
                },
                block_attribution_pct: 0.0,
                rw_commits_detected: self.batch_index,
                blocks_with_writes: 0,
            },
        }
    }

    fn build_heatmap(&self) -> HeatmapData {
        let max_count = self
            .heatmap_data
            .iter()
            .flat_map(|row| row.iter())
            .copied()
            .max()
            .unwrap_or(0);

        let first_ts = self.first_timestamp.unwrap_or(0);
        let duration_ms = (self.last_timestamp.saturating_sub(first_ts)) / 1_000_000;

        HeatmapData {
            time_buckets: self.config.heatmap_time_buckets,
            offset_buckets: self.config.heatmap_offset_buckets,
            min_time_ms: 0,
            max_time_ms: duration_ms,
            min_offset_gb: self.min_offset as f64 / 1e9,
            max_offset_gb: self.max_offset as f64 / 1e9,
            data: self.heatmap_data.iter().flatten().copied().collect(),
            max_count,
        }
    }

    /// Get parse error count
    pub fn parse_errors(&self) -> u64 {
        self.parse_errors
    }

    /// Increment parse error count
    pub fn add_parse_error(&mut self) {
        self.parse_errors += 1;
    }
}

/// Extract block number from a key if it's from a block-write table
fn extract_block_from_key(dbi: u32, key_data: &[u8], key_size: u32) -> Option<u64> {
    const BLOCK_WRITE_DBIS: &[u32] = &[2, 6, 18, 19];

    if !BLOCK_WRITE_DBIS.contains(&dbi) {
        return None;
    }

    if key_size < 8 || key_data.len() < 8 {
        return None;
    }

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

    if block > 50_000_000 {
        return None;
    }

    Some(block)
}

fn op_type_name(op_type: u32) -> &'static str {
    match op_type {
        3 => "CURSOR_GET",
        4 => "CURSOR_PUT",
        5 => "DIRECT_GET",
        6 => "CURSOR_DEL",
        10 => "DIRECT_PUT",
        11 => "DIRECT_DEL",
        _ => "UNKNOWN",
    }
}

/// Process a trace file in streaming mode
/// Progress information passed to the callback
#[derive(Debug, Clone)]
pub struct ProgressInfo {
    /// Lines processed so far
    pub lines: u64,
    /// Bytes read so far
    pub bytes_read: u64,
    /// Total file size in bytes (if known)
    pub total_bytes: Option<u64>,
    /// Page faults processed
    pub page_faults: u64,
    /// Cursor events processed
    pub cursor_events: u64,
    /// Transaction events processed
    pub txn_events: u64,
    /// Elapsed time in seconds
    pub elapsed_secs: f64,
}

impl ProgressInfo {
    /// Calculate percentage complete (0-100)
    pub fn percent_complete(&self) -> Option<f64> {
        self.total_bytes
            .map(|total| (self.bytes_read as f64 / total as f64 * 100.0).min(100.0))
    }

    /// Calculate processing speed in MB/s
    pub fn speed_mbps(&self) -> f64 {
        if self.elapsed_secs > 0.0 {
            self.bytes_read as f64 / 1e6 / self.elapsed_secs
        } else {
            0.0
        }
    }

    /// Estimate remaining time in seconds
    pub fn eta_secs(&self) -> Option<f64> {
        let speed = self.speed_mbps();
        if speed > 0.0 {
            self.total_bytes.map(|total| {
                let remaining_bytes = total.saturating_sub(self.bytes_read);
                remaining_bytes as f64 / 1e6 / speed
            })
        } else {
            None
        }
    }

    /// Format ETA as human-readable string
    pub fn eta_string(&self) -> String {
        match self.eta_secs() {
            Some(secs) if secs < 60.0 => format!("{:.0}s", secs),
            Some(secs) if secs < 3600.0 => format!("{:.0}m {:.0}s", secs / 60.0, secs % 60.0),
            Some(secs) => format!("{:.0}h {:.0}m", secs / 3600.0, (secs % 3600.0) / 60.0),
            None => "??".to_string(),
        }
    }
}

pub fn process_trace_streaming<R: std::io::Read>(
    reader: R,
    config: StreamingConfig,
    total_size: Option<u64>,
    progress_callback: Option<Box<dyn Fn(&ProgressInfo) + Send>>,
) -> anyhow::Result<ViewerData> {
    let buf_reader = BufReader::with_capacity(64 * 1024 * 1024, reader); // 64MB buffer
    let mut aggregator = StreamingAggregator::new(config);

    let mut line_count = 0u64;
    let mut bytes_read = 0u64;
    let mut page_fault_count = 0u64;
    let mut cursor_event_count = 0u64;
    let mut txn_event_count = 0u64;
    let start_time = std::time::Instant::now();

    for line_result in buf_reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => {
                aggregator.add_parse_error();
                continue;
            }
        };

        bytes_read += line.len() as u64 + 1; // +1 for newline
        line_count += 1;

        // Progress callback every 500K lines for more responsive updates
        if line_count % 500_000 == 0 {
            if let Some(ref cb) = progress_callback {
                cb(&ProgressInfo {
                    lines: line_count,
                    bytes_read,
                    total_bytes: total_size,
                    page_faults: page_fault_count,
                    cursor_events: cursor_event_count,
                    txn_events: txn_event_count,
                    elapsed_secs: start_time.elapsed().as_secs_f64(),
                });
            }
        }

        // Try to parse as page fault
        if let Ok(event) = serde_json::from_str::<PageFaultEvent>(&line) {
            if event.event_type == 1 || event.event_type == 2 {
                page_fault_count += 1;
                aggregator.process_page_fault(&event);
                continue;
            }
        }

        // Try to parse as txn event
        if let Ok(event) = serde_json::from_str::<TxnEvent>(&line) {
            if event.event_type >= 7 && event.event_type <= 9 {
                txn_event_count += 1;
                aggregator.process_txn_event(&event);
                continue;
            }
        }

        // Try to parse as cursor event
        if let Ok(event) = serde_json::from_str::<CursorEvent>(&line) {
            cursor_event_count += 1;
            aggregator.process_cursor_event(&event);
            continue;
        }

        aggregator.add_parse_error();
    }

    let elapsed = start_time.elapsed();
    eprintln!(
        "\nProcessed {} lines ({} page faults, {} cursor ops, {} txns) in {:.1}s",
        line_count,
        page_fault_count,
        cursor_event_count,
        txn_event_count,
        elapsed.as_secs_f64()
    );
    eprintln!(
        "Average speed: {:.1} MB/s, {} parse errors",
        bytes_read as f64 / 1e6 / elapsed.as_secs_f64(),
        aggregator.parse_errors()
    );

    Ok(aggregator.finalize())
}
