//! Process-level services: memory statistics, the release-integrity policy
//! and fatal error reporting.

use serde::Serialize;

/// Current memory usage snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryReport {
    /// Total physical RAM across the process tree in bytes.
    pub total_working_set_bytes: u64,
    /// Total private committed bytes across the process tree.
    pub total_private_bytes: u64,
    /// Working set size of the main Rust process only.
    pub main_working_set_bytes: u64,
    /// Number of processes in the application process tree.
    pub process_count: usize,
}

/// Result of a working set trim operation.
#[derive(Debug, Clone, Serialize)]
pub struct TrimResult {
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub trimmed_bytes: u64,
    pub process_count: usize,
}

pub trait SystemOps {
    /// Memory used by this process and the processes it spawned (webview).
    fn memory_usage() -> Result<MemoryReport, String>;

    /// Ask the OS to page out cold memory of the process tree.
    fn trim_memory() -> Result<TrimResult, String>;

    /// Whether release builds must match the SHA-256 recorded at build time.
    fn enforce_release_hash() -> bool;

    /// Report an error that ends the app, before any window exists.
    fn show_fatal_error(title: &str, message: &str);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::PlatformSystem;

    #[test]
    fn test_memory_introspection() {
        let usage = PlatformSystem::memory_usage().expect("failed to get memory usage");
        assert!(usage.total_working_set_bytes > 0);
        assert!(usage.process_count >= 1);
    }

    #[test]
    fn test_memory_trimming() {
        let result = PlatformSystem::trim_memory().expect("failed to trim memory");
        assert!(result.before_bytes > 0);
        assert!(result.after_bytes > 0);
    }
}
