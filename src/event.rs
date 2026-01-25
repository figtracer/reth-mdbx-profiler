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

/// Maximum key prefix size for active operation tracking (must match BPF ACTIVE_OP_KEY_PREFIX_SIZE)
pub const ACTIVE_OP_KEY_PREFIX_SIZE: usize = 16;

/// Maximum stack depth for slow operation traces (must match BPF MAX_STACK_DEPTH)
pub const MAX_STACK_DEPTH: usize = 32;

/// Default slow operation threshold in nanoseconds (1ms)
pub const DEFAULT_SLOW_OP_THRESHOLD_NS: u64 = 1_000_000;

/// Sentinel value indicating no active MDBX operation on this thread
/// (must match BPF NO_ACTIVE_OP_DBI)
pub const NO_ACTIVE_OP_DBI: u32 = 0xFFFFFFFF;

/// MDBX page types detected from page headers
/// (must match BPF PAGE_TYPE_* defines)
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MdbxPageType {
    /// Page type could not be determined
    #[default]
    Unknown = 0,
    /// Meta page (database metadata at page 0 and 1)
    Meta = 1,
    /// Branch page (internal B+ tree node for tree traversal)
    Branch = 2,
    /// Leaf page (contains actual key/value data)
    Leaf = 3,
    /// Overflow page (for large values that don't fit in leaf)
    Overflow = 4,
}

impl MdbxPageType {
    /// Convert from raw u8 value
    pub fn from_raw(value: u8) -> Self {
        match value {
            1 => Self::Meta,
            2 => Self::Branch,
            3 => Self::Leaf,
            4 => Self::Overflow,
            _ => Self::Unknown,
        }
    }

    /// Returns the display name of this page type
    pub fn name(&self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Meta => "Meta",
            Self::Branch => "Branch",
            Self::Leaf => "Leaf",
            Self::Overflow => "Overflow",
        }
    }

    /// Returns true if this is an internal tree traversal page
    pub fn is_traversal(&self) -> bool {
        matches!(self, Self::Branch | Self::Meta)
    }

    /// Returns true if this is a data-carrying page
    pub fn is_data(&self) -> bool {
        matches!(self, Self::Leaf | Self::Overflow)
    }
}

/// Event types matching the BPF definitions
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventType {
    PageFault = 1,
    Mmap = 2,
    CursorGet = 3,
    CursorPut = 4,
    DirectGet = 5,
    CursorDel = 6,
    TxnBegin = 7,
    TxnCommit = 8,
    TxnAbort = 9,
    DirectPut = 10,
    DirectDel = 11,
    CursorOpen = 12,
    CursorClose = 13,
    SlowOpStack = 14,
}

/// Write flags for cursor put operations (from libmdbx)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteFlags(pub u32);

impl WriteFlags {
    pub const UPSERT: u32 = 0x00000;
    pub const NO_OVERWRITE: u32 = 0x00010;
    pub const NO_DUP_DATA: u32 = 0x00020;
    pub const CURRENT: u32 = 0x00040;
    pub const APPEND: u32 = 0x20000;
    pub const APPEND_DUP: u32 = 0x40000;

    pub fn is_upsert(&self) -> bool {
        self.0 == Self::UPSERT
    }

    pub fn is_append(&self) -> bool {
        (self.0 & Self::APPEND) != 0
    }

    pub fn is_append_dup(&self) -> bool {
        (self.0 & Self::APPEND_DUP) != 0
    }

    pub fn is_no_overwrite(&self) -> bool {
        (self.0 & Self::NO_OVERWRITE) != 0
    }

    pub fn name(&self) -> &'static str {
        if self.0 == Self::UPSERT {
            "UPSERT"
        } else if (self.0 & Self::APPEND_DUP) != 0 {
            "APPEND_DUP"
        } else if (self.0 & Self::APPEND) != 0 {
            "APPEND"
        } else if (self.0 & Self::NO_OVERWRITE) != 0 {
            "NO_OVERWRITE"
        } else if (self.0 & Self::CURRENT) != 0 {
            "CURRENT"
        } else if (self.0 & Self::NO_DUP_DATA) != 0 {
            "NO_DUP_DATA"
        } else {
            "UNKNOWN"
        }
    }
}

impl std::fmt::Display for WriteFlags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Transaction flags (from libmdbx)
/// See: https://github.com/erthink/libmdbx/blob/master/mdbx.h
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxnFlags(pub u32);

impl TxnFlags {
    /// MDBX_TXN_READWRITE = 0 (default, read-write transaction)
    pub const READWRITE: u32 = 0;
    /// MDBX_TXN_RDONLY = 0x20000 (read-only transaction)
    pub const RDONLY: u32 = 0x20000;

    pub fn is_read_only(&self) -> bool {
        (self.0 & Self::RDONLY) != 0
    }

    pub fn is_read_write(&self) -> bool {
        !self.is_read_only()
    }

    pub fn name(&self) -> &'static str {
        if self.is_read_only() {
            "RO"
        } else {
            "RW"
        }
    }
}

impl std::fmt::Display for TxnFlags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
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
///
/// Layout on x86_64 Linux (96 bytes total):
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
///   - page_type: offset 65, size 1
///   - _pad1: offset 66, size 2 (alignment)
///   - active_dbi: offset 68, size 4
///   - active_op_type: offset 72, size 4
///   - active_cursor_op: offset 76, size 4
///   - active_key_prefix: offset 80, size 16
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
    /// MDBX page type (Branch, Leaf, Overflow, Meta, or Unknown)
    pub page_type: u8,
    /// Padding for alignment
    pub _pad1: [u8; 2],
    /// DBI of active MDBX operation (NO_ACTIVE_OP_DBI if none)
    pub active_dbi: u32,
    /// Operation type of active operation (EVENT_CURSOR_GET, etc.)
    pub active_op_type: u32,
    /// Cursor operation (SET_RANGE, NEXT, etc.) for get operations
    pub active_cursor_op: u32,
    /// First 16 bytes of key from active operation
    pub active_key_prefix: [u8; ACTIVE_OP_KEY_PREFIX_SIZE],
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

    /// Returns true if this page fault has an associated active MDBX operation.
    /// This means we know exactly which operation caused this fault.
    pub fn has_active_op(&self) -> bool {
        self.active_dbi != NO_ACTIVE_OP_DBI
    }

    /// Returns the active operation type as an EventType enum, if available
    pub fn active_op_event_type(&self) -> Option<EventType> {
        if !self.has_active_op() {
            return None;
        }
        match self.active_op_type {
            3 => Some(EventType::CursorGet),
            4 => Some(EventType::CursorPut),
            5 => Some(EventType::DirectGet),
            6 => Some(EventType::CursorDel),
            10 => Some(EventType::DirectPut),
            11 => Some(EventType::DirectDel),
            _ => None,
        }
    }

    /// Returns the active cursor operation as a CursorOp enum, if applicable
    pub fn active_cursor_op(&self) -> Option<CursorOp> {
        if !self.has_active_op() || self.active_op_type != 3 {
            // Only cursor_get has a meaningful cursor_op
            return None;
        }
        Some(CursorOp::from_raw(self.active_cursor_op))
    }

    /// Returns the active key prefix as a hex string
    pub fn active_key_prefix_hex(&self) -> String {
        if !self.has_active_op() {
            return String::new();
        }
        hex::encode(&self.active_key_prefix)
    }

    /// Returns the table name for the active operation, if available
    pub fn active_table_name(&self) -> Option<&'static str> {
        if !self.has_active_op() {
            return None;
        }
        Some(dbi_to_table_name(self.active_dbi))
    }

    /// Returns the MDBX page type that was faulted
    pub fn page_type(&self) -> MdbxPageType {
        MdbxPageType::from_raw(self.page_type)
    }
}

/// MDBX cursor operation event from BPF
///
/// This struct must match the layout of cursor_event in mdbx_tracer.bpf.c exactly.
/// Layout on x86_64 Linux (152 bytes total):
///   - timestamp_ns: offset 0, size 8
///   - pid: offset 8, size 4
///   - tid: offset 12, size 4
///   - event_type: offset 16, size 4
///   - cursor_op: offset 20, size 4
///   - dbi: offset 24, size 4
///   - key_size: offset 28, size 4
///   - key_data: offset 32, size 64
///   - return_code: offset 96, size 4
///   - value_size: offset 100, size 4
///   - latency_ns: offset 104, size 8
///   - write_flags: offset 112, size 4
///   - faults_during_op: offset 116, size 4
///   - major_faults_during_op: offset 120, size 4
///   - branch_faults: offset 124, size 4
///   - leaf_faults: offset 128, size 4
///   - overflow_faults: offset 132, size 4
///   - max_tree_depth: offset 136, size 4
///   - fault_latency_ns: offset 140, size 8 (aligned to 8)
#[repr(C)]
#[derive(Debug, Clone)]
pub struct CursorEvent {
    /// Kernel timestamp in nanoseconds
    pub timestamp_ns: u64,
    /// Process ID
    pub pid: u32,
    /// Thread ID
    pub tid: u32,
    /// Event type (EVENT_CURSOR_GET=3, EVENT_CURSOR_PUT=4, EVENT_DIRECT_GET=5, EVENT_CURSOR_DEL=6)
    pub event_type: u32,
    /// MDBX cursor operation (SET_RANGE, NEXT, etc.) - for get operations
    pub cursor_op: u32,
    /// Database index (table identifier in MDBX)
    pub dbi: u32,
    /// Size of the key
    pub key_size: u32,
    /// First MAX_KEY_SIZE bytes of the key
    pub key_data: [u8; MAX_KEY_SIZE],
    /// Return code from the operation
    pub return_code: i32,
    /// Size of value (for put operations)
    pub value_size: u32,
    /// Time spent in the operation (nanoseconds)
    pub latency_ns: u64,
    /// Write flags (for put/del operations): UPSERT, APPEND, etc.
    pub write_flags: u32,
    /// Total page faults that occurred during this operation
    pub faults_during_op: u32,
    /// Major faults (disk I/O required) during this operation
    pub major_faults_during_op: u32,
    /// Faults on branch pages (B+ tree traversal) during this operation
    pub branch_faults: u32,
    /// Faults on leaf pages (actual data access) during this operation
    pub leaf_faults: u32,
    /// Faults on overflow pages (large values) during this operation
    pub overflow_faults: u32,
    /// Maximum B+ tree depth observed (consecutive branch pages before leaf)
    pub max_tree_depth: u32,
    /// Cumulative time spent in fault handlers during this operation (nanoseconds)
    pub fault_latency_ns: u64,
    /// Cursor pointer (for linking ops to cursor lifecycle, 0 for direct ops)
    pub cursor_ptr: u64,
}

// Custom Serialize implementation for CursorEvent since [u8; 64] doesn't impl Serialize
impl Serialize for CursorEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("CursorEvent", 20)?;
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
        state.serialize_field("value_size", &self.value_size)?;
        state.serialize_field("latency_ns", &self.latency_ns)?;
        state.serialize_field("write_flags", &self.write_flags)?;
        // Per-operation fault statistics
        state.serialize_field("faults_during_op", &self.faults_during_op)?;
        state.serialize_field("major_faults_during_op", &self.major_faults_during_op)?;
        state.serialize_field("branch_faults", &self.branch_faults)?;
        state.serialize_field("leaf_faults", &self.leaf_faults)?;
        state.serialize_field("overflow_faults", &self.overflow_faults)?;
        // B+ tree depth tracking
        state.serialize_field("max_tree_depth", &self.max_tree_depth)?;
        state.serialize_field("fault_latency_ns", &self.fault_latency_ns)?;
        // Cursor pointer for lifecycle tracking
        state.serialize_field("cursor_ptr", &self.cursor_ptr)?;
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
            #[serde(default)]
            value_size: u32,
            latency_ns: u64,
            #[serde(default)]
            write_flags: u32,
            #[serde(default)]
            faults_during_op: u32,
            #[serde(default)]
            major_faults_during_op: u32,
            #[serde(default)]
            branch_faults: u32,
            #[serde(default)]
            leaf_faults: u32,
            #[serde(default)]
            overflow_faults: u32,
            #[serde(default)]
            max_tree_depth: u32,
            #[serde(default)]
            fault_latency_ns: u64,
            #[serde(default)]
            cursor_ptr: u64,
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
            value_size: helper.value_size,
            latency_ns: helper.latency_ns,
            write_flags: helper.write_flags,
            faults_during_op: helper.faults_during_op,
            major_faults_during_op: helper.major_faults_during_op,
            branch_faults: helper.branch_faults,
            leaf_faults: helper.leaf_faults,
            overflow_faults: helper.overflow_faults,
            max_tree_depth: helper.max_tree_depth,
            fault_latency_ns: helper.fault_latency_ns,
            cursor_ptr: helper.cursor_ptr,
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

    /// Returns true if this is a direct put (mdbx_put) rather than a cursor operation
    pub fn is_direct_put(&self) -> bool {
        self.event_type == 10
    }

    /// Returns true if this is a direct del (mdbx_del) rather than a cursor operation
    pub fn is_direct_del(&self) -> bool {
        self.event_type == 11
    }

    /// Returns true if this is any direct operation (not cursor-based)
    pub fn is_direct_op(&self) -> bool {
        self.event_type == 5 || self.event_type == 10 || self.event_type == 11
    }

    /// Returns true if this is a write operation (put or del)
    pub fn is_write_op(&self) -> bool {
        self.event_type == 4
            || self.event_type == 6
            || self.event_type == 10
            || self.event_type == 11
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

/// MDBX transaction lifecycle event from BPF
///
/// This struct must match the layout of txn_event in mdbx_tracer.bpf.c exactly.
/// Layout on x86_64 Linux (48 bytes total):
///   - timestamp_ns: offset 0, size 8
///   - pid: offset 8, size 4
///   - tid: offset 12, size 4
///   - event_type: offset 16, size 4
///   - txn_flags: offset 20, size 4
///   - txn_ptr: offset 24, size 8
///   - parent_txn_ptr: offset 32, size 8
///   - latency_ns: offset 40, size 8
///   - return_code: offset 48, size 4
///   - _pad: offset 52, size 4
#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TxnEvent {
    /// Kernel timestamp in nanoseconds
    pub timestamp_ns: u64,
    /// Process ID
    pub pid: u32,
    /// Thread ID
    pub tid: u32,
    /// Event type (EVENT_TXN_BEGIN=7, EVENT_TXN_COMMIT=8, EVENT_TXN_ABORT=9)
    pub event_type: u32,
    /// Transaction flags (0=RW, 1=RO, see TxnFlags)
    pub txn_flags: u32,
    /// Transaction pointer (for correlation between begin/commit/abort)
    pub txn_ptr: u64,
    /// Parent transaction pointer (0 if not nested)
    pub parent_txn_ptr: u64,
    /// Time spent in operation (for commit: total commit latency)
    pub latency_ns: u64,
    /// Return code from the operation
    pub return_code: i32,
    /// Padding for alignment
    pub _pad: u32,
}

impl TxnEvent {
    /// Get the transaction flags as a TxnFlags wrapper
    pub fn flags(&self) -> TxnFlags {
        TxnFlags(self.txn_flags)
    }

    /// Returns true if this is a read-only transaction
    pub fn is_read_only(&self) -> bool {
        self.flags().is_read_only()
    }

    /// Returns true if this is a read-write transaction
    pub fn is_read_write(&self) -> bool {
        self.flags().is_read_write()
    }

    /// Returns true if this is a nested transaction
    pub fn is_nested(&self) -> bool {
        self.parent_txn_ptr != 0
    }

    /// Returns true if the operation succeeded (return code 0)
    pub fn is_success(&self) -> bool {
        self.return_code == 0
    }

    /// Get the event type as an EventType enum
    pub fn event_type(&self) -> Option<EventType> {
        match self.event_type {
            7 => Some(EventType::TxnBegin),
            8 => Some(EventType::TxnCommit),
            9 => Some(EventType::TxnAbort),
            _ => None,
        }
    }

    /// Get the latency in microseconds
    pub fn latency_us(&self) -> f64 {
        self.latency_ns as f64 / 1000.0
    }

    /// Get the latency in milliseconds
    pub fn latency_ms(&self) -> f64 {
        self.latency_ns as f64 / 1_000_000.0
    }

    /// Get a human-readable description of the event
    pub fn description(&self) -> String {
        let event_name = match self.event_type {
            7 => "TXN_BEGIN",
            8 => "TXN_COMMIT",
            9 => "TXN_ABORT",
            _ => "TXN_UNKNOWN",
        };
        let flags_str = self.flags().name();
        format!(
            "{} {} txn=0x{:x} tid={}",
            event_name, flags_str, self.txn_ptr, self.tid
        )
    }
}

/// MDBX cursor lifecycle event from BPF
///
/// This struct must match the layout of cursor_lifecycle_event in mdbx_tracer.bpf.c exactly.
/// Layout on x86_64 Linux (40 bytes total):
///   - timestamp_ns: offset 0, size 8
///   - pid: offset 8, size 4
///   - tid: offset 12, size 4
///   - event_type: offset 16, size 4
///   - dbi: offset 20, size 4
///   - cursor_ptr: offset 24, size 8
///   - return_code: offset 32, size 4
///   - _pad: offset 36, size 4
#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CursorLifecycleEvent {
    /// Kernel timestamp in nanoseconds
    pub timestamp_ns: u64,
    /// Process ID
    pub pid: u32,
    /// Thread ID
    pub tid: u32,
    /// Event type (EVENT_CURSOR_OPEN=12, EVENT_CURSOR_CLOSE=13)
    pub event_type: u32,
    /// Database index (table identifier)
    pub dbi: u32,
    /// Cursor pointer (unique identifier for this cursor instance)
    pub cursor_ptr: u64,
    /// Return code (for open: 0 = success)
    pub return_code: i32,
    /// Padding for alignment
    pub _pad: u32,
}

impl CursorLifecycleEvent {
    /// Returns true if this is a cursor open event
    pub fn is_open(&self) -> bool {
        self.event_type == 12
    }

    /// Returns true if this is a cursor close event
    pub fn is_close(&self) -> bool {
        self.event_type == 13
    }

    /// Returns true if the cursor open succeeded
    pub fn is_success(&self) -> bool {
        self.return_code == 0
    }

    /// Get the table name for this cursor
    pub fn table_name(&self) -> &'static str {
        dbi_to_table_name(self.dbi)
    }

    /// Get the event type as an EventType enum
    pub fn event_type_enum(&self) -> Option<EventType> {
        match self.event_type {
            12 => Some(EventType::CursorOpen),
            13 => Some(EventType::CursorClose),
            _ => None,
        }
    }

    /// Get a human-readable description of the event
    pub fn description(&self) -> String {
        let event_name = if self.is_open() {
            "CURSOR_OPEN"
        } else if self.is_close() {
            "CURSOR_CLOSE"
        } else {
            "CURSOR_UNKNOWN"
        };
        format!(
            "{} table={} cursor=0x{:x} tid={}",
            event_name,
            self.table_name(),
            self.cursor_ptr,
            self.tid
        )
    }
}

/// Slow operation with stack trace event from BPF
///
/// This struct captures slow database operations along with a user-space stack trace
/// for call site attribution. This allows distinguishing critical path operations
/// from background work (e.g., prefetch vs state updates in reth).
///
/// Layout on x86_64 Linux (320 bytes total):
///   - timestamp_ns: offset 0, size 8
///   - pid: offset 8, size 4
///   - tid: offset 12, size 4
///   - event_type: offset 16, size 4
///   - op_event_type: offset 20, size 4
///   - cursor_op: offset 24, size 4
///   - dbi: offset 28, size 4
///   - latency_ns: offset 32, size 8
///   - faults_during_op: offset 40, size 4
///   - major_faults: offset 44, size 4
///   - branch_faults: offset 48, size 4
///   - leaf_faults: offset 52, size 4
///   - max_tree_depth: offset 56, size 4
///   - stack_depth: offset 60, size 4
///   - stack: offset 64, size 256 (32 * 8)
///   - key_prefix: offset 320, size 16
#[repr(C)]
#[derive(Debug, Clone)]
pub struct SlowOpStackEvent {
    /// Kernel timestamp in nanoseconds
    pub timestamp_ns: u64,
    /// Process ID
    pub pid: u32,
    /// Thread ID
    pub tid: u32,
    /// Event type (EVENT_SLOW_OP_STACK=14)
    pub event_type: u32,
    /// Original operation type (CURSOR_GET, DIRECT_GET, etc.)
    pub op_event_type: u32,
    /// Cursor operation (for CURSOR_GET: SET_RANGE, NEXT, etc.)
    pub cursor_op: u32,
    /// Database index (table identifier)
    pub dbi: u32,
    /// Operation latency in nanoseconds
    pub latency_ns: u64,
    /// Total faults during this operation
    pub faults_during_op: u32,
    /// Major faults during this operation
    pub major_faults: u32,
    /// Branch page faults (B+ tree traversal)
    pub branch_faults: u32,
    /// Leaf page faults (actual data access)
    pub leaf_faults: u32,
    /// Maximum B+ tree depth observed
    pub max_tree_depth: u32,
    /// Number of valid stack frames
    pub stack_depth: u32,
    /// User-space instruction pointers (need symbol resolution)
    pub stack: [u64; MAX_STACK_DEPTH],
    /// First 16 bytes of key for identification
    pub key_prefix: [u8; ACTIVE_OP_KEY_PREFIX_SIZE],
}

impl Serialize for SlowOpStackEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("SlowOpStackEvent", 15)?;
        state.serialize_field("timestamp_ns", &self.timestamp_ns)?;
        state.serialize_field("pid", &self.pid)?;
        state.serialize_field("tid", &self.tid)?;
        state.serialize_field("event_type", &self.event_type)?;
        state.serialize_field("op_event_type", &self.op_event_type)?;
        state.serialize_field("cursor_op", &self.cursor_op)?;
        state.serialize_field("dbi", &self.dbi)?;
        state.serialize_field("latency_ns", &self.latency_ns)?;
        state.serialize_field("faults_during_op", &self.faults_during_op)?;
        state.serialize_field("major_faults", &self.major_faults)?;
        state.serialize_field("branch_faults", &self.branch_faults)?;
        state.serialize_field("leaf_faults", &self.leaf_faults)?;
        state.serialize_field("max_tree_depth", &self.max_tree_depth)?;
        state.serialize_field("stack_depth", &self.stack_depth)?;
        // Serialize only valid stack frames as hex addresses
        let valid_stack: Vec<String> = self.stack[..self.stack_depth as usize]
            .iter()
            .map(|addr| format!("0x{:x}", addr))
            .collect();
        state.serialize_field("stack", &valid_stack)?;
        state.serialize_field("key_prefix", &hex::encode(&self.key_prefix))?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for SlowOpStackEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper {
            timestamp_ns: u64,
            pid: u32,
            tid: u32,
            event_type: u32,
            op_event_type: u32,
            cursor_op: u32,
            dbi: u32,
            latency_ns: u64,
            faults_during_op: u32,
            major_faults: u32,
            branch_faults: u32,
            leaf_faults: u32,
            max_tree_depth: u32,
            stack_depth: u32,
            stack: Vec<String>,
            key_prefix: String,
        }

        let helper = Helper::deserialize(deserializer)?;

        let mut stack = [0u64; MAX_STACK_DEPTH];
        for (i, addr_str) in helper.stack.iter().enumerate() {
            if i >= MAX_STACK_DEPTH {
                break;
            }
            let addr_str = addr_str.trim_start_matches("0x");
            if let Ok(addr) = u64::from_str_radix(addr_str, 16) {
                stack[i] = addr;
            }
        }

        let mut key_prefix = [0u8; ACTIVE_OP_KEY_PREFIX_SIZE];
        if let Ok(bytes) = hex::decode(&helper.key_prefix) {
            let len = bytes.len().min(ACTIVE_OP_KEY_PREFIX_SIZE);
            key_prefix[..len].copy_from_slice(&bytes[..len]);
        }

        Ok(SlowOpStackEvent {
            timestamp_ns: helper.timestamp_ns,
            pid: helper.pid,
            tid: helper.tid,
            event_type: helper.event_type,
            op_event_type: helper.op_event_type,
            cursor_op: helper.cursor_op,
            dbi: helper.dbi,
            latency_ns: helper.latency_ns,
            faults_during_op: helper.faults_during_op,
            major_faults: helper.major_faults,
            branch_faults: helper.branch_faults,
            leaf_faults: helper.leaf_faults,
            max_tree_depth: helper.max_tree_depth,
            stack_depth: helper.stack_depth,
            stack,
            key_prefix,
        })
    }
}

impl SlowOpStackEvent {
    /// Get the operation type as an EventType enum
    pub fn op_event_type(&self) -> Option<EventType> {
        match self.op_event_type {
            3 => Some(EventType::CursorGet),
            4 => Some(EventType::CursorPut),
            5 => Some(EventType::DirectGet),
            6 => Some(EventType::CursorDel),
            10 => Some(EventType::DirectPut),
            11 => Some(EventType::DirectDel),
            _ => None,
        }
    }

    /// Get the cursor operation as a CursorOp enum (for CursorGet)
    pub fn cursor_op(&self) -> CursorOp {
        CursorOp::from_raw(self.cursor_op)
    }

    /// Get the table name
    pub fn table_name(&self) -> &'static str {
        dbi_to_table_name(self.dbi)
    }

    /// Get latency in microseconds
    pub fn latency_us(&self) -> f64 {
        self.latency_ns as f64 / 1000.0
    }

    /// Get latency in milliseconds
    pub fn latency_ms(&self) -> f64 {
        self.latency_ns as f64 / 1_000_000.0
    }

    /// Get the key prefix as a hex string
    pub fn key_prefix_hex(&self) -> String {
        hex::encode(&self.key_prefix)
    }

    /// Get the valid stack frames (up to stack_depth)
    pub fn stack_frames(&self) -> &[u64] {
        &self.stack[..self.stack_depth as usize]
    }

    /// Returns true if this is a seek operation (likely on critical path)
    pub fn is_seek(&self) -> bool {
        if self.op_event_type == 3 {
            // CursorGet
            self.cursor_op().is_seek()
        } else {
            false
        }
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
    // Cursor read operation stats
    pub cursor_ops: u64,
    pub cursor_seeks: u64,
    pub cursor_nexts: u64,
    pub cursor_errors: u64,
    // Direct get stats (mdbx_get calls, not cursor-based)
    pub direct_gets: u64,
    // Cursor write operation stats
    pub cursor_puts: u64,
    pub cursor_dels: u64,
    // Transaction lifecycle stats
    pub txn_begins: u64,
    pub txn_commits: u64,
    pub txn_aborts: u64,
    pub txn_ro_count: u64,
    pub txn_rw_count: u64,
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
