//! Symbol resolution for user-space stack traces
//!
//! This module provides functionality to resolve instruction pointers from
//! BPF-captured stack traces to human-readable function names and source locations.
//!
//! The key use case is differentiating critical path operations from background
//! work in reth. For example:
//!
//! Critical path (bad latency):
//!   reth_trie::walker::TrieWalker::seek
//!   reth_engine_tree::payload_processor::multiproof::on_state_update
//!
//! Background work (latency OK):
//!   reth_trie::walker::TrieWalker::seek
//!   reth_engine_tree::payload_processor::multiproof::on_prefetch_proof

use blazesym::symbolize::source::{Elf, Process, Source};
use blazesym::symbolize::{Symbolized, Symbolizer};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// A resolved stack frame with symbol information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedFrame {
    /// Original instruction pointer address
    pub address: u64,
    /// Symbol name (function name), if resolved
    pub symbol: Option<String>,
    /// Source file path, if available
    pub file: Option<String>,
    /// Line number in source file, if available
    pub line: Option<u32>,
    /// Offset within the function
    pub offset: Option<u64>,
}

impl ResolvedFrame {
    /// Create an unresolved frame (just the address)
    pub fn unresolved(address: u64) -> Self {
        Self {
            address,
            symbol: None,
            file: None,
            line: None,
            offset: None,
        }
    }

    /// Returns a formatted string for this frame
    pub fn format(&self) -> String {
        match (&self.symbol, &self.file, self.line) {
            (Some(sym), Some(file), Some(line)) => {
                format!("{} ({}:{})", sym, file, line)
            }
            (Some(sym), Some(file), None) => {
                format!("{} ({})", sym, file)
            }
            (Some(sym), None, _) => {
                if let Some(offset) = self.offset {
                    format!("{}+0x{:x}", sym, offset)
                } else {
                    sym.clone()
                }
            }
            (None, _, _) => {
                format!("0x{:x}", self.address)
            }
        }
    }

    /// Returns just the function name, or a hex address if unknown
    pub fn function_name(&self) -> String {
        self.symbol
            .clone()
            .unwrap_or_else(|| format!("0x{:x}", self.address))
    }
}

/// A fully resolved stack trace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedStack {
    /// The resolved frames (from innermost to outermost)
    pub frames: Vec<ResolvedFrame>,
    /// The PID this stack was captured from
    pub pid: u32,
}

impl ResolvedStack {
    /// Check if this stack trace contains a frame matching the given pattern
    pub fn contains_pattern(&self, pattern: &str) -> bool {
        self.frames.iter().any(|f| {
            f.symbol
                .as_ref()
                .map(|s| s.contains(pattern))
                .unwrap_or(false)
        })
    }

    /// Get the "call site" - the first frame that's not in libmdbx
    /// This identifies what reth code triggered the database operation
    pub fn get_call_site(&self) -> Option<&ResolvedFrame> {
        // Skip frames that are in libmdbx internals
        // The call site is typically the first frame after mdbx_* functions
        let mut past_mdbx = false;
        for frame in &self.frames {
            if let Some(ref sym) = frame.symbol {
                if sym.starts_with("mdbx_") || sym.contains("libmdbx") {
                    past_mdbx = true;
                    continue;
                }
                if past_mdbx {
                    return Some(frame);
                }
            }
        }
        // If we never saw mdbx, return the first frame
        self.frames.first()
    }

    /// Determine if this stack trace is on the critical path
    /// Returns true if the stack contains state update functions (critical)
    /// Returns false if the stack contains prefetch functions (background)
    pub fn is_critical_path(&self) -> Option<bool> {
        // Patterns that indicate critical path (state updates)
        let critical_patterns = [
            "on_state_update",
            "state_root",
            "execute_block",
            "apply_state",
            "commit",
            "new_payload",
        ];

        // Patterns that indicate background work (prefetch)
        let background_patterns = [
            "on_prefetch",
            "prefetch_proof",
            "background",
            "spawn",
            "rayon",
            "thread_pool",
        ];

        for frame in &self.frames {
            if let Some(ref sym) = frame.symbol {
                for pattern in &critical_patterns {
                    if sym.contains(pattern) {
                        return Some(true);
                    }
                }
                for pattern in &background_patterns {
                    if sym.contains(pattern) {
                        return Some(false);
                    }
                }
            }
        }
        None // Unknown
    }

    /// Get a summary of this stack for grouping purposes
    /// Returns a key that represents the unique call path
    pub fn call_path_key(&self) -> String {
        // Take up to 5 significant frames (skip low-level details)
        let significant_frames: Vec<String> = self
            .frames
            .iter()
            .filter_map(|f| f.symbol.clone())
            .filter(|s| {
                !s.starts_with("mdbx_")
                    && !s.contains("__")
                    && !s.starts_with("std::")
                    && !s.starts_with("core::")
            })
            .take(5)
            .collect();

        if significant_frames.is_empty() {
            "unknown".to_string()
        } else {
            significant_frames.join(" <- ")
        }
    }
}

/// Symbol resolver that caches resolved symbols
pub struct SymbolResolver {
    symbolizer: Symbolizer,
    /// Cache of resolved addresses per PID
    /// Key: (pid, address), Value: resolved frame
    cache: HashMap<(u32, u64), ResolvedFrame>,
    /// Path to the binary for symbol resolution (optional, uses /proc/pid/exe if not set)
    binary_path: Option<PathBuf>,
}

impl SymbolResolver {
    /// Create a new symbol resolver
    pub fn new() -> Self {
        Self {
            symbolizer: Symbolizer::new(),
            cache: HashMap::new(),
            binary_path: None,
        }
    }

    /// Create a resolver with a specific binary path
    pub fn with_binary(binary_path: PathBuf) -> Self {
        Self {
            symbolizer: Symbolizer::new(),
            cache: HashMap::new(),
            binary_path: Some(binary_path),
        }
    }

    /// Resolve a single address for a given PID
    pub fn resolve_address(&mut self, pid: u32, address: u64) -> ResolvedFrame {
        // Check cache first
        if let Some(frame) = self.cache.get(&(pid, address)) {
            return frame.clone();
        }

        // Resolve using blazesym
        let source = if let Some(ref path) = self.binary_path {
            Source::Elf(Elf::new(path))
        } else {
            Source::Process(Process::new(pid.into()))
        };

        let result = self
            .symbolizer
            .symbolize_single(&source, blazesym::symbolize::Input::AbsAddr(address));

        let frame = match result {
            Ok(Symbolized::Sym(sym)) => {
                let code_info = sym.code_info.as_ref();
                ResolvedFrame {
                    address,
                    symbol: Some(sym.name.to_string()),
                    file: code_info
                        .and_then(|c| {
                            c.dir.as_ref().map(|d| {
                                let file = c.file.to_string_lossy();
                                let dir = d.to_string_lossy();
                                format!("{}/{}", dir, file)
                            })
                        })
                        .or_else(|| code_info.map(|c| c.file.to_string_lossy().into_owned())),
                    line: code_info.and_then(|c| c.line),
                    offset: Some(sym.offset as u64),
                }
            }
            _ => ResolvedFrame::unresolved(address),
        };

        // Cache the result
        self.cache.insert((pid, address), frame.clone());
        frame
    }

    /// Resolve a full stack trace
    pub fn resolve_stack(&mut self, pid: u32, addresses: &[u64]) -> ResolvedStack {
        let frames: Vec<ResolvedFrame> = addresses
            .iter()
            .map(|&addr| self.resolve_address(pid, addr))
            .collect();

        ResolvedStack { frames, pid }
    }

    /// Clear the symbol cache
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> (usize, usize) {
        let total = self.cache.len();
        let resolved = self.cache.values().filter(|f| f.symbol.is_some()).count();
        (resolved, total)
    }
}

impl Default for SymbolResolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about call sites causing slow operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallSiteStats {
    /// The call path (function chain)
    pub call_path: String,
    /// Number of slow operations from this call site
    pub count: u64,
    /// Total latency in nanoseconds
    pub total_latency_ns: u64,
    /// Maximum latency in nanoseconds
    pub max_latency_ns: u64,
    /// Total faults
    pub total_faults: u64,
    /// Major faults
    pub major_faults: u64,
    /// Whether this is on the critical path
    pub is_critical_path: Option<bool>,
    /// Sample stack trace (first occurrence)
    pub sample_stack: Option<Vec<String>>,
}

impl CallSiteStats {
    pub fn new(call_path: String) -> Self {
        Self {
            call_path,
            count: 0,
            total_latency_ns: 0,
            max_latency_ns: 0,
            total_faults: 0,
            major_faults: 0,
            is_critical_path: None,
            sample_stack: None,
        }
    }

    pub fn avg_latency_us(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            (self.total_latency_ns as f64 / self.count as f64) / 1000.0
        }
    }

    pub fn max_latency_us(&self) -> f64 {
        self.max_latency_ns as f64 / 1000.0
    }

    pub fn avg_faults(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.total_faults as f64 / self.count as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolved_frame_format() {
        let frame = ResolvedFrame {
            address: 0x12345678,
            symbol: Some("my_function".to_string()),
            file: Some("/path/to/file.rs".to_string()),
            line: Some(42),
            offset: Some(0x10),
        };
        assert!(frame.format().contains("my_function"));
        assert!(frame.format().contains("file.rs:42"));

        let unresolved = ResolvedFrame::unresolved(0x87654321);
        assert_eq!(unresolved.format(), "0x87654321");
    }

    #[test]
    fn test_critical_path_detection() {
        let stack = ResolvedStack {
            pid: 1234,
            frames: vec![
                ResolvedFrame {
                    address: 0x1,
                    symbol: Some("mdbx_cursor_get".to_string()),
                    file: None,
                    line: None,
                    offset: None,
                },
                ResolvedFrame {
                    address: 0x2,
                    symbol: Some("reth_trie::walker::TrieWalker::seek".to_string()),
                    file: None,
                    line: None,
                    offset: None,
                },
                ResolvedFrame {
                    address: 0x3,
                    symbol: Some("on_state_update".to_string()),
                    file: None,
                    line: None,
                    offset: None,
                },
            ],
        };

        assert_eq!(stack.is_critical_path(), Some(true));

        let bg_stack = ResolvedStack {
            pid: 1234,
            frames: vec![
                ResolvedFrame {
                    address: 0x1,
                    symbol: Some("mdbx_cursor_get".to_string()),
                    file: None,
                    line: None,
                    offset: None,
                },
                ResolvedFrame {
                    address: 0x2,
                    symbol: Some("on_prefetch_proof".to_string()),
                    file: None,
                    line: None,
                    offset: None,
                },
            ],
        };

        assert_eq!(bg_stack.is_critical_path(), Some(false));
    }
}
