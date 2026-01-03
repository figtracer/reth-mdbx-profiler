//! Trace analyzer - analyzes traces and generates web-based visualizations

use clap::Parser;
use std::{io::BufRead, path::PathBuf};

mod event;
mod mdbx_metadata;
mod viewer;

use event::PageFaultEvent;
use mdbx_metadata::PageAttribution;

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

    /// Path to MDBX database file for table attribution (optional)
    #[arg(long)]
    mdbx_path: Option<PathBuf>,

    /// Output format: html (default), json (raw data), csv
    #[arg(short, long, default_value = "html")]
    format: String,

    /// Time bucket size in milliseconds (for pattern analysis)
    #[arg(long, default_value = "100")]
    bucket_ms: u64,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    eprintln!("Loading trace from {:?}...", cli.input);

    let file = std::fs::File::open(&cli.input)?;
    let reader = std::io::BufReader::new(file);

    let mut events: Vec<PageFaultEvent> = Vec::new();
    let mut parse_errors = 0;

    for line in reader.lines() {
        let line = line?;
        match serde_json::from_str::<PageFaultEvent>(&line) {
            Ok(event) => events.push(event),
            Err(_) => parse_errors += 1,
        }
    }

    eprintln!(
        "Loaded {} events ({} parse errors)",
        events.len(),
        parse_errors
    );

    if events.is_empty() {
        eprintln!("No events to analyze");
        return Ok(());
    }

    // Sort by timestamp
    events.sort_by_key(|e| e.timestamp_ns);

    // Load MDBX metadata if path provided
    let attribution = if let Some(mdbx_path) = &cli.mdbx_path {
        match mdbx_metadata::extract_table_stats(mdbx_path) {
            Ok(attr) => {
                eprintln!("Loaded MDBX metadata from {:?}", mdbx_path);
                Some(attr)
            }
            Err(e) => {
                eprintln!("Warning: Could not load MDBX metadata: {}", e);
                None
            }
        }
    } else {
        None
    };

    match cli.format.as_str() {
        "html" => {
            generate_html_viewer(&events, attribution.as_ref(), &cli)?;
        }
        "json" => {
            let data = viewer::generate_viewer_data(&events, attribution.as_ref());
            println!("{}", serde_json::to_string_pretty(&data)?);
        }
        "csv" => {
            print_csv(&events, attribution.as_ref());
        }
        _ => {
            eprintln!("Unknown format: {}. Use: html, json, csv", cli.format);
            std::process::exit(1);
        }
    }

    Ok(())
}

fn generate_html_viewer(
    events: &[PageFaultEvent],
    attribution: Option<&PageAttribution>,
    cli: &Cli,
) -> anyhow::Result<()> {
    eprintln!("Generating viewer data...");

    let data = viewer::generate_viewer_data(events, attribution);

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

    // Print summary to stderr
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
    eprintln!("Prefetch score:  {:.1}%", data.prefetch.prediction_hit_rate);

    eprintln!("\nViewer written to: {}", output_path.display());

    Ok(())
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
