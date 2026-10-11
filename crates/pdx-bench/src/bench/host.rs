//! What the measuring machine is, so a reader can weigh the numbers: its operating
//! system, architecture, processors and memory, and the benchmark's own settings.
//! Never what would identify it or its user: no host name, user name, home or other
//! path, machine identifier or time.

use serde::{Deserialize, Serialize};
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

/// The longest processor name recorded.
const BRAND_LIMIT: usize = 128;

/// The machine a run measured on, and the limits it ran with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Host {
    /// `linux`, `macos`, `windows`.
    pub os: String,
    /// `x86_64`, `aarch64`.
    pub architecture: String,
    /// The logical processors the system reports.
    pub logical_cpus: u64,
    /// The memory the system reports.
    pub total_memory_bytes: u64,
    /// The processor's name, when every processor reports the same plain one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_brand: Option<String>,
    /// Stage 2's workers in a performance build.
    pub benchmark_workers: u64,
    /// Stage 2's memory budget in a performance build.
    pub benchmark_memory_budget_bytes: u64,
}

impl Host {
    /// This machine, with the benchmark's limits.
    pub fn current(workers: usize, memory_budget_bytes: u64) -> Self {
        let system = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::nothing())
                .with_memory(MemoryRefreshKind::nothing().with_ram()),
        );
        let brands: Vec<&str> = system.cpus().iter().map(sysinfo::Cpu::brand).collect();
        Self {
            os: std::env::consts::OS.to_owned(),
            architecture: std::env::consts::ARCH.to_owned(),
            logical_cpus: system.cpus().len() as u64,
            total_memory_bytes: system.total_memory(),
            cpu_brand: reliable_brand(&brands),
            benchmark_workers: workers as u64,
            benchmark_memory_budget_bytes: memory_budget_bytes,
        }
    }
}

/// The processors' shared name: only when there is one, every processor reports it,
/// and it is plain printable ASCII of a sensible length.
pub fn reliable_brand(brands: &[&str]) -> Option<String> {
    let first = brands.first()?.trim();
    let plain = !first.is_empty()
        && first.len() <= BRAND_LIMIT
        && first.bytes().all(|b| b.is_ascii_graphic() || b == b' ');
    (plain && brands.iter().all(|b| b.trim() == first)).then(|| first.to_owned())
}

/// The workers a performance build runs: the parallelism available, at least 1.
pub fn available_workers() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_brand_is_kept_only_when_reliable() {
        assert_eq!(
            reliable_brand(&["Some CPU 9000", "Some CPU 9000 "]).as_deref(),
            Some("Some CPU 9000")
        );
        assert_eq!(reliable_brand(&[]), None);
        assert_eq!(reliable_brand(&["", ""]), None);
        assert_eq!(reliable_brand(&["A", "B"]), None);
        assert_eq!(reliable_brand(&["A\u{0}B"]), None);
        assert_eq!(reliable_brand(&["Ünïcode"]), None);
        assert_eq!(reliable_brand(&[&"x".repeat(BRAND_LIMIT + 1)]), None);
    }

    #[test]
    fn the_host_names_no_machine_or_user() {
        let host = Host::current(3, 512);
        assert!(host.logical_cpus >= 1);
        assert!(host.total_memory_bytes > 0);
        assert_eq!(host.benchmark_workers, 3);
        assert_eq!(host.benchmark_memory_budget_bytes, 512);
        let json = serde_json::to_value(&host).expect("json");
        let keys: Vec<&String> = json.as_object().expect("an object").keys().collect();
        for key in keys {
            assert!(
                [
                    "os",
                    "architecture",
                    "logical_cpus",
                    "total_memory_bytes",
                    "cpu_brand",
                    "benchmark_workers",
                    "benchmark_memory_budget_bytes"
                ]
                .contains(&key.as_str()),
                "{key}"
            );
        }
        let text = json.to_string();
        for private in ["HOME", "USER", "HOSTNAME", "USERNAME", "COMPUTERNAME"] {
            if let Some(value) = std::env::var_os(private).and_then(|v| v.into_string().ok())
                && value.len() > 3
            {
                assert!(!text.contains(&value), "{private} leaked");
            }
        }
        assert!(available_workers() >= 1);
    }
}
