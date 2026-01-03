//! Web-based trace viewer
//!
//! Generates a self-contained HTML file with interactive visualizations
//! for MDBX page fault traces.

mod template;

use crate::event::PageFaultEvent;
use crate::mdbx_metadata::{PageAttribution, RethTable};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

/// Data structure for the web viewer
#[derive(Debug, Serialize)]
pub struct ViewerData {
    /// Summary statistics
    pub summary: TraceSummary,
    /// Timeline data (sampled for performance)
    pub timeline: Vec<TimelinePoint>,
    /// Table breakdown
    pub tables: Vec<TableStats>,
    /// Thread distribution
    pub threads: Vec<ThreadStats>,
    /// Hot pages
    pub hot_pages: Vec<HotPage>,
    /// Access pattern analysis
    pub patterns: PatternAnalysis,
    /// Prefetch analysis
    pub prefetch: PrefetchAnalysis,
    /// Heatmap data (2D grid)
    pub heatmap: HeatmapData,
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
}

#[derive(Debug, Serialize)]
pub struct ThreadStats {
    pub tid: u32,
    pub faults: u64,
    pub percentage: f64,
}

#[derive(Debug, Serialize)]
pub struct HotPage {
    pub page_number: u64,
    pub offset_gb: f64,
    pub accesses: u64,
    pub table: String,
    pub major_faults: u64,
}

#[derive(Debug, Serialize)]
pub struct PatternAnalysis {
    pub sequential_ratio: f64,
    pub random_ratio: f64,
    pub stride_distribution: Vec<StrideInfo>,
    pub burst_stats: BurstStats,
}

#[derive(Debug, Serialize)]
pub struct StrideInfo {
    pub stride_pages: i64,
    pub count: u64,
    pub pattern_type: String,
}

#[derive(Debug, Serialize)]
pub struct BurstStats {
    pub median_events: u32,
    pub p95_events: u32,
    pub max_events: u32,
    pub bucket_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct PrefetchAnalysis {
    pub prediction_hit_rate: f64,
    pub locality_score: f64,
    pub recommendation: String,
    pub prefetch_benefit_estimate: f64,
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
    attribution: Option<&PageAttribution>,
) -> ViewerData {
    let page_faults: Vec<_> = events.iter().filter(|e| e.event_type == 1).collect();

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
            },
            timeline: vec![],
            tables: vec![],
            threads: vec![],
            hot_pages: vec![],
            patterns: PatternAnalysis {
                sequential_ratio: 0.0,
                random_ratio: 0.0,
                stride_distribution: vec![],
                burst_stats: BurstStats {
                    median_events: 0,
                    p95_events: 0,
                    max_events: 0,
                    bucket_ms: 100,
                },
            },
            prefetch: PrefetchAnalysis {
                prediction_hit_rate: 0.0,
                locality_score: 0.0,
                recommendation: "No data".to_string(),
                prefetch_benefit_estimate: 0.0,
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
    };

    // Generate timeline (bucket by 100ms intervals, sample if too many)
    let timeline = generate_timeline(&page_faults, min_ts, duration_ns);

    // Table breakdown
    let tables = generate_table_stats(&page_faults, attribution);

    // Thread distribution
    let threads = generate_thread_stats(&page_faults);

    // Hot pages
    let hot_pages = generate_hot_pages(&page_faults, attribution);

    // Pattern analysis
    let patterns = analyze_patterns(&page_faults);

    // Prefetch analysis
    let prefetch = analyze_prefetch(&page_faults);

    // Heatmap
    let heatmap = generate_heatmap(&page_faults, min_ts, duration_ns, min_offset, max_offset);

    ViewerData {
        summary,
        timeline,
        tables,
        threads,
        hot_pages,
        patterns,
        prefetch,
        heatmap,
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

fn generate_table_stats(
    events: &[&PageFaultEvent],
    attribution: Option<&PageAttribution>,
) -> Vec<TableStats> {
    // If we have mdbx_stat data, use proportion-based attribution
    // Since we can't map individual pages to tables, we distribute faults
    // proportionally based on each table's share of total pages
    if let Some(attr) = attribution {
        if let Some(mdbx_stats) = attr.get_mdbx_stats() {
            return generate_table_stats_from_mdbx(events, mdbx_stats);
        }
    }

    // Fallback: try to attribute based on page-level mapping
    let mut table_counts: HashMap<RethTable, (u64, u64)> = HashMap::new();
    let page_size = attribution.map(|a| a.page_size()).unwrap_or(4096);

    for e in events {
        let table = if let Some(attr) = attribution {
            attr.get_table_for_offset(e.file_offset)
                .unwrap_or(RethTable::Unknown(0))
        } else {
            // Use heuristic based on offset
            crate::mdbx_metadata::estimate_table_from_pattern(e.file_offset, page_size, 0, None)
        };

        let entry = table_counts.entry(table).or_insert((0, 0));
        entry.0 += 1;
        if e.is_major_fault() {
            entry.1 += 1;
        }
    }

    let total = events.len() as f64;
    let mut stats: Vec<_> = table_counts
        .into_iter()
        .map(|(table, (faults, major))| TableStats {
            name: table.to_string(),
            category: table.category().to_string(),
            faults,
            major_faults: major,
            percentage: faults as f64 / total * 100.0,
        })
        .collect();

    stats.sort_by(|a, b| b.faults.cmp(&a.faults));
    stats
}

/// Generate table stats using mdbx_stat proportions
/// This distributes observed faults proportionally based on table sizes
fn generate_table_stats_from_mdbx(
    events: &[&PageFaultEvent],
    mdbx_stats: &[crate::mdbx_metadata::MdbxStatOutput],
) -> Vec<TableStats> {
    let total_faults = events.len() as u64;
    let major_faults = events.iter().filter(|e| e.is_major_fault()).count() as u64;

    // Calculate total pages across all tables (excluding @main which is the root)
    let total_pages: u64 = mdbx_stats
        .iter()
        .filter(|s| s.name != "@main")
        .map(|s| s.total_pages)
        .sum();

    if total_pages == 0 {
        return vec![TableStats {
            name: "Unknown".to_string(),
            category: "Unknown".to_string(),
            faults: total_faults,
            major_faults,
            percentage: 100.0,
        }];
    }

    let mut stats: Vec<_> = mdbx_stats
        .iter()
        .filter(|s| s.name != "@main" && s.total_pages > 0)
        .map(|s| {
            let proportion = s.total_pages as f64 / total_pages as f64;
            let estimated_faults = (total_faults as f64 * proportion).round() as u64;
            let estimated_major = (major_faults as f64 * proportion).round() as u64;
            let table = RethTable::from_name(&s.name);

            TableStats {
                name: s.name.clone(),
                category: table.category().to_string(),
                faults: estimated_faults,
                major_faults: estimated_major,
                percentage: proportion * 100.0,
            }
        })
        .collect();

    stats.sort_by(|a, b| b.faults.cmp(&a.faults));
    stats
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

fn generate_hot_pages(
    events: &[&PageFaultEvent],
    attribution: Option<&PageAttribution>,
) -> Vec<HotPage> {
    let mut page_stats: HashMap<u64, (u64, u64)> = HashMap::new();
    let page_size = attribution.map(|a| a.page_size()).unwrap_or(4096);

    // Get table proportions if available for probabilistic assignment
    let table_proportions: Vec<(String, f64)> = attribution
        .and_then(|a| a.get_mdbx_stats())
        .map(|stats| {
            let total: u64 = stats
                .iter()
                .filter(|s| s.name != "@main")
                .map(|s| s.total_pages)
                .sum();
            if total == 0 {
                return vec![];
            }
            stats
                .iter()
                .filter(|s| s.name != "@main" && s.total_pages > 0)
                .map(|s| (s.name.clone(), s.total_pages as f64 / total as f64))
                .collect()
        })
        .unwrap_or_default();

    for e in events {
        let page = e.page_number();
        let entry = page_stats.entry(page).or_insert((0, 0));
        entry.0 += 1;
        if e.is_major_fault() {
            entry.1 += 1;
        }
    }

    let mut hot_pages: Vec<_> = page_stats
        .into_iter()
        .map(|(page, (accesses, major))| {
            let offset = page * 4096;

            // Determine table name
            let table_name = if let Some(attr) = attribution {
                let table = attr.get_table(page).unwrap_or(RethTable::Unknown(0));
                if matches!(table, RethTable::Unknown(_)) && !table_proportions.is_empty() {
                    // Use deterministic assignment based on page number for consistency
                    // Pick table based on page position in the distribution
                    let hash = (page as f64 / 1000.0).fract();
                    let mut cumulative = 0.0;
                    let mut assigned = "Unknown".to_string();
                    for (name, prop) in &table_proportions {
                        cumulative += prop;
                        if hash < cumulative {
                            assigned = name.clone();
                            break;
                        }
                    }
                    assigned
                } else {
                    table.to_string()
                }
            } else {
                crate::mdbx_metadata::estimate_table_from_pattern(offset, page_size, 0, None)
                    .to_string()
            };

            HotPage {
                page_number: page,
                offset_gb: offset as f64 / 1e9,
                accesses,
                table: table_name,
                major_faults: major,
            }
        })
        .collect();

    hot_pages.sort_by(|a, b| b.accesses.cmp(&a.accesses));
    hot_pages.truncate(100); // Top 100 hot pages
    hot_pages
}

fn analyze_patterns(events: &[&PageFaultEvent]) -> PatternAnalysis {
    if events.len() < 2 {
        return PatternAnalysis {
            sequential_ratio: 0.0,
            random_ratio: 0.0,
            stride_distribution: vec![],
            burst_stats: BurstStats {
                median_events: 0,
                p95_events: 0,
                max_events: 0,
                bucket_ms: 100,
            },
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

    // Get top strides
    let mut strides: Vec<_> = stride_counts.into_iter().collect();
    strides.sort_by(|a, b| b.1.cmp(&a.1));

    let stride_distribution: Vec<_> = strides
        .into_iter()
        .take(15)
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
        stride_distribution,
        burst_stats,
    }
}

fn analyze_prefetch(events: &[&PageFaultEvent]) -> PrefetchAnalysis {
    if events.len() < 100 {
        return PrefetchAnalysis {
            prediction_hit_rate: 0.0,
            locality_score: 0.0,
            recommendation: "Not enough data for analysis".to_string(),
            prefetch_benefit_estimate: 0.0,
        };
    }

    // Stride-based prediction
    let window_size = 10;
    let lookahead = 5;
    let mut correct_predictions = 0;
    let mut total_predictions = 0;

    for i in window_size..(events.len() - lookahead) {
        let recent_strides: Vec<i64> = (0..window_size - 1)
            .map(|j| {
                events[i - window_size + j + 1].file_offset as i64
                    - events[i - window_size + j].file_offset as i64
            })
            .collect();

        let avg_stride: i64 = recent_strides.iter().sum::<i64>() / recent_strides.len() as i64;

        let current_offset = events[i].file_offset;
        let predictions: Vec<u64> = (1..=lookahead)
            .map(|j| (current_offset as i64 + avg_stride * j as i64) as u64)
            .collect();

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

    let hit_rate = if total_predictions > 0 {
        correct_predictions as f64 / total_predictions as f64 * 100.0
    } else {
        0.0
    };

    // Locality analysis
    let locality_window = 100;
    let mut locality_scores: Vec<f64> = Vec::new();

    for chunk in events.chunks(locality_window) {
        let unique_pages: std::collections::HashSet<u64> =
            chunk.iter().map(|e| e.page_number()).collect();
        let locality = unique_pages.len() as f64 / chunk.len() as f64;
        locality_scores.push(locality);
    }

    let avg_locality: f64 =
        locality_scores.iter().sum::<f64>() / locality_scores.len().max(1) as f64;

    let (recommendation, benefit) = if hit_rate > 30.0 {
        (
            "Good predictability - prefetching would significantly reduce page faults".to_string(),
            hit_rate * 0.8,
        )
    } else if hit_rate > 15.0 {
        (
            "Moderate predictability - prefetching may help for some access patterns".to_string(),
            hit_rate * 0.5,
        )
    } else {
        (
            "Poor predictability - consider larger page sizes, caching, or mlock()".to_string(),
            hit_rate * 0.2,
        )
    };

    PrefetchAnalysis {
        prediction_hit_rate: hit_rate,
        locality_score: 1.0 - avg_locality, // Invert so higher is better
        recommendation,
        prefetch_benefit_estimate: benefit,
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
