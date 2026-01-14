# Changelog

All notable changes to the MDBX profiler will be documented in this file.

This document serves as both a changelog and a design rationale document for major features.

---

## [Unreleased]

### Added

#### Working Set Analysis

**Purpose**: Understand memory requirements for efficient MDBX operation.

**Problem Statement**:
The profiler currently shows page fault counts and patterns, but doesn't answer the fundamental question: "How much RAM do I need for good performance?" This feature adds working set analysis to:

1. **Track page reuse** - Which pages are accessed multiple times vs one-time access
2. **Calculate reuse distance** - How many unique pages are accessed between repeated accesses to the same page
3. **Estimate RAM requirements** - What cache size would give X% hit rate
4. **Per-table working sets** - Which tables have hot data that benefits from caching

**Key Metrics**:

- **Working Set Size (WSS)**: Number of unique pages accessed in a time window
- **Reuse Distance**: Number of unique pages accessed between two accesses to the same page
- **Cache Hit Rate Simulation**: Given N pages of RAM, what % of accesses would hit cache
- **Hot Page Ratio**: % of pages that account for X% of accesses (Pareto analysis)

**Implementation Approach**:

The analysis happens in two passes conceptually, but is implemented in a single streaming pass:

1. **During streaming**: Track page access timestamps and counts using bounded data structures
2. **At finalization**: Compute reuse distance histograms and cache simulation from sampled data

**Memory Budget**:
To keep memory bounded while processing multi-GB traces:
- Use reservoir sampling for reuse distance tracking (keep ~100K samples)
- Use approximate counting (Count-Min Sketch) for access frequency
- Downsample timeline data for visualization

**Output**:
```json
{
  "working_set": {
    "total_unique_pages": 7452350,
    "total_accesses": 13079652,
    "reuse_ratio": 0.43,  // 43% of accesses are to previously-seen pages
    
    "cache_simulation": [
      { "cache_size_gb": 8, "hit_rate": 0.45 },
      { "cache_size_gb": 16, "hit_rate": 0.62 },
      { "cache_size_gb": 32, "hit_rate": 0.78 },
      { "cache_size_gb": 64, "hit_rate": 0.89 },
      { "cache_size_gb": 128, "hit_rate": 0.95 }
    ],
    
    "reuse_distance_histogram": [
      { "bucket": "immediate (0-100)", "count": 2341234, "percentage": 45.2 },
      { "bucket": "short (100-1K)", "count": 1234567, "percentage": 23.8 },
      { "bucket": "medium (1K-10K)", "count": 876543, "percentage": 16.9 },
      { "bucket": "long (10K-100K)", "count": 543210, "percentage": 10.5 },
      { "bucket": "very long (>100K)", "count": 187654, "percentage": 3.6 }
    ],
    
    "per_table": [
      {
        "name": "HashedAccounts",
        "unique_pages": 1234567,
        "accesses": 2345678,
        "reuse_ratio": 0.47,
        "hot_pages": 12345,  // pages accounting for 80% of accesses
        "hot_page_ratio": 0.01  // 1% of pages = 80% of accesses
      }
    ],
    
    "time_windowed": [
      { "window_secs": 60, "avg_wss_pages": 123456 },
      { "window_secs": 300, "avg_wss_pages": 456789 },
      { "window_secs": 600, "avg_wss_pages": 678901 }
    ]
  }
}
```

**Use Cases**:

1. **Hardware Planning**: "My current trace shows 95% hit rate requires 64GB RAM"
2. **Regression Detection**: "After update X, WSS increased by 20%"
3. **Table Optimization Priority**: "HashedStorages has 0.1% hot pages - perfect LRU candidate"
4. **Cold Start Analysis**: "First 5 minutes have 2x WSS compared to steady state"

---

## Design Decisions

### Why Reservoir Sampling for Reuse Distance?

Tracking exact reuse distance requires storing the last access time for every page. With 7M+ unique pages, this is ~56MB just for timestamps. Instead, we use reservoir sampling:

1. Sample a subset of page accesses (configurable, default 100K)
2. For sampled pages, track exact reuse distances
3. Extrapolate to full dataset using statistical methods

This gives accurate percentile estimates with bounded memory.

### Why Count-Min Sketch for Frequency?

Exact frequency counting for 7M pages requires 56MB+ of counters. Count-Min Sketch provides:
- Bounded memory (configurable, default 1MB)
- O(1) update and query
- Guaranteed no under-counting (only over-estimation)
- Good accuracy for identifying hot pages

### Why Time-Windowed WSS?

A single WSS number doesn't capture temporal behavior:
- Cold start has high WSS (loading caches)
- Steady state has lower WSS (working within hot set)
- Batch boundaries may show WSS spikes

Time-windowed analysis reveals these patterns for capacity planning.

---

#### Working Set Visualization (HTML Viewer)

**Added "Memory" Tab**:

A new tab in the HTML viewer provides interactive visualization of working set analysis:

1. **Summary Banner**: Natural language summary of findings and RAM recommendation
2. **Key Metrics Row**:
   - Unique pages traced
   - Total working set size (GB)
   - Page reuse ratio
   - Average accesses per page
   - Recommended RAM (highlighted)

3. **Cache Hit Rate Chart**: 
   - Bar chart showing cache hit rate at different RAM sizes (1GB to 256GB)
   - Table with detailed simulation results including faults avoided per second

4. **Hot Page Distribution (Pareto)**:
   - Visual representation of how few pages account for most accesses
   - Statistics: pages needed for 50%/80%/90%/95% of accesses
   - Hot set size in GB for quick capacity planning

5. **Reuse Distance Histogram**:
   - Distribution of page reuse distances (immediate to extreme)
   - Helps understand cache-friendliness of workload

6. **Per-Table Working Set**:
   - Table breakdown showing unique pages, working set size, and reuse ratio per MDBX table
   - Identifies which tables would benefit most from caching

7. **Time-Windowed WSS** (when available):
   - Average, min, max working set per minute
   - Reveals temporal patterns in memory usage

---

## Future Considerations

### Prefetch Simulation (Planned)

Building on working set analysis, we can simulate prefetch strategies:
- "If we prefetched N pages ahead, what hit rate improvement?"
- "Which tables benefit from readahead vs random access?"

### CPU Time Attribution (Planned)

Complement I/O analysis with CPU profiling:
- Hash computation time
- Serialization overhead
- Lock contention

---
