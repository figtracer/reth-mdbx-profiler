//! MDBX metadata parsing for table-level page attribution
//!
//! MDBX stores its database as a B+ tree with the following structure:
//! - Page 0-1: Meta pages (alternating for atomic updates)
//! - Page 2: Free list (garbage collector)
//! - Pages 3+: B+ tree nodes and data
//!
//! Each table (DBI - Database Index) has its own B+ tree with a root page.
//! We can map page ranges to tables by reading the meta pages and traversing roots.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Known Reth MDBX tables
/// These correspond to the tables defined in reth's db-api crate
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RethTable {
    // Block-related tables
    CanonicalHeaders,
    HeaderTerminalDifficulties,
    HeaderNumbers,
    Headers,
    BlockBodyIndices,
    BlockOmmers,
    BlockWithdrawals,

    // Transaction tables
    Transactions,
    TransactionHashNumbers,
    TransactionBlocks,
    Receipts,
    TransactionSenders,

    // State tables
    PlainAccountState,
    PlainStorageState,
    Bytecodes,

    // History tables
    AccountsHistory,
    StoragesHistory,
    AccountChangeSets,
    StorageChangeSets,

    // Hashed state (for merkle computation)
    HashedAccounts,
    HashedStorages,

    // Trie tables (the hot ones for state root computation)
    AccountsTrie,
    StoragesTrie,
    AccountsTrieChangeSets,
    StoragesTrieChangeSets,

    // Checkpoint/metadata tables
    StageCheckpoints,
    StageCheckpointProgresses,
    PruneCheckpoints,
    VersionHistory,
    ChainState,
    Metadata,

    // Internal MDBX tables
    MdbxFreeList,
    MdbxMainDb,

    // Unknown table
    Unknown(u32),
}

impl RethTable {
    /// Get table name as string
    pub fn name(&self) -> &'static str {
        match self {
            Self::CanonicalHeaders => "CanonicalHeaders",
            Self::HeaderTerminalDifficulties => "HeaderTerminalDifficulties",
            Self::HeaderNumbers => "HeaderNumbers",
            Self::Headers => "Headers",
            Self::BlockBodyIndices => "BlockBodyIndices",
            Self::BlockOmmers => "BlockOmmers",
            Self::BlockWithdrawals => "BlockWithdrawals",
            Self::Transactions => "Transactions",
            Self::TransactionHashNumbers => "TransactionHashNumbers",
            Self::TransactionBlocks => "TransactionBlocks",
            Self::Receipts => "Receipts",
            Self::TransactionSenders => "TransactionSenders",
            Self::PlainAccountState => "PlainAccountState",
            Self::PlainStorageState => "PlainStorageState",
            Self::Bytecodes => "Bytecodes",
            Self::AccountsHistory => "AccountsHistory",
            Self::StoragesHistory => "StoragesHistory",
            Self::AccountChangeSets => "AccountChangeSets",
            Self::StorageChangeSets => "StorageChangeSets",
            Self::HashedAccounts => "HashedAccounts",
            Self::HashedStorages => "HashedStorages",
            Self::AccountsTrie => "AccountsTrie",
            Self::StoragesTrie => "StoragesTrie",
            Self::AccountsTrieChangeSets => "AccountsTrieChangeSets",
            Self::StoragesTrieChangeSets => "StoragesTrieChangeSets",
            Self::StageCheckpoints => "StageCheckpoints",
            Self::StageCheckpointProgresses => "StageCheckpointProgresses",
            Self::PruneCheckpoints => "PruneCheckpoints",
            Self::VersionHistory => "VersionHistory",
            Self::ChainState => "ChainState",
            Self::Metadata => "Metadata",
            Self::MdbxFreeList => "MdbxFreeList",
            Self::MdbxMainDb => "MdbxMainDb",
            Self::Unknown(_) => "Unknown",
        }
    }

    /// Get category for grouping in visualization
    pub fn category(&self) -> &'static str {
        match self {
            Self::CanonicalHeaders
            | Self::HeaderTerminalDifficulties
            | Self::HeaderNumbers
            | Self::Headers
            | Self::BlockBodyIndices
            | Self::BlockOmmers
            | Self::BlockWithdrawals => "Blocks",

            Self::Transactions
            | Self::TransactionHashNumbers
            | Self::TransactionBlocks
            | Self::Receipts
            | Self::TransactionSenders => "Transactions",

            Self::PlainAccountState | Self::PlainStorageState | Self::Bytecodes => "State",

            Self::AccountsHistory
            | Self::StoragesHistory
            | Self::AccountChangeSets
            | Self::StorageChangeSets => "History",

            Self::HashedAccounts | Self::HashedStorages => "HashedState",

            Self::AccountsTrie
            | Self::StoragesTrie
            | Self::AccountsTrieChangeSets
            | Self::StoragesTrieChangeSets => "Trie",

            Self::StageCheckpoints
            | Self::StageCheckpointProgresses
            | Self::PruneCheckpoints
            | Self::VersionHistory
            | Self::ChainState
            | Self::Metadata => "Metadata",

            Self::MdbxFreeList | Self::MdbxMainDb => "MdbxInternal",

            Self::Unknown(_) => "Unknown",
        }
    }

    /// Parse table name from string
    pub fn from_name(name: &str) -> Self {
        match name {
            "CanonicalHeaders" => Self::CanonicalHeaders,
            "HeaderTerminalDifficulties" => Self::HeaderTerminalDifficulties,
            "HeaderNumbers" => Self::HeaderNumbers,
            "Headers" => Self::Headers,
            "BlockBodyIndices" => Self::BlockBodyIndices,
            "BlockOmmers" => Self::BlockOmmers,
            "BlockWithdrawals" => Self::BlockWithdrawals,
            "Transactions" => Self::Transactions,
            "TransactionHashNumbers" => Self::TransactionHashNumbers,
            "TransactionBlocks" => Self::TransactionBlocks,
            "Receipts" => Self::Receipts,
            "TransactionSenders" => Self::TransactionSenders,
            "PlainAccountState" => Self::PlainAccountState,
            "PlainStorageState" => Self::PlainStorageState,
            "Bytecodes" => Self::Bytecodes,
            "AccountsHistory" => Self::AccountsHistory,
            "StoragesHistory" => Self::StoragesHistory,
            "AccountChangeSets" => Self::AccountChangeSets,
            "StorageChangeSets" => Self::StorageChangeSets,
            "HashedAccounts" => Self::HashedAccounts,
            "HashedStorages" => Self::HashedStorages,
            "AccountsTrie" => Self::AccountsTrie,
            "StoragesTrie" => Self::StoragesTrie,
            "AccountsTrieChangeSets" => Self::AccountsTrieChangeSets,
            "StoragesTrieChangeSets" => Self::StoragesTrieChangeSets,
            "StageCheckpoints" => Self::StageCheckpoints,
            "StageCheckpointProgresses" => Self::StageCheckpointProgresses,
            "PruneCheckpoints" => Self::PruneCheckpoints,
            "VersionHistory" => Self::VersionHistory,
            "ChainState" => Self::ChainState,
            "Metadata" => Self::Metadata,
            _ => Self::Unknown(0),
        }
    }
}

impl std::fmt::Display for RethTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(id) => write!(f, "Unknown({})", id),
            _ => write!(f, "{}", self.name()),
        }
    }
}

impl serde::Serialize for RethTable {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for RethTable {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s.starts_with("Unknown(") && s.ends_with(')') {
            let id_str = &s[8..s.len() - 1];
            if let Ok(id) = id_str.parse::<u32>() {
                return Ok(Self::Unknown(id));
            }
        }
        Ok(Self::from_name(&s))
    }
}

/// Information about an MDBX table's storage
#[derive(Debug, Clone)]
pub struct TableInfo {
    /// Table identifier
    pub table: RethTable,
    /// DBI index in MDBX
    pub dbi: u32,
    /// Root page number
    pub root_page: u64,
    /// Depth of the B+ tree
    pub depth: u32,
    /// Number of branch pages
    pub branch_pages: u64,
    /// Number of leaf pages
    pub leaf_pages: u64,
    /// Number of overflow pages
    pub overflow_pages: u64,
    /// Total entries in the table
    pub entries: u64,
}

/// Page attribution - maps page numbers to tables
#[derive(Debug, Clone, Default)]
pub struct PageAttribution {
    /// Map from page number to table
    page_to_table: HashMap<u64, RethTable>,
    /// Map from table to list of page ranges (start, end)
    table_pages: HashMap<RethTable, Vec<(u64, u64)>>,
    /// Table statistics
    table_info: HashMap<RethTable, TableInfo>,
    /// Page size
    page_size: u32,
}

impl PageAttribution {
    /// Create a new empty page attribution
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the table for a given page number
    pub fn get_table(&self, page_num: u64) -> Option<RethTable> {
        // First check direct mapping
        if let Some(table) = self.page_to_table.get(&page_num) {
            return Some(*table);
        }

        // Check ranges
        for (table, ranges) in &self.table_pages {
            for (start, end) in ranges {
                if page_num >= *start && page_num < *end {
                    return Some(*table);
                }
            }
        }

        // Use heuristic based on page number
        Some(self.heuristic_table_guess(page_num))
    }

    /// Get the table for a given file offset
    pub fn get_table_for_offset(&self, offset: u64) -> Option<RethTable> {
        let page_num = offset / self.page_size as u64;
        self.get_table(page_num)
    }

    /// Heuristic guess for table based on page position
    /// This is used when we don't have exact metadata
    fn heuristic_table_guess(&self, page_num: u64) -> RethTable {
        // MDBX layout heuristics based on typical Reth database structure:
        // - Pages 0-1: Meta pages
        // - Page 2: Free list
        // - Pages 3-100: Main DB and small tables
        // - The rest: Data tables, with trie tables often accessed during state root

        if page_num <= 1 {
            RethTable::MdbxMainDb
        } else if page_num == 2 {
            RethTable::MdbxFreeList
        } else if page_num < 100 {
            // Small pages are often metadata tables
            RethTable::StageCheckpoints
        } else {
            // Default to unknown - will be refined with actual metadata
            RethTable::Unknown(0)
        }
    }

    /// Register a page range for a table
    pub fn add_page_range(&mut self, table: RethTable, start: u64, end: u64) {
        self.table_pages
            .entry(table)
            .or_default()
            .push((start, end));
    }

    /// Register a single page for a table
    pub fn add_page(&mut self, table: RethTable, page_num: u64) {
        self.page_to_table.insert(page_num, table);
    }

    /// Add table info
    pub fn add_table_info(&mut self, info: TableInfo) {
        self.table_info.insert(info.table, info);
    }

    /// Get all table info
    pub fn get_table_info(&self) -> &HashMap<RethTable, TableInfo> {
        &self.table_info
    }

    /// Set page size
    pub fn set_page_size(&mut self, size: u32) {
        self.page_size = size;
    }

    /// Get page size
    pub fn page_size(&self) -> u32 {
        if self.page_size == 0 {
            4096
        } else {
            self.page_size
        }
    }
}

/// MDBX meta page structure (simplified)
/// The actual structure is more complex, but we only need key fields
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct MdbxMetaPage {
    magic: u64,
    version: u32,
    flags: u32,
    page_size: u32,
    // ... more fields
}

const MDBX_MAGIC: u64 = 0xBEEFC0DE;
const MDBX_PAGE_SIZE_DEFAULT: u32 = 4096;

/// Read MDBX metadata from database file
pub fn read_mdbx_metadata(path: impl AsRef<Path>) -> std::io::Result<PageAttribution> {
    let mut file = File::open(path.as_ref())?;
    let mut attribution = PageAttribution::new();

    // Read meta page 0
    let mut header = [0u8; 256];
    file.read_exact(&mut header)?;

    // Check magic (at offset 0)
    let magic = u64::from_le_bytes(header[0..8].try_into().unwrap());
    if magic != MDBX_MAGIC {
        // Try alternate magic location or assume defaults
        attribution.set_page_size(MDBX_PAGE_SIZE_DEFAULT);
        return Ok(attribution);
    }

    // Read page size (typically at offset 20 in MDBX header)
    let page_size = u32::from_le_bytes(header[20..24].try_into().unwrap());
    let page_size = if page_size == 0 || page_size > 65536 {
        MDBX_PAGE_SIZE_DEFAULT
    } else {
        page_size
    };
    attribution.set_page_size(page_size);

    // Mark meta pages
    attribution.add_page(RethTable::MdbxMainDb, 0);
    attribution.add_page(RethTable::MdbxMainDb, 1);
    attribution.add_page(RethTable::MdbxFreeList, 2);

    Ok(attribution)
}

/// Try to extract table information by running mdbx_stat or parsing the file
/// This is a best-effort approach since MDBX internal structure is complex
pub fn extract_table_stats(path: impl AsRef<Path>) -> std::io::Result<PageAttribution> {
    // Start with basic metadata
    let mut attribution = read_mdbx_metadata(path.as_ref())?;

    // Try to get file size for estimation
    let metadata = std::fs::metadata(path.as_ref())?;
    let file_size = metadata.len();
    let page_size = attribution.page_size() as u64;
    let total_pages = file_size / page_size;

    // Without running mdbx_stat or parsing the B+ tree structure,
    // we use file offset heuristics based on typical Reth database patterns.
    // In a real implementation, we would:
    // 1. Parse the main DB to find all named databases (tables)
    // 2. Read each table's root page from the main DB
    // 3. Traverse each table's B+ tree to build the page mapping
    //
    // For now, we provide size-based estimates that will be refined
    // during trace analysis based on access patterns.

    // Register all known tables with Unknown page ranges initially
    // The actual attribution will be refined during analysis
    let tables = [
        RethTable::CanonicalHeaders,
        RethTable::HeaderNumbers,
        RethTable::Headers,
        RethTable::BlockBodyIndices,
        RethTable::Transactions,
        RethTable::Receipts,
        RethTable::PlainAccountState,
        RethTable::PlainStorageState,
        RethTable::HashedAccounts,
        RethTable::HashedStorages,
        RethTable::AccountsTrie,
        RethTable::StoragesTrie,
        RethTable::AccountsHistory,
        RethTable::StoragesHistory,
    ];

    for table in tables {
        attribution.add_table_info(TableInfo {
            table,
            dbi: 0,
            root_page: 0,
            depth: 0,
            branch_pages: 0,
            leaf_pages: 0,
            overflow_pages: 0,
            entries: 0,
        });
    }

    // Log estimation
    eprintln!(
        "MDBX file: {} pages ({} bytes), page size: {}",
        total_pages, file_size, page_size
    );

    Ok(attribution)
}

/// Estimate table from file offset using access pattern heuristics
/// This is used when we don't have exact B+ tree metadata
pub fn estimate_table_from_pattern(
    offset: u64,
    page_size: u32,
    _total_pages: u64,
    recent_pattern: Option<&[u64]>,
) -> RethTable {
    let page_num = offset / page_size as u64;

    // Meta pages
    if page_num <= 2 {
        return if page_num <= 1 {
            RethTable::MdbxMainDb
        } else {
            RethTable::MdbxFreeList
        };
    }

    // Look at access pattern for hints
    if let Some(pattern) = recent_pattern {
        // If we see many accesses to nearby pages, likely a table scan
        // If we see sparse accesses, likely trie traversal
        if pattern.len() >= 3 {
            let strides: Vec<i64> = pattern
                .windows(2)
                .map(|w| (w[1] as i64) - (w[0] as i64))
                .collect();

            // Check if strides are mostly sequential (within 4 pages)
            let sequential_count = strides.iter().filter(|s| s.abs() <= 4).count();
            let sequential_ratio = sequential_count as f64 / strides.len() as f64;

            if sequential_ratio > 0.7 {
                // Likely a table scan - could be state, history, or transactions
                return RethTable::PlainAccountState;
            } else if sequential_ratio < 0.3 {
                // Likely trie traversal - random access pattern
                return RethTable::AccountsTrie;
            }
        }
    }

    // Default to unknown
    RethTable::Unknown(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_name_roundtrip() {
        let tables = [
            RethTable::CanonicalHeaders,
            RethTable::AccountsTrie,
            RethTable::PlainStorageState,
            RethTable::Unknown(42),
        ];

        for table in tables {
            let name = table.to_string();
            let parsed = RethTable::from_name(&name);
            // Note: Unknown doesn't roundtrip the ID through from_name
            if !matches!(table, RethTable::Unknown(_)) {
                assert_eq!(table.name(), parsed.name());
            }
        }
    }

    #[test]
    fn test_page_attribution() {
        let mut attr = PageAttribution::new();
        attr.set_page_size(4096);

        attr.add_page(RethTable::AccountsTrie, 100);
        attr.add_page_range(RethTable::PlainAccountState, 1000, 2000);

        assert_eq!(attr.get_table(100), Some(RethTable::AccountsTrie));
        assert_eq!(attr.get_table(1500), Some(RethTable::PlainAccountState));
    }
}
