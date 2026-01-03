//! MDBX file detection and analysis utilities

use std::{
    fs,
    path::{Path, PathBuf},
};

/// Information about an MDBX database file
#[derive(Debug, Clone)]
pub struct MdbxFile {
    /// Path to the MDBX data file
    pub path: PathBuf,
    /// Inode number (for eBPF tracking)
    pub inode: u64,
    /// File size in bytes
    pub size: u64,
    /// Whether this is the main data file or lock file
    pub file_type: MdbxFileType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MdbxFileType {
    /// Main data file (mdbx.dat)
    Data,
    /// Lock file (mdbx.lck)
    Lock,
}

impl MdbxFile {
    /// Detect MDBX file from path
    pub fn from_path(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref();
        let metadata = fs::metadata(path)?;

        use std::os::unix::fs::MetadataExt;
        let inode = metadata.ino();
        let size = metadata.len();

        let file_type = if path.extension().map(|e| e == "lck").unwrap_or(false) {
            MdbxFileType::Lock
        } else {
            MdbxFileType::Data
        };

        Ok(Self { path: path.to_path_buf(), inode, size, file_type })
    }
}

/// Find MDBX files in a Reth data directory
pub fn find_mdbx_files(data_dir: impl AsRef<Path>) -> std::io::Result<Vec<MdbxFile>> {
    let data_dir = data_dir.as_ref();
    let mut files = Vec::new();

    // Common locations for MDBX in applications using MDBX
    let candidates = [
        data_dir.join("db/mdbx.dat"),
        data_dir.join("mdbx.dat"),
        data_dir.join("chaindata/mdbx.dat"),
    ];

    for candidate in candidates {
        if candidate.exists() {
            if let Ok(file) = MdbxFile::from_path(&candidate) {
                files.push(file);
            }
        }
    }

    // Also check for the lock file
    for file in files.clone() {
        let lock_path = file.path.with_extension("lck");
        if lock_path.exists() {
            if let Ok(lock_file) = MdbxFile::from_path(&lock_path) {
                files.push(lock_file);
            }
        }
    }

    Ok(files)
}

/// Get memory mappings for a process from /proc
pub fn get_mmap_regions(pid: u32) -> std::io::Result<Vec<MmapRegion>> {
    let maps_path = format!("/proc/{}/maps", pid);
    let content = fs::read_to_string(&maps_path)?;

    let mut regions = Vec::new();

    for line in content.lines() {
        if let Some(region) = MmapRegion::parse(line) {
            regions.push(region);
        }
    }

    Ok(regions)
}

/// A memory-mapped region from /proc/[pid]/maps
#[derive(Debug, Clone)]
pub struct MmapRegion {
    pub start: u64,
    pub end: u64,
    pub permissions: String,
    pub offset: u64,
    pub device: String,
    pub inode: u64,
    pub path: Option<PathBuf>,
}

impl MmapRegion {
    /// Parse a line from /proc/[pid]/maps
    pub fn parse(line: &str) -> Option<Self> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 {
            return None;
        }

        // Parse address range
        let addr_parts: Vec<&str> = parts[0].split('-').collect();
        if addr_parts.len() != 2 {
            return None;
        }

        let start = u64::from_str_radix(addr_parts[0], 16).ok()?;
        let end = u64::from_str_radix(addr_parts[1], 16).ok()?;
        let permissions = parts[1].to_string();
        let offset = u64::from_str_radix(parts[2], 16).ok()?;
        let device = parts[3].to_string();
        let inode = parts[4].parse().ok()?;

        let path = if parts.len() > 5 { Some(PathBuf::from(parts[5..].join(" "))) } else { None };

        Some(Self { start, end, permissions, offset, device, inode, path })
    }

    /// Check if this region is an MDBX data file
    pub fn is_mdbx(&self) -> bool {
        self.path
            .as_ref()
            .map(|p| {
                let s = p.to_string_lossy();
                s.contains("mdbx") || s.ends_with(".dat")
            })
            .unwrap_or(false)
    }

    /// Size of this mapping in bytes
    pub fn size(&self) -> u64 {
        self.end - self.start
    }
}

/// MDBX page size (default is usually 4096 but can vary)
pub const MDBX_PAGE_SIZE: u64 = 4096;

/// Convert a virtual address to file offset given mmap info
pub fn vaddr_to_file_offset(vaddr: u64, region: &MmapRegion) -> Option<u64> {
    if vaddr >= region.start && vaddr < region.end {
        Some(region.offset + (vaddr - region.start))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_maps_line() {
        let line = "7f1234560000-7f1234570000 r--p 00000000 08:01 12345 /path/to/mdbx.dat";
        let region = MmapRegion::parse(line).unwrap();

        assert_eq!(region.start, 0x7f1234560000);
        assert_eq!(region.end, 0x7f1234570000);
        assert_eq!(region.permissions, "r--p");
        assert_eq!(region.inode, 12345);
        assert!(region.is_mdbx());
    }
}
