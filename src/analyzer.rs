//! Trace analyzer - analyzes traces and generates web-based visualizations

use clap::Parser;
use std::{io::BufRead, path::PathBuf};

mod event;
mod mdbx_metadata;
mod streaming;
mod viewer;

use event::{CursorEvent, PageFaultEvent, TxnEvent};
use mdbx_metadata::PageAttribution;
use streaming::StreamingConfig;

/// Analyze MDBX page fault traces and generate interactive visualizations
#[derive(Parser)]
#[command(name = "mdbx-trace-analyzer")]
#[command(about = "Analyze MDBX page fault traces and generate interactive web visualizations")]
struct Cli {
    /// Input trace file (JSON lines format)
    #[arg(short, long)]
    input: PathBuf,

    /// Output HTML file (default: trace-viewer.html)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Path to MDBX database file for table attribution
    #[arg(long)]
    mdbx_path: PathBuf,

    /// Output format: html (default), json (raw data), compact (for comparison), csv
    #[arg(short, long, default_value = "html")]
    format: String,

    /// Label for this trace (used in compact export for comparison)
    #[arg(long)]
    label: Option<String>,

    /// Time bucket size in milliseconds (for pattern analysis)
    #[arg(long, default_value = "100")]
    bucket_ms: u64,

    /// Use streaming mode for large files (constant memory usage)
    /// Recommended for trace files larger than available RAM
    #[arg(long)]
    streaming: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Check file size to recommend streaming mode
    let file_size = std::fs::metadata(&cli.input)?.len();
    let file_size_gb = file_size as f64 / 1e9;

    if file_size_gb > 10.0 && !cli.streaming {
        eprintln!(
            "WARNING: Trace file is {:.1}GB. Consider using --streaming mode to avoid OOM.",
            file_size_gb
        );
        eprintln!("         Run with --streaming for constant memory usage.\n");
    }

    if cli.streaming {
        return run_streaming_mode(&cli);
    }

    // Original in-memory mode
    eprintln!("Loading trace from {:?}...", cli.input);

    let file = std::fs::File::open(&cli.input)?;
    let reader = std::io::BufReader::new(file);

    let mut events: Vec<PageFaultEvent> = Vec::new();
    let mut cursor_events: Vec<CursorEvent> = Vec::new();
    let mut txn_events: Vec<TxnEvent> = Vec::new();
    let mut parse_errors = 0;

    for line in reader.lines() {
        let line = line?;
        // Try to parse as page fault event first
        if let Ok(event) = serde_json::from_str::<PageFaultEvent>(&line) {
            if event.event_type == 1 || event.event_type == 2 {
                events.push(event);
                continue;
            }
        }
        // Try to parse as txn event
        if let Ok(event) = serde_json::from_str::<TxnEvent>(&line) {
            if event.event_type >= 7 && event.event_type <= 9 {
                txn_events.push(event);
                continue;
            }
        }
        // Try to parse as cursor event
        if let Ok(event) = serde_json::from_str::<CursorEvent>(&line) {
            cursor_events.push(event);
            continue;
        }
        parse_errors += 1;
    }

    eprintln!(
        "Loaded {} page fault events, {} cursor events, {} txn events ({} parse errors)",
        events.len(),
        cursor_events.len(),
        txn_events.len(),
        parse_errors
    );

    if events.is_empty() && cursor_events.is_empty() && txn_events.is_empty() {
        eprintln!("No events to analyze");
        return Ok(());
    }

    // Sort by timestamp
    events.sort_by_key(|e| e.timestamp_ns);
    cursor_events.sort_by_key(|e| e.timestamp_ns);
    txn_events.sort_by_key(|e| e.timestamp_ns);

    // Load MDBX metadata
    let attribution = match mdbx_metadata::extract_table_stats(&cli.mdbx_path) {
        Ok(attr) => {
            eprintln!("Loaded MDBX metadata from {:?}", cli.mdbx_path);
            Some(attr)
        }
        Err(e) => {
            eprintln!("Warning: Could not load MDBX metadata: {}", e);
            None
        }
    };

    match cli.format.as_str() {
        "html" => {
            generate_html_viewer(
                &events,
                &cursor_events,
                &txn_events,
                attribution.as_ref(),
                &cli,
            )?;
        }
        "json" => {
            let data = viewer::generate_viewer_data(
                &events,
                &cursor_events,
                &txn_events,
                attribution.as_ref(),
            );
            println!("{}", serde_json::to_string_pretty(&data)?);
        }
        "compact" => {
            let data = viewer::generate_viewer_data(
                &events,
                &cursor_events,
                &txn_events,
                attribution.as_ref(),
            );
            let compact = generate_compact_export(&data, cli.label.as_deref());
            println!("{}", serde_json::to_string_pretty(&compact)?);
        }
        "csv" => {
            print_csv(&events, attribution.as_ref());
        }
        _ => {
            eprintln!(
                "Unknown format: {}. Use: html, json, compact, csv",
                cli.format
            );
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Run the analyzer in streaming mode for large files
fn run_streaming_mode(cli: &Cli) -> anyhow::Result<()> {
    let file_size = std::fs::metadata(&cli.input)?.len();
    let file_size_gb = file_size as f64 / 1e9;

    eprintln!(
        "Streaming mode: Processing {:.2}GB trace file...",
        file_size_gb
    );
    eprintln!("Memory usage will remain constant regardless of file size.\n");

    let file = std::fs::File::open(&cli.input)?;

    let config = StreamingConfig {
        bucket_ms: cli.bucket_ms,
        ..Default::default()
    };

    // Progress callback
    let start_time = std::time::Instant::now();
    let progress_callback: Option<Box<dyn Fn(u64, u64) + Send>> =
        Some(Box::new(move |lines, bytes| {
            let elapsed = start_time.elapsed().as_secs_f64();
            let mb_processed = bytes as f64 / 1e6;
            let rate = mb_processed / elapsed;
            let eta_secs = if rate > 0.0 {
                ((file_size as f64 / 1e6) - mb_processed) / rate
            } else {
                0.0
            };
            eprint!(
                "\rProcessed: {}M lines, {:.1}GB ({:.0} MB/s, ETA: {:.0}s)   ",
                lines / 1_000_000,
                bytes as f64 / 1e9,
                rate,
                eta_secs
            );
        }));

    let data = streaming::process_trace_streaming(file, config, progress_callback)?;

    eprintln!("\n"); // Clear progress line

    match cli.format.as_str() {
        "html" => {
            let output_path = cli.output.clone().unwrap_or_else(|| {
                let input_stem = cli
                    .input
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
            let compact = generate_compact_export(&data, cli.label.as_deref());
            println!("{}", serde_json::to_string_pretty(&compact)?);
        }
        "csv" => {
            eprintln!("CSV format not supported in streaming mode (events not stored)");
            std::process::exit(1);
        }
        _ => {
            eprintln!("Unknown format: {}. Use: html, json, compact", cli.format);
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
            "Avg latency:     {:.1} μs",
            data.cursor_data.summary.avg_latency_us
        );
        eprintln!(
            "P99 latency:     {:.1} μs",
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
            "Avg commit lat:  {:.1} μs",
            data.txn_data.summary.avg_commit_latency_us
        );
        eprintln!(
            "Max concurrent:  {} RO, {} RW",
            data.txn_data.concurrency.max_concurrent_ro,
            data.txn_data.concurrency.max_concurrent_rw
        );
    }
}

fn generate_html_viewer(
    events: &[PageFaultEvent],
    cursor_events: &[CursorEvent],
    txn_events: &[TxnEvent],
    attribution: Option<&PageAttribution>,
    cli: &Cli,
) -> anyhow::Result<()> {
    eprintln!("Generating viewer data...");

    let data = viewer::generate_viewer_data(events, cursor_events, txn_events, attribution);

    // Determine output path
    let output_path = cli.output.clone().unwrap_or_else(|| {
        let input_stem = cli
            .input
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("trace");
        PathBuf::from(format!("{}-viewer.html", input_stem))
    });

    eprintln!("Writing HTML viewer to {:?}...", output_path);
    viewer::write_html(&data, &output_path)?;

    print_summary(&data);

    eprintln!("\nViewer written to: {}", output_path.display());

    Ok(())
}

/// Generate compact export format (same as browser export button)
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
            "random_ratio": data.patterns.random_ratio
        },
        "tables": data.unified_tables.iter().take(20).map(|t| {
            serde_json::json!({
                "name": t.name,
                "faults": t.faults,
                "major_faults": t.major_faults,
                "fault_pct": t.fault_percentage,
                "slow_ops": t.slow_ops,
                "time_lost_ms": t.time_lost_ms,
                "top_op": t.top_operation
            })
        }).collect::<Vec<_>>(),
        "threads": data.threads.iter().take(10).collect::<Vec<_>>()
    });

    // Add cursor data if available
    if data.cursor_data.has_data {
        compact["cursor_ops"] = serde_json::json!({
            "total": data.cursor_data.summary.total_ops,
            "rate_per_sec": data.cursor_data.summary.op_rate_per_sec,
            "seek_ratio": data.cursor_data.summary.seek_ratio,
            "latency_avg_us": data.cursor_data.summary.avg_latency_us,
            "latency_p95_us": data.cursor_data.summary.p95_latency_us,
            "latency_p99_us": data.cursor_data.summary.p99_latency_us,
            "by_table": data.cursor_data.table_stats.iter().take(15).map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "ops": t.ops,
                    "pct": t.percentage,
                    "avg_latency_us": t.avg_latency_us,
                    "p95_latency_us": t.p95_latency_us
                })
            }).collect::<Vec<_>>()
        });
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
            "commit_latency_avg_us": data.txn_data.summary.avg_commit_latency_us,
            "commit_latency_p99_us": data.txn_data.summary.p99_commit_latency_us,
            "max_concurrent_ro": data.txn_data.concurrency.max_concurrent_ro,
            "max_concurrent_rw": data.txn_data.concurrency.max_concurrent_rw
        });
    }

    // Add direct attribution stats if available
    if data.direct_fault_attribution.has_data {
        compact["attribution"] = serde_json::json!({
            "directly_attributed": data.direct_fault_attribution.directly_attributed_count,
            "timestamp_fallback": data.direct_fault_attribution.timestamp_fallback_count,
            "uncorrelated": data.direct_fault_attribution.uncorrelated_count
        });
    }

    compact
}

fn print_csv(events: &[PageFaultEvent], attribution: Option<&PageAttribution>) {
    println!("timestamp_ns,file_offset,page_number,address,tid,is_major,table");

    let page_size = attribution.map(|a| a.page_size()).unwrap_or(4096);

    for e in events {
        if e.event_type != 1 {
            continue; // Only page faults
        }

        let table = if let Some(attr) = attribution {
            attr.get_table_for_offset(e.file_offset)
                .map(|t| t.to_string())
                .unwrap_or_else(|| "Unknown".to_string())
        } else {
            mdbx_metadata::estimate_table_from_pattern(e.file_offset, page_size, 0, None)
                .to_string()
        };

        println!(
            "{},{},{},{},{},{},{}",
            e.timestamp_ns,
            e.file_offset,
            e.page_number(),
            e.address,
            e.tid,
            e.is_major_fault() as u8,
            table
        );
    }
}
