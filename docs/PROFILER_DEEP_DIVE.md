# MDBX Profiler Deep Dive: Understanding Reth's Database Performance

A comprehensive guide to understanding the reth-mdbx-profiler, its internals, and how to interpret results in the context of reth's architecture.

---

## Table of Contents

1. [Introduction](#introduction)
2. [Part I: eBPF Tracing Fundamentals](#part-i-ebpf-tracing-fundamentals)
3. [Part II: Page Faults - The Physical Layer](#part-ii-page-faults---the-physical-layer)
4. [Part III: MDBX Internals and B+ Trees](#part-iii-mdbx-internals-and-b-trees)
5. [Part IV: Reth's Database Architecture](#part-iv-reths-database-architecture) *(Expanded)*
6. [Part V: The Profiler Implementation](#part-v-the-profiler-implementation) *(Expanded)*
7. [Part VI: Interpreting Results](#part-vi-interpreting-results)
8. [Part VII: Optimization Strategies](#part-vii-optimization-strategies)
9. [Appendices](#appendices)

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

---

## Part I: eBPF Tracing Fundamentals

### What is eBPF?

eBPF (extended Berkeley Packet Filter) is a revolutionary Linux kernel technology that allows running sandboxed programs inside the kernel without changing kernel source code or loading kernel modules.

**Key Properties of eBPF:**
- **Safe**: The BPF verifier ensures programs terminate and don't crash the kernel
- **Fast**: JIT-compiled to native code, runs at near-native speed
- **Limited**: No unbounded loops, no arbitrary memory access, fixed stack size (512 bytes)

### Kprobes vs Uprobes

| Type | Attaches To | Used For | Example |
|------|-------------|----------|---------|
| **Kprobe** | Kernel functions | Page fault handling | `handle_mm_fault` |
| **Kretprobe** | Kernel function returns | Get return values | Major fault detection |
| **Uprobe** | User-space functions | MDBX operations | `mdbx_cursor_get` |
| **Uretprobe** | User-space returns | Latency, return codes | Operation completion |

### How the Profiler Attaches

```
┌─────────────────────────────────────────────────────────────┐
│                      Linux Kernel                            │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │  handle_mm_fault()                                       │ │
│  │    ↓                                                     │ │
│  │  [KPROBE] → Save context (address, timestamp, VMA)      │ │
│  │    ↓                                                     │ │
│  │  [KRETPROBE] → Check VM_FAULT_MAJOR, read page type,    │ │
│  │                emit event with latency                   │ │
│  └─────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                    User Space (reth)                         │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │  libmdbx.so                                              │ │
│  │    mdbx_cursor_get()                                     │ │
│  │      ↓                                                   │ │
│  │    [UPROBE] → Register active_op, init fault stats      │ │
│  │      ↓                                                   │ │
│  │    (page faults here are attributed to this operation)  │ │
│  │      ↓                                                   │ │
│  │    [URETPROBE] → Emit event with fault stats, latency   │ │
│  └─────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

---

## Part II: Page Faults - The Physical Layer

### What is a Page Fault?

A **page fault** occurs when a process accesses memory that isn't currently mapped to physical RAM. The CPU traps to the kernel, which must resolve the mapping.

### Major vs Minor Faults

| Type | Page Cache | Disk I/O | Latency | Cost |
|------|------------|----------|---------|------|
| **Minor** | HIT | No | ~1-10μs | Cheap |
| **Major** | MISS | Yes | ~1-10ms | 100-1000x slower |

```
Minor Fault:                    Major Fault:
┌──────────────┐               ┌──────────────┐
│ Virtual Addr │               │ Virtual Addr │
└──────┬───────┘               └──────┬───────┘
       ↓                              ↓
┌──────────────┐               ┌──────────────┐
│ Page Cache   │               │ Page Cache   │
│ (HIT!)       │               │ (MISS)       │
└──────┬───────┘               └──────┬───────┘
       ↓                              ↓
┌──────────────┐               ┌──────────────┐
│ Update PTEs  │               │ Read from    │
│ ~5μs         │               │ Disk: ~5ms   │
└──────────────┘               └──────────────┘
```

### How We Detect Major Faults

The kernel's `handle_mm_fault` returns a `vm_fault_t` bitmap. We check the `VM_FAULT_MAJOR` flag:

```c
// In kretprobe:
SEC("kretprobe/handle_mm_fault")
int BPF_KRETPROBE(trace_page_fault_ret, vm_fault_t ret) {
    __u8 is_major = (ret & VM_FAULT_MAJOR) ? 1 : 0;
    // ...
}
```

---

## Part III: MDBX Internals and B+ Trees

### B+ Tree Structure

MDBX stores all data in B+ trees. Understanding this structure is key to interpreting profiler output.

```
                         ┌─────────────────────┐
                         │    Root Branch      │
                         │  [K1=50, K2=100]    │
                         └─────────┬───────────┘
                                   │
              ┌────────────────────┼────────────────────┐
              ↓                    ↓                    ↓
    ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
    │  Branch: <50    │  │ Branch: 50-100  │  │  Branch: >100   │
    │  [K=10, K=30]   │  │  [K=60, K=80]   │  │ [K=120, K=150]  │
    └────────┬────────┘  └────────┬────────┘  └────────┬────────┘
             │                    │                    │
      ┌──────┴──────┐      ┌──────┴──────┐      ┌──────┴──────┐
      ↓      ↓      ↓      ↓      ↓      ↓      ↓      ↓      ↓
    ┌───┐  ┌───┐  ┌───┐  ┌───┐  ┌───┐  ┌───┐  ┌───┐  ┌───┐  ┌───┐
    │L1 │→ │L2 │→ │L3 │→ │L4 │→ │L5 │→ │L6 │→ │L7 │→ │L8 │→ │L9 │
    │   │  │   │  │   │  │   │  │   │  │   │  │   │  │   │  │   │
    └───┘  └───┘  └───┘  └───┘  └───┘  └───┘  └───┘  └───┘  └───┘
     ↑                                                         ↑
     └─────────────── Leaves linked for range scans ───────────┘
```

**Key Properties:**
- **Branch pages**: Only keys and child pointers (for navigation)
- **Leaf pages**: Actual key-value data
- **Leaf links**: Sequential iteration doesn't revisit branches
- **Wide fanout**: Hundreds of keys per node → shallow trees

### Page Types

| Type | Flag | Purpose | Profile Impact |
|------|------|---------|----------------|
| **Meta** | `0x08` | DB metadata (pages 0-1) | Rare access |
| **Branch** | `0x01` | Tree navigation | Traversal overhead |
| **Leaf** | `0x02` | Key-value storage | Data access |
| **Overflow** | `0x04` | Large values | Big value access |
| **DupFixed** | `0x20` | Sorted duplicates | DUPSORT tables |

### Page Header Layout

The profiler reads page headers to detect types:

```
MDBX Page Header (20 bytes):
┌────────────────────────────────────────────────────────────┐
│ Offset │ Size │ Field        │ Description                 │
├────────┼──────┼──────────────┼─────────────────────────────┤
│   0    │  8   │ txnid        │ Transaction ID              │
│   8    │  2   │ dupfix_ksize │ Key size for DUPFIXED       │
│  10    │  2   │ flags        │ Page type flags ← WE READ   │
│  12    │  2   │ lower        │ Lower free space bound      │
│  14    │  2   │ upper        │ Upper free space bound      │
│  16    │  4   │ pgno         │ Page number                 │
└────────────────────────────────────────────────────────────┘
```

---

## Part IV: Reth's Database Architecture

### The Big Picture

Reth uses MDBX to store all Ethereum state in a single database file (`mdbx.dat`). This file contains ~30 separate "tables" (MDBX calls them "sub-databases"), each with its own B+ tree.

```
┌─────────────────────────────────────────────────────────────────┐
│                        mdbx.dat (~500GB+)                        │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │ Meta Pages (0-1)                                            ││
│  ├─────────────────────────────────────────────────────────────┤│
│  │ Free List (page 2)                                          ││
│  ├─────────────────────────────────────────────────────────────┤│
│  │ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐            ││
│  │ │CanonicalHdr│ │   Headers   │ │Transactions │  ...        ││
│  │ │  B+ Tree   │ │   B+ Tree   │ │   B+ Tree   │             ││
│  │ └─────────────┘ └─────────────┘ └─────────────┘            ││
│  │ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐            ││
│  │ │PlainAcctSt │ │PlainStorage │ │HashedAccts  │  ...        ││
│  │ │  B+ Tree   │ │   B+ Tree   │ │   B+ Tree   │             ││
│  │ └─────────────┘ └─────────────┘ └─────────────┘            ││
│  │ ┌─────────────┐ ┌─────────────┐                            ││
│  │ │AccountsTrie│ │StoragesTrie │  ... (30+ tables total)    ││
│  │ │  B+ Tree   │ │   B+ Tree   │                             ││
│  │ └─────────────┘ └─────────────┘                            ││
│  └─────────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────┘
```

### DBI (Database Index) Assignment

MDBX assigns each table a numeric **DBI** when opened. The profiler maps DBIs to table names:

```rust
// From src/event.rs - DBI to table name mapping
pub fn dbi_to_table_name(dbi: u32) -> &'static str {
    match dbi {
        0 => "FREE_DBI (internal)",
        1 => "MAIN_DBI (internal)",
        2 => "CanonicalHeaders",
        3 => "HeaderTerminalDifficulties",
        4 => "HeaderNumbers",
        5 => "Headers",
        6 => "BlockBodyIndices",
        // ... (see full mapping in src/event.rs)
        22 => "AccountsTrie",
        23 => "StoragesTrie",
        // ...
    }
}
```

**Important**: DBI assignment follows declaration order in reth's `tables!` macro. If reth reorders tables, this mapping needs updating.

### Complete Table Reference by Category

#### Category 1: Block Structure Tables

These tables store the blockchain structure itself.

| DBI | Table | Key | Value | Size | Access Pattern |
|-----|-------|-----|-------|------|----------------|
| 2 | **CanonicalHeaders** | `BlockNumber` (u64 BE) | `HeaderHash` (B256) | Small | Sequential during sync |
| 3 | **HeaderTerminalDifficulties** | `BlockNumber` (u64 BE) | `U256` | Small | Sequential |
| 4 | **HeaderNumbers** | `HeaderHash` (B256) | `BlockNumber` (u64) | Medium | Random (by hash) |
| 5 | **Headers** | `BlockNumber` (u64 BE) | `Header` (RLP) | Large | Sequential sync, random RPC |
| 6 | **BlockBodyIndices** | `BlockNumber` (u64 BE) | `StoredBlockBodyIndices` | Medium | Sequential |
| 7 | **BlockOmmers** | `BlockNumber` (u64 BE) | `StoredBlockOmmers` | Small | Rare (post-merge) |
| 8 | **BlockWithdrawals** | `BlockNumber` (u64 BE) | `StoredBlockWithdrawals` | Medium | Sequential (post-Shanghai) |

**Key Format - BlockNumber:**
```
┌────────────────────────────────────┐
│ 8 bytes: BlockNumber (big-endian)  │
│ Example: 0x0000000001312D00        │
│          = Block 20,000,000        │
└────────────────────────────────────┘
```

**Usage During Sync:**
- Written sequentially as blocks are processed
- Read randomly for reorg handling
- CanonicalHeaders is the "source of truth" for chain head

#### Category 2: Transaction Tables

| DBI | Table | Key | Value | Size | Access Pattern |
|-----|-------|-----|-------|------|----------------|
| 9 | **Transactions** | `TxNumber` (u64 BE) | `TransactionSigned` (RLP) | Very Large | Sequential write, random read |
| 10 | **TransactionHashNumbers** | `TxHash` (B256) | `TxNumber` (u64) | Large | Random (by hash) |
| 11 | **TransactionBlocks** | `TxNumber` (u64 BE) | `BlockNumber` (u64) | Medium | Random lookups |
| 12 | **Receipts** | `TxNumber` (u64 BE) | `Receipt` (compact) | Very Large | Sequential write, random read |
| 26 | **TransactionSenders** | `TxNumber` (u64 BE) | `Address` (20 bytes) | Large | Sequential |

**Key Insight - TxNumber vs TxHash:**
- Reth uses a global `TxNumber` (monotonic counter) as the primary key
- `TransactionHashNumbers` provides hash→number lookup for RPC
- This design enables efficient sequential storage and retrieval

```
Transaction Indexing:
Block 1: Tx 0, Tx 1, Tx 2          BlockBodyIndices[1] = {first_tx: 0, tx_count: 3}
Block 2: Tx 3, Tx 4                 BlockBodyIndices[2] = {first_tx: 3, tx_count: 2}  
Block 3: Tx 5, Tx 6, Tx 7, Tx 8     BlockBodyIndices[3] = {first_tx: 5, tx_count: 4}
```

#### Category 3: State Tables (Current World State)

These are the "hot" tables during execution - they represent the current Ethereum world state.

| DBI | Table | Key | Value | Size | Access Pattern |
|-----|-------|-----|-------|------|----------------|
| 14 | **PlainAccountState** | `Address` (20 bytes) | `Account` | Large | Random (by address) |
| 15 | **PlainStorageState** | `Address ++ StorageKey` (52 bytes) | `StorageValue` (32 bytes) | Very Large | Random |
| 13 | **Bytecodes** | `CodeHash` (B256) | `Bytecode` (bytes) | Large | Random, cached |

**Key Formats:**

```
PlainAccountState Key:
┌────────────────────────────────────┐
│ 20 bytes: Raw Ethereum address     │
│ Example: 0xdAC17F958D2ee523a2206206994597C13D831ec7 (USDT) │
└────────────────────────────────────┘

PlainStorageState Key (DUPSORT table):
┌────────────────────────────────────┬────────────────────────────────────┐
│ 20 bytes: Contract address         │ 32 bytes: Storage slot (B256)     │
└────────────────────────────────────┴────────────────────────────────────┘
Total: 52 bytes

Account Value:
┌──────────┬──────────┬──────────────┬──────────────┐
│ nonce    │ balance  │ bytecode_hash│ (compact)    │
│ (varint) │ (u256)   │ (optional)   │              │
└──────────┴──────────┴──────────────┴──────────────┘
```

**Why PlainStorageState is Huge:**
- Every storage slot ever written is here
- Key is 52 bytes, value is 32 bytes
- A single contract like Uniswap can have millions of slots
- This table often exceeds 200GB on mainnet

#### Category 4: Hashed State Tables (Merkle Preparation)

These tables store the same data as Plain tables but with **keccak256-hashed keys** for Merkle trie construction.

| DBI | Table | Key | Value | Size | Access Pattern |
|-----|-------|-----|-------|------|----------------|
| 20 | **HashedAccounts** | `keccak256(Address)` (32 bytes) | `Account` | Large | Sequential iteration |
| 21 | **HashedStorages** | `keccak256(Address) ++ keccak256(Slot)` (64 bytes) | `Value` | Very Large | Sequential iteration |

**Why Hashed Keys?**

The Ethereum state trie requires keys to be uniformly distributed for balanced tree construction. Hashing addresses ensures this:

```
Original addresses (clustered):          Hashed keys (uniformly distributed):
0x0000000000000000000000000000000001     0x5fe7f977e71dba2ea1a68e21057beebb...
0x0000000000000000000000000000000002  →  0xf2ee15ea639b73fa3db9b34a245bdfa0...
0x0000000000000000000000000000000003     0x69c322e3248a5dfc29d73c5b0553b066...
```

**Access Pattern During State Root Calculation:**
1. Walk `HashedAccounts` in sorted order (SET_RANGE + NEXT)
2. For each account, walk its storage in `HashedStorages`
3. Build trie nodes bottom-up
4. This is I/O intensive - sequential but massive data volume

#### Category 5: Merkle Trie Tables

These store the actual trie nodes for state root computation.

| DBI | Table | Key | Value | Size | Access Pattern |
|-----|-------|-----|-------|------|----------------|
| 22 | **AccountsTrie** | `Nibbles` (variable) | `BranchNodeCompact` | Large | Random traversal |
| 23 | **StoragesTrie** | `HashedAddress ++ Nibbles` | `BranchNodeCompact` | Very Large | Random traversal |
| 24 | **AccountsTrieChangeSets** | `BlockNumber` | `TrieUpdates` | Medium | Sequential |
| 25 | **StoragesTrieChangeSets** | `BlockNumber` | `TrieUpdates` | Large | Sequential |

**Nibble Path Keys:**

Ethereum's Modified Merkle Patricia Trie uses nibble paths (4-bit units):

```
Address: 0xABCD...
Hash:    0x5f3a...
Nibbles: [5, f, 3, a, ...]

Trie Key Examples:
Root level:      []           (empty nibbles)
First branch:    [5]          (first nibble of path)
Deeper:          [5, f]       (first two nibbles)
Leaf approach:   [5, f, 3, a, ...] (full or partial path)
```

**BranchNodeCompact Value:**
```
┌──────────────────────────────────────────────────────────────┐
│ Compact encoding of:                                         │
│ - state_mask: u16 (which children 0-15 exist)               │
│ - tree_mask: u16 (which children have subtrees)             │
│ - hash_mask: u16 (which children are hash references)       │
│ - hashes: Vec<B256> (child hashes, packed)                  │
│ - root_hash: Option<B256> (if this is a root)               │
└──────────────────────────────────────────────────────────────┘
```

**Why These Tables Are "Hot":**
- State root calculation touches every changed trie path
- Deep traversal through AccountsTrie for each changed account
- Even deeper traversal through StoragesTrie for storage changes
- Random access pattern (trie paths don't correlate spatially)

#### Category 6: History Tables

These enable historical state queries and reorg handling.

| DBI | Table | Key | Value | Size | Access Pattern |
|-----|-------|-----|-------|------|----------------|
| 16 | **AccountsHistory** | `ShardedKey<Address>` | `BlockNumberList` | Large | Random |
| 17 | **StoragesHistory** | `ShardedKey<(Address, Slot)>` | `BlockNumberList` | Very Large | Random |
| 18 | **AccountChangeSets** | `BlockNumber` | `AccountBeforeImage[]` | Large | Sequential |
| 19 | **StorageChangeSets** | `BlockNumber ++ Address` | `StorageBeforeImage[]` | Very Large | Sequential |

**ShardedKey Explained:**

To prevent unbounded value growth, history is sharded:

```
ShardedKey Format:
┌────────────────────────────────────┬────────────────┐
│ Key (Address or Address+Slot)      │ HighestBlock   │
└────────────────────────────────────┴────────────────┘

Example for address 0xABC... changed at blocks 100, 500, 1000, 5000:

If shard size is 1000 blocks:
  Key: [0xABC..., 1000] → Value: [100, 500, 1000]
  Key: [0xABC..., 5000] → Value: [5000]
```

**ChangeSets - The Reorg Safety Net:**

```
AccountChangeSets[BlockNumber] = [
    (Address1, OldNonce, OldBalance, OldCodeHash),
    (Address2, OldNonce, OldBalance, OldCodeHash),
    ...
]

On reorg to block N:
1. For each block > N, read ChangeSets
2. Restore previous values to PlainAccountState
3. Delete the changeset entries
```

#### Category 7: Metadata Tables

| DBI | Table | Key | Value | Purpose |
|-----|-------|-----|-------|---------|
| 27 | **StageCheckpoints** | `StageId` (string) | `StageCheckpoint` | Sync progress |
| 28 | **StageCheckpointProgresses** | `StageId` | `Vec<u8>` | Stage-specific state |
| 29 | **PruneCheckpoints** | `PruneSegment` | `PruneCheckpoint` | Pruning progress |
| 30 | **VersionHistory** | `u64` | `ClientVersion` | DB version tracking |
| 31 | **ChainState** | `ChainStateKey` | `ChainStateValue` | Chain metadata |
| 32 | **Metadata** | `MetadataKey` | `MetadataValue` | General metadata |

### Data Flow During Block Execution

Understanding the data flow helps interpret profiler output:

```
                          Block Execution Flow
                          
┌─────────────────────────────────────────────────────────────────┐
│  1. FETCH BLOCK HEADER                                          │
│     Headers[block_num] → Header                                 │
│     BlockBodyIndices[block_num] → {first_tx, tx_count}         │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│  2. EXECUTE TRANSACTIONS                                        │
│     For each transaction:                                       │
│     ├── Transactions[tx_num] → Transaction data                │
│     ├── PlainAccountState[from] → Sender account               │
│     ├── PlainAccountState[to] → Recipient/contract             │
│     ├── Bytecodes[code_hash] → Contract code (if call)         │
│     └── PlainStorageState[contract, slot] → Storage values     │
│                                                                 │
│     Writes accumulated in memory (not DB yet)                   │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│  3. PERSIST STATE CHANGES                                       │
│     PlainAccountState[addr] ← New account state                │
│     PlainStorageState[addr, slot] ← New storage values         │
│     AccountChangeSets[block] ← Old values (for reorg)          │
│     StorageChangeSets[block, addr] ← Old storage               │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│  4. UPDATE HASHED STATE (Merkle Prep)                           │
│     HashedAccounts[keccak(addr)] ← Account                     │
│     HashedStorages[keccak(addr), keccak(slot)] ← Value         │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│  5. COMPUTE STATE ROOT                                          │
│     Traverse AccountsTrie from root                             │
│     For each changed path: read/update nodes                    │
│     Traverse StoragesTrie for each changed contract            │
│     Compute root hash                                           │
│                                                                 │
│     *** THIS IS THE EXPENSIVE PART - RANDOM I/O HEAVY ***      │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│  6. COMMIT TRANSACTION (MDBX)                                   │
│     All changes flushed to disk                                 │
│     Copy-on-write pages become visible                          │
└─────────────────────────────────────────────────────────────────┘
```

### Table Size Estimates (Mainnet, ~21M blocks)

| Table | Approx Size | % of Total | Notes |
|-------|-------------|------------|-------|
| PlainStorageState | 200-250 GB | 40-50% | Largest by far |
| Transactions | 80-100 GB | 15-20% | Grows with chain |
| Receipts | 50-70 GB | 10-15% | Logs are big |
| StoragesTrie | 40-60 GB | 8-12% | Trie nodes |
| HashedStorages | 30-50 GB | 6-10% | Duplicates storage keys |
| Headers | 20-30 GB | 4-6% | ~500 bytes each |
| AccountsTrie | 10-20 GB | 2-4% | Account trie |
| *Others* | 20-40 GB | 4-8% | Combined |
| **Total** | ~500 GB | 100% | Varies with history |

---

## Part V: The Profiler Implementation

### Architecture Overview

```
┌────────────────────────────────────────────────────────────────────┐
│                        BPF Program (Kernel)                         │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │                        Maps (Shared State)                    │  │
│  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────────────────┐ │  │
│  │  │ active_ops  │ │cursor_to_dbi│ │ op_fault_stats_map      │ │  │
│  │  │ (per-thread │ │ (cursor ptr │ │ (per-op fault counters) │ │  │
│  │  │  operation) │ │  → table)   │ │                         │ │  │
│  │  └─────────────┘ └─────────────┘ └─────────────────────────┘ │  │
│  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────────────────┐ │  │
│  │  │pending_faults│ │vma_to_offset│ │  events (ring buffer)   │ │  │
│  │  │(kprobe ctx) │ │ (VMA → file)│ │  → userspace            │ │  │
│  │  └─────────────┘ └─────────────┘ └─────────────────────────┘ │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                                                                     │
│  ┌──────────────────┐    ┌──────────────────┐                      │
│  │ Kprobes          │    │ Uprobes          │                      │
│  │ • handle_mm_fault│    │ • mdbx_cursor_get│                      │
│  │ • (kretprobe)    │    │ • mdbx_cursor_put│                      │
│  │                  │    │ • mdbx_get/put   │                      │
│  │                  │    │ • mdbx_txn_*     │                      │
│  └──────────────────┘    └──────────────────┘                      │
└────────────────────────────────────────────────────────────────────┘
                              ↓ Ring Buffer
┌────────────────────────────────────────────────────────────────────┐
│                    Userspace (Rust)                                 │
│  ┌────────────────┐ ┌─────────────────┐ ┌────────────────────────┐ │
│  │ Event Consumer │ │ Event Processor │ │ Viewer Generator       │ │
│  │ (ring buffer)  │→│ (correlation)   │→│ (HTML/JSON output)     │ │
│  └────────────────┘ └─────────────────┘ └────────────────────────┘ │
└────────────────────────────────────────────────────────────────────┘
```

### BPF Maps In Detail

#### 1. `active_ops` - The Heart of Attribution

This map tracks what MDBX operation each thread is currently executing:

```c
struct active_op {
    __u64 start_ns;          // When operation started
    __u32 dbi;               // Which table (DBI)
    __u32 op_type;           // EVENT_CURSOR_GET, EVENT_CURSOR_PUT, etc.
    __u32 cursor_op;         // MDBX_SET_RANGE, MDBX_NEXT, etc.
    __u32 _pad;              // Alignment
    __u8  key_prefix[16];    // First 16 bytes of key
};

struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 4096);
    __type(key, __u64);          // pid_tgid (thread ID)
    __type(value, struct active_op);
} active_ops SEC(".maps");
```

**How it enables 100% accurate attribution:**

```
Timeline:
────────────────────────────────────────────────────────────────
Thread 1:  ┌──── mdbx_cursor_get (HashedAccounts, SET_RANGE) ────┐
           │                                                      │
           │     [PAGE FAULT]   [PAGE FAULT]   [PAGE FAULT]      │
           │         ↓               ↓               ↓            │
           │    Check active_ops[thread_1]                        │
           │    → dbi=20 (HashedAccounts)                         │
           │    → cursor_op=SET_RANGE                             │
           │    → 100% accurate attribution!                      │
           │                                                      │
           └──────────────────────────────────────────────────────┘
────────────────────────────────────────────────────────────────
```

#### 2. `cursor_to_dbi` - Cursor Tracking

MDBX cursors are opaque pointers. We track which table each cursor operates on:

```c
// Populated when cursor is opened:
SEC("uprobe/mdbx_cursor_open")
int BPF_UPROBE(trace_cursor_open, void *txn, __u32 dbi, void **cursor_ptr) {
    // Save dbi for uretprobe
    struct cursor_open_context octx = { .dbi = dbi, .cursor_ptr_ptr = cursor_ptr };
    bpf_map_update_elem(&pending_cursor_opens, &pid_tgid, &octx, BPF_ANY);
}

SEC("uretprobe/mdbx_cursor_open")
int BPF_URETPROBE(trace_cursor_open_ret, int ret) {
    // Read cursor pointer from output parameter, map it to DBI
    void *cursor = *octx->cursor_ptr_ptr;
    bpf_map_update_elem(&cursor_to_dbi, &cursor, &octx->dbi, BPF_ANY);
}
```

**The Pre-Trace Cursor Problem:**

If a cursor was opened before tracing started, we can't map it:

```c
// In mdbx_cursor_get uprobe:
__u32 *dbi_ptr = bpf_map_lookup_elem(&cursor_to_dbi, &cursor_addr);
if (dbi_ptr) {
    ctx.dbi = *dbi_ptr;  // Known cursor
} else {
    // Cursor opened before tracing - try to read from struct
    // Fallback: use sentinel value 0xFFFFFFFE
    ctx.dbi = 0xFFFFFFFE;  // "Unknown (pre-trace cursor)"
}
```

The profiler reports these as "Unknown (pre-trace cursor)" and warns if they're significant.

#### 3. `op_fault_stats_map` - Per-Operation Statistics

Each operation accumulates fault statistics:

```c
struct op_fault_stats {
    __u32 fault_count;         // Total page faults
    __u32 major_fault_count;   // Major faults (disk I/O)
    __u32 branch_faults;       // B+ tree branch pages
    __u32 leaf_faults;         // B+ tree leaf pages
    __u32 overflow_faults;     // Large value pages
    __u32 current_depth;       // Current tree depth (resets on leaf)
    __u32 max_depth;           // Maximum depth seen
    __u32 _pad;                // Alignment
    __u64 total_fault_latency_ns;  // Cumulative fault time
};
```

**Lifecycle:**

```
1. UPROBE (mdbx_cursor_get entry):
   - Initialize: op_fault_stats_map[thread] = {0}
   - Register:   active_ops[thread] = {dbi, op_type, ...}

2. PAGE FAULTS (during operation):
   - Lookup:     stats = op_fault_stats_map[thread]
   - Increment:  stats->fault_count++
   - If major:   stats->major_fault_count++
   - If branch:  stats->branch_faults++; stats->current_depth++
   - If leaf:    stats->leaf_faults++; stats->current_depth = 0

3. URETPROBE (mdbx_cursor_get return):
   - Copy stats to event
   - Emit event via ring buffer
   - Delete:     op_fault_stats_map[thread]
   - Delete:     active_ops[thread]
```

### Tree Depth Measurement Algorithm

The profiler measures B+ tree traversal depth by tracking consecutive branch page faults:

```c
// In page fault handler, after determining page_type:
if (page_type == PAGE_TYPE_BRANCH) {
    __sync_fetch_and_add(&stats->branch_faults, 1);
    
    // Track tree depth: each branch page = one level deeper
    stats->current_depth += 1;
    if (stats->current_depth > stats->max_depth) {
        stats->max_depth = stats->current_depth;
    }
    
} else if (page_type == PAGE_TYPE_LEAF) {
    __sync_fetch_and_add(&stats->leaf_faults, 1);
    
    // Reset depth when we reach data (leaf = end of traversal)
    stats->current_depth = 0;
    
} else if (page_type == PAGE_TYPE_OVERFLOW) {
    __sync_fetch_and_add(&stats->overflow_faults, 1);
    // Overflow is leaf extension, also reset
    stats->current_depth = 0;
}
```

**Important Caveats:**

1. **Only measures uncached depth**: If branch pages are in cache (minor faults), we see them but they don't represent "disk depth"

2. **Resets on each leaf hit**: If an operation touches multiple leaf pages, depth resets between them

3. **Maximum observed**: `max_depth` captures the deepest single traversal, not cumulative

**Visualization of depth tracking:**

```
SET_RANGE operation seeking key "0xABCD...":

        ┌─────────┐
        │ Branch  │ ← Page fault #1: current_depth = 1
        └────┬────┘
             ↓
        ┌─────────┐
        │ Branch  │ ← Page fault #2: current_depth = 2
        └────┬────┘
             ↓
        ┌─────────┐
        │ Branch  │ ← Page fault #3: current_depth = 3, max_depth = 3
        └────┬────┘
             ↓
        ┌─────────┐
        │  Leaf   │ ← Page fault #4: leaf_faults++, current_depth = 0
        └─────────┘

Final stats: branch_faults=3, leaf_faults=1, max_depth=3
```

### Page Type Detection

After a page fault completes, the page is mapped and readable. We read the MDBX page header:

```c
// Page is now mapped, safe to read
__u64 page_start = address & ~0xFFFULL;  // Align to 4KB boundary
__u16 flags = 0;

// Read flags field at offset 10 in page header
if (bpf_probe_read_user(&flags, sizeof(flags), 
                        (void *)(page_start + MDBX_PAGE_FLAGS_OFFSET)) == 0) {
    
    if (flags & MDBX_P_META) {
        page_type = PAGE_TYPE_META;
    } else if (flags & MDBX_P_LARGE) {
        page_type = PAGE_TYPE_OVERFLOW;
    } else if (flags & MDBX_P_BRANCH) {
        page_type = PAGE_TYPE_BRANCH;
    } else if (flags & (MDBX_P_LEAF | MDBX_P_DUPFIX)) {
        page_type = PAGE_TYPE_LEAF;
    }
    // else: stays PAGE_TYPE_UNKNOWN
}
```

**Why this works:**
- The kretprobe fires *after* the page fault is resolved
- The page is now mapped into user space
- `bpf_probe_read_user` safely reads from user memory

### Event Structures

#### Page Fault Event (96 bytes)

```c
struct page_fault_event {
    __u64 timestamp_ns;      // When fault occurred
    __u64 address;           // Faulting virtual address
    __u64 file_offset;       // Offset in mdbx.dat
    __u64 vma_start;         // VMA bounds (for validation)
    __u64 vma_end;
    __u32 pid;               // Process ID
    __u32 tid;               // Thread ID
    __u32 event_type;        // EVENT_PAGE_FAULT
    __u32 fault_flags;       // Kernel fault flags
    __u64 latency_ns;        // Time in fault handler
    __u8  is_major;          // 1 if major fault (disk I/O)
    __u8  page_type;         // Branch/Leaf/Overflow/Meta
    __u8  _pad1[2];
    
    // Active operation context (for attribution):
    __u32 active_dbi;        // Which table (0xFFFFFFFF if none)
    __u32 active_op_type;    // CURSOR_GET, CURSOR_PUT, etc.
    __u32 active_cursor_op;  // SET_RANGE, NEXT, etc.
    __u8  active_key_prefix[16];  // First 16 bytes of key
};
```

#### Cursor Event (152 bytes)

```c
struct cursor_event {
    __u64 timestamp_ns;
    __u32 pid;
    __u32 tid;
    __u32 event_type;        // CURSOR_GET, CURSOR_PUT, DIRECT_GET, etc.
    __u32 cursor_op;         // SET_RANGE, NEXT, FIRST, etc.
    __u32 dbi;               // Table identifier
    __u32 key_size;
    __u8  key_data[64];      // First 64 bytes of key
    __s32 return_code;       // 0 = success, negative = error
    __u32 value_size;        // For put operations
    __u64 latency_ns;        // Total operation time
    __u32 write_flags;       // For put: UPSERT, APPEND, etc.
    
    // Per-operation fault statistics:
    __u32 faults_during_op;
    __u32 major_faults_during_op;
    __u32 branch_faults;
    __u32 leaf_faults;
    __u32 overflow_faults;
    __u32 max_tree_depth;    // Measured B+ tree depth
    __u64 fault_latency_ns;  // Time spent in fault handlers
};
```

### Ring Buffer and Userspace Communication

Events flow through a 16MB ring buffer:

```c
struct {
    __uint(type, BPF_MAP_TYPE_RINGBUF);
    __uint(max_entries, 16 * 1024 * 1024);  // 16MB
} events SEC(".maps");

// Emitting an event:
struct cursor_event *e = bpf_ringbuf_reserve(&events, sizeof(*e), 0);
if (!e) {
    inc_stat(STAT_EVENTS_DROPPED);
    return 0;
}

// Fill event fields...
e->timestamp_ns = ...;
e->dbi = ...;

bpf_ringbuf_submit(e, 0);
```

**Why ring buffer (vs perf buffer)?**
- More efficient for high-frequency events
- Variable-size events supported
- No per-CPU overhead for aggregation
- Better for our use case (~100K events/sec possible)

### Atomic Operations and BPF Limitations

BPF has restrictions on atomic operations:

```c
// ✓ Works: 64-bit atomic increment
__sync_fetch_and_add(&stats->fault_count, 1);

// ✗ Doesn't work: 32-bit compare-and-swap
// Error: "unsupported atomic operation, please use 64 bit version"
__sync_bool_compare_and_swap(&stats->max_depth, old, new);

// ✗ Doesn't work: using XADD return value
// Error: "Invalid usage of the XADD return value"
__u32 new = __sync_fetch_and_add(&x, 1) + 1;

// ✓ Our solution: non-atomic for per-thread data
// Safe because op_fault_stats_map is keyed by thread ID
stats->current_depth += 1;
if (stats->current_depth > stats->max_depth) {
    stats->max_depth = stats->current_depth;
}
```

### Complete Probe List

| Probe | Type | Function | Purpose |
|-------|------|----------|---------|
| `trace_page_fault` | kprobe | `handle_mm_fault` | Capture fault context |
| `trace_page_fault_ret` | kretprobe | `handle_mm_fault` | Detect major/minor, page type |
| `trace_mmap` | kprobe | `do_mmap` | Detect new MDBX mappings |
| `trace_cursor_open` | uprobe | `mdbx_cursor_open` | Record cursor→DBI mapping |
| `trace_cursor_open_ret` | uretprobe | `mdbx_cursor_open` | Get cursor pointer |
| `trace_cursor_close` | uprobe | `mdbx_cursor_close` | Clean up mapping |
| `trace_cursor_get` | uprobe | `mdbx_cursor_get` | Start tracking get operation |
| `trace_cursor_get_ret` | uretprobe | `mdbx_cursor_get` | Emit get event with stats |
| `trace_cursor_put` | uprobe | `mdbx_cursor_put` | Start tracking put operation |
| `trace_cursor_put_ret` | uretprobe | `mdbx_cursor_put` | Emit put event |
| `trace_cursor_del` | uprobe | `mdbx_cursor_del` | Start tracking delete |
| `trace_cursor_del_ret` | uretprobe | `mdbx_cursor_del` | Emit delete event |
| `trace_direct_get` | uprobe | `mdbx_get` | Track direct get (no cursor) |
| `trace_direct_get_ret` | uretprobe | `mdbx_get` | Emit direct get event |
| `trace_direct_put` | uprobe | `mdbx_put` | Track direct put |
| `trace_direct_put_ret` | uretprobe | `mdbx_put` | Emit direct put event |
| `trace_direct_del` | uprobe | `mdbx_del` | Track direct delete |
| `trace_direct_del_ret` | uretprobe | `mdbx_del` | Emit direct delete event |
| `trace_txn_begin` | uprobe | `mdbx_txn_begin_ex` | Track transaction start |
| `trace_txn_begin_ret` | uretprobe | `mdbx_txn_begin_ex` | Record txn pointer |
| `trace_txn_commit` | uprobe | `mdbx_txn_commit_ex` | Track commit start |
| `trace_txn_commit_ret` | uretprobe | `mdbx_txn_commit_ex` | Record commit latency |
| `trace_txn_abort` | uprobe | `mdbx_txn_abort` | Record transaction abort |

---

## Part VI: Interpreting Results

### Key Metrics

| Metric | Good | Concerning | Critical |
|--------|------|------------|----------|
| Major Fault Ratio | <5% | 5-20% | >20% |
| Branch:Leaf Ratio | <0.1 | 0.1-0.3 | >0.3 |
| Avg Tree Depth | 1-2 | 3-5 | >5 |
| Slow Ops (>100μs) | <1% | 1-10% | >10% |
| Pre-trace Cursors | <5% | 5-20% | >20% |

### Table Analysis Strategy

1. **Sort by time lost** (slow ops × avg latency)
2. **Check major fault ratio per table**
3. **Compare branch vs leaf faults** (traversal overhead)
4. **Look for hot keys** (specific keys with repeated slow access)

---

## Part VII: Optimization Strategies

### By Table Category

| Category | Problem | Solution |
|----------|---------|----------|
| **State Tables** | Random access | Increase RAM, use SSD |
| **Trie Tables** | Deep traversal | Consider flat storage |
| **Hashed Tables** | Sequential but huge | Parallelize Merkle |
| **History Tables** | Grows unbounded | Enable pruning |

### Quick Wins

1. **More RAM**: Most effective for reducing major faults
2. **NVMe SSD**: 10-100x faster than HDD for random I/O
3. **Larger batches**: Amortize commit overhead
4. **Start tracing early**: Avoid pre-trace cursor problem

---

## Appendices

### A: MDBX Return Codes

| Code | Name | Meaning |
|------|------|---------|
| 0 | SUCCESS | Operation completed |
| -30798 | NOTFOUND | Key not found |
| -30799 | PAGE_NOTFOUND | Page missing (corruption) |
| -30797 | CORRUPTED | Database corrupted |
| -30796 | PANIC | Unrecoverable error |
| -30795 | VERSION_MISMATCH | Wrong MDBX version |
| -30794 | INVALID | Invalid parameter |

### B: BPF Map Sizes

| Map | Max Entries | Entry Size | Total Size |
|-----|-------------|------------|------------|
| events | N/A | Variable | 16 MB |
| tracked_inodes | 16 | 9 bytes | 144 B |
| vma_to_offset | 256 | 16 bytes | 4 KB |
| cursor_to_dbi | 10,240 | 12 bytes | 120 KB |
| active_ops | 4,096 | 32 bytes | 128 KB |
| op_fault_stats_map | 4,096 | 40 bytes | 160 KB |
| pending_* | 1,024-10,240 | Varies | ~1 MB total |

### C: Build and Run

```bash
# Build
cargo build --release

# Run (requires root for BPF)
sudo ./target/release/mdbx-profiler \
    --mdbx-path /path/to/mdbx.dat \
    --pid $(pgrep reth) \
    --duration 300 \
    --output trace.html
```

---

*Last updated: January 2025*
*Profiler version: Compatible with reth main branch*
