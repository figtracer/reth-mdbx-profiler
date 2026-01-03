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
    /// Total file size in bytes
    file_size: u64,
    /// Table stats from mdbx_stat (for proportion-based attribution)
    mdbx_stats: Option<Vec<MdbxStatOutput>>,
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
        // - Pages 3+: Data tables

        if page_num <= 1 {
            return RethTable::MdbxMainDb;
        } else if page_num == 2 {
            return RethTable::MdbxFreeList;
        }

        // If we have mdbx_stat data, we can't map exact pages but we know proportions.
        // Return Unknown and let the caller use proportion-based attribution
        RethTable::Unknown(0)
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

    /// Set file size
    pub fn set_file_size(&mut self, size: u64) {
        self.file_size = size;
    }

    /// Get file size
    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    /// Set table stats from mdbx_stat
    pub fn set_table_stats(&mut self, stats: Vec<MdbxStatOutput>) {
        self.mdbx_stats = Some(stats);
    }

    /// Get table stats
    pub fn get_mdbx_stats(&self) -> Option<&Vec<MdbxStatOutput>> {
        self.mdbx_stats.as_ref()
    }

    /// Check if we have real table statistics
    pub fn has_table_stats(&self) -> bool {
        self.mdbx_stats.is_some()
    }

    /// Get table proportions for weighted attribution
    /// Returns (table_name, proportion) pairs sorted by size descending
    pub fn get_table_proportions(&self) -> Vec<(String, f64)> {
        let Some(stats) = &self.mdbx_stats else {
            return Vec::new();
        };

        let total_pages: u64 = stats.iter().map(|s| s.total_pages).sum();
        if total_pages == 0 {
            return Vec::new();
        }

        let mut proportions: Vec<_> = stats
            .iter()
            .filter(|s| s.name != "@main" && s.total_pages > 0)
            .map(|s| (s.name.clone(), s.total_pages as f64 / total_pages as f64))
            .collect();

        proportions.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        proportions
    }
}

/// MDBX page header flags
const P_BRANCH: u16 = 0x01;
const P_LEAF: u16 = 0x02;
const P_OVERFLOW: u16 = 0x04;
const P_META: u16 = 0x08;
const P_DUPDATA: u16 = 0x10;
const P_LEAF2: u16 = 0x20;
const P_SUBP: u16 = 0x40;

const MDBX_PAGE_SIZE_DEFAULT: u32 = 4096;

/// MDBX page header (first 16 bytes of each page)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct PageHeader {
    pgno: u64,     // Page number
    flags: u16,    // Page flags
    num_keys: u16, // Number of keys/entries
    lower: u16,    // Lower bound of free space
    upper: u16,    // Upper bound of free space
}

/// Table info from mdbx_stat output
#[derive(Debug, Clone)]
pub struct MdbxStatOutput {
    pub name: String,
    pub entries: u64,
    pub depth: u32,
    pub branch_pages: u64,
    pub leaf_pages: u64,
    pub overflow_pages: u64,
    pub total_pages: u64,
}

/// Run mdbx_stat to get real table statistics
pub fn run_mdbx_stat(mdbx_path: impl AsRef<Path>) -> Option<Vec<MdbxStatOutput>> {
    let path = mdbx_path.as_ref();

    // Try to find mdbx_stat binary
    let mdbx_stat_paths = [
        "mdbx_stat",
        "/usr/bin/mdbx_stat",
        "/usr/local/bin/mdbx_stat",
        "/opt/homebrew/bin/mdbx_stat",
    ];

    let mut mdbx_stat_bin = None;
    for bin_path in &mdbx_stat_paths {
        if std::process::Command::new(bin_path)
            .arg("-V")
            .output()
            .is_ok()
        {
            mdbx_stat_bin = Some(*bin_path);
            break;
        }
    }

    let bin = mdbx_stat_bin?;

    // Run mdbx_stat -a (all databases) on the mdbx file
    // Note: mdbx_stat outputs info messages to stderr even on success
    let output = std::process::Command::new(bin)
        .arg("-a")
        .arg(path)
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Check if we got useful output (contains "Status of" lines)
    // mdbx_stat may output info to stderr but still produce valid output
    if stdout.contains("Status of") {
        return parse_mdbx_stat_output(&stdout);
    }

    // If no useful output and command failed, report error
    if !output.status.success() || stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Only show error if it's not just info messages
        if !stderr.contains("MADV_DONTNEED") && !stderr.contains("readahead") {
            eprintln!("mdbx_stat failed: {}", stderr);
        }
        return None;
    }

    parse_mdbx_stat_output(&stdout)
}

/// Parse mdbx_stat output to extract table information
fn parse_mdbx_stat_output(output: &str) -> Option<Vec<MdbxStatOutput>> {
    let mut tables = Vec::new();
    let mut current_table: Option<String> = None;
    let mut current_stats = MdbxStatOutput {
        name: String::new(),
        entries: 0,
        depth: 0,
        branch_pages: 0,
        leaf_pages: 0,
        overflow_pages: 0,
        total_pages: 0,
    };

    for line in output.lines() {
        let line = line.trim();

        // Database name line: "Status of Main DB" or "Status of TableName"
        if line.starts_with("Status of ") {
            // Save previous table if exists
            if current_table.is_some() && !current_stats.name.is_empty() {
                current_stats.total_pages = current_stats.branch_pages
                    + current_stats.leaf_pages
                    + current_stats.overflow_pages;
                tables.push(current_stats.clone());
            }

            // Extract table name - handle various formats:
            // "Status of Main DB"
            // "Status of TableName"
            // "Status of database 'TableName'"
            let name_part = &line["Status of ".len()..];
            let table_name = if name_part == "Main DB" {
                "@main".to_string()
            } else if name_part.starts_with("database '") && name_part.ends_with('\'') {
                // Format: database 'TableName'
                name_part["database '".len()..name_part.len() - 1].to_string()
            } else {
                // Format: just TableName
                name_part.to_string()
            };

            current_table = Some(table_name.clone());
            current_stats.name = table_name;

            // Reset stats
            current_stats.entries = 0;
            current_stats.depth = 0;
            current_stats.branch_pages = 0;
            current_stats.leaf_pages = 0;
            current_stats.overflow_pages = 0;
            current_stats.total_pages = 0;
        }

        // Parse stat lines
        if current_table.is_some() {
            if let Some(value) = extract_stat_value(line, "Tree depth:") {
                current_stats.depth = value as u32;
            } else if let Some(value) = extract_stat_value(line, "Branch pages:") {
                current_stats.branch_pages = value;
            } else if let Some(value) = extract_stat_value(line, "Leaf pages:") {
                current_stats.leaf_pages = value;
            } else if let Some(value) = extract_stat_value(line, "Overflow pages:") {
                current_stats.overflow_pages = value;
            } else if let Some(value) = extract_stat_value(line, "Entries:") {
                current_stats.entries = value;
            }
        }
    }

    // Save last table
    if current_table.is_some() && !current_stats.name.is_empty() {
        current_stats.total_pages =
            current_stats.branch_pages + current_stats.leaf_pages + current_stats.overflow_pages;
        tables.push(current_stats);
    }

    // Calculate total pages for all tables
    for table in &mut tables {
        table.total_pages = table.branch_pages + table.leaf_pages + table.overflow_pages;
    }

    if tables.is_empty() {
        None
    } else {
        Some(tables)
    }
}

fn extract_stat_value(line: &str, prefix: &str) -> Option<u64> {
    if line.trim().starts_with(prefix) {
        let value_part = line.trim().strip_prefix(prefix)?.trim();
        // Handle values like "123" or "123 (some note)"
        let value_str = value_part.split_whitespace().next()?;
        value_str.parse().ok()
    } else {
        None
    }
}

/// Read MDBX metadata from database file
pub fn read_mdbx_metadata(path: impl AsRef<Path>) -> std::io::Result<PageAttribution> {
    let mut file = File::open(path.as_ref())?;
    let mut attribution = PageAttribution::new();

    // Read first page (meta page)
    let mut header = [0u8; 4096];
    file.read_exact(&mut header)?;

    // MDBX stores page size at offset 20 in the header
    // Try common page sizes if we can't read it
    let page_size = u32::from_le_bytes(header[20..24].try_into().unwrap());
    let page_size = if page_size >= 256 && page_size <= 65536 && page_size.is_power_of_two() {
        page_size
    } else {
        MDBX_PAGE_SIZE_DEFAULT
    };
    attribution.set_page_size(page_size);

    // Mark meta pages
    attribution.add_page(RethTable::MdbxMainDb, 0);
    attribution.add_page(RethTable::MdbxMainDb, 1);
    attribution.add_page(RethTable::MdbxFreeList, 2);

    Ok(attribution)
}

/// Try to extract table information by running mdbx_stat or parsing the file
pub fn extract_table_stats(path: impl AsRef<Path>) -> std::io::Result<PageAttribution> {
    let path = path.as_ref();

    // Start with basic metadata
    let mut attribution = read_mdbx_metadata(path)?;

    // Try to get file size for estimation
    let metadata = std::fs::metadata(path)?;
    let file_size = metadata.len();
    let page_size = attribution.page_size() as u64;
    let total_pages = file_size / page_size;

    eprintln!(
        "MDBX file: {} pages ({:.2} GB), page size: {}",
        total_pages,
        file_size as f64 / 1024.0 / 1024.0 / 1024.0,
        page_size
    );

    // Try to run mdbx_stat for real table info
    if let Some(stats) = run_mdbx_stat(path) {
        eprintln!("Got table stats from mdbx_stat ({} tables):", stats.len());

        let mut total_table_pages: u64 = 0;

        for stat in &stats {
            let table = RethTable::from_name(&stat.name);

            eprintln!(
                "  {}: {} pages ({} branch, {} leaf, {} overflow), {} entries, depth {}",
                stat.name,
                stat.total_pages,
                stat.branch_pages,
                stat.leaf_pages,
                stat.overflow_pages,
                stat.entries,
                stat.depth
            );

            attribution.add_table_info(TableInfo {
                table,
                dbi: 0,
                root_page: 0, // We don't know root page from mdbx_stat
                depth: stat.depth,
                branch_pages: stat.branch_pages,
                leaf_pages: stat.leaf_pages,
                overflow_pages: stat.overflow_pages,
                entries: stat.entries,
            });

            total_table_pages += stat.total_pages;
        }

        // Store the stats for proportion-based attribution
        attribution.set_table_stats(stats);

        eprintln!(
            "Total table pages: {} ({:.1}% of file)",
            total_table_pages,
            total_table_pages as f64 / total_pages as f64 * 100.0
        );
    } else {
        eprintln!("mdbx_stat not available, using heuristic attribution");

        // Register all known tables
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
    }

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
