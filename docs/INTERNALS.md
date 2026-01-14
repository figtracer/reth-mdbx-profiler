# MDBX Profiler Deep Dive: Understanding Reth's Database Performance

A comprehensive guide to understanding the reth-mdbx-profiler, its internals, and how to interpret results in the context of reth's architecture.

---

## Table of Contents

1. [Introduction](#introduction)
2. [Part I: eBPF Tracing Fundamentals](#part-i-ebpf-tracing-fundamentals)
   - [What is eBPF?](#what-is-ebpf)
   - [Kprobes vs Uprobes](#kprobes-vs-uprobes)
   - [How the Profiler Attaches](#how-the-profiler-attaches)
3. [Part II: Page Faults - The Physical Layer](#part-ii-page-faults---the-physical-layer)
   - [What is a Page Fault?](#what-is-a-page-fault)
   - [Major vs Minor Faults](#major-vs-minor-faults)
   - [Memory-Mapped I/O and MDBX](#memory-mapped-io-and-mdbx)
   - [Why Major Faults Matter for Performance](#why-major-faults-matter-for-performance)
4. [Part III: MDBX Internals and B+ Trees](#part-iii-mdbx-internals-and-b-trees)
   - [What is MDBX?](#what-is-mdbx)
   - [B+ Tree Structure](#b-tree-structure)
   - [Page Types in MDBX](#page-types-in-mdbx)
   - [How MDBX Uses Memory-Mapped Files](#how-mdbx-uses-memory-mapped-files)
   - [DBI: Database Index](#dbi-database-index)
5. [Part IV: Reth's Database Architecture](#part-iv-reths-database-architecture)
   - [Overview of Reth Tables](#overview-of-reth-tables)
   - [Table Categories](#table-categories)
   - [Key Encoding Conventions](#key-encoding-conventions)
   - [Hot Tables During Sync](#hot-tables-during-sync)
6. [Part V: The Profiler Implementation](#part-v-the-profiler-implementation)
   - [BPF Program Structure](#bpf-program-structure)
   - [Active Operation Tracking](#active-operation-tracking)
   - [Per-Operation Fault Statistics](#per-operation-fault-statistics)
   - [Tree Depth Measurement](#tree-depth-measurement)
7. [Part VI: Interpreting Results](#part-vi-interpreting-results)
   - [Understanding the Summary](#understanding-the-summary)
   - [Table Analysis](#table-analysis)
   - [Page Type Distribution](#page-type-distribution)
   - [Tree Depth Analysis](#tree-depth-analysis)
   - [Slow Operations](#slow-operations)
8. [Part VII: Optimization Strategies](#part-vii-optimization-strategies)
   - [Reducing Major Faults](#reducing-major-faults)
   - [Improving Cache Hit Rates](#improving-cache-hit-rates)
   - [Batch Processing Optimization](#batch-processing-optimization)
   - [Table-Specific Optimizations](#table-specific-optimizations)
9. [Appendices](#appendices)
   - [A: Complete Reth Table Reference](#a-complete-reth-table-reference)
   - [B: MDBX Cursor Operations](#b-mdbx-cursor-operations)
   - [C: BPF Map Reference](#c-bpf-map-reference)

---

## Introduction

The reth-mdbx-profiler is a specialized tool for understanding how reth interacts with its MDBX database at the lowest level. It bridges two worlds:

1. **The Physical Layer**: Page faults, disk I/O, memory mapping
2. **The Logical Layer**: MDBX operations, cursor movements, B+ tree traversals

By correlating these two layers, we can answer critical questions:
- Which database tables cause the most disk I/O?
- How deep are the B+ tree traversals for different operations?
- Which operations are cache-friendly vs cache-hostile?
- Where should optimization efforts be focused?

This document is designed to be read while waiting for a long profiling session. By the end, you'll understand exactly what the profiler measures, what assumptions it makes, and how to act on its findings.

---

## Part I: eBPF Tracing Fundamentals

### What is eBPF?

eBPF (extended Berkeley Packet Filter) is a revolutionary Linux kernel technology that allows running sandboxed programs inside the kernel without changing kernel source code or loading kernel modules.

Think of eBPF as a safe, programmable hook into the kernel. The profiler uses eBPF to:
- Intercept page fault handling (`handle_mm_fault`)
- Intercept MDBX library calls (cursor operations, transactions)
- Collect statistics with minimal overhead

**Key Properties of eBPF:**
- **Safe**: The BPF verifier ensures programs terminate and don't crash the kernel
- **Fast**: JIT-compiled to native code, runs at near-native speed
- **Limited**: No unbounded loops, no arbitrary memory access, fixed stack size

### Kprobes vs Uprobes

The profiler uses two types of probes:

#### Kprobes (Kernel Probes)
Attach to kernel functions. Used for:
- `handle_mm_fault` - The kernel's page fault handler
- `do_mmap` - Memory mapping creation

When the kernel handles a page fault for the MDBX file, our kprobe fires.

#### Uprobes (User-space Probes)  
Attach to user-space library functions. Used for:
- `mdbx_cursor_get` - Read operations
- `mdbx_cursor_put` - Write operations
- `mdbx_get` / `mdbx_put` - Direct operations
- `mdbx_txn_begin_ex` / `mdbx_txn_commit_ex` - Transactions

The key insight: uprobes let us trace libmdbx function calls without modifying reth or libmdbx.

### How the Profiler Attaches

```
┌─────────────────────────────────────────────────────────────┐
│                      Linux Kernel                           │
│  ┌─────────────────────────────────────────────────────┐    │
│  │  handle_mm_fault()                                  │    │
│  │    ↓                                                │    │
│  │  [KPROBE] → BPF program records fault context       │    │
│  │    ↓                                                │    │
│  │  [KRETPROBE] → BPF program records major/minor,     │    │
│  │                page type, latency                   │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                    User Space (reth)                        │
│  ┌─────────────────────────────────────────────────────┐    │
│  │  libmdbx.so                                         │    │
│  │    mdbx_cursor_get()                                │    │
│  │      ↓                                              │    │
│  │    [UPROBE] → BPF registers active operation        │    │
│  │      ↓                                              │    │
│  │    (page faults occur here, attributed to this op)  │    │
│  │      ↓                                              │    │
│  │    [URETPROBE] → BPF records latency, fault stats   │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

**The Magic**: When a page fault occurs *during* an MDBX operation, we know exactly which table and operation caused it. This is called "direct BPF attribution" and provides 100% accurate correlation.

---

## Part II: Page Faults - The Physical Layer

### What is a Page Fault?

Modern operating systems use **virtual memory**. Each process sees a contiguous address space, but the OS maps this to physical RAM on demand.

A **page fault** occurs when a process accesses memory that isn't currently mapped to physical RAM. The CPU traps to the kernel, which must:
1. Find the data (in RAM cache or on disk)
2. Map it into the process's address space
3. Resume execution

```
┌────────────────────────────────────────────────────────────┐
│  Virtual Address Space (reth sees this)                    │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ 0x7f0000000000  ┌─────────────────────────────────┐  │  │
│  │                 │  MDBX file mapped here          │  │  │
│  │                 │  (appears as regular memory)    │  │  │
│  │ 0x7f0040000000  └─────────────────────────────────┘  │  │
│  └──────────────────────────────────────────────────────┘  │
│                           ↓                                │
│              Access to unmapped page                       │
│                           ↓                                │
│                    PAGE FAULT                              │
│                           ↓                                │
│              Kernel's handle_mm_fault()                    │
└────────────────────────────────────────────────────────────┘
```

### Major vs Minor Faults

This is the most important distinction for performance:

#### Minor Faults (Soft Faults)
- **The page is already in RAM** (in the page cache)
- Kernel just needs to update page tables
- **Cost: ~1-10 microseconds**
- These are cheap - the data was already read from disk previously

#### Major Faults (Hard Faults)
- **The page must be read from disk**
- Kernel initiates I/O, process blocks until complete
- **Cost: ~1-10 milliseconds** (100-1000x slower!)
- These are expensive - they represent actual disk I/O

```
┌─────────────────────────────────────────────────────────────┐
│                    Page Fault Types                         │
│                                                             │
│   Minor Fault:                    Major Fault:              │
│   ┌──────────────┐               ┌──────────────┐           │
│   │ Virtual Addr │               │ Virtual Addr │           │
│   └──────┬───────┘               └──────┬───────┘           │
│          ↓                              ↓                   │
│   ┌──────────────┐               ┌──────────────┐           │
│   │ Page Cache   │               │ Page Cache   │           │
│   │ (HIT!)       │               │ (miss)       │           │
│   └──────┬───────┘               └──────┬───────┘           │
│          ↓                              ↓                   │
│   ┌──────────────┐               ┌──────────────┐           │
│   │ Map to RAM   │               │ Read from    │           │
│   │ ~5μs         │               │ Disk ~5ms    │           │
│   └──────────────┘               └──────────────┘           │
└─────────────────────────────────────────────────────────────┘
```

### Memory-Mapped I/O and MDBX

MDBX uses **memory-mapped I/O** for database access. Instead of explicit read/write system calls, the database file is mapped into memory:

```c
// Traditional I/O:
read(fd, buffer, size);   // Explicit system call

// Memory-mapped I/O:
char *data = mmap(NULL, size, PROT_READ, MAP_SHARED, fd, 0);
char value = data[offset];  // Just access memory - kernel handles I/O!
```

**Benefits:**
- No buffer copying between kernel and user space
- OS manages caching automatically
- Simple programming model

**Implications for profiling:**
- All database I/O appears as page faults
- We can measure exactly which pages are accessed
- We can distinguish cached (minor) vs disk (major) reads

### Why Major Faults Matter for Performance

The **major fault ratio** is a key metric:

```
Major Fault Ratio = Major Faults / Total Faults
```

- **0-5%**: Excellent - data is mostly cached in RAM
- **5-20%**: Good - some disk I/O but manageable
- **20-50%**: Concerning - significant disk I/O
- **50%+**: Critical - thrashing, database larger than RAM

Each major fault means reth blocks for milliseconds waiting for disk. During sync, thousands of major faults can accumulate into minutes of I/O wait time.

---

## Part III: MDBX Internals and B+ Trees

### What is MDBX?

MDBX (Memory-Mapped Database eXtreme) is a high-performance embedded database:
- Fork of LMDB with additional features
- Single-file database
- ACID compliant
- Copy-on-write (no write-ahead log needed)
- B+ tree based

Reth chose MDBX because:
- Excellent read performance (critical for serving RPC queries)
- Memory-mapped design minimizes overhead
- Robust crash recovery
- Mature codebase

### B+ Tree Structure

MDBX stores data in **B+ trees**, a self-balancing tree structure optimized for disk:

```
                    ┌─────────────────────┐
                    │    Root (Branch)    │
                    │   Keys: [K1, K2]    │
                    └─────────┬───────────┘
                              │
           ┌──────────────────┼──────────────────┐
           ↓                  ↓                  ↓
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│ Branch Page     │  │ Branch Page     │  │ Branch Page     │
│ < K1            │  │ K1 ≤ x < K2     │  │ ≥ K2            │
└────────┬────────┘  └────────┬────────┘  └────────┬────────┘
         │                    │                    │
    ┌────┴────┐          ┌────┴────┐          ┌────┴────┐
    ↓         ↓          ↓         ↓          ↓         ↓
┌───────┐ ┌───────┐  ┌───────┐ ┌───────┐  ┌───────┐ ┌───────┐
│ Leaf  │ │ Leaf  │  │ Leaf  │ │ Leaf  │  │ Leaf  │ │ Leaf  │
│ Data  │ │ Data  │  │ Data  │ │ Data  │  │ Data  │ │ Data  │
└───────┘ └───────┘  └───────┘ └───────┘  └───────┘ └───────┘
```

**Key Properties:**
- **Branch pages**: Contain only keys and child pointers, used for navigation
- **Leaf pages**: Contain actual key-value data
- **All data at leaves**: Unlike binary trees, data only lives at the bottom
- **Leaves are linked**: Sequential scans don't need to revisit branch pages

**Why B+ Trees for databases?**
- Wide nodes (hundreds of keys per page) → shallow trees
- Shallow trees → few disk reads per lookup
- Sequential leaf links → efficient range scans

### Page Types in MDBX

The profiler detects four page types by reading MDBX page headers:

| Type | Flag | Description | Profiler Impact |
|------|------|-------------|-----------------|
| **Meta** | `0x08` | Database metadata (pages 0-1) | Rare, ignore |
| **Branch** | `0x01` | Internal B+ tree nodes | Tree traversal overhead |
| **Leaf** | `0x02` | Key-value data storage | Actual data access |
| **Overflow** | `0x04` | Large values spanning pages | Big value access |

```c
// From bpf/mdbx_tracer.bpf.c - how we detect page types:
#define MDBX_PAGE_FLAGS_OFFSET 10  // Offset of flags in page header
#define MDBX_P_BRANCH   0x01       // Branch page
#define MDBX_P_LEAF     0x02       // Leaf page  
#define MDBX_P_LARGE    0x04       // Overflow page
#define MDBX_P_META     0x08       // Meta page

// After page fault completes, we read the flags:
__u16 flags = 0;
bpf_probe_read_user(&flags, sizeof(flags), 
                    (void *)(page_start + MDBX_PAGE_FLAGS_OFFSET));
```

### How MDBX Uses Memory-Mapped Files

```
┌─────────────────────────────────────────────────────────────────┐
│                    mdbx.dat File Layout                         │
│                                                                 │
│  ┌────────┬────────┬────────────────────────────────────────┐   │
│  │ Meta 0 │ Meta 1 │  B+ Tree Pages...                      │   │
│  │ 4KB    │ 4KB    │  (Branch, Leaf, Overflow mixed)        │   │
│  └────────┴────────┴────────────────────────────────────────┘   │
│     ↑                  ↑                                        │
│  Page 0             Page 2+                                     │
│                                                                 │
│  File is mmap'd into process address space:                     │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │  0x7f0000000000: Meta0                                   │   │
│  │  0x7f0000001000: Meta1                                   │   │
│  │  0x7f0000002000: Root Branch Page                        │   │
│  │  0x7f0000003000: Child Branch Page                       │   │
│  │  ...                                                     │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

### DBI: Database Index

MDBX supports multiple "databases" (tables) within a single file. Each table is identified by a **DBI (Database Index)**:

- **DBI 0**: `FREE_DBI` - Internal free page tracking
- **DBI 1**: `MAIN_DBI` - Root database
- **DBI 2+**: User databases (reth's tables)

When you open a cursor with `mdbx_cursor_open(txn, dbi, &cursor)`, the DBI tells MDBX which B+ tree to access.

**The profiler tracks cursor→DBI mapping:**
```c
// When cursor opens, we record the DBI:
SEC("uprobe/mdbx_cursor_open")
int BPF_UPROBE(trace_cursor_open, void *txn, __u32 dbi, void **cursor_ptr) {
    // Save for later lookup
    struct cursor_open_context octx = { .dbi = dbi, ... };
    bpf_map_update_elem(&pending_cursor_opens, &pid_tgid, &octx, BPF_ANY);
}

// On return, map cursor address → DBI:
SEC("uretprobe/mdbx_cursor_open")
int BPF_URETPROBE(trace_cursor_open_ret, int ret) {
    // cursor_to_dbi[cursor_address] = dbi
    bpf_map_update_elem(&cursor_to_dbi, &cursor_addr, &octx->dbi, BPF_ANY);
}
```

---

## Part IV: Reth's Database Architecture

### Overview of Reth Tables

Reth uses ~30 MDBX tables to store all Ethereum state. The DBI assignment follows the order in reth's `tables!` macro:

```rust
// From reth crates/storage/db-api/src/tables/mod.rs
tables! {
    CanonicalHeaders,           // DBI 2
    HeaderTerminalDifficulties, // DBI 3
    HeaderNumbers,              // DBI 4
    Headers,                    // DBI 5
    BlockBodyIndices,           // DBI 6
    // ... and so on
}
```

**Important**: The DBI numbers are determined by declaration order. If reth reorders tables, the profiler's DBI→name mapping needs updating.

### Table Categories

Tables fall into these functional categories:

#### 1. Block Structure Tables
| Table | DBI | Key | Value | Purpose |
|-------|-----|-----|-------|---------|
| `CanonicalHeaders` | 2 | BlockNumber | HeaderHash | Chain head tracking |
| `Headers` | 5 | BlockNumber | Header | Block headers |
| `BlockBodyIndices` | 6 | BlockNumber | BodyIndices | Tx range in block |
| `Transactions` | 9 | TxNumber | Transaction | All transactions |

#### 2. State Tables (The Hot Ones)
| Table | DBI | Key | Value | Purpose |
|-------|-----|-----|-------|---------|
| `PlainAccountState` | 14 | Address | Account | Current account state |
| `PlainStorageState` | 15 | Address++Slot | Value | Current storage values |
| `Bytecodes` | 13 | CodeHash | Bytecode | Contract code |

#### 3. Hashed State Tables (Merkle Computation)
| Table | DBI | Key | Value | Purpose |
|-------|-----|-----|-------|---------|
| `HashedAccounts` | 20 | HashedAddress | Account | Keccak-hashed keys |
| `HashedStorages` | 21 | HashedAddress++HashedSlot | Value | Hashed storage |

#### 4. Merkle Trie Tables
| Table | DBI | Key | Value | Purpose |
|-------|-----|-----|-------|---------|
| `AccountsTrie` | 22 | Nibbles | BranchNode | Account trie nodes |
| `StoragesTrie` | 23 | HashedAddress++Nibbles | BranchNode | Storage trie nodes |

#### 5. History Tables (State at Past Blocks)
| Table | DBI | Key | Value | Purpose |
|-------|-----|-----|-------|---------|
| `AccountsHistory` | 16 | ShardedKey | BlockNumberList | Block nums where acc changed |
| `StoragesHistory` | 17 | ShardedKey | BlockNumberList | Block nums where storage changed |
| `AccountChangeSets` | 18 | BlockNumber | AccountBeforeImage | Pre-state for revert |
| `StorageChangeSets` | 19 | BlockNumber++Address | StorageBeforeImage | Pre-storage for revert |

### Key Encoding Conventions

Understanding key formats helps interpret profiler output:

```
PlainAccountState key:
  ┌────────────────────────────────────┐
  │ 20 bytes: Ethereum address         │
  └────────────────────────────────────┘

PlainStorageState key:  
  ┌────────────────────────────────────┬────────────────────────────────────┐
  │ 20 bytes: Contract address         │ 32 bytes: Storage slot             │
  └────────────────────────────────────┴────────────────────────────────────┘

HashedAccounts key:
  ┌────────────────────────────────────┐
  │ 32 bytes: keccak256(address)       │
  └────────────────────────────────────┘

Block-keyed tables (CanonicalHeaders, etc.):
  ┌────────────────────────────────────┐
  │ 8 bytes: BlockNumber (big-endian)  │
  └────────────────────────────────────┘
```

### Hot Tables During Sync

During block execution and sync, certain tables see disproportionate access:

**Execution Phase:**
1. `PlainAccountState` - Read/update balances, nonces
2. `PlainStorageState` - Read/update contract storage  
3. `Bytecodes` - Load contract code

**Merkle Computation Phase:**
1. `HashedAccounts` / `HashedStorages` - Sorted key iteration
2. `AccountsTrie` / `StoragesTrie` - Trie node updates

**State Root Calculation:**
The heaviest operation is computing the state root after each block:
1. Walk all changed accounts/storage
2. Hash keys, update trie nodes
3. Compute root hash

This is why `HashedAccounts`, `HashedStorages`, and the trie tables often dominate profiles.

---

## Part V: The Profiler Implementation

### BPF Program Structure

The BPF program (`bpf/mdbx_tracer.bpf.c`) has several key components:

#### Event Ring Buffer
```c
struct {
    __uint(type, BPF_MAP_TYPE_RINGBUF);
    __uint(max_entries, 16 * 1024 * 1024);  // 16MB
} events SEC(".maps");
```
All events (page faults, cursor ops, transactions) flow through this buffer to userspace.

#### Tracking Maps
```c
// Track which inodes to trace (the MDBX file)
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __type(key, __u64);    // inode
    __type(value, __u8);
} tracked_inodes SEC(".maps");

// Map VMA addresses to file offsets
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __type(key, __u64);    // vma->vm_start
    __type(value, __u64);  // file offset base
} vma_to_offset SEC(".maps");

// Map cursor pointers to DBI numbers
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __type(key, __u64);    // cursor address
    __type(value, __u32);  // DBI
} cursor_to_dbi SEC(".maps");
```

### Active Operation Tracking

The key innovation is tracking *what operation* is running when a page fault occurs:

```c
struct active_op {
    __u64 start_ns;          // When operation started
    __u32 dbi;               // Which table
    __u32 op_type;           // CURSOR_GET, CURSOR_PUT, etc.
    __u32 cursor_op;         // SET_RANGE, NEXT, etc.
    __u8  key_prefix[16];    // First 16 bytes of key
};

// Map: thread_id → current operation
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __type(key, __u64);      // pid_tgid
    __type(value, struct active_op);
} active_ops SEC(".maps");
```

**Flow:**
1. `mdbx_cursor_get` uprobe fires → register active_op for this thread
2. Page fault occurs → lookup active_op, attribute fault to that operation
3. `mdbx_cursor_get` uretprobe fires → clear active_op, emit event with stats

This provides **100% accurate** attribution - we know exactly which MDBX operation caused each fault.

### Per-Operation Fault Statistics

Each operation accumulates fault statistics:

```c
struct op_fault_stats {
    __u32 fault_count;         // Total page faults
    __u32 major_fault_count;   // Major faults (disk I/O)
    __u32 branch_faults;       // Faults on branch pages
    __u32 leaf_faults;         // Faults on leaf pages
    __u32 overflow_faults;     // Faults on overflow pages
    __u32 current_depth;       // Current tree traversal depth
    __u32 max_depth;           // Maximum depth reached
    __u64 total_fault_latency_ns;
};
```

### Tree Depth Measurement

The profiler measures B+ tree traversal depth by counting consecutive branch page faults:

```c
// In page fault handler:
if (page_type == PAGE_TYPE_BRANCH) {
    stats->branch_faults++;
    stats->current_depth++;
    if (stats->current_depth > stats->max_depth) {
        stats->max_depth = stats->current_depth;
    }
} else if (page_type == PAGE_TYPE_LEAF) {
    stats->leaf_faults++;
    stats->current_depth = 0;  // Reset - reached data
}
```

**Interpretation:**
- `max_depth = 1`: Very shallow tree or hot cache (branch in memory)
- `max_depth = 3-5`: Normal for large tables
- `max_depth = 7+`: Very deep trees, consider optimization

**Caveat**: This measures *uncached* depth. If branch pages are cached (minor faults), we don't see them. The measured depth represents "cold" traversals.

---

## Part VI: Interpreting Results

### Understanding the Summary

```
Trace Summary:
  Duration: 300.5s
  Total Page Faults: 1,234,567
  Major Faults: 234,567 (19.0%)
  Minor Faults: 1,000,000 (81.0%)
  Fault Rate: 4,108/sec
  Unique Pages: 456,789
```

**What to look for:**
- **Major Fault Ratio**: Above 20% indicates significant disk I/O
- **Fault Rate**: Higher during sync, lower during steady-state
- **Unique Pages**: Indicates working set size

### Table Analysis

```
Table Breakdown:
  HashedAccounts:     45.2% (557,983 faults, 98,234 major)
  AccountsTrie:       23.1% (285,234 faults, 67,891 major)  
  StoragesTrie:       15.4% (190,123 faults, 45,678 major)
  PlainAccountState:   8.7% (107,456 faults, 12,345 major)
```

**Analysis approach:**
1. **Highest fault tables**: Where is time spent?
2. **Major fault ratio per table**: Which tables cause disk I/O?
3. **Operations per table**: Seek-heavy or scan-heavy?

### Page Type Distribution

```
Page Type Distribution:
  Branch: 234,567 (19%)  - Tree traversal overhead
  Leaf:   950,000 (77%)  - Actual data access
  Overflow: 50,000 (4%)  - Large values
```

**Key ratios:**
- **Branch:Leaf ratio**: Higher = deeper tree traversals
  - < 0.1: Excellent (sequential scans, shallow trees)
  - 0.1-0.3: Good (mixed workload)
  - > 0.3: Concerning (many random lookups)

### Tree Depth Analysis

```
Measured B+ Tree Depth:
  Max Depth Observed: 10
  Average Depth: 2.34
  
  Depth by Table:
    AccountsTrie:      avg 4.2, max 10
    StoragesTrie:      avg 3.8, max 9
    HashedAccounts:    avg 1.2, max 4
    
  Depth by Operation:
    SET_RANGE:         avg 3.1, max 10 (seek)
    NEXT:              avg 0.8, max 3  (navigation)
```

**Interpretation:**
- High depth on seek operations = tree is deep, consider caching
- High depth on NEXT = inefficient scan pattern
- `AccountsTrie` depth 10 = trie has grown very large

### Slow Operations

```
Slow Operations (>100μs):
  HashedAccounts SET_RANGE: 45,678 ops, avg 234μs, max 12ms
  AccountsTrie SET_RANGE:   23,456 ops, avg 567μs, max 45ms
```

Operations >100μs almost certainly hit disk (major faults). These are optimization targets.

---

## Part VII: Optimization Strategies

### Reducing Major Faults

**1. Increase RAM / Page Cache**
More RAM = more database cached = fewer major faults.

**2. Use Faster Storage**
NVMe SSDs reduce major fault latency from 10ms (HDD) to 0.1ms.

**3. Prefetching**
OS readahead helps sequential scans. Tune `/sys/block/*/queue/read_ahead_kb`.

### Improving Cache Hit Rates

**1. Batch Similar Operations**
Access same tables together to keep pages in cache:
```
// Bad: interleaved access
read_account(addr1)
read_storage(addr1, slot1)
read_account(addr2)
read_storage(addr2, slot1)

// Better: grouped access  
read_account(addr1)
read_account(addr2)
read_storage(addr1, slot1)
read_storage(addr2, slot1)
```

**2. Locality-Aware Key Design**
Keys that sort together are stored together. Reth's `HashedAccounts` uses hashed keys specifically to randomize access patterns (security), which hurts cache locality.

### Batch Processing Optimization

Reth batches blocks into transactions. Larger batches:
- Amortize commit overhead
- May increase working set (more pages touched)

Monitor RW commit latency and batch size to find sweet spot.

### Table-Specific Optimizations

**HashedAccounts / HashedStorages:**
- Random key distribution by design (keccak256 hashes)
- Large tables with poor locality
- Consider: bloom filters, caching hot accounts

**AccountsTrie / StoragesTrie:**
- Trie depth grows with state size
- Consider: trie pruning, flat storage alternatives

**PlainAccountState / PlainStorageState:**
- More sequential access patterns
- Benefit from larger page cache

---

## Appendices

### A: Complete Reth Table Reference

| DBI | Table Name | Key Type | Value Type | Category |
|-----|-----------|----------|------------|----------|
| 0 | FREE_DBI (internal) | - | - | System |
| 1 | MAIN_DBI (internal) | - | - | System |
| 2 | CanonicalHeaders | BlockNumber | HeaderHash | Blocks |
| 3 | HeaderTerminalDifficulties | BlockNumber | U256 | Blocks |
| 4 | HeaderNumbers | HeaderHash | BlockNumber | Blocks |
| 5 | Headers | BlockNumber | Header | Blocks |
| 6 | BlockBodyIndices | BlockNumber | BodyIndices | Blocks |
| 7 | BlockOmmers | BlockNumber | Ommers | Blocks |
| 8 | BlockWithdrawals | BlockNumber | Withdrawals | Blocks |
| 9 | Transactions | TxNumber | Transaction | Transactions |
| 10 | TransactionHashNumbers | TxHash | TxNumber | Transactions |
| 11 | TransactionBlocks | TxNumber | BlockNumber | Transactions |
| 12 | Receipts | TxNumber | Receipt | Transactions |
| 13 | Bytecodes | CodeHash | Bytecode | State |
| 14 | PlainAccountState | Address | Account | State |
| 15 | PlainStorageState | Address++Slot | Value | State |
| 16 | AccountsHistory | ShardedKey | BlockList | History |
| 17 | StoragesHistory | ShardedKey | BlockList | History |
| 18 | AccountChangeSets | BlockNumber | Changes | History |
| 19 | StorageChangeSets | BlockNumber++Addr | Changes | History |
| 20 | HashedAccounts | HashedAddr | Account | Merkle |
| 21 | HashedStorages | HashedAddr++Slot | Value | Merkle |
| 22 | AccountsTrie | Nibbles | BranchNode | Merkle |
| 23 | StoragesTrie | HashedAddr++Nibbles | BranchNode | Merkle |
| 24 | AccountsTrieChangeSets | BlockNumber | Changes | Merkle |
| 25 | StoragesTrieChangeSets | BlockNumber | Changes | Merkle |
| 26 | TransactionSenders | TxNumber | Address | Transactions |
| 27 | StageCheckpoints | StageId | Checkpoint | Meta |
| 28 | StageCheckpointProgresses | StageId | Progress | Meta |
| 29 | PruneCheckpoints | Segment | Checkpoint | Meta |
| 30 | VersionHistory | U64 | Version | Meta |
| 31 | ChainState | Key | Value | Meta |
| 32 | Metadata | Key | Value | Meta |

### B: MDBX Cursor Operations

| Operation | Code | Type | Description |
|-----------|------|------|-------------|
| FIRST | 0 | Navigate | Move to first key |
| FIRST_DUP | 1 | Navigate | First duplicate of current key |
| GET_BOTH | 2 | Seek | Position at key+value pair |
| GET_BOTH_RANGE | 3 | Seek | Position at key, nearest value |
| GET_CURRENT | 4 | Navigate | Get current position |
| GET_MULTIPLE | 5 | Navigate | Get multiple values |
| LAST | 6 | Navigate | Move to last key |
| LAST_DUP | 7 | Navigate | Last duplicate of current key |
| NEXT | 8 | Navigate | Move to next key |
| NEXT_DUP | 9 | Navigate | Next duplicate |
| NEXT_MULTIPLE | 10 | Navigate | Next multiple values |
| NEXT_NODUP | 11 | Navigate | Next unique key |
| PREV | 12 | Navigate | Previous key |
| PREV_DUP | 13 | Navigate | Previous duplicate |
| PREV_NODUP | 14 | Navigate | Previous unique key |
| SET | 15 | Seek | Position at exact key |
| SET_KEY | 16 | Seek | Position at key, return key |
| SET_RANGE | 17 | Seek | Position at key or next greater |
| PREV_MULTIPLE | 18 | Navigate | Previous multiple values |
| SET_LOWERBOUND | 19 | Seek | Position at lower bound |
| SET_UPPERBOUND | 20 | Seek | Position at upper bound |

**Seek operations** require B+ tree traversal from root.
**Navigate operations** can often use leaf page links.

### C: BPF Map Reference

| Map Name | Type | Key | Value | Purpose |
|----------|------|-----|-------|---------|
| events | RINGBUF | - | Events | Event stream to userspace |
| tracked_inodes | HASH | inode | u8 | Filter MDBX file |
| vma_to_offset | HASH | vma_start | offset | VMA→file offset |
| profiler_config | ARRAY | 0 | pid | Target PID filter |
| stats | PERCPU_ARRAY | stat_id | count | Statistics counters |
| pending_faults | HASH | pid_tgid | context | Fault entry→return correlation |
| pending_cursors | HASH | pid_tgid | context | Cursor entry→return |
| cursor_to_dbi | HASH | cursor_ptr | dbi | Cursor→table mapping |
| active_ops | HASH | pid_tgid | active_op | Current operation per thread |
| op_fault_stats_map | HASH | pid_tgid | stats | Per-op fault accumulator |
| active_txn_flags | HASH | txn_ptr | flags | Transaction type tracking |

---

## Part VIII: Streaming Mode for Large Traces

### The Problem

Long-running traces (hours) can generate massive trace files - 75GB or more. The default analyzer loads all events into memory before processing, which fails when the trace is larger than available RAM.

### The Solution: Streaming Analysis

The `--streaming` flag enables single-pass processing with constant memory usage:

```bash
./target/release/mdbx-trace-analyzer \
    --input trace.jsonl \
    --mdbx-path /data/reth/db/mdbx.dat \
    --streaming
```

### How Streaming Mode Works

| Component | Default Mode | Streaming Mode |
|-----------|--------------|----------------|
| Event storage | All events in `Vec<T>` | None - process and discard |
| Memory usage | O(n) - grows with trace size | O(1) - constant ~500MB |
| Percentiles | Exact (sort all latencies) | Approximate (reservoir sampling) |
| Timeline | Exact | Downsampled to 1000 points max |
| Hot keys | Track all, sort at end | Bounded tracking with pruning |

### Key Techniques

**1. Online Statistics (Welford's Algorithm)**

Instead of storing all latencies to compute mean/variance:
```rust
// Streaming mean update
self.count += 1;
let delta = value - self.mean;
self.mean += delta / self.count as f64;
```

**2. Reservoir Sampling for Percentiles**

Keep a fixed-size sample (10,000 values) that represents the full distribution:
```rust
fn add(&mut self, value: u64) {
    self.count += 1;
    if self.samples.len() < self.capacity {
        self.samples.push(value);
    } else {
        // Replace with probability capacity/count
        let idx = fastrand::u64(0..self.count);
        if idx < self.capacity as u64 {
            self.samples[idx as usize] = value;
        }
    }
}
```

**3. Bounded Data Structures**

- Timeline buckets: HashMap with fixed bucket size, downsampled at end
- Hot keys: Prune to top-N when exceeding 2x capacity
- Transaction timeline: Stop collecting after 1000 entries

### Progress Display

Streaming mode shows detailed progress:

```
[████████████░░░░░░░░░░░░░░░░░░]  40.5% | 30.4GB/75.0GB | 85 MB/s | ETA: 8m 45s | 125M faults, 89M ops
```

Components:
- Visual progress bar (30 chars)
- Percentage complete
- Bytes processed / total
- Processing speed (MB/s)
- Estimated time remaining
- Running event counts

### When to Use Streaming Mode

| Scenario | Recommended Mode |
|----------|------------------|
| Trace < 10GB | Default (more accurate) |
| Trace 10-50GB | Either (streaming if RAM limited) |
| Trace > 50GB | Streaming (required) |
| OOM errors | Streaming |
| Multi-hour traces | Streaming |

### Accuracy Trade-offs

Streaming mode makes these approximations:

1. **Percentiles**: ±1-2% accuracy vs exact (usually negligible)
2. **Unique page count**: Exact (HashSet still used, but bounded by actual unique pages)
3. **Hot keys**: May miss some if >50,000 unique keys (keeps top by slow-access-count)
4. **Timeline resolution**: May be lower for very long traces

For most analysis purposes, these trade-offs are acceptable and the results are statistically equivalent.

---

## Conclusion

The reth-mdbx-profiler provides deep visibility into database performance by bridging kernel-level page fault tracking with application-level MDBX operations.

**Key takeaways:**
1. **Major faults are expensive** - each one is ~5ms of disk I/O
2. **Table attribution is 100% accurate** - thanks to active operation tracking
3. **Tree depth correlates with cache misses** - deeper traversals on cold data
4. **Seek operations are costlier than navigation** - they must traverse from root
5. **Streaming mode handles any trace size** - use `--streaming` for large files
