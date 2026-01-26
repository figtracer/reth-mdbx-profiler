//! eBPF-based profiler for MDBX page fault patterns and cursor operations
//!
//! This tool provides two main functions:
//! 1. `trace` - Traces page faults and cursor operations using eBPF
//! 2. `analyze` - Analyzes collected traces and generates visualizations

use clap::{Parser, Subcommand};
use libbpf_rs::{MapCore, MapFlags, ObjectBuilder, RingBufferBuilder};

/// Parse a hexadecimal address string (with or without 0x prefix)
fn parse_hex_address(s: &str) -> Result<u64, String> {
    let s = s.trim();
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    u64::from_str_radix(s, 16).map_err(|e| format!("Invalid hex address: {}", e))
}
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tracing::{debug, info, warn};

mod event;
mod streaming;
mod symbolize;
mod viewer;

use event::{CursorEvent, CursorLifecycleEvent, PageFaultEvent, SlowOpStackEvent, TxnEvent};
use streaming::StreamingConfig;

/// eBPF profiler for MDBX page fault patterns and cursor operations
#[derive(Parser)]
#[command(name = "mdbx-profiler")]
#[command(about = "Trace and analyze MDBX page faults and cursor operations")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Trace a running process (collect page faults and cursor operations)
    Trace {
        /// PID of the process to trace (use this OR --process-name)
        #[arg(short, long, required_unless_present = "process_name")]
        pid: Option<u32>,

        /// Process name to trace (e.g., "reth"). Allows restarting the process.
        /// The profiler will automatically detect when the process restarts and
        /// update tracking to the new PID.
        #[arg(long, conflicts_with = "pid")]
        process_name: Option<String>,

        /// Path to MDBX data directory (e.g., /data/reth/db/mdbx.dat)
        #[arg(short, long)]
        mdbx_path: PathBuf,

        /// Output file for trace data (JSON lines format)
        #[arg(short, long, default_value = "trace.jsonl")]
        output: PathBuf,

        /// Duration to trace (e.g., "30s", "5m")
        #[arg(short, long)]
        duration: Option<humantime::Duration>,

        /// Print live statistics every N seconds
        #[arg(long, default_value = "5")]
        stats_interval: u64,

        /// Also trace cursor operations (requires path to reth binary)
        #[arg(long)]
        trace_cursors: bool,

        /// Path to the reth binary (for cursor tracing)
        #[arg(long)]
        reth_binary: Option<PathBuf>,
    },

    /// Analyze a trace file and generate visualizations
    Analyze {
        /// Input trace file (JSON lines format)
        #[arg(short, long)]
        input: PathBuf,

        /// Output HTML file (default: <input>-viewer.html)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Path to MDBX database file for table attribution
        #[arg(long)]
        mdbx_path: Option<PathBuf>,

        /// Output format: html (default), json (raw data), compact (for comparison)
        #[arg(short, long, default_value = "html")]
        format: String,

        /// Label for this trace (used in compact export for comparison)
        #[arg(long)]
        label: Option<String>,

        /// Time bucket size in milliseconds (for pattern analysis)
        #[arg(long, default_value = "100")]
        bucket_ms: u64,

        /// Path to binary for symbol resolution in call site analysis
        #[arg(long)]
        binary: Option<PathBuf>,

        /// Base address of the binary in memory (from /proc/pid/maps)
        /// Use this if automatic detection fails. Example: --base-address 0x56cff0000000
        #[arg(long, value_parser = parse_hex_address)]
        base_address: Option<u64>,
    },
}

fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("mdbx_profiler=info".parse()?),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Trace {
            pid,
            process_name,
            mdbx_path,
            output,
            duration,
            stats_interval,
            trace_cursors,
            reth_binary,
        } => {
            let dur: Option<Duration> = duration.map(|d| d.into());
            run_trace(
                pid,
                process_name,
                mdbx_path,
                output,
                dur,
                stats_interval,
                trace_cursors,
                reth_binary,
            )?;
        }
        Commands::Analyze {
            input,
            output,
            mdbx_path,
            format,
            label,
            bucket_ms,
            binary,
            base_address,
        } => {
            run_analyze(
                input,
                output,
                mdbx_path,
                format,
                label,
                bucket_ms,
                binary,
                base_address,
            )?;
        }
    }

    Ok(())
}

/// Run the trace analyzer
fn run_analyze(
    input: PathBuf,
    output: Option<PathBuf>,
    _mdbx_path: Option<PathBuf>,
    format: String,
    label: Option<String>,
    bucket_ms: u64,
    binary: Option<PathBuf>,
    base_address: Option<u64>,
) -> anyhow::Result<()> {
    let file_size = std::fs::metadata(&input)?.len();
    let file_size_gb = file_size as f64 / 1e9;

    eprintln!("Processing {:.2}GB trace file: {:?}", file_size_gb, input);

    let file = std::fs::File::open(&input)?;

    let config = StreamingConfig {
        bucket_ms,
        binary_path: binary,
        binary_base_address: base_address,
        ..Default::default()
    };

    // Progress callback with detailed info
    use streaming::ProgressInfo;
    let progress_callback: Option<Box<dyn Fn(&ProgressInfo) + Send>> =
        Some(Box::new(move |info: &ProgressInfo| {
            let pct = info.percent_complete().unwrap_or(0.0);
            let speed = info.speed_mbps();
            let eta = info.eta_string();

            // Create a simple progress bar
            let bar_width = 30;
            let filled = (pct / 100.0 * bar_width as f64) as usize;
            let bar: String = "█".repeat(filled) + &"░".repeat(bar_width - filled);

            eprint!(
                "\r[{}] {:5.1}% | {:.1}GB/{:.1}GB | {:.0} MB/s | ETA: {} | {}M faults, {}M ops   ",
                bar,
                pct,
                info.bytes_read as f64 / 1e9,
                info.total_bytes.unwrap_or(0) as f64 / 1e9,
                speed,
                eta,
                info.page_faults / 1_000_000,
                info.cursor_events / 1_000_000,
            );
        }));

    let data =
        streaming::process_trace_streaming(file, config, Some(file_size), progress_callback)?;

    eprintln!("\n"); // Clear progress line

    match format.as_str() {
        "html" => {
            let output_path = output.clone().unwrap_or_else(|| {
                let input_stem = input
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("trace");
                PathBuf::from(format!("{}-viewer.html", input_stem))
            });

            eprintln!("Writing HTML viewer to {:?}...", output_path);
            viewer::write_html(&data, &output_path)?;
            print_summary(&data);
            eprintln!("\nViewer written to: {}", output_path.display());
        }
        "json" => {
            println!("{}", serde_json::to_string_pretty(&data)?);
        }
        "compact" => {
            let compact = generate_compact_export(&data, label.as_deref());
            println!("{}", serde_json::to_string_pretty(&compact)?);
        }
        _ => {
            eprintln!("Unknown format: {}. Use: html, json, compact", format);
            std::process::exit(1);
        }
    }

    Ok(())
}

fn print_summary(data: &viewer::ViewerData) {
    eprintln!("\n=== Trace Summary ===");
    eprintln!("Duration:        {:.2}s", data.summary.duration_secs);
    eprintln!("Total faults:    {}", data.summary.page_faults);
    eprintln!(
        "Major faults:    {} ({:.1}%)",
        data.summary.major_faults,
        data.summary.major_fault_ratio * 100.0
    );
    eprintln!("Minor faults:    {}", data.summary.minor_faults);
    eprintln!("Unique pages:    {}", data.summary.unique_pages);
    eprintln!("Fault rate:      {:.1}/s", data.summary.fault_rate_per_sec);
    eprintln!(
        "Sequential:      {:.1}%",
        data.patterns.sequential_ratio * 100.0
    );

    if data.cursor_data.has_data {
        eprintln!("\n=== Cursor Operations ===");
        eprintln!("Total ops:       {}", data.cursor_data.summary.total_ops);
        eprintln!(
            "Op rate:         {:.1}/s",
            data.cursor_data.summary.op_rate_per_sec
        );
        eprintln!(
            "Avg latency:     {:.1} us",
            data.cursor_data.summary.avg_latency_us
        );
        eprintln!(
            "P99 latency:     {:.1} us",
            data.cursor_data.summary.p99_latency_us
        );
        eprintln!(
            "Seek ratio:      {:.1}%",
            data.cursor_data.summary.seek_ratio
        );
    }

    if data.txn_data.has_data {
        eprintln!("\n=== Transactions ===");
        eprintln!("Total txns:      {}", data.txn_data.summary.begin_count);
        eprintln!(
            "Txn rate:        {:.1}/s",
            data.txn_data.summary.txn_rate_per_sec
        );
        eprintln!(
            "RO/RW:           {} / {}",
            data.txn_data.summary.ro_count, data.txn_data.summary.rw_count
        );
        eprintln!(
            "Commits/Aborts:  {} / {}",
            data.txn_data.summary.commit_count, data.txn_data.summary.abort_count
        );
        eprintln!(
            "Avg commit lat:  {:.1} us",
            data.txn_data.summary.avg_commit_latency_us
        );
        eprintln!(
            "Max concurrent:  {} RO, {} RW",
            data.txn_data.concurrency.max_concurrent_ro,
            data.txn_data.concurrency.max_concurrent_rw
        );
    }
}

/// Generate compact export format with comprehensive data for analysis
/// Target: ~200-300KB of structured data from 40GB traces
fn generate_compact_export(data: &viewer::ViewerData, label: Option<&str>) -> serde_json::Value {
    let mut compact = serde_json::json!({
        "label": label.unwrap_or("trace"),
        "trace": {
            "duration_secs": data.summary.duration_secs,
            "total_events": data.summary.total_events,
            "file_size_gb": data.summary.file_size_gb,
            "block_range": data.summary.block_range
        },
        "page_faults": {
            "total": data.summary.page_faults,
            "major": data.summary.major_faults,
            "minor": data.summary.minor_faults,
            "major_ratio": data.summary.major_fault_ratio,
            "rate_per_sec": data.summary.fault_rate_per_sec,
            "unique_pages": data.summary.unique_pages,
            "sequential_ratio": data.patterns.sequential_ratio,
            "random_ratio": data.patterns.random_ratio,
            "top_strides": data.patterns.top_strides.iter().take(5).map(|s| {
                serde_json::json!({
                    "stride": s.stride_pages,
                    "count": s.count,
                    "pct": s.percentage,
                    "pattern": s.pattern_type
                })
            }).collect::<Vec<_>>()
        },
        // Expanded table stats with drill-down data
        "tables": data.unified_tables.iter().take(30).map(|t| {
            serde_json::json!({
                "name": t.name,
                "dbi": t.dbi,
                "faults": t.faults,
                "major_faults": t.major_faults,
                "fault_pct": t.fault_percentage,
                "branch_faults": t.branch_faults,
                "leaf_faults": t.leaf_faults,
                "overflow_faults": t.overflow_faults,
                "total_ops": t.total_ops,
                "slow_ops": t.slow_ops,
                "slow_ops_pct": t.slow_ops_percentage,
                "time_lost_ms": t.time_lost_ms,
                "avg_latency_us": t.avg_latency_us,
                "max_latency_us": t.max_latency_us,
                "wall_time_ms": t.total_wall_time_ms,
                "cpu_time_ms": t.total_cpu_time_ms,
                "cpu_efficiency": t.cpu_efficiency,
                "is_io_bound": t.is_io_bound,
                "top_op": t.top_operation,
                "severity": t.severity,
                "faults_by_op": t.details.faults_by_op.iter().take(5).map(|o| {
                    serde_json::json!({"op": o.operation, "faults": o.faults, "major": o.major_faults})
                }).collect::<Vec<_>>(),
                "faults_by_cursor_op": t.details.faults_by_cursor_op.iter().take(5).map(|o| {
                    serde_json::json!({"op": o.operation, "faults": o.faults, "major": o.major_faults})
                }).collect::<Vec<_>>(),
                "slow_breakdown": t.details.slow_ops_breakdown.iter().take(5).map(|s| {
                    serde_json::json!({"op": s.operation, "count": s.count, "avg_us": s.avg_latency_us, "max_us": s.max_latency_us})
                }).collect::<Vec<_>>(),
                "hot_keys": t.details.hot_keys.iter().take(5).map(|k| {
                    serde_json::json!({"key": k.key_hex, "slow": k.slow_count, "total": k.total_count, "avg_us": k.avg_latency_us})
                }).collect::<Vec<_>>()
            })
        }).collect::<Vec<_>>(),
        // Per-thread stats with timeline samples
        "threads": data.threads.iter().take(15).map(|t| {
            serde_json::json!({
                "tid": t.tid,
                "faults": t.faults,
                "major_faults": t.major_faults,
                "pct": t.percentage,
                "top_tables": t.top_tables.iter().take(3).map(|tt| {
                    serde_json::json!({"table": tt.table_name, "faults": tt.faults, "major_pct": tt.major_pct})
                }).collect::<Vec<_>>(),
                // Sample timeline (every 10th point to reduce size)
                "timeline_sample": t.timeline.iter().step_by(10).take(50).map(|p| {
                    serde_json::json!({"t": p.time_ms, "f": p.faults, "m": p.major_faults})
                }).collect::<Vec<_>>()
            })
        }).collect::<Vec<_>>()
    });

    // Add cursor data if available
    if data.cursor_data.has_data {
        compact["cursor_ops"] = serde_json::json!({
            "total": data.cursor_data.summary.total_ops,
            "rate_per_sec": data.cursor_data.summary.op_rate_per_sec,
            "seek_count": data.cursor_data.summary.seek_count,
            "seek_ratio": data.cursor_data.summary.seek_ratio,
            "nav_count": data.cursor_data.summary.nav_count,
            "error_count": data.cursor_data.summary.error_count,
            "direct_get_count": data.cursor_data.summary.direct_get_count,
            "direct_get_ratio": data.cursor_data.summary.direct_get_ratio,
            "latency": {
                "avg_us": data.cursor_data.summary.avg_latency_us,
                "p50_us": data.cursor_data.summary.p50_latency_us,
                "p95_us": data.cursor_data.summary.p95_latency_us,
                "p99_us": data.cursor_data.summary.p99_latency_us
            },
            "by_table": data.cursor_data.table_stats.iter().take(20).map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "dbi": t.dbi,
                    "ops": t.ops,
                    "pct": t.percentage,
                    "seeks": t.seeks,
                    "navs": t.navs,
                    "avg_us": t.avg_latency_us,
                    "p95_us": t.p95_latency_us,
                    "p99_us": t.p99_latency_us
                })
            }).collect::<Vec<_>>(),
            "operations": data.cursor_data.operations.iter().take(15).map(|o| {
                serde_json::json!({
                    "name": o.name,
                    "count": o.count,
                    "pct": o.percentage,
                    "is_seek": o.is_seek
                })
            }).collect::<Vec<_>>(),
            // Slow operations by table
            "slow_by_table": data.cursor_data.slow_ops_by_table.iter().take(15).map(|s| {
                serde_json::json!({
                    "table": s.table,
                    "slow_count": s.slow_op_count,
                    "slow_pct": s.slow_op_percentage,
                    "total_slow_ms": s.total_slow_time_ms,
                    "avg_slow_us": s.avg_slow_latency_us,
                    "max_us": s.max_latency_us,
                    "by_op": s.by_operation.iter().take(3).map(|o| {
                        serde_json::json!({"op": o.operation, "count": o.count, "avg_us": o.avg_latency_us})
                    }).collect::<Vec<_>>()
                })
            }).collect::<Vec<_>>(),
            // Slow keys
            "slow_keys": data.cursor_data.slow_keys.iter().take(20).map(|k| {
                serde_json::json!({
                    "table": k.table,
                    "key": k.key_prefix,
                    "slow_count": k.slow_access_count,
                    "total_count": k.total_access_count,
                    "avg_us": k.avg_latency_us,
                    "max_us": k.max_latency_us
                })
            }).collect::<Vec<_>>(),
            // Timeline sample
            "timeline_sample": data.cursor_data.timeline.iter().step_by(5).take(100).map(|p| {
                serde_json::json!({"t": p.time_ms, "ops": p.ops, "seeks": p.seeks, "avg_us": p.avg_latency_us})
            }).collect::<Vec<_>>()
        });

        // Tree depth stats (crucial for understanding B+ tree traversal)
        let depth = &data.cursor_data.summary.tree_depth_stats;
        if depth.ops_with_depth_data > 0 {
            compact["tree_depth"] = serde_json::json!({
                "ops_with_data": depth.ops_with_depth_data,
                "max_observed": depth.max_depth_observed,
                "avg_depth": depth.avg_depth,
                "histogram": depth.depth_histogram.iter().map(|b| {
                    serde_json::json!({
                        "depth": b.depth,
                        "count": b.count,
                        "pct": b.percentage,
                        "avg_faults": b.avg_faults,
                        "avg_us": b.avg_latency_us
                    })
                }).collect::<Vec<_>>(),
                "by_table": depth.by_table.iter().take(15).map(|t| {
                    serde_json::json!({
                        "table": t.table_name,
                        "ops": t.ops_count,
                        "max_depth": t.max_depth,
                        "avg_depth": t.avg_depth,
                        "avg_faults": t.avg_faults,
                        "avg_us": t.avg_latency_us
                    })
                }).collect::<Vec<_>>(),
                "by_operation": depth.by_operation.iter().take(10).map(|o| {
                    serde_json::json!({
                        "op": o.operation,
                        "ops": o.ops_count,
                        "max_depth": o.max_depth,
                        "avg_depth": o.avg_depth,
                        "avg_faults": o.avg_faults,
                        "is_seek": o.is_seek
                    })
                }).collect::<Vec<_>>()
            });
        }
    }

    // Add transaction data if available
    if data.txn_data.has_data {
        compact["transactions"] = serde_json::json!({
            "total": data.txn_data.summary.begin_count,
            "rate_per_sec": data.txn_data.summary.txn_rate_per_sec,
            "ro_count": data.txn_data.summary.ro_count,
            "rw_count": data.txn_data.summary.rw_count,
            "commits": data.txn_data.summary.commit_count,
            "aborts": data.txn_data.summary.abort_count,
            "latency": {
                "avg_us": data.txn_data.summary.avg_commit_latency_us,
                "p50_us": data.txn_data.summary.p50_commit_latency_us,
                "p95_us": data.txn_data.summary.p95_commit_latency_us,
                "p99_us": data.txn_data.summary.p99_commit_latency_us,
                "max_us": data.txn_data.summary.max_commit_latency_us
            },
            "concurrency": {
                "max_ro": data.txn_data.concurrency.max_concurrent_ro,
                "max_rw": data.txn_data.concurrency.max_concurrent_rw,
                "max_total": data.txn_data.concurrency.max_concurrent_total
            },
            "by_thread": data.txn_data.thread_stats.iter().take(10).map(|t| {
                serde_json::json!({
                    "tid": t.tid,
                    "total": t.total_txns,
                    "ro": t.ro_txns,
                    "rw": t.rw_txns,
                    "commits": t.commits,
                    "aborts": t.aborts,
                    "avg_commit_us": t.avg_commit_latency_us
                })
            }).collect::<Vec<_>>(),
            // RW commit timeline sample
            "rw_commits_sample": data.txn_data.rw_commit_timeline.iter().step_by(5).take(100).map(|p| {
                serde_json::json!({"t": p.time_secs, "lat_ms": p.latency_ms})
            }).collect::<Vec<_>>()
        });
    }

    // Add direct attribution stats
    if data.direct_fault_attribution.has_data {
        compact["attribution"] = serde_json::json!({
            "directly_attributed": data.direct_fault_attribution.directly_attributed_count,
            "timestamp_fallback": data.direct_fault_attribution.timestamp_fallback_count,
            "uncorrelated": data.direct_fault_attribution.uncorrelated_count,
            "by_op_type": data.direct_fault_attribution.faults_by_op_type.iter().map(|o| {
                serde_json::json!({"op": o.op_type, "faults": o.total_faults, "major": o.major_faults, "pct": o.percentage})
            }).collect::<Vec<_>>(),
            "by_cursor_op": data.direct_fault_attribution.faults_by_cursor_op.iter().map(|o| {
                serde_json::json!({"op": o.cursor_op, "faults": o.total_faults, "major": o.major_faults, "pct": o.percentage})
            }).collect::<Vec<_>>()
        });
    }

    // Add page type stats
    if data.page_type_stats.has_data {
        compact["page_types"] = serde_json::json!({
            "total_faults": data.page_type_stats.total_faults,
            "traversal_to_data_ratio": data.page_type_stats.traversal_to_data_ratio,
            "by_type": data.page_type_stats.by_type.iter().map(|p| {
                serde_json::json!({
                    "type": p.page_type,
                    "faults": p.total_faults,
                    "major": p.major_faults,
                    "pct": p.percentage
                })
            }).collect::<Vec<_>>()
        });
    }

    // Add B-tree visualization data (batch analysis, tree depth estimates)
    if data.btree_viz.has_data {
        compact["btree"] = serde_json::json!({
            "traversal_efficiency": data.btree_viz.traversal_efficiency_score,
            "attribution": {
                "total_faults": data.btree_viz.attribution_stats.total_faults,
                "batch_attributed": data.btree_viz.attribution_stats.batch_attributed_faults,
                "batch_pct": data.btree_viz.attribution_stats.batch_attribution_pct,
                "rw_commits": data.btree_viz.attribution_stats.rw_commits_detected
            },
            "tree_depth_estimates": data.btree_viz.tree_depth_estimates.iter().take(15).map(|e| {
                serde_json::json!({
                    "table": e.table_name,
                    "depth": e.estimated_depth,
                    "branch_leaf_ratio": e.branch_leaf_ratio,
                    "confidence": e.confidence,
                    "sample_size": e.sample_size
                })
            }).collect::<Vec<_>>(),
            "operation_page_types": data.btree_viz.operation_page_types.iter().take(10).map(|o| {
                serde_json::json!({
                    "op": o.cursor_op,
                    "branch": o.branch_faults,
                    "leaf": o.leaf_faults,
                    "overflow": o.overflow_faults,
                    "major": o.major_faults,
                    "fault_pct": o.fault_percentage
                })
            }).collect::<Vec<_>>(),
            // Per-batch analysis (RW transaction batches)
            "batches": data.btree_viz.batch_analysis.iter().take(50).map(|b| {
                serde_json::json!({
                    "idx": b.batch_index,
                    "blocks": format!("{}-{}", b.first_block, b.last_block),
                    "block_count": b.block_count,
                    "faults": b.total_faults,
                    "branch": b.branch_faults,
                    "leaf": b.leaf_faults,
                    "major": b.major_faults,
                    "io_us": b.io_time_us,
                    "commit_us": b.commit_latency_us,
                    "tables": b.tables_touched.iter().take(5).collect::<Vec<_>>()
                })
            }).collect::<Vec<_>>()
        });
    }

    // Add working set analysis
    if data.working_set.has_data {
        compact["working_set"] = serde_json::json!({
            "unique_pages": data.working_set.total_unique_pages,
            "total_accesses": data.working_set.total_accesses,
            "reuse_ratio": data.working_set.reuse_ratio,
            "avg_accesses_per_page": data.working_set.avg_accesses_per_page,
            "hot_pages": {
                "for_50pct": data.working_set.hot_page_analysis.pages_for_50pct,
                "for_80pct": data.working_set.hot_page_analysis.pages_for_80pct,
                "for_90pct": data.working_set.hot_page_analysis.pages_for_90pct,
                "pareto_ratio": data.working_set.hot_page_analysis.pareto_ratio,
                "curve": data.working_set.hot_page_analysis.distribution_curve.iter().step_by(5).take(20).map(|p| {
                    serde_json::json!({"pages_pct": p.pages_pct, "accesses_pct": p.accesses_pct})
                }).collect::<Vec<_>>()
            },
            "cache_simulation": data.working_set.cache_simulation.iter().map(|c| {
                serde_json::json!({
                    "size_gb": c.cache_size_gb,
                    "hit_rate": c.hit_rate,
                    "faults_avoided_per_sec": c.faults_avoided_per_sec
                })
            }).collect::<Vec<_>>(),
            "access_distribution": data.working_set.access_count_distribution.iter().map(|a| {
                serde_json::json!({"label": a.label, "pages": a.page_count, "pct": a.percentage})
            }).collect::<Vec<_>>(),
            "per_table": data.working_set.per_table.iter().take(15).map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "unique_pages": t.unique_pages,
                    "accesses": t.total_accesses,
                    "reuse_ratio": t.reuse_ratio,
                    "hot_pages": t.hot_pages,
                    "hot_set_mb": t.hot_set_mb,
                    "working_set_mb": t.working_set_mb
                })
            }).collect::<Vec<_>>(),
            "time_windowed": data.working_set.time_windowed.iter().map(|w| {
                serde_json::json!({
                    "window_secs": w.window_secs,
                    "avg_pages": w.avg_wss_pages,
                    "max_pages": w.max_wss_pages,
                    "avg_mb": w.avg_wss_mb
                })
            }).collect::<Vec<_>>()
        });
    }

    // Add CPU profile summary
    if data.cpu_profile.has_data {
        compact["cpu_profile"] = serde_json::json!({
            "wall_time_ms": data.cpu_profile.total_wall_time_ms,
            "cpu_time_ms": data.cpu_profile.total_cpu_time_ms,
            "io_wait_ms": data.cpu_profile.total_io_wait_ms,
            "cpu_efficiency": data.cpu_profile.cpu_efficiency,
            "bottleneck": data.cpu_profile.bottleneck,
            "io_bound_tables": data.cpu_profile.top_io_bound_tables.iter().take(10).map(|t| {
                serde_json::json!({"table": t.name, "wall_ms": t.wall_time_ms, "cpu_ms": t.cpu_time_ms, "efficiency": t.cpu_efficiency})
            }).collect::<Vec<_>>(),
            "cpu_bound_tables": data.cpu_profile.top_cpu_bound_tables.iter().take(10).map(|t| {
                serde_json::json!({"table": t.name, "wall_ms": t.wall_time_ms, "cpu_ms": t.cpu_time_ms, "efficiency": t.cpu_efficiency})
            }).collect::<Vec<_>>()
        });
    }

    // Add call site analysis (call paths for slow operations)
    if data.call_site_analysis.has_data {
        compact["call_sites"] = serde_json::json!({
            "total_slow_ops": data.call_site_analysis.total_slow_ops,
            "unique_sites": data.call_site_analysis.unique_call_sites,
            "path_summary": {
                "critical_path_count": data.call_site_analysis.path_summary.critical_path_count,
                "critical_path_latency_ns": data.call_site_analysis.path_summary.critical_path_latency_ns,
                "background_count": data.call_site_analysis.path_summary.background_count,
                "background_latency_ns": data.call_site_analysis.path_summary.background_latency_ns,
                "unknown_count": data.call_site_analysis.path_summary.unknown_count,
                "critical_pct": data.call_site_analysis.path_summary.critical_path_percentage
            },
            // Subsystem breakdown (grouped by caller module)
            "subsystems": data.call_site_analysis.subsystems.iter().take(15).map(|s| {
                serde_json::json!({
                    "module": s.caller_module,
                    "name": s.name,
                    "slow_ops": s.slow_ops,
                    "total_faults": s.total_faults,
                    "major_faults": s.major_faults,
                    "major_pct": s.major_fault_pct,
                    "total_latency_ns": s.total_latency_ns,
                    "avg_us": s.avg_latency_us,
                    "pct": s.percentage,
                    "patterns": s.top_patterns.iter().take(5).map(|p| {
                        serde_json::json!({
                            "pattern": p.pattern,
                            "count": p.count,
                            "avg_us": p.avg_latency_us,
                            "faults": p.faults,
                            "major": p.major_faults
                        })
                    }).collect::<Vec<_>>()
                })
            }).collect::<Vec<_>>(),
            // Top call sites with full call paths
            "top_sites": data.call_site_analysis.top_call_sites.iter().take(30).map(|c| {
                serde_json::json!({
                    "path": c.call_path,
                    "module": c.caller_module,
                    "count": c.count,
                    "total_ns": c.total_latency_ns,
                    "avg_us": c.avg_latency_us,
                    "max_us": c.max_latency_us,
                    "faults": c.total_faults,
                    "major": c.major_faults,
                    "avg_faults": c.avg_faults,
                    "stack": c.sample_stack.as_ref().map(|s| s.iter().take(10).collect::<Vec<_>>())
                })
            }).collect::<Vec<_>>(),
            // Detected issues
            "issues": data.call_site_analysis.detected_issues.iter().take(10).map(|i| {
                serde_json::json!({
                    "severity": i.severity,
                    "subsystem": i.subsystem_name,
                    "pattern": i.pattern,
                    "description": i.description,
                    "evidence": {
                        "ops": i.evidence.affected_ops,
                        "major_rate": i.evidence.major_fault_rate,
                        "avg_us": i.evidence.avg_latency_us,
                        "table": i.evidence.table,
                        "context": i.evidence.context
                    }
                })
            }).collect::<Vec<_>>()
        });
    }

    // Add cursor lifecycle data
    if data.cursor_lifecycle.has_data {
        compact["cursor_lifecycle"] = serde_json::json!({
            "opens": data.cursor_lifecycle.total_opens,
            "closes": data.cursor_lifecycle.total_closes,
            "still_open": data.cursor_lifecycle.still_open,
            "lifetime": {
                "avg_us": data.cursor_lifecycle.avg_lifetime_us,
                "p50_us": data.cursor_lifecycle.p50_lifetime_us,
                "p95_us": data.cursor_lifecycle.p95_lifetime_us,
                "p99_us": data.cursor_lifecycle.p99_lifetime_us
            },
            "ops_per_cursor": data.cursor_lifecycle.avg_ops_per_cursor,
            "by_table": data.cursor_lifecycle.by_table.iter().take(15).map(|t| {
                serde_json::json!({
                    "table": t.table,
                    "opens": t.opens,
                    "closes": t.closes,
                    "avg_lifetime_us": t.avg_lifetime_us,
                    "ops": t.total_ops,
                    "ops_per_cursor": t.avg_ops_per_cursor
                })
            }).collect::<Vec<_>>()
        });
    }

    // Add operation fault histogram
    if data.operation_histogram.has_data {
        compact["fault_histogram"] = serde_json::json!({
            "avg_per_op": data.operation_histogram.avg_faults_per_op,
            "max_per_op": data.operation_histogram.max_faults_per_op,
            "distribution": data.operation_histogram.distribution.iter().map(|b| {
                serde_json::json!({"label": b.label, "count": b.count, "pct": b.percentage})
            }).collect::<Vec<_>>()
        });
    }

    // Add tree traversal data
    if data.tree_traversal.has_data {
        compact["tree_traversal"] = serde_json::json!({
            "tables": data.tree_traversal.tables.iter().take(15).map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "faults": t.total_faults,
                    "branch": t.branch_faults,
                    "leaf": t.leaf_faults,
                    "overflow": t.overflow_faults,
                    "branch_leaf_ratio": t.branch_leaf_ratio
                })
            }).collect::<Vec<_>>()
        });
    }

    // Add sampled timeline for fault patterns
    compact["timeline_sample"] = serde_json::json!(
        data.timeline
            .iter()
            .step_by(10)
            .take(200)
            .map(|p| {
                serde_json::json!({
                    "t": p.time_ms,
                    "f": p.faults,
                    "m": p.major_faults,
                    "u": p.unique_pages
                })
            })
            .collect::<Vec<_>>()
    );

    compact
}

// ============================================================================
// Trace collection functions (from original main.rs)
// ============================================================================

/// Find PID(s) by process name
fn find_pids_by_name(name: &str) -> Vec<u32> {
    let mut pids = Vec::new();

    // Read /proc to find matching processes
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let file_name_str = file_name.to_string_lossy();

            // Check if this is a PID directory (numeric name)
            if let Ok(pid) = file_name_str.parse::<u32>() {
                // Read the comm file to get process name
                let comm_path = format!("/proc/{}/comm", pid);
                if let Ok(comm) = std::fs::read_to_string(&comm_path) {
                    let comm = comm.trim();
                    if comm == name {
                        pids.push(pid);
                    }
                }
            }
        }
    }

    pids
}

/// Find the libmdbx shared library in the process's memory mappings
fn find_libmdbx_path(pid: u32) -> Option<PathBuf> {
    let maps_path = format!("/proc/{}/maps", pid);
    let content = std::fs::read_to_string(&maps_path).ok()?;

    for line in content.lines() {
        // Look for libmdbx shared library
        if line.contains("libmdbx") && line.contains(".so") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 6 {
                let path = parts[5..].join(" ");
                if path.contains("libmdbx") {
                    return Some(PathBuf::from(path));
                }
            }
        }
    }

    None
}

/// Find the offset of a symbol in a binary using nm
fn find_symbol_offset(binary_path: &PathBuf, symbol: &str) -> Option<u64> {
    // Try nm with regular symbols first (for statically linked binaries)
    for nm_flag in &[&[] as &[&str], &["-D"]] {
        let mut cmd = std::process::Command::new("nm");
        for flag in *nm_flag {
            cmd.arg(flag);
        }
        cmd.arg(binary_path);

        let output = match cmd.output() {
            Ok(o) => o,
            Err(_) => continue,
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.contains(symbol) && !line.contains("@@") {
                // Format: "0000000000123456 T symbol_name"
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 && parts[2] == symbol {
                    if let Some(offset) = u64::from_str_radix(parts[0], 16).ok() {
                        debug!(
                            "Found symbol {} at offset 0x{:x} using nm {:?}",
                            symbol, offset, nm_flag
                        );
                        return Some(offset);
                    }
                }
            }
        }
    }

    None
}

/// Update the target PID in the BPF config map
fn update_target_pid(obj: &mut libbpf_rs::Object, pid: u32) -> anyhow::Result<()> {
    let config_map = obj
        .maps_mut()
        .find(|m| m.name().to_string_lossy() == "profiler_config")
        .expect("profiler_config map not found");
    let key: u32 = 0;
    config_map.update(&key.to_ne_bytes(), &pid.to_ne_bytes(), MapFlags::ANY)?;
    Ok(())
}

/// Clear the cursor_to_dbi map (needed when process restarts to capture fresh cursor opens)
fn clear_cursor_to_dbi_map(obj: &mut libbpf_rs::Object) -> anyhow::Result<()> {
    let cursor_map = obj
        .maps_mut()
        .find(|m| m.name().to_string_lossy() == "cursor_to_dbi");

    if let Some(map) = cursor_map {
        // Iterate and delete all keys
        let mut keys_to_delete = Vec::new();

        // First collect all keys
        let mut key = vec![0u8; 8]; // cursor pointer is u64
        while let Ok(Some(next_key)) = map.lookup(&key, MapFlags::ANY) {
            keys_to_delete.push(next_key.clone());
            key = next_key;
        }

        // Delete all keys
        for k in keys_to_delete {
            let _ = map.delete(&k);
        }

        info!("Cleared cursor_to_dbi map");
    }
    Ok(())
}

/// Check if a process with the given PID is still running
fn is_process_running(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{}", pid)).exists()
}

fn run_trace(
    pid: Option<u32>,
    process_name: Option<String>,
    mdbx_path: PathBuf,
    output: PathBuf,
    duration: Option<Duration>,
    stats_interval: u64,
    trace_cursors: bool,
    reth_binary: Option<PathBuf>,
) -> anyhow::Result<()> {
    // Determine initial PID
    let initial_pid = if let Some(pid) = pid {
        pid
    } else if let Some(ref name) = process_name {
        let pids = find_pids_by_name(name);
        match pids.len() {
            0 => {
                info!("Process '{}' not found. Waiting for it to start...", name);
                0 // Will be updated when process starts
            }
            1 => {
                info!("Found process '{}' with PID {}", name, pids[0]);
                pids[0]
            }
            _ => {
                warn!(
                    "Found multiple processes named '{}': {:?}. Using first one.",
                    name, pids
                );
                pids[0]
            }
        }
    } else {
        anyhow::bail!("Either --pid or --process-name must be specified");
    };

    info!(
        "Starting trace for {} on MDBX path {:?}",
        if let Some(ref name) = process_name {
            format!("process '{}'", name)
        } else {
            format!("PID {}", initial_pid)
        },
        mdbx_path
    );

    // Get inode of MDBX file
    let metadata = std::fs::metadata(&mdbx_path)?;
    use std::os::unix::fs::MetadataExt;
    let inode = metadata.ino();
    info!("MDBX file inode: {}", inode);

    // Load BPF program
    let obj_path = std::env::current_exe()?
        .parent()
        .unwrap()
        .join("mdbx_tracer.bpf.o");

    info!("Loading BPF object from {:?}", obj_path);

    let mut builder = ObjectBuilder::default();
    let open_obj = builder.open_file(&obj_path)?;
    let mut obj = open_obj.load()?;

    // Configure target PID (use initial_pid, will be updated if process restarts)
    let mut current_pid = initial_pid;
    update_target_pid(&mut obj, current_pid)?;
    if current_pid > 0 {
        info!("Configured target PID: {}", current_pid);
    } else {
        info!("No target PID yet, waiting for process to start");
    }

    // Register MDBX inode for tracking
    {
        let tracked_inodes = obj
            .maps_mut()
            .find(|m| m.name().to_string_lossy() == "tracked_inodes")
            .expect("tracked_inodes map not found");
        let track_val: u8 = 1;
        tracked_inodes.update(&inode.to_ne_bytes(), &[track_val], MapFlags::ANY)?;
        info!("Registered inode {} for tracking", inode);
    }

    // Keep track of attached links
    let mut _links: Vec<libbpf_rs::Link> = Vec::new();

    // Attach kprobes for page fault tracing
    for prog in obj.progs_mut() {
        let name = prog.name().to_string_lossy().to_string();
        let section = prog.section().to_string_lossy().to_string();

        // Skip uprobe programs (cursor/direct get/txn tracing) unless trace_cursors is enabled
        // Check both program name and section name
        let is_uprobe = section.contains("uprobe")
            || name.contains("cursor")
            || name.contains("direct_get")
            || name.contains("direct_put")
            || name.contains("direct_del")
            || name.contains("txn_");
        if is_uprobe && !trace_cursors {
            debug!("Skipping cursor probe: {} (section: {})", name, section);
            continue;
        }

        // Handle uprobes separately - they need manual attachment
        if is_uprobe {
            if trace_cursors {
                // Find the binary to attach to
                let binary = if let Some(ref bin) = reth_binary {
                    bin.clone()
                } else if current_pid > 0 {
                    // Try to find libmdbx in the process
                    match find_libmdbx_path(current_pid) {
                        Some(p) => p,
                        None => {
                            warn!(
                                "Could not find libmdbx for PID {}. Use --reth-binary to specify.",
                                current_pid
                            );
                            continue;
                        }
                    }
                } else {
                    warn!(
                        "No process running yet and --reth-binary not specified. Skipping uprobe {}.",
                        name
                    );
                    continue;
                };

                info!("Attaching uprobe {} to {:?}", name, binary);

                // Find the symbol offset based on program name
                let func_name = if name.contains("cursor_get") {
                    "mdbx_cursor_get"
                } else if name.contains("cursor_put") {
                    "mdbx_cursor_put"
                } else if name.contains("cursor_del") {
                    "mdbx_cursor_del"
                } else if name.contains("cursor_open") {
                    "mdbx_cursor_open"
                } else if name.contains("cursor_close") {
                    "mdbx_cursor_close"
                } else if name.contains("direct_get") {
                    "mdbx_get"
                } else if name.contains("direct_put") {
                    "mdbx_put"
                } else if name.contains("direct_del") {
                    "mdbx_del"
                } else if name.contains("txn_begin") {
                    "mdbx_txn_begin_ex"
                } else if name.contains("txn_commit") {
                    "mdbx_txn_commit_ex"
                } else if name.contains("txn_abort") {
                    "mdbx_txn_abort"
                } else {
                    debug!("Skipping unknown cursor probe: {}", name);
                    continue;
                };

                match find_symbol_offset(&binary, func_name) {
                    Some(offset) => {
                        // Check both section name and program name for return probe
                        let is_ret = section.contains("uretprobe") || name.ends_with("_ret");
                        // Use pid=-1 to attach globally, then filter by PID in BPF
                        // This is more reliable than per-process uprobe attachment
                        let opts = libbpf_rs::UprobeOpts {
                            retprobe: is_ret,
                            func_name: func_name.to_string(),
                            ..Default::default()
                        };
                        match prog.attach_uprobe_with_opts(-1, &binary, 0, opts) {
                            Ok(link) => {
                                info!(
                                    "Attached {} for {} (retprobe={}) at offset 0x{:x}",
                                    name, func_name, is_ret, offset
                                );
                                _links.push(link);
                            }
                            Err(e) => {
                                warn!("Failed to attach {} with opts: {:?}", name, e);
                                // Fallback to offset-based attachment
                                match prog.attach_uprobe(is_ret, -1, &binary, offset as usize) {
                                    Ok(link) => {
                                        info!(
                                            "Attached {} at offset 0x{:x} (fallback)",
                                            name, offset
                                        );
                                        _links.push(link);
                                    }
                                    Err(e2) => {
                                        warn!("Failed to attach {} with offset: {:?}", name, e2);
                                    }
                                }
                            }
                        }
                    }
                    None => {
                        warn!("Could not find symbol {} in {:?}", func_name, binary);
                    }
                }
            }
            continue;
        }

        // Attach kprobes
        info!("Attaching program: {}", name);
        match prog.attach() {
            Ok(link) => {
                info!("Attached {} successfully", name);
                _links.push(link);
            }
            Err(e) => {
                warn!("Failed to attach {}: {} (continuing anyway)", name, e);
            }
        }
    }

    // Open output file
    let output_file = std::fs::File::create(&output)?;
    let writer = std::io::BufWriter::new(output_file);
    let writer = Arc::new(std::sync::Mutex::new(writer));

    // Set up signal handling
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        info!("Received Ctrl+C, stopping...");
        r.store(false, Ordering::SeqCst);
    })?;

    // Set up ring buffer consumer
    let events_map = obj
        .maps()
        .find(|m| m.name().to_string_lossy() == "events")
        .expect("events map not found");

    let mut ring_builder = RingBufferBuilder::new();

    let event_count = Arc::new(AtomicU64::new(0));
    let cursor_count = Arc::new(AtomicU64::new(0));
    let txn_count = Arc::new(AtomicU64::new(0));
    let event_count_clone = event_count.clone();
    let cursor_count_clone = cursor_count.clone();
    let txn_count_clone = txn_count.clone();
    let writer_clone = writer.clone();

    ring_builder.add(&events_map, move |data: &[u8]| {
        // Check event type to determine which struct to use
        // Struct layouts (from C compiler on x86_64 Linux):
        // PageFaultEvent: event_type at offset 48 (after 5x u64 + 2x u32)
        // CursorEvent: event_type at offset 16 (after u64 + 2x u32)
        // TxnEvent: event_type at offset 16 (after u64 + 2x u32) - same offset as CursorEvent
        //
        // PageFaultEvent is 72 bytes, CursorEvent is 120 bytes, TxnEvent is 56 bytes
        // We differentiate by checking event_type at both possible offsets

        const PAGE_FAULT_EVENT_SIZE: usize = 72;
        const CURSOR_EVENT_SIZE: usize = 128; // 120 + 8 for cursor_ptr
        const TXN_EVENT_SIZE: usize = 56;
        const PAGE_FAULT_EVENT_TYPE_OFFSET: usize = 48;
        const CURSOR_EVENT_TYPE_OFFSET: usize = 16;

        if data.len() < CURSOR_EVENT_TYPE_OFFSET + 4 {
            return 0;
        }

        // First, try to read event_type at cursor/txn event offset (16)
        // This works for cursor events, txn events, and page fault events
        let event_type = u32::from_ne_bytes(
            data[CURSOR_EVENT_TYPE_OFFSET..CURSOR_EVENT_TYPE_OFFSET + 4]
                .try_into()
                .unwrap_or([0; 4]),
        );

        // Check if this is a cursor event (event_type 3-6, 10-11 at offset 16)
        // 3 = CursorGet, 4 = CursorPut, 5 = DirectGet, 6 = CursorDel
        // 10 = DirectPut, 11 = DirectDel
        if ((event_type >= 3 && event_type <= 6) || event_type == 10 || event_type == 11)
            && data.len() >= CURSOR_EVENT_SIZE
        {
            let event: CursorEvent =
                unsafe { std::ptr::read_unaligned(data.as_ptr() as *const CursorEvent) };

            cursor_count_clone.fetch_add(1, Ordering::Relaxed);

            if let Ok(json) = serde_json::to_string(&event) {
                use std::io::Write;
                if let Ok(mut w) = writer_clone.lock() {
                    let _ = writeln!(w, "{}", json);
                }
            }
            return 0;
        }

        // Check if this is a transaction event (event_type 7, 8, or 9 at offset 16)
        // 7 = TxnBegin, 8 = TxnCommit, 9 = TxnAbort
        if (event_type == 7 || event_type == 8 || event_type == 9) && data.len() >= TXN_EVENT_SIZE {
            let event: TxnEvent =
                unsafe { std::ptr::read_unaligned(data.as_ptr() as *const TxnEvent) };

            txn_count_clone.fetch_add(1, Ordering::Relaxed);

            if let Ok(json) = serde_json::to_string(&event) {
                use std::io::Write;
                if let Ok(mut w) = writer_clone.lock() {
                    let _ = writeln!(w, "{}", json);
                }
            }
            return 0;
        }

        // Check if this is a cursor lifecycle event (event_type 12 or 13 at offset 16)
        // 12 = CursorOpen, 13 = CursorClose
        const CURSOR_LIFECYCLE_EVENT_SIZE: usize = 40;
        if (event_type == 12 || event_type == 13) && data.len() >= CURSOR_LIFECYCLE_EVENT_SIZE {
            let event: CursorLifecycleEvent =
                unsafe { std::ptr::read_unaligned(data.as_ptr() as *const CursorLifecycleEvent) };

            if let Ok(json) = serde_json::to_string(&event) {
                use std::io::Write;
                if let Ok(mut w) = writer_clone.lock() {
                    let _ = writeln!(w, "{}", json);
                }
            }
            return 0;
        }

        // Check if this is a slow operation stack event (event_type 14 at offset 16)
        // 14 = SlowOpStack - contains user-space stack trace for call site attribution
        // Size: 64 + 256 (stack) + 16 (key_prefix) = 336 bytes
        const SLOW_OP_STACK_EVENT_SIZE: usize = 336;
        if event_type == 14 && data.len() >= SLOW_OP_STACK_EVENT_SIZE {
            let event: SlowOpStackEvent =
                unsafe { std::ptr::read_unaligned(data.as_ptr() as *const SlowOpStackEvent) };

            if let Ok(json) = serde_json::to_string(&event) {
                use std::io::Write;
                if let Ok(mut w) = writer_clone.lock() {
                    let _ = writeln!(w, "{}", json);
                }
            }
            return 0;
        }

        // Check if this is a page fault event (event_type 1 or 2 at offset 48)
        if data.len() >= PAGE_FAULT_EVENT_SIZE {
            let page_fault_event_type = u32::from_ne_bytes(
                data[PAGE_FAULT_EVENT_TYPE_OFFSET..PAGE_FAULT_EVENT_TYPE_OFFSET + 4]
                    .try_into()
                    .unwrap_or([0; 4]),
            );

            if page_fault_event_type == 1 || page_fault_event_type == 2 {
                let event: PageFaultEvent =
                    unsafe { std::ptr::read_unaligned(data.as_ptr() as *const PageFaultEvent) };

                event_count_clone.fetch_add(1, Ordering::Relaxed);

                if let Ok(json) = serde_json::to_string(&event) {
                    use std::io::Write;
                    if let Ok(mut w) = writer_clone.lock() {
                        let _ = writeln!(w, "{}", json);
                    }
                }
            }
        }

        0
    })?;

    let ring = ring_builder.build()?;

    // Main loop
    let start = Instant::now();
    let mut last_stats = Instant::now();
    let mut last_process_check = Instant::now();
    let process_check_interval = Duration::from_secs(1);

    info!("Tracing started. Press Ctrl+C to stop.");

    while running.load(Ordering::SeqCst) {
        // Check duration limit
        if let Some(dur) = duration {
            if start.elapsed() >= dur {
                info!("Duration limit reached");
                break;
            }
        }

        // Poll ring buffer
        let _ = ring.poll(Duration::from_millis(100));

        // Check for process restart if using --process-name
        if let Some(ref name) = process_name {
            if last_process_check.elapsed() >= process_check_interval {
                last_process_check = Instant::now();

                if current_pid > 0 && !is_process_running(current_pid) {
                    // Process died, look for new one
                    info!(
                        "Process {} (PID {}) exited. Waiting for restart...",
                        name, current_pid
                    );
                    current_pid = 0;
                    update_target_pid(&mut obj, 0)?;
                    clear_cursor_to_dbi_map(&mut obj)?;
                }

                if current_pid == 0 {
                    // Look for new process
                    let pids = find_pids_by_name(name);
                    if !pids.is_empty() {
                        current_pid = pids[0];
                        update_target_pid(&mut obj, current_pid)?;
                        info!(
                            "Process '{}' started with PID {}. Tracing resumed.",
                            name, current_pid
                        );
                    }
                }
            }
        }

        // Print stats periodically
        if last_stats.elapsed() >= Duration::from_secs(stats_interval) {
            let pf_count = event_count.load(Ordering::Relaxed);
            let cur_count = cursor_count.load(Ordering::Relaxed);
            let txn_cnt = txn_count.load(Ordering::Relaxed);
            let elapsed = start.elapsed().as_secs_f64();
            info!(
                "Page faults: {} ({:.1}/s), Cursor ops: {} ({:.1}/s), Txns: {} ({:.1}/s), Elapsed: {:.1}s{}",
                pf_count,
                pf_count as f64 / elapsed,
                cur_count,
                cur_count as f64 / elapsed,
                txn_cnt,
                txn_cnt as f64 / elapsed,
                elapsed,
                if current_pid == 0 {
                    " [waiting for process]"
                } else {
                    ""
                }
            );
            last_stats = Instant::now();
        }
    }

    // Flush the writer
    {
        use std::io::Write;
        if let Ok(mut w) = writer.lock() {
            let _ = w.flush();
        }
    }

    // Print final stats
    let stats_map = obj
        .maps()
        .find(|m| m.name().to_string_lossy() == "stats")
        .expect("stats map not found");
    print_stats(&stats_map)?;

    let pf_total = event_count.load(Ordering::Relaxed);
    let cur_total = cursor_count.load(Ordering::Relaxed);
    let txn_total = txn_count.load(Ordering::Relaxed);
    info!(
        "Trace saved to {:?} ({} page faults, {} cursor ops, {} txn events)",
        output, pf_total, cur_total, txn_total
    );
    Ok(())
}

fn print_stats(stats_map: &libbpf_rs::Map) -> anyhow::Result<()> {
    let stat_names = [
        "Total faults",
        "MDBX faults",
        "Major faults",
        "Events dropped",
        "Cursor ops",
        "Cursor seeks",
        "Cursor nexts",
        "Cursor errors",
        "Direct gets",
        "Cursor puts",
        "Cursor dels",
        "Txn begins",
        "Txn commits",
        "Txn aborts",
        "Direct puts",
        "Direct dels",
    ];

    info!("=== Statistics ===");
    for (i, name) in stat_names.iter().enumerate() {
        let key = (i as u32).to_ne_bytes();
        match stats_map.lookup_percpu(&key, MapFlags::ANY) {
            Ok(Some(percpu_vals)) => {
                let total: u64 = percpu_vals
                    .iter()
                    .map(|v| {
                        if v.len() >= 8 {
                            u64::from_ne_bytes(v[..8].try_into().unwrap_or([0; 8]))
                        } else {
                            0
                        }
                    })
                    .sum();
                info!("{}: {}", name, total);
            }
            Ok(None) => {
                info!("{}: 0", name);
            }
            Err(e) => {
                warn!("Failed to read stat {}: {}", name, e);
            }
        }
    }
    Ok(())
}
