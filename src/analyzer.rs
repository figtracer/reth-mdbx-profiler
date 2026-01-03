//! Trace analyzer - runs on your Mac to analyze traces collected from the node

use clap::Parser;
use std::{
    collections::{BTreeMap, HashMap},
    io::BufRead,
    path::PathBuf,
};

mod event;
use event::{PageFaultEvent, TraceStats};

/// Analyze MDBX page fault traces from MDBX
#[derive(Parser)]
#[command(name = "mdbx-trace-analyzer")]
struct Cli {
    /// Input trace file (JSON lines format)
    #[arg(short, long)]
    input: PathBuf,

    /// Output format: summary, csv, heatmap, pattern
    #[arg(short, long, default_value = "summary")]
    format: String,

    /// Time bucket size in milliseconds (for pattern analysis)
    #[arg(long, default_value = "100")]
    bucket_ms: u64,

    /// Output file (stdout if not specified)
    #[arg(short, long)]
    output: Option<PathBuf>,
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

    eprintln!("Loaded {} events ({} parse errors)", events.len(), parse_errors);

    if events.is_empty() {
        eprintln!("No events to analyze");
        return Ok(());
    }

    // Sort by timestamp
    events.sort_by_key(|e| e.timestamp_ns);

    match cli.format.as_str() {
        "summary" => print_summary(&events),
        "csv" => print_csv(&events),
        "heatmap" => print_heatmap(&events),
        "pattern" => print_pattern_analysis(&events, cli.bucket_ms),
        "prefetch" => print_prefetch_analysis(&events),
        _ => {
            eprintln!("Unknown format: {}", cli.format);
            std::process::exit(1);
        }
    }

    Ok(())
}

fn print_summary(events: &[PageFaultEvent]) {
    let stats = compute_stats(events);

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║              MDBX Page Fault Trace Summary                    ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!(
        "║ Duration:           {:>10.2} seconds                       ║",
        stats.duration_ns as f64 / 1e9
    );
    println!("║ Total Events:       {:>10}                               ║", stats.total_events);
    println!("║ Page Faults:        {:>10}                               ║", stats.page_faults);
    println!("║ Major Faults:       {:>10}                               ║", stats.major_faults);
    println!("║ Unique Pages:       {:>10}                               ║", stats.unique_pages);
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!(
        "║ Fault Rate:         {:>10.1} /sec                         ║",
        stats.fault_rate_per_sec()
    );
    println!(
        "║ Sequential Ratio:   {:>10.1}%                             ║",
        stats.sequential_ratio() * 100.0
    );
    println!("╚══════════════════════════════════════════════════════════════╝");

    // Page access frequency distribution
    println!("\n=== Page Access Frequency ===\n");
    let mut page_counts: HashMap<u64, usize> = HashMap::new();
    for e in events {
        *page_counts.entry(e.page_number()).or_insert(0) += 1;
    }

    // Histogram of access counts
    let mut freq_dist: BTreeMap<usize, usize> = BTreeMap::new();
    for &count in page_counts.values() {
        let bucket = match count {
            1 => 1,
            2..=5 => 5,
            6..=10 => 10,
            11..=50 => 50,
            51..=100 => 100,
            _ => 1000,
        };
        *freq_dist.entry(bucket).or_insert(0) += 1;
    }

    println!("Access count | Pages");
    println!("-------------|-------");
    for (bucket, count) in &freq_dist {
        let label = match *bucket {
            1 => "     1".to_string(),
            5 => "   2-5".to_string(),
            10 => "  6-10".to_string(),
            50 => " 11-50".to_string(),
            100 => "51-100".to_string(),
            _ => "  >100".to_string(),
        };
        let bar = "█".repeat((*count as f64).log2() as usize * 2);
        println!("{}      | {:>6} {}", label, count, bar);
    }

    // Hot pages (most accessed)
    println!("\n=== Hot Pages (Top 10) ===\n");
    let mut sorted_pages: Vec<_> = page_counts.iter().collect();
    sorted_pages.sort_by(|a, b| b.1.cmp(a.1));

    println!("Page Number    | Offset (GB)  | Accesses");
    println!("---------------|--------------|----------");
    for (page, count) in sorted_pages.iter().take(10) {
        let offset_gb = (*page * 4096) as f64 / 1024.0 / 1024.0 / 1024.0;
        println!("{:>14} | {:>11.4} | {:>8}", page, offset_gb, count);
    }

    // Thread analysis
    println!("\n=== Thread Distribution ===\n");
    let mut thread_counts: HashMap<u32, usize> = HashMap::new();
    for e in events {
        *thread_counts.entry(e.tid).or_insert(0) += 1;
    }

    let mut sorted_threads: Vec<_> = thread_counts.iter().collect();
    sorted_threads.sort_by(|a, b| b.1.cmp(a.1));

    println!("Thread ID  | Events  | Percentage");
    println!("-----------|---------|------------");
    for (tid, count) in sorted_threads.iter().take(8) {
        let pct = **count as f64 / events.len() as f64 * 100.0;
        println!("{:>10} | {:>7} | {:>6.1}%", tid, count, pct);
    }
}

fn compute_stats(events: &[PageFaultEvent]) -> TraceStats {
    let mut stats = TraceStats::default();

    if events.is_empty() {
        return stats;
    }

    stats.total_events = events.len() as u64;
    stats.duration_ns = events.last().unwrap().timestamp_ns - events.first().unwrap().timestamp_ns;

    let mut unique_pages: std::collections::HashSet<u64> = std::collections::HashSet::new();

    for e in events {
        if e.event_type == 1 {
            // PageFault
            stats.page_faults += 1;
        }
        if e.is_major_fault() {
            stats.major_faults += 1;
        }
        unique_pages.insert(e.page_number());
    }

    stats.unique_pages = unique_pages.len() as u64;

    // Sequential vs random analysis
    for window in events.windows(2) {
        let page_diff = (window[1].page_number() as i64 - window[0].page_number() as i64).abs();
        if page_diff <= 4 {
            stats.sequential_accesses += 1;
        } else {
            stats.random_accesses += 1;
        }
    }

    stats
}

fn print_csv(events: &[PageFaultEvent]) {
    println!("timestamp_ns,file_offset,page_number,address,tid,is_major,tree_level");
    for e in events {
        println!(
            "{},{},{},{},{},{},{}",
            e.timestamp_ns,
            e.file_offset,
            e.page_number(),
            e.address,
            e.tid,
            e.is_major_fault() as u8,
            e.estimated_tree_level()
        );
    }
}

fn print_heatmap(events: &[PageFaultEvent]) {
    // Create a 2D heatmap: time (x) vs file offset (y)
    println!("\n=== Access Heatmap (Time vs File Offset) ===\n");

    if events.len() < 2 {
        println!("Not enough events for heatmap");
        return;
    }

    let min_ts = events.first().unwrap().timestamp_ns;
    let max_ts = events.last().unwrap().timestamp_ns;
    let min_offset = events.iter().map(|e| e.file_offset).min().unwrap();
    let max_offset = events.iter().map(|e| e.file_offset).max().unwrap();

    let time_buckets = 60; // columns
    let offset_buckets = 20; // rows

    let time_bucket_size = (max_ts - min_ts) / time_buckets as u64;
    let offset_bucket_size = (max_offset - min_offset) / offset_buckets as u64;

    if time_bucket_size == 0 || offset_bucket_size == 0 {
        println!("Trace too short for meaningful heatmap");
        return;
    }

    let mut heatmap = vec![vec![0u32; time_buckets]; offset_buckets];

    for e in events {
        let time_idx =
            ((e.timestamp_ns - min_ts) / time_bucket_size).min(time_buckets as u64 - 1) as usize;
        let offset_idx = ((e.file_offset - min_offset) / offset_bucket_size)
            .min(offset_buckets as u64 - 1) as usize;
        heatmap[offset_idx][time_idx] += 1;
    }

    let max_count = heatmap.iter().flat_map(|r| r.iter()).max().copied().unwrap_or(1);

    // Print with ASCII art
    let chars = [' ', '░', '▒', '▓', '█'];

    for (row_idx, row) in heatmap.iter().enumerate().rev() {
        let offset_gb = (min_offset + row_idx as u64 * offset_bucket_size) as f64 / 1e9;
        print!("{:>6.2}GB │", offset_gb);
        for &count in row {
            let intensity = (count as f64 / max_count as f64 * 4.0) as usize;
            print!("{}", chars[intensity.min(4)]);
        }
        println!();
    }

    println!("         └{}", "─".repeat(time_buckets));
    println!("          Time →");
}

fn print_pattern_analysis(events: &[PageFaultEvent], bucket_ms: u64) {
    println!("\n=== Access Pattern Analysis ===\n");

    if events.len() < 10 {
        println!("Not enough events for pattern analysis");
        return;
    }

    // Analyze stride patterns
    let mut strides: Vec<i64> = Vec::new();
    for window in events.windows(2) {
        let stride = window[1].file_offset as i64 - window[0].file_offset as i64;
        strides.push(stride);
    }

    // Find common strides
    let mut stride_counts: HashMap<i64, usize> = HashMap::new();
    for &stride in &strides {
        // Bucket strides to page granularity
        let bucketed = (stride / 4096) * 4096;
        *stride_counts.entry(bucketed).or_insert(0) += 1;
    }

    let mut sorted_strides: Vec<_> = stride_counts.iter().collect();
    sorted_strides.sort_by(|a, b| b.1.cmp(a.1));

    println!("Most Common Strides (page-aligned):");
    println!("Stride (pages) | Stride (bytes) | Count | Pattern");
    println!("---------------|----------------|-------|--------");

    for (stride, count) in sorted_strides.iter().take(10) {
        let pages = *stride / 4096;
        let pattern = match pages {
            0 => "same-page",
            1 => "sequential-forward",
            -1 => "sequential-backward",
            2..=4 => "near-sequential",
            -4..=-2 => "near-sequential-back",
            _ if pages > 100 => "random-jump",
            _ if pages < -100 => "random-jump-back",
            _ => "medium-jump",
        };
        println!("{:>14} | {:>14} | {:>5} | {}", pages, stride, count, pattern);
    }

    // Burst analysis - events clustered in time
    println!("\n=== Burst Analysis ===\n");

    let bucket_ns = bucket_ms * 1_000_000;
    let min_ts = events.first().unwrap().timestamp_ns;

    let mut buckets: BTreeMap<u64, Vec<&PageFaultEvent>> = BTreeMap::new();
    for e in events {
        let bucket = (e.timestamp_ns - min_ts) / bucket_ns;
        buckets.entry(bucket).or_default().push(e);
    }

    // Find bursty periods
    let mut burst_sizes: Vec<usize> = buckets.values().map(|v| v.len()).collect();
    burst_sizes.sort();

    let median = burst_sizes[burst_sizes.len() / 2];
    let p95 = burst_sizes[(burst_sizes.len() as f64 * 0.95) as usize];
    let max = *burst_sizes.last().unwrap();

    println!("Events per {}ms bucket:", bucket_ms);
    println!("  Median: {}", median);
    println!("  P95:    {}", p95);
    println!("  Max:    {}", max);

    // Identify high-activity bursts
    println!("\nHigh-activity bursts (>{} events):", p95);
    let mut burst_count = 0;
    for (bucket_idx, events) in &buckets {
        if events.len() > p95 {
            burst_count += 1;
            if burst_count <= 5 {
                let time_ms = bucket_idx * bucket_ms;
                println!("  +{}ms: {} events", time_ms, events.len());
            }
        }
    }
    if burst_count > 5 {
        println!("  ... and {} more bursts", burst_count - 5);
    }
}

fn print_prefetch_analysis(events: &[PageFaultEvent]) {
    println!("\n=== Prefetch Opportunity Analysis ===\n");

    if events.len() < 100 {
        println!("Not enough events for prefetch analysis");
        return;
    }

    // Analyze if we could have predicted accesses
    let window_size = 10; // Look at last N accesses to predict next
    let lookahead = 5; // How many future accesses to check

    let mut correct_predictions = 0;
    let mut total_predictions = 0;

    for i in window_size..(events.len() - lookahead) {
        // Simple heuristic: predict continuation of stride pattern
        let recent_strides: Vec<i64> = (0..window_size - 1)
            .map(|j| {
                events[i - window_size + j + 1].file_offset as i64 -
                    events[i - window_size + j].file_offset as i64
            })
            .collect();

        // Average stride
        let avg_stride: i64 = recent_strides.iter().sum::<i64>() / recent_strides.len() as i64;

        // Predict next pages
        let current_offset = events[i].file_offset;
        let predictions: Vec<u64> = (1..=lookahead)
            .map(|j| (current_offset as i64 + avg_stride * j as i64) as u64)
            .collect();

        // Check if predictions match actual
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

    let hit_rate = correct_predictions as f64 / total_predictions as f64 * 100.0;

    println!("Simple stride-based prediction:");
    println!("  Total predictions:   {}", total_predictions);
    println!("  Correct predictions: {}", correct_predictions);
    println!("  Hit rate:            {:.1}%", hit_rate);

    if hit_rate > 30.0 {
        println!("\n✓ GOOD NEWS: Access patterns are somewhat predictable!");
        println!("  Prefetching could reduce ~{:.0}% of page faults", hit_rate * 0.8);
    } else if hit_rate > 15.0 {
        println!("\n~ MIXED: Some predictability, but limited benefit");
        println!("  Prefetching might help for ~{:.0}% of accesses", hit_rate * 0.8);
    } else {
        println!("\n✗ CHALLENGING: Access patterns are mostly random");
        println!("  Simple prefetching unlikely to help significantly");
        println!("  Consider: larger page sizes, caching strategies, or mlock()");
    }

    // Locality analysis
    println!("\n=== Locality Analysis ===\n");

    let mut locality_windows: Vec<f64> = Vec::new();
    let locality_window = 100;

    for chunk in events.chunks(locality_window) {
        let unique_pages: std::collections::HashSet<u64> =
            chunk.iter().map(|e| e.page_number()).collect();
        let locality = unique_pages.len() as f64 / chunk.len() as f64;
        locality_windows.push(locality);
    }

    let avg_locality: f64 = locality_windows.iter().sum::<f64>() / locality_windows.len() as f64;

    println!("Locality (unique pages / accesses in window of {}):", locality_window);
    println!("  Average: {:.2}", avg_locality);
    println!("  (1.0 = every access is new page, lower = more reuse)");

    if avg_locality < 0.5 {
        println!("\n✓ Good temporal locality - caching will help!");
    } else if avg_locality < 0.8 {
        println!("\n~ Moderate locality - some caching benefit");
    } else {
        println!("\n✗ Poor locality - mostly unique page accesses");
    }
}
