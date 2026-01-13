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

use event::{CursorEvent, PageFaultEvent, TxnEvent};

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
    }

    Ok(())
}

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
        const CURSOR_EVENT_SIZE: usize = 120;
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
