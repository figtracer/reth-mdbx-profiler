# reth-mdbx-profiler

ebpf-based profiler for analyzing mdbx page fault patterns and cursor operations in reth.

## the problem: bridging physical and logical layers

database profiling traditionally operates at two disconnected layers:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           LOGICAL LAYER                                     │
│                                                                             │
│  what the application is doing:                                             │
│  • mdbx_cursor_get(HashedStorages, key=0xabc..., SET_RANGE)                │
│  • mdbx_cursor_put(AccountsTrie, key=0x123..., UPSERT)                     │
│                                                                             │
│  we know: table, key, operation type, latency                              │
│  we DON'T know: why it's slow, which pages it touched                      │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                         ??? (the gap) ???
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           PHYSICAL LAYER                                    │
│                                                                             │
│  what the kernel is doing:                                                  │
│  • page fault at offset 0x1a3f000 (major - disk I/O)                       │
│  • page fault at offset 0x8bc2000 (minor - from cache)                     │
│                                                                             │
│  we know: file offset, major/minor, latency                                │
│  we DON'T know: which table, which key, which operation caused it          │
└─────────────────────────────────────────────────────────────────────────────┘
```

mdbx stores all tables interleaved in a single file - you can't map a file offset to a table. page 1000 might be `HashedStorages`, page 1001 might be `AccountsTrie`.

## our solution: direct bpf attribution

we bridge these layers by tracking the **active mdbx operation** on each thread:

```c
// bpf map: what is each thread currently doing?
struct active_op {
    u32 dbi;           // which table
    u32 op_type;       // GET, PUT, DEL
    u32 cursor_op;     // SET_RANGE, NEXT, etc
    u8  key_prefix[16]; // first 16 bytes of key
};
```

when a page fault occurs, we look up the active operation on that thread:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                      UNIFIED EVENT (Physical + Logical)                     │
│                                                                             │
│  page fault at offset 0x1a3f000:                                           │
│    physical: major fault, latency 450μs, thread 1234                       │
│    logical:  during cursor_get on HashedStorages, key=0xabc..., SET_RANGE  │
│                                                                             │
│  → 100% accurate attribution, not statistical correlation                  │
└─────────────────────────────────────────────────────────────────────────────┘
```

**result:** 97%+ of page faults are directly attributed to the exact mdbx operation that caused them.

## what it does

- traces page faults on mdbx memory-mapped regions with **direct operation attribution**
- traces mdbx cursor operations (seeks, gets, navigation, puts, deletes)
- shows which tables, operations, and keys cause the most disk I/O
- generates interactive html visualizations

## requirements

- linux kernel 5.8+ with btf enabled
- root access
- reth node with mdbx database

## quick start

```bash
# build
cargo build --release

# trace (with cursor operations for full attribution)
./target/release/mdbx-profiler trace \
    --process-name reth \
    --mdbx-path /data/reth/db/mdbx.dat \
    --duration 60s \
    --output trace.jsonl \
    --trace-cursors \
    --reth-binary /path/to/reth

# generate visualization
./target/release/mdbx-trace-analyzer \
    --input trace.jsonl \
    --mdbx-path /data/reth/db/mdbx.dat
```

## example output

```
Page Faults by Operation Type         [Direct BPF Attribution]
────────────────────────────────────────────────────────────────
144.1K directly attributed (97.3%)
  0 timestamp fallback
3.9K uncorrelated

Operation      Faults    Major     %
CURSOR_GET     88.3K    39.7K   61.3%
CURSOR_DEL     21.7K      521   15.1%
CURSOR_PUT     20.5K      733   14.2%
DIRECT_PUT      7.4K    2.1K    5.1%
DIRECT_GET      6.2K    3.0K    4.3%

By Cursor Operation (GET only)
SET_RANGE      30.0K    12.8K   20.8%
GET_BOTH_RANGE 32.6K    16.1K   22.6%
NEXT           15.6K     6.4K   10.8%
```

## commands

### trace

```bash
# recommended: use process name (survives restarts)
./target/release/mdbx-profiler trace \
    --process-name reth \
    --mdbx-path /data/reth/db/mdbx.dat \
    --duration 60s \
    --output trace.jsonl \
    --trace-cursors \
    --reth-binary /path/to/reth

# alternative: use pid directly
./target/release/mdbx-profiler trace \
    --pid $(pgrep reth) \
    --mdbx-path /data/reth/db/mdbx.dat \
    --duration 30s \
    --output trace.jsonl
```

options:
- `--pid`: target process id (use this OR --process-name)
- `--process-name`: process name to trace (auto-detects restarts)
- `--mdbx-path`: path to mdbx.dat file
- `--duration`: how long to trace (e.g., 30s, 5m)
- `--output`: output file (default: trace.jsonl)
- `--trace-cursors`: trace cursor operations (required for attribution)
- `--reth-binary`: path to reth binary (required for cursor tracing)

### analyze

```bash
./target/release/mdbx-profiler analyze --input trace.jsonl --format summary
```

formats: `summary`, `csv`, `json`, `logs`

### web viewer

```bash
./target/release/mdbx-trace-analyzer \
    --input trace.jsonl \
    --mdbx-path /data/reth/db/mdbx.dat
```

the analyzer runs on macos/linux without ebpf - collect traces on your node and analyze locally.

## how it works

see [INTERNALS.md](INTERNALS.md) for the full technical details on:
- ebpf probe architecture
- active operation tracking
- page fault enrichment
- viewer visualizations

## license

MIT OR Apache-2.0
