# MDBX Profiler - Roadmap & Technical Notes

## Current Architecture Summary

The profiler uses eBPF to bridge kernel-space page faults with user-space MDBX operations:

```
User Space (uprobes)              Kernel Space (kprobes)
┌─────────────────────┐          ┌─────────────────────┐
│ mdbx_cursor_get     │          │ handle_mm_fault     │
│ mdbx_cursor_put     │          │                     │
│ mdbx_cursor_del     │   BPF    │ Knows:              │
│ mdbx_txn_commit     │  MAPS    │ - page address      │
│                     │ ←─────→  │ - major/minor       │
│ Knows:              │          │ - thread ID         │
│ - DBI (table)       │          │                     │
│ - key               │          │                     │
│ - operation type    │          │                     │
└─────────────────────┘          └─────────────────────┘
           │                               │
           └───────────┬───────────────────┘
                       │
                       ▼
              ┌─────────────────┐
              │   Ring Buffer   │
              │   (16MB)        │
              └────────┬────────┘
                       │
                       ▼
              ┌─────────────────┐
              │ Userspace Rust  │
              │ - Streaming     │
              │ - Aggregation   │
              │ - HTML Viewer   │
              └─────────────────┘
```

---

## High Priority Improvements

### 1. Automatic ASLR Base Address Detection

**Problem:** Users must manually provide `--base-address` for PIE binaries.

**Solution:**
```rust
fn detect_base_address(pid: u32, binary_path: &Path) -> Option<u64> {
    let maps = std::fs::read_to_string(format!("/proc/{}/maps", pid)).ok()?;
    for line in maps.lines() {
        if line.contains(binary_path.to_str()?) && line.contains("r-xp") {
            // Parse: "55d4a8c00000-55d4a8c01000 r-xp ..."
            let addr = line.split('-').next()?;
            return u64::from_str_radix(addr, 16).ok();
        }
    }
    None
}
```

**Files to modify:** `src/main.rs`, `src/symbolize.rs`

---

### 2. Per-Transaction Operation Grouping

**Problem:** Can't answer "what did this specific transaction do?"

**Solution:**
- Track `txn_ptr` in `active_ops` map
- Create `TxnOperations` struct aggregating ops per transaction
- Link cursor operations to their parent transaction

**New output:**
```json
{
  "txn_ptr": "0x7f1234567890",
  "duration_ms": 1423,
  "operations": [
    {"table": "AccountsTrie", "op": "CURSOR_PUT", "count": 4521},
    {"table": "HashedStorages", "op": "CURSOR_DEL", "count": 892}
  ],
  "total_faults": 12453,
  "major_faults": 1893
}
```

**Files to modify:** `bpf/mdbx_tracer.bpf.c`, `src/event.rs`, `src/streaming.rs`

---

### 3. Concurrent Reader Transaction Tracking

**Problem:** High concurrent RO transactions block freelist reclamation.

**Solution:**
- Track `txn_begin` / `txn_commit` / `txn_abort` to count active readers
- Alert when concurrent readers exceed threshold
- Correlate with freelist growth patterns

**New metrics:**
```json
{
  "reader_timeline": [
    {"timestamp_ms": 0, "concurrent_ro": 12},
    {"timestamp_ms": 100, "concurrent_ro": 45},
    {"timestamp_ms": 200, "concurrent_ro": 566}  // Alert threshold!
  ],
  "longest_ro_txn_ms": 45000,
  "reader_histogram": {...}
}
```

**Files to modify:** `bpf/mdbx_tracer.bpf.c` (txn tracking), `src/streaming.rs`

---

### 4. File Offset to Table Heuristic

**Problem:** Heatmap shows offset but not which table owns that region.

**Solution:**
- During tracing, record `(dbi, file_offset)` pairs from page faults
- Build approximate offset→table mapping
- Annotate heatmap with likely table regions

**Implementation:**
```rust
struct OffsetTableMap {
    // Bucket file into 1GB regions, track which DBIs write there
    regions: Vec<HashMap<u32, u64>>,  // region_idx -> (dbi -> fault_count)
}

fn infer_table_for_offset(&self, offset: u64) -> Option<&str> {
    let region = (offset / (1024 * 1024 * 1024)) as usize;
    self.regions.get(region)?.iter()
        .max_by_key(|(_, count)| *count)
        .map(|(dbi, _)| dbi_to_table_name(*dbi))
}
```

---

## Medium Priority Improvements

### 5. Live Streaming Mode

**Problem:** Must wait for trace to complete before analysis.

**Solution:**
- WebSocket server in Rust during tracing
- Stream aggregated stats every N seconds
- Browser dashboard with live charts

**Architecture:**
```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│ BPF Ring Buffer │ ──► │ Rust Aggregator │ ──► │ WebSocket Server│
└─────────────────┘     └─────────────────┘     └────────┬────────┘
                                                         │
                                                         ▼
                                                ┌─────────────────┐
                                                │ Browser Dashboard│
                                                │ (Live Charts)    │
                                                └─────────────────┘
```

---

### 6. Freelist Size Tracking

**Problem:** Freelist spikes correlate with performance issues but aren't tracked.

**Solution:**
- Hook `mdbx_txn_commit_ex` to read freelist size from txn metadata
- Track freelist size over time
- Correlate with operation patterns

**New uprobe:**
```c
SEC("uretprobe/mdbx_txn_commit_ex")
int trace_commit_ret(struct pt_regs *ctx) {
    // Read env->me_pghead (freelist size) after commit
    // Emit FreelitEvent with size, txn_id
}
```

---

### 7. Write Path Analysis

**Problem:** Write amplification and page splits not tracked.

**Metrics to add:**
- Page splits (new branch nodes created)
- Write amplification per table
- Dirty page count per commit

**Implementation:** Hook `mdbx_page_split` or detect via branch fault patterns.

---

### 8. Memory Pressure Correlation

**Problem:** Can't distinguish "working set too large" from "system under memory pressure."

**Solution:**
```rust
fn sample_memory_pressure() -> MemoryStats {
    let meminfo = std::fs::read_to_string("/proc/meminfo")?;
    // Parse: MemAvailable, Cached, Buffers, Dirty

    let vmstat = std::fs::read_to_string("/proc/vmstat")?;
    // Parse: pgfault, pgmajfault, pswpin, pswpout
}
```

Correlate major fault spikes with system memory availability.

---

## Lower Priority / Research

### 9. Predictive Working Set Estimation

**Goal:** Given trace, estimate RAM needed for X% cache hit rate.

**Model:**
```
For each cache_size in [1GB, 2GB, 4GB, 8GB, 16GB, 32GB, 64GB]:
    Simulate LRU cache with unique pages from trace
    Calculate: hit_rate = cache_hits / total_accesses
    Output: "With {cache_size}GB RAM, expect {hit_rate}% cache hits"
```

---

### 10. Contract Address Resolution

**Goal:** Auto-label hot keys with known contract names.

**Implementation:**
```rust
lazy_static! {
    static ref KNOWN_CONTRACTS: HashMap<[u8; 20], &'static str> = {
        let mut m = HashMap::new();
        m.insert(hex!("dac17f958d2ee523a2206206994597c13d831ec7"), "USDT");
        m.insert(hex!("a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"), "USDC");
        m.insert(hex!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"), "WETH");
        // ... more contracts
        m
    };
}
```

---

### 11. Differential Analysis

**Goal:** Compare two traces (before/after optimization).

**Output:**
```
Table          | Before    | After     | Change
---------------|-----------|-----------|--------
AccountsTrie   | 1.2M      | 800K      | -33%
HashedStorages | 2.1M      | 2.0M      | -5%
Commit Latency | 1.4s p99  | 0.8s p99  | -43%
```

---

### 12. Physical I/O Correlation

**Goal:** Correlate page faults with actual disk operations.

**Implementation:** Hook block layer:
```c
SEC("kprobe/blk_mq_start_request")
int trace_block_io(struct pt_regs *ctx, struct request *rq) {
    // Capture: sector, size, direction (read/write)
    // Correlate with pending page faults by address
}
```

---

### 13. Commit Phase Breakdown

**Goal:** Understand where commit time goes.

**New event:**
```rust
struct CommitPhaseEvent {
    txn_ptr: u64,
    dirty_pages: u32,
    freelist_search_ns: u64,
    page_write_ns: u64,
    fsync1_ns: u64,
    meta_write_ns: u64,
    fsync2_ns: u64,
}
```

Requires hooking internal MDBX functions or adding timing around commit phases.

---

## MDBX Freelist Deep Dive

### How Freelist Search Works (Verified from Source)

From [reth issue #5228](https://github.com/paradigmxyz/reth/issues/5228) and [libmdbx source](https://github.com/erthink/libmdbx):

```
MDBX Page Allocation Algorithm (Simplified):
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. Check reclaimed-list for N consecutive pages
   └─ If found → allocate, done

2. While reclaimed-list too small AND freelist not exhausted:
   a. Fetch next freelist entry (oldest txn first)
   b. Add pages to reclaimed-list (maintaining sorted order)
   c. Search reclaimed-list for N consecutive pages
   d. If rp_augment_limit reached → stop searching, allocate new pages

3. If found → allocate from reclaimed-list
   Else → allocate new pages (extend file)
```

### Why It's O(N) and Slow

1. **No index on consecutive sequences** - Must scan entire list
2. **Sorted array maintenance** - Each insertion is O(n)
3. **Repeated searches** - Each freelist iteration re-scans reclaimed-list
4. **Fragmentation** - B+ tree creates scattered free pages

### Key Configuration: `rp_augment_limit`

```rust
// In reth/libmdbx configuration:
env.set_option(MDBX_opt_rp_augment_limit, 16_777_216)?;

// This limits how many pages MDBX will accumulate before giving up
// and allocating new pages. Higher = more searching, lower = more file growth.
```

### Potential Data Structure Improvements

Your intuition about linked lists with break detection is close to what would help:

**Option 1: Bitmap with Run-Length Encoding**
```
Instead of: [3, 5, 6, 7, 8, 12, 15, 16, 17]
Store:      [(3,1), (5,4), (12,1), (15,3)]  // (start, length) pairs

Finding 3 consecutive pages: O(number of runs) instead of O(total pages)
```

**Option 2: Skip List with Size Metadata**
```
Level 2: [5 consecutive] ────────────────► [3 consecutive]
Level 1: [5] ──► [12] ──────────► [15] ──► [17]
Level 0: [5,6,7,8,9] ─► [12] ─► [15,16,17]
```

**Option 3: Buddy Allocator Style**
```
Maintain separate lists by power-of-2 sizes:
- 1-page runs: [3, 12]
- 2-page runs: [(15,16)]
- 4-page runs: [(5,6,7,8)]
- 8-page runs: []

Finding 3 consecutive: Check 4-page list first, split if found
```

### Why MDBX Doesn't Use These

1. **Complexity** - MDBX prioritizes simplicity and correctness
2. **MVCC Constraints** - Freelist entries are tied to transaction IDs
3. **SIMD Optimization** - Modern libmdbx uses SIMD for faster linear search
4. **rp_augment_limit** - Provides escape hatch for worst cases

---

## Known Issues & Limitations

### Current Profiler Limitations

1. **DBI mapping is version-dependent** - Must match reth's table order
2. **Pre-trace cursors use fragile struct introspection** - Offsets vary by libmdbx build
3. **No backpressure** - Events dropped silently when ring buffer full
4. **Stack traces require frame pointers** - Binary must be built with `-Cforce-frame-pointers=yes`

### MDBX Operational Considerations

1. **Long RO transactions block freelist reclamation**
2. **Large values trigger expensive consecutive page search**
3. **Freelist search is O(N) with N = number of free pages**
4. **Commit latency dominated by fsync + freelist search**

---

## References

- [MDBX Freelist Issues - reth #5228](https://github.com/paradigmxyz/reth/issues/5228)
- [libmdbx freelist growth - Issue #158](https://github.com/erthink/libmdbx/issues/158)
- [libmdbx source code](https://github.com/erthink/libmdbx)
- [LMDB freelist illustrated guide](https://github.com/ledgerwatch/erigon/wiki/LMDB-freelist)
- [reth libmdbx-rs bindings](https://github.com/paradigmxyz/reth/tree/main/crates/storage/libmdbx-rs)
