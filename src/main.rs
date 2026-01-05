//! eBPF-based profiler for MDBX page fault patterns and cursor operations
//!
//! This tool traces:
//! 1. Page faults in MDBX memory-mapped regions to understand I/O patterns
//! 2. MDBX cursor operations (seeks, gets) to understand database access patterns

use clap::{Parser, Subcommand};
use libbpf_rs::{MapCore, MapFlags, ObjectBuilder, ProgramMut, RingBufferBuilder};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tracing::{debug, info, warn};

mod event;
mod mdbx;

use event::{CursorEvent, PageFaultEvent};

/// eBPF profiler for MDBX page fault patterns and cursor operations
#[derive(Parser)]
#[command(name = "mdbx-profiler")]
#[command(
    about = "Trace MDBX page faults and cursor operations to analyze database access patterns"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Trace a running process (page faults only)
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

        /// Also trace cursor operations (requires path to reth binary)
        #[arg(long)]
        trace_cursors: bool,

        /// Path to the reth binary (for cursor tracing)
        #[arg(long)]
        reth_binary: Option<PathBuf>,
    },

    /// Trace cursor operations only (experimental)
    TraceCursors {
        /// PID of the process to trace
        #[arg(short, long)]
        pid: u32,

        /// Path to the reth binary
        #[arg(short, long)]
        binary: PathBuf,

        /// Output file for trace data (JSON lines format)
        #[arg(short, long, default_value = "cursor-trace.jsonl")]
        output: PathBuf,

        /// Duration to trace (e.g., "30s", "5m")
        #[arg(short, long)]
        duration: Option<humantime::Duration>,

        /// Print live statistics every N seconds
        #[arg(long, default_value = "5")]
        stats_interval: u64,

        /// Also print events to stdout in log format (like issue 14558)
        #[arg(long)]
        print_logs: bool,
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
        Commands::Trace {
            pid,
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
                mdbx_path,
                output,
                dur,
                stats_interval,
                trace_cursors,
                reth_binary,
            )?;
        }
        Commands::TraceCursors {
            pid,
            binary,
            output,
            duration,
            stats_interval,
            print_logs,
        } => {
            let dur: Option<Duration> = duration.map(|d| d.into());
            run_cursor_trace(pid, binary, output, dur, stats_interval, print_logs)?;
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

/// Find the offset of a symbol in a binary using nm or objdump
fn find_symbol_offset(binary_path: &PathBuf, symbol: &str) -> Option<u64> {
    // Try nm first
    let output = std::process::Command::new("nm")
        .arg("-D")
        .arg(binary_path)
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains(symbol) && !line.contains("@@") {
            // Format: "0000000000123456 T symbol_name"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 && parts[2] == symbol {
                return u64::from_str_radix(parts[0], 16).ok();
            }
        }
    }

    None
}

fn run_trace(
    pid: u32,
    mdbx_path: PathBuf,
    output: PathBuf,
    duration: Option<Duration>,
    stats_interval: u64,
    trace_cursors: bool,
    reth_binary: Option<PathBuf>,
) -> anyhow::Result<()> {
    info!(
        "Starting trace for PID {} on MDBX path {:?}",
        pid, mdbx_path
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

    // Configure target PID
    {
        let config_map = obj
            .maps_mut()
            .find(|m| m.name().to_string_lossy() == "profiler_config")
            .expect("profiler_config map not found");
        let key: u32 = 0;
        config_map.update(&key.to_ne_bytes(), &pid.to_ne_bytes(), MapFlags::ANY)?;
        info!("Configured target PID: {}", pid);
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

        // Skip uprobe programs in this mode unless trace_cursors is enabled
        if name.contains("cursor") && !trace_cursors {
            debug!("Skipping cursor probe: {}", name);
            continue;
        }

        // Handle uprobes separately
        if name.contains("uprobe") {
            if trace_cursors {
                // Find the binary to attach to
                let binary = if let Some(ref bin) = reth_binary {
                    bin.clone()
                } else {
                    // Try to find libmdbx in the process
                    match find_libmdbx_path(pid) {
                        Some(p) => p,
                        None => {
                            warn!(
                                "Could not find libmdbx for PID {}. Use --reth-binary to specify.",
                                pid
                            );
                            continue;
                        }
                    }
                };

                info!("Attaching uprobe {} to {:?}", name, binary);

                // Find the symbol offset
                let func_name = if name.contains("cursor_get") {
                    "mdbx_cursor_get"
                } else {
                    continue;
                };

                match find_symbol_offset(&binary, func_name) {
                    Some(offset) => {
                        let is_ret = name.contains("uretprobe");
                        match prog.attach_uprobe(is_ret, pid as i32, &binary, offset as usize) {
                            Ok(link) => {
                                info!("Attached {} at offset 0x{:x}", name, offset);
                                _links.push(link);
                            }
                            Err(e) => {
                                warn!("Failed to attach {}: {}", name, e);
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
    let event_count_clone = event_count.clone();
    let cursor_count_clone = cursor_count.clone();
    let writer_clone = writer.clone();

    ring_builder.add(&events_map, move |data: &[u8]| {
        // Check event type to determine which struct to use
        // Event type is at different offsets for different structs
        // PageFaultEvent: offset 40 (after timestamp, address, file_offset, vma_start, vma_end)
        // CursorEvent: offset 16 (after timestamp, pid, tid)

        if data.len() >= std::mem::size_of::<PageFaultEvent>() {
            // Try to read event type at PageFaultEvent offset
            let event_type = if data.len() >= 44 {
                u32::from_ne_bytes(data[40..44].try_into().unwrap_or([0; 4]))
            } else {
                0
            };

            if event_type == 1 || event_type == 2 {
                // Page fault or mmap event
                let event: PageFaultEvent =
                    unsafe { std::ptr::read_unaligned(data.as_ptr() as *const PageFaultEvent) };

                event_count_clone.fetch_add(1, Ordering::Relaxed);

                if let Ok(json) = serde_json::to_string(&event) {
                    use std::io::Write;
                    if let Ok(mut w) = writer_clone.lock() {
                        let _ = writeln!(w, "{}", json);
                    }
                }
            } else if event_type == 3 || event_type == 4 {
                // Cursor event - need to read from different struct layout
                if data.len() >= std::mem::size_of::<CursorEvent>() {
                    let event: CursorEvent =
                        unsafe { std::ptr::read_unaligned(data.as_ptr() as *const CursorEvent) };

                    cursor_count_clone.fetch_add(1, Ordering::Relaxed);

                    if let Ok(json) = serde_json::to_string(&event) {
                        use std::io::Write;
                        if let Ok(mut w) = writer_clone.lock() {
                            let _ = writeln!(w, "{}", json);
                        }
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
            let pf_count = event_count.load(Ordering::Relaxed);
            let cur_count = cursor_count.load(Ordering::Relaxed);
            let elapsed = start.elapsed().as_secs_f64();
            info!(
                "Page faults: {} ({:.1}/s), Cursor ops: {} ({:.1}/s), Elapsed: {:.1}s",
                pf_count,
                pf_count as f64 / elapsed,
                cur_count,
                cur_count as f64 / elapsed,
                elapsed
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
    info!(
        "Trace saved to {:?} ({} page faults, {} cursor ops)",
        output, pf_total, cur_total
    );
    Ok(())
}

fn run_cursor_trace(
    pid: u32,
    binary: PathBuf,
    output: PathBuf,
    duration: Option<Duration>,
    stats_interval: u64,
    print_logs: bool,
) -> anyhow::Result<()> {
    info!(
        "Starting cursor trace for PID {} on binary {:?}",
        pid, binary
    );

    // Verify binary exists
    if !binary.exists() {
        anyhow::bail!("Binary not found: {:?}", binary);
    }

    // Find mdbx_cursor_get symbol
    let offset = find_symbol_offset(&binary, "mdbx_cursor_get")
        .ok_or_else(|| anyhow::anyhow!("Could not find mdbx_cursor_get in {:?}", binary))?;
    info!("Found mdbx_cursor_get at offset 0x{:x}", offset);

    // Load BPF program
    let obj_path = std::env::current_exe()?
        .parent()
        .unwrap()
        .join("mdbx_tracer.bpf.o");

    info!("Loading BPF object from {:?}", obj_path);

    let mut builder = ObjectBuilder::default();
    let open_obj = builder.open_file(&obj_path)?;
    let mut obj = open_obj.load()?;

    // Configure target PID
    {
        let config_map = obj
            .maps_mut()
            .find(|m| m.name().to_string_lossy() == "profiler_config")
            .expect("profiler_config map not found");
        let key: u32 = 0;
        config_map.update(&key.to_ne_bytes(), &pid.to_ne_bytes(), MapFlags::ANY)?;
        info!("Configured target PID: {}", pid);
    }

    // Keep track of attached links
    let mut _links: Vec<libbpf_rs::Link> = Vec::new();

    // Attach uprobes for cursor tracing
    for prog in obj.progs_mut() {
        let name = prog.name().to_string_lossy().to_string();

        // Only attach cursor-related uprobes
        if !name.contains("cursor") {
            continue;
        }

        let is_ret = name.contains("uretprobe") || name.contains("_ret");
        info!("Attaching {} (retprobe: {})", name, is_ret);

        match prog.attach_uprobe(is_ret, pid as i32, &binary, offset as usize) {
            Ok(link) => {
                info!("Attached {} successfully", name);
                _links.push(link);
            }
            Err(e) => {
                warn!("Failed to attach {}: {}", name, e);
            }
        }
    }

    if _links.is_empty() {
        anyhow::bail!("No probes were attached. Check that the BPF program has cursor probes.");
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
    let seek_count = Arc::new(AtomicU64::new(0));
    let event_count_clone = event_count.clone();
    let seek_count_clone = seek_count.clone();
    let writer_clone = writer.clone();

    // DBI to table name mapping (will be populated dynamically)
    // For now, use the DBI number directly
    let dbi_names: Arc<std::sync::Mutex<std::collections::HashMap<u32, String>>> =
        Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    let dbi_names_clone = dbi_names.clone();

    ring_builder.add(&events_map, move |data: &[u8]| {
        if data.len() < std::mem::size_of::<CursorEvent>() {
            return 0;
        }

        // Check if this is a cursor event (event_type at offset 16)
        let event_type = if data.len() >= 20 {
            u32::from_ne_bytes(data[16..20].try_into().unwrap_or([0; 4]))
        } else {
            return 0;
        };

        if event_type != 3 && event_type != 4 {
            return 0;
        }

        let event: CursorEvent =
            unsafe { std::ptr::read_unaligned(data.as_ptr() as *const CursorEvent) };

        event_count_clone.fetch_add(1, Ordering::Relaxed);

        // Track seeks
        if event.cursor_op().is_seek() {
            seek_count_clone.fetch_add(1, Ordering::Relaxed);
        }

        // Get table name (use DBI for now, could be enhanced later)
        let table_name = {
            let names = dbi_names_clone.lock().unwrap();
            names
                .get(&event.dbi)
                .cloned()
                .unwrap_or_else(|| format!("DBI_{}", event.dbi))
        };

        // Print log if requested
        if print_logs {
            println!("{}", event.format_log(&table_name));
        }

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

    info!("Cursor tracing started. Press Ctrl+C to stop.");
    if print_logs {
        info!("Printing cursor operations to stdout...\n");
    }

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

        // Print stats periodically (only if not printing logs)
        if !print_logs && last_stats.elapsed() >= Duration::from_secs(stats_interval) {
            let count = event_count.load(Ordering::Relaxed);
            let seeks = seek_count.load(Ordering::Relaxed);
            let elapsed = start.elapsed().as_secs_f64();
            info!(
                "Cursor ops: {} ({:.1}/s), Seeks: {} ({:.1}%), Elapsed: {:.1}s",
                count,
                count as f64 / elapsed,
                seeks,
                if count > 0 {
                    seeks as f64 / count as f64 * 100.0
                } else {
                    0.0
                },
                elapsed
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
    print_cursor_stats(&stats_map)?;

    let total = event_count.load(Ordering::Relaxed);
    info!("Trace saved to {:?} ({} cursor ops)", output, total);
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

fn print_cursor_stats(stats_map: &libbpf_rs::Map) -> anyhow::Result<()> {
    let stat_names = [
        ("Cursor ops", 4),
        ("Cursor seeks", 5),
        ("Cursor nexts", 6),
        ("Cursor errors", 7),
        ("Events dropped", 3),
    ];

    info!("=== Cursor Statistics ===");
    for (name, idx) in stat_names.iter() {
        let key = (*idx as u32).to_ne_bytes();
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

                    println!(
                        "0x{:016x} 0x{:016x} {:>10} {}",
                        start,
                        end,
                        format_size(size),
                        path
                    );
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

    let mut page_fault_events: Vec<PageFaultEvent> = Vec::new();
    let mut cursor_events: Vec<CursorEvent> = Vec::new();

    use std::io::BufRead;
    for line in reader.lines() {
        let line = line?;
        // Try to parse as page fault event first
        if let Ok(event) = serde_json::from_str::<PageFaultEvent>(&line) {
            if event.event_type == 1 || event.event_type == 2 {
                page_fault_events.push(event);
                continue;
            }
        }
        // Try to parse as cursor event
        if let Ok(event) = serde_json::from_str::<CursorEvent>(&line) {
            cursor_events.push(event);
        }
    }

    info!(
        "Loaded {} page fault events, {} cursor events",
        page_fault_events.len(),
        cursor_events.len()
    );

    match format.as_str() {
        "summary" => {
            if !page_fault_events.is_empty() {
                print_summary(&page_fault_events);
            }
            if !cursor_events.is_empty() {
                print_cursor_summary(&cursor_events);
            }
        }
        "csv" => {
            if !page_fault_events.is_empty() {
                print_csv(&page_fault_events);
            }
            if !cursor_events.is_empty() {
                print_cursor_csv(&cursor_events);
            }
        }
        "json" => {
            if !page_fault_events.is_empty() {
                println!("{}", serde_json::to_string_pretty(&page_fault_events)?);
            }
            if !cursor_events.is_empty() {
                println!("{}", serde_json::to_string_pretty(&cursor_events)?);
            }
        }
        "logs" => {
            // Print cursor events in issue 14558 log format
            for event in &cursor_events {
                let table_name = format!("DBI_{}", event.dbi);
                println!("{}", event.format_log(&table_name));
            }
        }
        other => anyhow::bail!("Unknown format: {}", other),
    }

    Ok(())
}

fn print_summary(events: &[PageFaultEvent]) {
    if events.is_empty() {
        println!("No page fault events to analyze");
        return;
    }

    let total = events.len();
    let page_faults = events.iter().filter(|e| e.event_type == 1).count();

    let min_ts = events.iter().map(|e| e.timestamp_ns).min().unwrap_or(0);
    let max_ts = events.iter().map(|e| e.timestamp_ns).max().unwrap_or(0);
    let duration_s = (max_ts - min_ts) as f64 / 1_000_000_000.0;

    let offsets: Vec<u64> = events.iter().map(|e| e.file_offset).collect();
    let min_offset = offsets.iter().min().copied().unwrap_or(0);
    let max_offset = offsets.iter().max().copied().unwrap_or(0);

    let mut sequential = 0;
    let mut random = 0;
    let page_size = 4096u64;

    for window in events.windows(2) {
        let diff = (window[1].file_offset as i64 - window[0].file_offset as i64).abs() as u64;
        if diff <= page_size * 4 {
            sequential += 1;
        } else {
            random += 1;
        }
    }

    println!("\n=== Page Fault Summary ===\n");
    println!("Duration:        {:.2}s", duration_s);
    println!("Total events:    {}", total);
    println!("Page faults:     {}", page_faults);
    println!("Fault rate:      {:.1}/s", page_faults as f64 / duration_s);
    println!();
    println!("File offset range:");
    println!(
        "  Min: {} ({:.2} GB)",
        min_offset,
        min_offset as f64 / 1024.0 / 1024.0 / 1024.0
    );
    println!(
        "  Max: {} ({:.2} GB)",
        max_offset,
        max_offset as f64 / 1024.0 / 1024.0 / 1024.0
    );
    println!();
    println!("Access pattern:");
    println!(
        "  Sequential: {} ({:.1}%)",
        sequential,
        sequential as f64 / total as f64 * 100.0
    );
    println!(
        "  Random:     {} ({:.1}%)",
        random,
        random as f64 / total as f64 * 100.0
    );

    let mut thread_counts: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for event in events {
        *thread_counts.entry(event.tid).or_insert(0) += 1;
    }

    println!();
    println!("Thread distribution:");
    let mut threads: Vec<_> = thread_counts.iter().collect();
    threads.sort_by(|a, b| b.1.cmp(a.1));
    for (tid, count) in threads.iter().take(5) {
        println!(
            "  TID {}: {} events ({:.1}%)",
            tid,
            count,
            **count as f64 / total as f64 * 100.0
        );
    }
}

fn print_cursor_summary(events: &[CursorEvent]) {
    if events.is_empty() {
        println!("No cursor events to analyze");
        return;
    }

    let total = events.len();

    let min_ts = events.iter().map(|e| e.timestamp_ns).min().unwrap_or(0);
    let max_ts = events.iter().map(|e| e.timestamp_ns).max().unwrap_or(0);
    let duration_s = (max_ts - min_ts) as f64 / 1_000_000_000.0;

    // Count by operation type
    let mut op_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut dbi_counts: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    let mut seek_count = 0;
    let mut nav_count = 0;
    let mut error_count = 0;
    let mut total_latency_ns: u64 = 0;

    for event in events {
        let op = event.cursor_op();
        *op_counts.entry(op.to_string()).or_insert(0) += 1;
        *dbi_counts.entry(event.dbi).or_insert(0) += 1;

        if op.is_seek() {
            seek_count += 1;
        }
        if op.is_navigation() {
            nav_count += 1;
        }
        if !event.is_success() && !event.is_not_found() {
            error_count += 1;
        }
        total_latency_ns += event.latency_ns;
    }

    let avg_latency_us = (total_latency_ns as f64 / total as f64) / 1000.0;

    println!("\n=== Cursor Operation Summary ===\n");
    println!("Duration:        {:.2}s", duration_s);
    println!("Total ops:       {}", total);
    println!("Op rate:         {:.1}/s", total as f64 / duration_s);
    println!("Avg latency:     {:.2} us", avg_latency_us);
    println!();
    println!("Operation breakdown:");
    println!(
        "  Seeks:         {} ({:.1}%)",
        seek_count,
        seek_count as f64 / total as f64 * 100.0
    );
    println!(
        "  Navigation:    {} ({:.1}%)",
        nav_count,
        nav_count as f64 / total as f64 * 100.0
    );
    println!("  Errors:        {}", error_count);

    println!();
    println!("Top operations:");
    let mut ops: Vec<_> = op_counts.iter().collect();
    ops.sort_by(|a, b| b.1.cmp(a.1));
    for (op, count) in ops.iter().take(10) {
        println!(
            "  {}: {} ({:.1}%)",
            op,
            count,
            **count as f64 / total as f64 * 100.0
        );
    }

    println!();
    println!("Top DBIs (tables):");
    let mut dbis: Vec<_> = dbi_counts.iter().collect();
    dbis.sort_by(|a, b| b.1.cmp(a.1));
    for (dbi, count) in dbis.iter().take(10) {
        println!(
            "  DBI {}: {} ({:.1}%)",
            dbi,
            count,
            **count as f64 / total as f64 * 100.0
        );
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

fn print_cursor_csv(events: &[CursorEvent]) {
    println!("timestamp_ns,tid,dbi,cursor_op,key_hex,return_code,latency_ns");
    for e in events {
        println!(
            "{},{},{},{},{},{},{}",
            e.timestamp_ns,
            e.tid,
            e.dbi,
            e.cursor_op().name(),
            e.key_hex(),
            e.return_code,
            e.latency_ns
        );
    }
}
