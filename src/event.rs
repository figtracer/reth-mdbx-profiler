//! Event types shared between BPF and userspace

use serde::{Deserialize, Serialize};

/// Event types matching the BPF definitions
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    PageFault = 1,
    Mmap = 2,
    CursorSeek = 3,
}

/// Page fault event from BPF
///
/// This struct must match the layout in mdbx_tracer.bpf.c exactly
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
    // Padding to match C struct alignment
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
            3 => Some(EventType::CursorSeek),
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
}
