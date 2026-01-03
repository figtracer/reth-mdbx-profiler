//! eBPF-based profiler for MDBX page fault patterns
//!
//! This tool traces page faults in MDBX memory-mapped regions to understand
//! trie traversal I/O patterns and identify optimization opportunities.

use clap::{Parser, Subcommand};
use libbpf_rs::{MapCore, MapFlags, ObjectBuilder, RingBufferBuilder};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tracing::{info, warn};

mod event;
mod mdbx;

use event::PageFaultEvent;

/// eBPF profiler for MDBX page fault patterns
#[derive(Parser)]
#[command(name = "mdbx-profiler")]
#[command(about = "Trace MDBX page faults to analyze trie access patterns")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Trace a running process
    Trace {
        /// PID of the process to trace
        #[arg(short, long)]
        pid: u32,

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
    },

    /// Find MDBX files for a process
    FindMdbx {
        /// PID of the process
        #[arg(short, long)]
        pid: u32,
    },

    /// Analyze a trace file
    Analyze {
        /// Input trace file
        #[arg(short, long)]
        input: PathBuf,

        /// Output format (json, csv, summary)
        #[arg(short, long, default_value = "summary")]
        format: String,
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
        Commands::Trace { pid, mdbx_path, output, duration, stats_interval } => {
            let dur: Option<Duration> = duration.map(|d| d.into());
            run_trace(pid, mdbx_path, output, dur, stats_interval)?;
        }
        Commands::FindMdbx { pid } => {
            find_mdbx_files(pid)?;
        }
        Commands::Analyze { input, format } => {
            analyze_trace(input, format)?;
        }
    }

    Ok(())
}

fn run_trace(
    pid: u32,
    mdbx_path: PathBuf,
    output: PathBuf,
    duration: Option<Duration>,
    stats_interval: u64,
) -> anyhow::Result<()> {
    info!("Starting trace for PID {} on MDBX path {:?}", pid, mdbx_path);

    // Get inode of MDBX file
    let metadata = std::fs::metadata(&mdbx_path)?;
    use std::os::unix::fs::MetadataExt;
    let inode = metadata.ino();
    info!("MDBX file inode: {}", inode);

    // Load BPF program
    let obj_path = std::env::current_exe()?.parent().unwrap().join("mdbx_tracer.bpf.o");

    info!("Loading BPF object from {:?}", obj_path);

    let mut builder = ObjectBuilder::default();
    let open_obj = builder.open_file(&obj_path)?;
    let mut obj = open_obj.load()?;

    // Configure target PID
    {
        let mut config_map = obj
            .maps_mut()
            .find(|m| m.name().to_string_lossy() == "profiler_config")
            .expect("profiler_config map not found");
        let key: u32 = 0;
        config_map.update(&key.to_ne_bytes(), &pid.to_ne_bytes(), MapFlags::ANY)?;
        info!("Configured target PID: {}", pid);
    }

    // Register MDBX inode for tracking
    {
        let mut tracked_inodes = obj
            .maps_mut()
            .find(|m| m.name().to_string_lossy() == "tracked_inodes")
            .expect("tracked_inodes map not found");
        let track_val: u8 = 1;
        tracked_inodes.update(&inode.to_ne_bytes(), &[track_val], MapFlags::ANY)?;
        info!("Registered inode {} for tracking", inode);
    }

    // Attach probes
    for prog in obj.progs_mut() {
        let name = prog.name().to_string_lossy().to_string();
        info!("Attaching program: {}", name);

        match prog.attach() {
            Ok(link) => {
                info!("Attached {} successfully", name);
                // Keep link alive
                std::mem::forget(link);
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
    let events_map =
        obj.maps().find(|m| m.name().to_string_lossy() == "events").expect("events map not found");

    let mut ring_builder = RingBufferBuilder::new();

    let event_count = Arc::new(AtomicU64::new(0));
    let event_count_clone = event_count.clone();
    let writer_clone = writer.clone();

    ring_builder.add(&events_map, move |data: &[u8]| {
        if data.len() < std::mem::size_of::<PageFaultEvent>() {
            return 0;
        }

        let event: PageFaultEvent =
            unsafe { std::ptr::read_unaligned(data.as_ptr() as *const PageFaultEvent) };

        event_count_clone.fetch_add(1, Ordering::Relaxed);

        // Write to output file as JSON line
        if let Ok(json) = serde_json::to_string(&event) {
            use std::io::Write;
            if let Ok(mut w) = writer_clone.lock() {
                let _ = writeln!(w, "{}", json);
            }
        }

        0
    })?;

    let ring = ring_builder.build()?;

    // Main loop
    let start = Instant::now();
    let mut last_stats = Instant::now();

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

        // Print stats periodically
        if last_stats.elapsed() >= Duration::from_secs(stats_interval) {
            let count = event_count.load(Ordering::Relaxed);
            let elapsed = start.elapsed().as_secs_f64();
            info!("Events: {} ({:.1}/s), Elapsed: {:.1}s", count, count as f64 / elapsed, elapsed);
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
    let stats_map =
        obj.maps().find(|m| m.name().to_string_lossy() == "stats").expect("stats map not found");
    print_stats(&stats_map)?;

    let event_total = event_count.load(Ordering::Relaxed);
    info!("Trace saved to {:?} ({} events)", output, event_total);
    Ok(())
}

fn print_stats(stats_map: &libbpf_rs::Map) -> anyhow::Result<()> {
    let stat_names = ["Total faults", "MDBX faults", "Major faults", "Events dropped"];

    info!("=== Statistics ===");
    for (i, name) in stat_names.iter().enumerate() {
        let key = (i as u32).to_ne_bytes();
        if let Some(val_bytes) = stats_map.lookup(&key, MapFlags::ANY)? {
            let arr: [u8; 8] = val_bytes.as_slice().try_into().unwrap_or([0; 8]);
            let val = u64::from_ne_bytes(arr);
            info!("{}: {}", name, val);
        }
    }
    Ok(())
}

fn find_mdbx_files(pid: u32) -> anyhow::Result<()> {
    info!("Finding MDBX files for PID {}", pid);

    let maps_path = format!("/proc/{}/maps", pid);
    let content = std::fs::read_to_string(&maps_path)?;

    println!("Memory-mapped files for PID {}:", pid);
    println!("{:<20} {:<20} {:<10} {}", "Start", "End", "Size", "Path");
    println!("{}", "-".repeat(80));

    for line in content.lines() {
        // Look for mdbx-related mappings
        if line.contains("mdbx") || line.contains(".dat") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 6 {
                let addr_range: Vec<&str> = parts[0].split('-').collect();
                if addr_range.len() == 2 {
                    let start = u64::from_str_radix(addr_range[0], 16).unwrap_or(0);
                    let end = u64::from_str_radix(addr_range[1], 16).unwrap_or(0);
                    let size = end - start;
                    let path = parts[5..].join(" ");

                    println!("0x{:016x} 0x{:016x} {:>10} {}", start, end, format_size(size), path);
                }
            }
        }
    }

    Ok(())
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn analyze_trace(input: PathBuf, format: String) -> anyhow::Result<()> {
    info!("Analyzing trace from {:?}", input);

    let file = std::fs::File::open(&input)?;
    let reader = std::io::BufReader::new(file);

    let mut events: Vec<PageFaultEvent> = Vec::new();

    use std::io::BufRead;
    for line in reader.lines() {
        let line = line?;
        if let Ok(event) = serde_json::from_str::<PageFaultEvent>(&line) {
            events.push(event);
        }
    }

    info!("Loaded {} events", events.len());

    match format.as_str() {
        "summary" => print_summary(&events),
        "csv" => print_csv(&events),
        "json" => println!("{}", serde_json::to_string_pretty(&events)?),
        other => anyhow::bail!("Unknown format: {}", other),
    }

    Ok(())
}

fn print_summary(events: &[PageFaultEvent]) {
    if events.is_empty() {
        println!("No events to analyze");
        return;
    }

    // Calculate statistics
    let total = events.len();
    let page_faults = events.iter().filter(|e| e.event_type == 1).count();

    // Time span
    let min_ts = events.iter().map(|e| e.timestamp_ns).min().unwrap_or(0);
    let max_ts = events.iter().map(|e| e.timestamp_ns).max().unwrap_or(0);
    let duration_s = (max_ts - min_ts) as f64 / 1_000_000_000.0;

    // File offset distribution
    let offsets: Vec<u64> = events.iter().map(|e| e.file_offset).collect();
    let min_offset = offsets.iter().min().copied().unwrap_or(0);
    let max_offset = offsets.iter().max().copied().unwrap_or(0);

    // Sequential vs random access analysis
    let mut sequential = 0;
    let mut random = 0;
    let page_size = 4096u64;

    for window in events.windows(2) {
        let diff = (window[1].file_offset as i64 - window[0].file_offset as i64).abs() as u64;
        if diff <= page_size * 4 {
            // Within 4 pages = sequential
            sequential += 1;
        } else {
            random += 1;
        }
    }

    println!("\n=== Trace Summary ===\n");
    println!("Duration:        {:.2}s", duration_s);
    println!("Total events:    {}", total);
    println!("Page faults:     {}", page_faults);
    println!("Fault rate:      {:.1}/s", page_faults as f64 / duration_s);
    println!();
    println!("File offset range:");
    println!("  Min: {} ({:.2} GB)", min_offset, min_offset as f64 / 1024.0 / 1024.0 / 1024.0);
    println!("  Max: {} ({:.2} GB)", max_offset, max_offset as f64 / 1024.0 / 1024.0 / 1024.0);
    println!();
    println!("Access pattern:");
    println!("  Sequential: {} ({:.1}%)", sequential, sequential as f64 / total as f64 * 100.0);
    println!("  Random:     {} ({:.1}%)", random, random as f64 / total as f64 * 100.0);

    // Thread distribution
    let mut thread_counts: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for event in events {
        *thread_counts.entry(event.tid).or_insert(0) += 1;
    }

    println!();
    println!("Thread distribution:");
    let mut threads: Vec<_> = thread_counts.iter().collect();
    threads.sort_by(|a, b| b.1.cmp(a.1));
    for (tid, count) in threads.iter().take(5) {
        println!("  TID {}: {} events ({:.1}%)", tid, count, **count as f64 / total as f64 * 100.0);
    }
}

fn print_csv(events: &[PageFaultEvent]) {
    println!("timestamp_ns,file_offset,address,tid,event_type,fault_flags");
    for e in events {
        println!(
            "{},{},{},{},{},{}",
            e.timestamp_ns, e.file_offset, e.address, e.tid, e.event_type, e.fault_flags
        );
    }
}
