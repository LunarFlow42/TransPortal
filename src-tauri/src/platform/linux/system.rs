//! Real /proc memory accounting. Private memory means resident private pages,
//! not a fabricated Windows commit counter. Summed RSS can share physical pages.

use crate::platform::system::{MemoryReport, SystemOps, TrimResult};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub struct LinuxSystem;

fn parent(stat: &str) -> Option<u32> {
    // A process name can contain spaces and parentheses. Fields after the final
    // ')' start at state, ppid, ... . Never split the complete stat by spaces.
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

fn kilobytes(line: &str) -> Option<u64> {
    line.split_whitespace()
        .nth(1)?
        .parse::<u64>()
        .ok()?
        .checked_mul(1024)
}

fn memory(status: &str, rollup: Option<&str>) -> (u64, u64) {
    let rss = status
        .lines()
        .find(|line| line.starts_with("VmRSS:"))
        .and_then(kilobytes)
        .unwrap_or(0);
    let private = rollup
        .map(|content| {
            content
                .lines()
                .filter(|line| {
                    line.starts_with("Private_Clean:")
                        || line.starts_with("Private_Dirty:")
                        || line.starts_with("Private_Hugetlb:")
                })
                .filter_map(kilobytes)
                .sum()
        })
        .unwrap_or_else(|| {
            // RssAnon is an explicitly documented approximation when smaps is
            // forbidden by ptrace/hidepid restrictions.
            status
                .lines()
                .find(|line| line.starts_with("RssAnon:"))
                .and_then(kilobytes)
                .unwrap_or(0)
        });
    (rss, private)
}

fn report(proc: &Path, root: u32) -> Result<MemoryReport, String> {
    let mut parents = HashMap::new();
    for entry in std::fs::read_dir(proc)
        .map_err(|error| error.to_string())?
        .flatten()
    {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        if let Ok(stat) = std::fs::read_to_string(entry.path().join("stat"))
            && let Some(ppid) = parent(&stat)
        {
            parents.insert(pid, ppid);
        }
    }
    let mut descendants = HashSet::from([root]);
    loop {
        let before = descendants.len();
        for (&pid, ppid) in &parents {
            if descendants.contains(ppid) {
                descendants.insert(pid);
            }
        }
        if descendants.len() == before {
            break;
        }
    }
    let mut result = MemoryReport {
        measurement: Some("linux_procfs"),
        total_working_set_bytes: 0,
        total_private_bytes: 0,
        main_working_set_bytes: 0,
        process_count: 0,
    };
    for pid in descendants {
        let base = proc.join(pid.to_string());
        let status = match std::fs::read_to_string(base.join("status")) {
            Ok(status) => status,
            Err(error) if pid == root => return Err(error.to_string()),
            Err(_) => continue, // WebKit child can exit during enumeration.
        };
        let rollup = std::fs::read_to_string(base.join("smaps_rollup")).ok();
        let (rss, private) = memory(&status, rollup.as_deref());
        result.total_working_set_bytes = result.total_working_set_bytes.saturating_add(rss);
        result.total_private_bytes = result.total_private_bytes.saturating_add(private);
        result.process_count += 1;
        if pid == root {
            result.main_working_set_bytes = rss;
        }
    }
    Ok(result)
}

fn language(values: [Option<&str>; 3]) -> String {
    for value in values.into_iter().flatten() {
        let value = value
            .trim()
            .split('.')
            .next()
            .unwrap_or_default()
            .split('@')
            .next()
            .unwrap_or_default();
        if value.is_empty() {
            continue;
        }
        if matches!(value, "C" | "POSIX") {
            return "en-US".into();
        }
        return value.replace('_', "-");
    }
    "en-US".into()
}

impl SystemOps for LinuxSystem {
    fn memory_usage() -> Result<MemoryReport, String> {
        report(Path::new("/proc"), std::process::id())
    }
    fn trim_memory() -> Result<TrimResult, String> {
        let before = Self::memory_usage()?;
        // Return unused glibc allocator pages only; do not touch WebKit children
        // or drop filesystem caches. This is a best effort, not guaranteed RAM.
        #[cfg(target_env = "gnu")]
        unsafe {
            libc::malloc_trim(0);
        }
        let after = Self::memory_usage()?;
        Ok(TrimResult {
            before_bytes: before.total_working_set_bytes,
            after_bytes: after.total_working_set_bytes,
            trimmed_bytes: before
                .total_working_set_bytes
                .saturating_sub(after.total_working_set_bytes),
            process_count: after.process_count,
        })
    }
    fn enforce_release_hash() -> bool {
        false
    }
    fn show_fatal_error(title: &str, message: &str) {
        eprintln!("{title}: {message}");
    }
    fn system_language() -> String {
        language([
            std::env::var("LC_ALL").ok().as_deref(),
            std::env::var("LC_MESSAGES").ok().as_deref(),
            std::env::var("LANG").ok().as_deref(),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_names_do_not_break_parent_parsing() {
        assert_eq!(parent("42 (WebKit renderer (test)) S 12 42 42"), Some(12));
        assert_eq!(parent("broken"), None);
    }
    #[test]
    fn real_private_accounting_and_permission_fallback() {
        let status = "VmRSS: 100 kB\nRssAnon: 50 kB\n";
        assert_eq!(
            memory(
                status,
                Some("Private_Clean: 10 kB\nPrivate_Dirty: 20 kB\nPrivate_Hugetlb: 0 kB\n")
            ),
            (102400, 30720)
        );
        assert_eq!(memory(status, None), (102400, 51200));
        assert_eq!(memory("VmRSS: invalid kB", None), (0, 0));
    }
    #[test]
    fn locale_precedence_and_bcp47_normalization() {
        assert_eq!(
            language([Some(""), Some("zh_CN.UTF-8"), Some("en_US.UTF-8")]),
            "zh-CN"
        );
        assert_eq!(language([Some("C.UTF-8"), Some("zh_CN"), None]), "en-US");
        assert_eq!(language([None, None, Some("sr_RS.UTF-8@latin")]), "sr-RS");
    }

    #[test]
    fn counts_only_the_process_tree_and_tolerates_a_disappearing_child() {
        let path = std::env::temp_dir().join(super::super::portal::token());
        std::fs::create_dir(&path).unwrap();
        for (pid, ppid, rss) in [
            (10, 1, 100),
            (11, 10, 50),
            (12, 11, 25),
            (13, 99, 1000),
            (14, 10, 0),
        ] {
            let process = path.join(pid.to_string());
            std::fs::create_dir(&process).unwrap();
            std::fs::write(
                process.join("stat"),
                format!("{pid} (WebKit child (test)) S {ppid} 0 0"),
            )
            .unwrap();
            if rss > 0 {
                std::fs::write(
                    process.join("status"),
                    format!("VmRSS: {rss} kB\nRssAnon: {} kB\n", rss / 2),
                )
                .unwrap();
            }
        }
        let result = report(&path, 10).unwrap();
        assert_eq!(result.process_count, 3);
        assert_eq!(result.main_working_set_bytes, 100 * 1024);
        assert_eq!(result.total_working_set_bytes, 175 * 1024);
        assert_eq!(result.total_private_bytes, 87 * 1024);
        std::fs::remove_dir_all(path).unwrap();
    }
}
