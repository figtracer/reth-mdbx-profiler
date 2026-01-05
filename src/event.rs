//! Event types shared between BPF and userspace

use serde::{Deserialize, Serialize};

/// Map DBI number to reth table name
///
/// These are based on reth's table definitions in:
/// https://github.com/paradigmxyz/reth/blob/main/crates/storage/db-api/src/tables/mod.rs
///
/// The order matches the `tables!` macro definition order, which determines DBI assignment.
/// DBI 0 and 1 are reserved for MDBX internal use (FREE_DBI, MAIN_DBI).
///
/// **IMPORTANT**: This mapping must match your reth version. If reth adds/removes/reorders
/// tables, this will be wrong. Verify against your reth version's tables/mod.rs.
///
/// Last verified against: reth main branch (2025-01)
/// Sentinel value for cursors opened before tracing started where DBI couldn't be determined
pub const DBI_UNKNOWN_SENTINEL: u32 = 0xFFFFFFFE;

/// Check if a DBI value represents a cursor opened before tracing
/// (either our sentinel or a memory address used as fallback)
pub fn is_pre_trace_cursor(dbi: u32) -> bool {
    dbi == DBI_UNKNOWN_SENTINEL || dbi > 100
}

pub fn dbi_to_table_name(dbi: u32) -> &'static str {
    match dbi {
        0 => "FREE_DBI (internal)",
        1 => "MAIN_DBI (internal)",
        2 => "CanonicalHeaders",
        3 => "HeaderTerminalDifficulties",
        4 => "HeaderNumbers",
        5 => "Headers",
        6 => "BlockBodyIndices",
        7 => "BlockOmmers",
        8 => "BlockWithdrawals",
        9 => "Transactions",
        10 => "TransactionHashNumbers",
        11 => "TransactionBlocks",
        12 => "Receipts",
        13 => "Bytecodes",
        14 => "PlainAccountState",
        15 => "PlainStorageState",
        16 => "AccountsHistory",
        17 => "StoragesHistory",
        18 => "AccountChangeSets",
        19 => "StorageChangeSets",
        20 => "HashedAccounts",
        21 => "HashedStorages",
        22 => "AccountsTrie",
        23 => "StoragesTrie",
        24 => "AccountsTrieChangeSets",
        25 => "StoragesTrieChangeSets",
        26 => "TransactionSenders",
        27 => "StageCheckpoints",
        28 => "StageCheckpointProgresses",
        29 => "PruneCheckpoints",
        30 => "VersionHistory",
        31 => "ChainState",
        32 => "Metadata",
        DBI_UNKNOWN_SENTINEL => "Unknown (pre-trace cursor)",
        _ if dbi > 100 => "Unknown (pre-trace cursor)",
        _ => "Unknown",
    }
}

/// Maximum key size captured in cursor events (must match BPF MAX_KEY_SIZE)
pub const MAX_KEY_SIZE: usize = 64;

/// Event types matching the BPF definitions
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventType {
    PageFault = 1,
    Mmap = 2,
    CursorGet = 3,
    CursorPut = 4,
    DirectGet = 5,
}

/// MDBX cursor operations matching libmdbx MDBX_cursor_op enum
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CursorOp {
    First = 0,
    FirstDup = 1,
    GetBoth = 2,
    GetBothRange = 3,
    GetCurrent = 4,
    GetMultiple = 5,
    Last = 6,
    LastDup = 7,
    Next = 8,
    NextDup = 9,
    NextMultiple = 10,
    NextNoDup = 11,
    Prev = 12,
    PrevDup = 13,
    PrevNoDup = 14,
    Set = 15,
    SetKey = 16,
    SetRange = 17,
    PrevMultiple = 18,
    SetLowerbound = 19,
    SetUpperbound = 20,
    Unknown(u32),
}

impl CursorOp {
    /// Create from raw u32 value
    pub fn from_raw(op: u32) -> Self {
        match op {
            0 => Self::First,
            1 => Self::FirstDup,
            2 => Self::GetBoth,
            3 => Self::GetBothRange,
            4 => Self::GetCurrent,
            5 => Self::GetMultiple,
            6 => Self::Last,
            7 => Self::LastDup,
            8 => Self::Next,
            9 => Self::NextDup,
            10 => Self::NextMultiple,
            11 => Self::NextNoDup,
            12 => Self::Prev,
            13 => Self::PrevDup,
            14 => Self::PrevNoDup,
            15 => Self::Set,
            16 => Self::SetKey,
            17 => Self::SetRange,
            18 => Self::PrevMultiple,
            19 => Self::SetLowerbound,
            20 => Self::SetUpperbound,
            other => Self::Unknown(other),
        }
    }

    /// Get the operation name as a string
    pub fn name(&self) -> &'static str {
        match self {
            Self::First => "FIRST",
            Self::FirstDup => "FIRST_DUP",
            Self::GetBoth => "GET_BOTH",
            Self::GetBothRange => "GET_BOTH_RANGE",
            Self::GetCurrent => "GET_CURRENT",
            Self::GetMultiple => "GET_MULTIPLE",
            Self::Last => "LAST",
            Self::LastDup => "LAST_DUP",
            Self::Next => "NEXT",
            Self::NextDup => "NEXT_DUP",
            Self::NextMultiple => "NEXT_MULTIPLE",
            Self::NextNoDup => "NEXT_NODUP",
            Self::Prev => "PREV",
            Self::PrevDup => "PREV_DUP",
            Self::PrevNoDup => "PREV_NODUP",
            Self::Set => "SET",
            Self::SetKey => "SET_KEY",
            Self::SetRange => "SET_RANGE",
            Self::PrevMultiple => "PREV_MULTIPLE",
            Self::SetLowerbound => "SET_LOWERBOUND",
            Self::SetUpperbound => "SET_UPPERBOUND",
            Self::Unknown(_) => "UNKNOWN",
        }
    }

    /// Returns true if this is a seek operation (positions cursor at specific key)
    pub fn is_seek(&self) -> bool {
        matches!(
            self,
            Self::Set
                | Self::SetKey
                | Self::SetRange
                | Self::SetLowerbound
                | Self::SetUpperbound
                | Self::GetBoth
                | Self::GetBothRange
        )
    }

    /// Returns true if this is a navigation operation (moves cursor sequentially)
    pub fn is_navigation(&self) -> bool {
        matches!(
            self,
            Self::First
                | Self::FirstDup
                | Self::Last
                | Self::LastDup
                | Self::Next
                | Self::NextDup
                | Self::NextMultiple
                | Self::NextNoDup
                | Self::Prev
                | Self::PrevDup
                | Self::PrevNoDup
                | Self::PrevMultiple
        )
    }
}

impl std::fmt::Display for CursorOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(v) => write!(f, "UNKNOWN({})", v),
            _ => write!(f, "{}", self.name()),
        }
    }
}

/// Page fault event from BPF
///
/// This struct must match the layout in mdbx_tracer.bpf.c exactly.
/// Layout on x86_64 Linux (72 bytes total):
///   - timestamp_ns: offset 0, size 8
///   - address: offset 8, size 8
///   - file_offset: offset 16, size 8
///   - vma_start: offset 24, size 8
///   - vma_end: offset 32, size 8
///   - pid: offset 40, size 4
///   - tid: offset 44, size 4
///   - event_type: offset 48, size 4
///   - fault_flags: offset 52, size 4
///   - latency_ns: offset 56, size 8
///   - is_major: offset 64, size 1
///   - padding: offset 65, size 7 (for 8-byte alignment)
#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PageFaultEvent {
    /// Kernel timestamp in nanoseconds
    pub timestamp_ns: u64,
    /// Faulting virtual address
    pub address: u64,
    /// Offset within the mmap'd file
    pub file_offset: u64,
    /// VMA start address
    pub vma_start: u64,
    /// VMA end address
    pub vma_end: u64,
    /// Process ID
    pub pid: u32,
    /// Thread ID
    pub tid: u32,
    /// Event type (see EventType enum)
    pub event_type: u32,
    /// Page fault flags (read/write/etc)
    pub fault_flags: u32,
    /// Time spent in fault handler (if available)
    pub latency_ns: u64,
    /// Major fault (disk I/O) vs minor (in page cache)
    pub is_major: u8,
    // Padding to match C struct alignment (7 bytes to reach 72 total)
    pub _pad: [u8; 7],
}

impl PageFaultEvent {
    /// Returns true if this was a major fault (required disk I/O)
    pub fn is_major_fault(&self) -> bool {
        self.is_major != 0
    }

    /// Returns the event type as an enum
    pub fn event_type(&self) -> Option<EventType> {
        match self.event_type {
            1 => Some(EventType::PageFault),
            2 => Some(EventType::Mmap),
            3 => Some(EventType::CursorGet),
            4 => Some(EventType::CursorPut),
            5 => Some(EventType::DirectGet),
            _ => None,
        }
    }

    /// Returns the page number (file_offset / page_size)
    pub fn page_number(&self) -> u64 {
        self.file_offset / 4096
    }

    /// Returns the MDBX B+ tree level hint based on offset patterns
    /// This is a heuristic - MDBX stores metadata at start, then B+ tree nodes
    pub fn estimated_tree_level(&self) -> &'static str {
        let page = self.page_number();
        if page < 16 {
            "metadata"
        } else if page < 1024 {
            "upper-tree"
        } else {
            "data-pages"
        }
    }
}

/// MDBX cursor operation event from BPF
///
/// This struct must match the layout of cursor_event in mdbx_tracer.bpf.c exactly.
/// Layout on x86_64 Linux (112 bytes total):
///   - timestamp_ns: offset 0, size 8
///   - pid: offset 8, size 4
///   - tid: offset 12, size 4
///   - event_type: offset 16, size 4
///   - cursor_op: offset 20, size 4
///   - dbi: offset 24, size 4
///   - key_size: offset 28, size 4
///   - key_data: offset 32, size 64
///   - return_code: offset 96, size 4
///   - padding: offset 100, size 4 (implicit for u64 alignment)
///   - latency_ns: offset 104, size 8
#[repr(C)]
#[derive(Debug, Clone)]
pub struct CursorEvent {
    /// Kernel timestamp in nanoseconds
    pub timestamp_ns: u64,
    /// Process ID
    pub pid: u32,
    /// Thread ID
    pub tid: u32,
    /// Event type (EVENT_CURSOR_GET=3, EVENT_CURSOR_PUT=4, or EVENT_DIRECT_GET=5)
    pub event_type: u32,
    /// MDBX cursor operation (SET_RANGE, NEXT, etc.)
    pub cursor_op: u32,
    /// Database index (table identifier in MDBX)
    pub dbi: u32,
    /// Size of the key
    pub key_size: u32,
    /// First MAX_KEY_SIZE bytes of the key
    pub key_data: [u8; MAX_KEY_SIZE],
    /// Return code from the operation
    pub return_code: i32,
    /// Explicit padding for u64 alignment (to match C struct layout)
    pub _pad: u32,
    /// Time spent in the operation (nanoseconds)
    pub latency_ns: u64,
}

// Custom Serialize implementation for CursorEvent since [u8; 64] doesn't impl Serialize
impl Serialize for CursorEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("CursorEvent", 10)?;
        state.serialize_field("timestamp_ns", &self.timestamp_ns)?;
        state.serialize_field("pid", &self.pid)?;
        state.serialize_field("tid", &self.tid)?;
        state.serialize_field("event_type", &self.event_type)?;
        state.serialize_field("cursor_op", &self.cursor_op)?;
        state.serialize_field("dbi", &self.dbi)?;
        state.serialize_field("key_size", &self.key_size)?;
        // Serialize key_data as hex string for readability
        state.serialize_field("key_data", &self.key_hex())?;
        state.serialize_field("return_code", &self.return_code)?;
        state.serialize_field("latency_ns", &self.latency_ns)?;
        state.end()
    }
}

// Custom Deserialize implementation for CursorEvent
impl<'de> Deserialize<'de> for CursorEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct CursorEventHelper {
            timestamp_ns: u64,
            pid: u32,
            tid: u32,
            event_type: u32,
            cursor_op: u32,
            dbi: u32,
            key_size: u32,
            key_data: String, // Hex string
            return_code: i32,
            latency_ns: u64,
        }

        let helper = CursorEventHelper::deserialize(deserializer)?;

        let mut key_data = [0u8; MAX_KEY_SIZE];
        if let Ok(bytes) = hex::decode(&helper.key_data) {
            let len = bytes.len().min(MAX_KEY_SIZE);
            key_data[..len].copy_from_slice(&bytes[..len]);
        }

        Ok(CursorEvent {
            timestamp_ns: helper.timestamp_ns,
            pid: helper.pid,
            tid: helper.tid,
            event_type: helper.event_type,
            cursor_op: helper.cursor_op,
            dbi: helper.dbi,
            key_size: helper.key_size,
            key_data,
            return_code: helper.return_code,
            _pad: 0,
            latency_ns: helper.latency_ns,
        })
    }
}

impl CursorEvent {
    /// Get the cursor operation as an enum
    pub fn cursor_op(&self) -> CursorOp {
        CursorOp::from_raw(self.cursor_op)
    }

    /// Get the key as a byte slice (truncated to actual size)
    pub fn key(&self) -> &[u8] {
        let len = (self.key_size as usize).min(MAX_KEY_SIZE);
        &self.key_data[..len]
    }

    /// Get the key as a hex string
    pub fn key_hex(&self) -> String {
        hex::encode(self.key())
    }

    /// Returns true if the operation succeeded (return code 0)
    pub fn is_success(&self) -> bool {
        self.return_code == 0
    }

    /// Returns true if this was a NOTFOUND result (-30798 is MDBX_NOTFOUND)
    pub fn is_not_found(&self) -> bool {
        self.return_code == -30798
    }

    /// Returns true if this is a direct get (mdbx_get) rather than a cursor operation
    pub fn is_direct_get(&self) -> bool {
        self.event_type == 5
    }

    /// Get the latency in microseconds
    pub fn latency_us(&self) -> f64 {
        self.latency_ns as f64 / 1000.0
    }

    /// Format the event similar to the issue 14558 log format
    /// e.g., "HashedAccounts: DbCursorRO::seek( 0x0cc6... )"
    pub fn format_log(&self, table_name: &str) -> String {
        let op_name = match self.cursor_op() {
            CursorOp::SetRange => "seek",
            CursorOp::Set => "seek_exact",
            CursorOp::SetKey => "seek_key",
            CursorOp::Next => "next",
            CursorOp::NextDup => "next_dup",
            CursorOp::NextNoDup => "next_no_dup",
            CursorOp::Prev => "prev",
            CursorOp::First => "first",
            CursorOp::Last => "last",
            CursorOp::GetCurrent => "current",
            _ => self.cursor_op().name(),
        };

        let key_hex = if self.key_size > 0 {
            format!("0x{}", self.key_hex())
        } else {
            String::new()
        };

        format!("{}: DbCursorRO::{}( {} )", table_name, op_name, key_hex)
    }
}

/// Statistics for a trace session
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TraceStats {
    pub total_events: u64,
    pub page_faults: u64,
    pub major_faults: u64,
    pub mmap_events: u64,
    pub sequential_accesses: u64,
    pub random_accesses: u64,
    pub unique_pages: u64,
    pub duration_ns: u64,
    // Cursor operation stats
    pub cursor_ops: u64,
    pub cursor_seeks: u64,
    pub cursor_nexts: u64,
    pub cursor_errors: u64,
    // Direct get stats (mdbx_get calls, not cursor-based)
    pub direct_gets: u64,
}

impl TraceStats {
    pub fn fault_rate_per_sec(&self) -> f64 {
        if self.duration_ns == 0 {
            return 0.0;
        }
        self.page_faults as f64 / (self.duration_ns as f64 / 1_000_000_000.0)
    }

    pub fn sequential_ratio(&self) -> f64 {
        let total = self.sequential_accesses + self.random_accesses;
        if total == 0 {
            return 0.0;
        }
        self.sequential_accesses as f64 / total as f64
    }

    pub fn cursor_op_rate_per_sec(&self) -> f64 {
        if self.duration_ns == 0 {
            return 0.0;
        }
        self.cursor_ops as f64 / (self.duration_ns as f64 / 1_000_000_000.0)
    }

    pub fn seek_ratio(&self) -> f64 {
        if self.cursor_ops == 0 {
            return 0.0;
        }
        self.cursor_seeks as f64 / self.cursor_ops as f64
    }
}
