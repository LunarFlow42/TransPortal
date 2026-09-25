//! Process memory statistics and the release-integrity policy.

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

pub trait IntegrityPlatform {
    fn enforce_release_hash(&self) -> bool;
    fn show_failure(&self, message: &str);
}

pub fn platform() -> Box<dyn IntegrityPlatform> {
    #[cfg(target_os = "windows")]
    {
        Box::new(super::windows::WindowsIntegrity)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(super::unsupported::UnsupportedIntegrity)
    }
}

#[cfg(target_os = "windows")]
pub use super::windows::{get_memory_usage, trim_memory};

#[cfg(not(target_os = "windows"))]
pub use super::unsupported::{get_memory_usage, trim_memory};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_introspection() {
        let usage = get_memory_usage().expect("failed to get memory usage");
        assert!(usage.total_working_set_bytes > 0);
        assert!(usage.process_count >= 1);
    }

    #[test]
    fn test_memory_trimming() {
        let result = trim_memory().expect("failed to trim memory");
        assert!(result.before_bytes > 0);
        assert!(result.after_bytes > 0);
    }
}
