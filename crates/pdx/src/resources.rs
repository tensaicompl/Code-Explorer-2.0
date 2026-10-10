//! Stage 2's worker and memory limits for a local build (4.5, "Worker and memory
//! limits"). `build_segment` takes them resolved; resolving them is its caller's, and
//! this is the local caller.
//!
//! - **Workers** = the least of the processors available to this process, the
//!   cgroup's CPU quota and `PDX_WORKERS`, whichever of them apply, and at least one.
//!   A quota of `quota / period` processors is rounded down (decision 37).
//! - **Memory budget** = `PDX_MEM_BUDGET_MB` MiB when it is set; otherwise 60% of the
//!   cgroup's memory limit, rounded down; with no such limit,
//!   [`DEFAULT_MEMORY_BUDGET_BYTES`] (decision 37). At least one byte.
//!
//! An operator's setting that is not a positive whole number is an error, never
//! ignored. What the host reports ([`Facts`]) is read apart from how the limits follow
//! from it ([`resolve`]), which is pure. None of this reaches a segment or a report.

use std::fmt;
use std::path::Path;

use pdx_core::consts::DEFAULT_MEMORY_BUDGET_BYTES;
use pdx_core::index::extract::ExtractLimits;

/// The operator's worker cap.
pub const WORKERS_VAR: &str = "PDX_WORKERS";
/// The operator's memory budget, in MiB.
pub const MEMORY_VAR: &str = "PDX_MEM_BUDGET_MB";

const MIB: u64 = 1024 * 1024;

/// What the host reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// The processors this process may run on, when the runtime knows.
    pub available_parallelism: Option<u64>,
    /// The cgroup's CPU quota: `quota` microseconds per `period`.
    pub cpu_quota: Option<CpuQuota>,
    /// The cgroup's memory limit in bytes, when there is a finite one.
    pub memory_limit: Option<u64>,
}

/// A CPU quota: `quota` microseconds of CPU time per `period` microseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuQuota {
    /// CPU time allowed per period.
    pub quota: u64,
    /// The period.
    pub period: u64,
}

impl CpuQuota {
    /// The whole processors the quota allows, rounded down, at least one.
    pub fn processors(self) -> u64 {
        (self.quota / self.period.max(1)).max(1)
    }
}

/// The operator's settings, as the environment gives them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// `PDX_WORKERS`, when set (a value that is not Unicode reads as empty, which is
    /// not a number).
    pub workers: Option<String>,
    /// `PDX_MEM_BUDGET_MB`, when set, likewise.
    pub memory_mb: Option<String>,
}

impl Settings {
    /// The settings in this process's environment.
    pub fn from_env() -> Self {
        let read = |name: &str| {
            std::env::var_os(name).map(|value| value.into_string().unwrap_or_default())
        };
        Self {
            workers: read(WORKERS_VAR),
            memory_mb: read(MEMORY_VAR),
        }
    }
}

/// An operator's setting that cannot be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceError {
    /// `PDX_WORKERS` is not a positive whole number.
    Workers,
    /// `PDX_MEM_BUDGET_MB` is not a positive whole number of MiB a byte count can hold.
    MemoryBudget,
}

impl fmt::Display for ResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Workers => write!(f, "{WORKERS_VAR} must be a positive whole number"),
            Self::MemoryBudget => write!(
                f,
                "{MEMORY_VAR} must be a positive whole number of MiB, at most {} MiB",
                u64::MAX / MIB
            ),
        }
    }
}

/// A positive whole number written in decimal digits only: no sign, no space, no
/// fraction, no exponent.
fn positive(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u64>().ok().filter(|n| *n > 0)
}

/// The limits that follow from the operator's settings and the host's facts.
///
/// # Errors
///
/// [`ResourceError`] for a setting that is present but not usable.
pub fn resolve(settings: &Settings, facts: &Facts) -> Result<ExtractLimits, ResourceError> {
    let mut workers = facts.available_parallelism.unwrap_or(1).max(1);
    if let Some(quota) = facts.cpu_quota {
        workers = workers.min(quota.processors());
    }
    if let Some(value) = &settings.workers {
        workers = workers.min(positive(value).ok_or(ResourceError::Workers)?);
    }
    let memory = match &settings.memory_mb {
        Some(value) => positive(value)
            .and_then(|mb| mb.checked_mul(MIB))
            .ok_or(ResourceError::MemoryBudget)?,
        None => facts
            .memory_limit
            .map_or(DEFAULT_MEMORY_BUDGET_BYTES, sixty_percent),
    };
    Ok(ExtractLimits {
        requested_workers: usize::try_from(workers.max(1)).unwrap_or(usize::MAX),
        memory_budget_bytes: memory.max(1),
    })
}

/// 60% of `limit`, rounded down, in integers wide enough never to overflow.
fn sixty_percent(limit: u64) -> u64 {
    let budget = u128::from(limit) * 60 / 100;
    u64::try_from(budget).unwrap_or(u64::MAX).max(1)
}

/// What this host reports.
pub fn host_facts() -> Facts {
    let available = std::thread::available_parallelism()
        .ok()
        .and_then(|n| u64::try_from(n.get()).ok());
    let cgroup = cgroup::read(Path::new("/"));
    Facts {
        available_parallelism: available,
        cpu_quota: cgroup.cpu_quota,
        memory_limit: cgroup.memory_limit,
    }
}

/// Linux control groups, read below a root (`/` on the host, a directory in tests).
/// Version 2 (`cpu.max`, `memory.max`) and version 1 (`cpu.cfs_quota_us`,
/// `cpu.cfs_period_us`, `memory.limit_in_bytes`), each limit the tightest of the
/// process's group and its ancestors. Anything missing or malformed is no limit known:
/// on a host without control groups (macOS, Windows) nothing is found.
pub mod cgroup {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::CpuQuota;

    /// Larger than this, a version 1 memory limit is the kernel's "no limit".
    const V1_UNLIMITED: u64 = 1 << 62;

    /// The limits found.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Limits {
        /// The tightest CPU quota.
        pub cpu_quota: Option<CpuQuota>,
        /// The tightest memory limit.
        pub memory_limit: Option<u64>,
    }

    /// Version 2's `cpu.max`: `max <period>` is no limit, `<quota> <period>` one.
    pub fn parse_cpu_max(text: &str) -> Option<CpuQuota> {
        let mut parts = text.split_whitespace();
        let quota = parts.next()?;
        let period = parts.next()?.parse::<u64>().ok().filter(|p| *p > 0)?;
        if parts.next().is_some() || quota == "max" {
            return None;
        }
        let quota = quota.parse::<u64>().ok().filter(|q| *q > 0)?;
        Some(CpuQuota { quota, period })
    }

    /// Version 2's `memory.max`: `max` is no limit.
    pub fn parse_memory_max(text: &str) -> Option<u64> {
        let text = text.trim();
        if text == "max" {
            return None;
        }
        text.parse::<u64>().ok().filter(|n| *n > 0)
    }

    /// Version 1's quota and period: a quota of `-1` is no limit.
    pub fn parse_cfs(quota: &str, period: &str) -> Option<CpuQuota> {
        let quota = quota.trim().parse::<i64>().ok()?;
        let period = period.trim().parse::<u64>().ok().filter(|p| *p > 0)?;
        let quota = u64::try_from(quota).ok().filter(|q| *q > 0)?;
        Some(CpuQuota { quota, period })
    }

    /// Version 1's `memory.limit_in_bytes`: a value near `i64::MAX` is no limit.
    pub fn parse_limit_in_bytes(text: &str) -> Option<u64> {
        text.trim()
            .parse::<u64>()
            .ok()
            .filter(|n| *n > 0 && *n < V1_UNLIMITED)
    }

    /// The tighter of two quotas: the one allowing fewer processors.
    fn tighter(a: Option<CpuQuota>, b: Option<CpuQuota>) -> Option<CpuQuota> {
        match (a, b) {
            (Some(a), Some(b)) => Some(if b.processors() < a.processors() {
                b
            } else {
                a
            }),
            (a, b) => a.or(b),
        }
    }

    /// The directories from a group up to its hierarchy's root, the group first; just
    /// the root when the group is not visible (a container's own namespace).
    fn ancestry(mount: &Path, group: &str) -> Vec<PathBuf> {
        let relative = group.trim_start_matches('/');
        let leaf = mount.join(relative);
        let mut dirs = Vec::new();
        if relative.is_empty() || !leaf.is_dir() {
            dirs.push(mount.to_path_buf());
            return dirs;
        }
        let mut dir = leaf.as_path();
        loop {
            dirs.push(dir.to_path_buf());
            if dir == mount {
                break;
            }
            match dir.parent() {
                Some(parent) if parent.starts_with(mount) => dir = parent,
                _ => break,
            }
        }
        dirs
    }

    fn read_small(path: &Path) -> Option<String> {
        let text = fs::read_to_string(path).ok()?;
        (text.len() <= 4096).then_some(text)
    }

    /// The limits that apply to this process, read below `root`.
    pub fn read(root: &Path) -> Limits {
        let Some(groups) = read_small(&root.join("proc/self/cgroup")) else {
            return Limits::default();
        };
        let base = root.join("sys/fs/cgroup");
        let mut limits = Limits::default();
        for line in groups.lines() {
            let mut fields = line.splitn(3, ':');
            let (Some(id), Some(controllers), Some(group)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            if id == "0" && controllers.is_empty() {
                // Version 2: one unified hierarchy.
                if !base.join("cgroup.controllers").is_file() {
                    continue;
                }
                for dir in ancestry(&base, group) {
                    let cpu = read_small(&dir.join("cpu.max")).and_then(|t| parse_cpu_max(&t));
                    limits.cpu_quota = tighter(limits.cpu_quota, cpu);
                    if let Some(memory) =
                        read_small(&dir.join("memory.max")).and_then(|t| parse_memory_max(&t))
                    {
                        limits.memory_limit =
                            Some(limits.memory_limit.map_or(memory, |m| m.min(memory)));
                    }
                }
                continue;
            }
            let controllers: Vec<&str> = controllers.split(',').collect();
            if controllers.contains(&"cpu") {
                let mount = ["cpu,cpuacct", "cpuacct,cpu", "cpu"]
                    .iter()
                    .map(|m| base.join(m))
                    .find(|m| m.is_dir());
                if let Some(mount) = mount {
                    for dir in ancestry(&mount, group) {
                        let quota = read_small(&dir.join("cpu.cfs_quota_us"));
                        let period = read_small(&dir.join("cpu.cfs_period_us"));
                        if let (Some(quota), Some(period)) = (quota, period) {
                            limits.cpu_quota =
                                tighter(limits.cpu_quota, parse_cfs(&quota, &period));
                        }
                    }
                }
            }
            if controllers.contains(&"memory") {
                let mount = base.join("memory");
                if mount.is_dir() {
                    for dir in ancestry(&mount, group) {
                        if let Some(memory) = read_small(&dir.join("memory.limit_in_bytes"))
                            .and_then(|t| parse_limit_in_bytes(&t))
                        {
                            limits.memory_limit =
                                Some(limits.memory_limit.map_or(memory, |m| m.min(memory)));
                        }
                    }
                }
            }
        }
        limits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(available: u64) -> Facts {
        Facts {
            available_parallelism: Some(available),
            ..Facts::default()
        }
    }

    fn workers(settings: &Settings, facts: &Facts) -> usize {
        resolve(settings, facts).expect("limits").requested_workers
    }

    #[test]
    fn workers_are_the_least_of_what_applies() {
        let none = Settings::default();
        assert_eq!(workers(&none, &facts(8)), 8);
        // The operator's cap, below and above the processors.
        let two = Settings {
            workers: Some("2".to_owned()),
            ..Settings::default()
        };
        assert_eq!(workers(&two, &facts(8)), 2);
        let many = Settings {
            workers: Some("64".to_owned()),
            ..Settings::default()
        };
        assert_eq!(workers(&many, &facts(8)), 8);
        // A quota below both: 2.5 processors is 2.
        let quota = Facts {
            available_parallelism: Some(8),
            cpu_quota: Some(CpuQuota {
                quota: 250_000,
                period: 100_000,
            }),
            memory_limit: None,
        };
        assert_eq!(workers(&many, &quota), 2);
        // A quota under one processor is one worker; unknown parallelism is one.
        let tiny = Facts {
            cpu_quota: Some(CpuQuota {
                quota: 50_000,
                period: 100_000,
            }),
            ..facts(8)
        };
        assert_eq!(workers(&none, &tiny), 1);
        assert_eq!(workers(&none, &Facts::default()), 1);
        // Exactly one and exactly four processors.
        for (quota, expected) in [(100_000, 1), (400_000, 4)] {
            let f = Facts {
                cpu_quota: Some(CpuQuota {
                    quota,
                    period: 100_000,
                }),
                ..facts(16)
            };
            assert_eq!(workers(&none, &f), expected);
        }
    }

    #[test]
    fn an_unusable_worker_setting_is_an_error() {
        for value in [
            "0",
            "-1",
            "1.5",
            " 2",
            "2 ",
            "+2",
            "two",
            "",
            "1e3",
            "99999999999999999999",
        ] {
            let settings = Settings {
                workers: Some(value.to_owned()),
                ..Settings::default()
            };
            assert_eq!(
                resolve(&settings, &facts(8)),
                Err(ResourceError::Workers),
                "{value:?}"
            );
        }
        // What `from_env` reads a value that is not Unicode as.
        let not_unicode = Settings {
            workers: Some(String::new()),
            ..Settings::default()
        };
        assert_eq!(
            resolve(&not_unicode, &facts(8)),
            Err(ResourceError::Workers)
        );
    }

    #[test]
    fn the_memory_budget_follows_its_sources() {
        let budget = |settings: &Settings, facts: &Facts| {
            resolve(settings, facts)
                .expect("limits")
                .memory_budget_bytes
        };
        // The operator's MiB, exactly.
        let eight = Settings {
            memory_mb: Some("8".to_owned()),
            ..Settings::default()
        };
        assert_eq!(budget(&eight, &facts(4)), 8 * 1024 * 1024);
        // The setting wins over a cgroup limit.
        let limited = Facts {
            memory_limit: Some(1_000_000_000),
            ..facts(4)
        };
        assert_eq!(budget(&eight, &limited), 8 * 1024 * 1024);
        // 60% of a cgroup limit, rounded down: 1000000000 → 600000000; 999 → 599.
        let none = Settings::default();
        assert_eq!(budget(&none, &limited), 600_000_000);
        let small = Facts {
            memory_limit: Some(999),
            ..facts(4)
        };
        assert_eq!(budget(&none, &small), 599);
        // 60% of one byte is no bytes, and the budget is at least one.
        let one = Facts {
            memory_limit: Some(1),
            ..facts(4)
        };
        assert_eq!(budget(&none, &one), 1);
        // No overflow at the top: 60% of u64::MAX, computed exactly.
        let huge = Facts {
            memory_limit: Some(u64::MAX),
            ..facts(4)
        };
        assert_eq!(budget(&none, &huge), 11_068_046_444_225_730_969);
        // No limit anywhere: the fixed default (decision 37), 512 MiB.
        assert_eq!(budget(&none, &facts(4)), 512 * 1024 * 1024);
        assert_eq!(DEFAULT_MEMORY_BUDGET_BYTES, 536_870_912);
    }

    #[test]
    fn an_unusable_memory_setting_is_an_error() {
        // 17592186044416 MiB is 2^64 bytes: one more than a u64 holds.
        for value in ["0", "-8", "1.5", "8MB", "", " 8", "17592186044416"] {
            let settings = Settings {
                memory_mb: Some(value.to_owned()),
                ..Settings::default()
            };
            assert_eq!(
                resolve(&settings, &facts(4)),
                Err(ResourceError::MemoryBudget),
                "{value:?}"
            );
        }
        // The largest that fits.
        let largest = Settings {
            memory_mb: Some("17592186044415".to_owned()),
            ..Settings::default()
        };
        assert_eq!(
            resolve(&largest, &facts(4))
                .expect("limits")
                .memory_budget_bytes,
            17_592_186_044_415 * 1024 * 1024
        );
    }

    #[test]
    fn cgroup_files_parse() {
        use cgroup::{parse_cfs, parse_cpu_max, parse_limit_in_bytes, parse_memory_max};
        assert_eq!(parse_cpu_max("max 100000\n"), None);
        assert_eq!(
            parse_cpu_max("150000 100000\n"),
            Some(CpuQuota {
                quota: 150_000,
                period: 100_000
            })
        );
        assert_eq!(parse_cpu_max("garbage"), None);
        assert_eq!(parse_cpu_max("100000"), None);
        assert_eq!(parse_cpu_max("100000 0"), None);
        assert_eq!(parse_memory_max("max\n"), None);
        assert_eq!(parse_memory_max("536870912\n"), Some(536_870_912));
        assert_eq!(parse_memory_max("lots"), None);
        assert_eq!(parse_cfs("-1\n", "100000\n"), None);
        assert_eq!(
            parse_cfs("200000\n", "100000\n"),
            Some(CpuQuota {
                quota: 200_000,
                period: 100_000
            })
        );
        assert_eq!(parse_limit_in_bytes("9223372036854771712\n"), None);
        assert_eq!(parse_limit_in_bytes("1073741824\n"), Some(1 << 30));
    }

    fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("a directory");
        for (path, content) in files {
            let path = root.path().join(path);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("directories");
            std::fs::write(path, content).expect("a file");
        }
        root
    }

    #[test]
    fn cgroup_v2_takes_the_tightest_of_the_hierarchy() {
        let root = tree(&[
            ("proc/self/cgroup", "0::/user.slice/job\n"),
            ("sys/fs/cgroup/cgroup.controllers", "cpu memory\n"),
            ("sys/fs/cgroup/user.slice/cpu.max", "300000 100000\n"),
            ("sys/fs/cgroup/user.slice/memory.max", "2000000000\n"),
            ("sys/fs/cgroup/user.slice/job/cpu.max", "max 100000\n"),
            ("sys/fs/cgroup/user.slice/job/memory.max", "1000000000\n"),
        ]);
        let limits = cgroup::read(root.path());
        assert_eq!(limits.cpu_quota.map(CpuQuota::processors), Some(3));
        assert_eq!(limits.memory_limit, Some(1_000_000_000));
        // Missing files and no control groups: nothing known.
        assert_eq!(cgroup::read(tree(&[]).path()), cgroup::Limits::default());
        let unlimited = tree(&[
            ("proc/self/cgroup", "0::/\n"),
            ("sys/fs/cgroup/cgroup.controllers", "cpu memory\n"),
            ("sys/fs/cgroup/cpu.max", "max 100000\n"),
            ("sys/fs/cgroup/memory.max", "max\n"),
        ]);
        assert_eq!(cgroup::read(unlimited.path()), cgroup::Limits::default());
    }

    #[test]
    fn cgroup_v1_is_read_from_its_controllers() {
        let root = tree(&[
            (
                "proc/self/cgroup",
                "4:memory:/docker/abc\n3:cpu,cpuacct:/docker/abc\n1:name=systemd:/\n",
            ),
            (
                "sys/fs/cgroup/cpu,cpuacct/docker/abc/cpu.cfs_quota_us",
                "150000\n",
            ),
            (
                "sys/fs/cgroup/cpu,cpuacct/docker/abc/cpu.cfs_period_us",
                "100000\n",
            ),
            (
                "sys/fs/cgroup/memory/docker/abc/memory.limit_in_bytes",
                "268435456\n",
            ),
        ]);
        let limits = cgroup::read(root.path());
        assert_eq!(limits.cpu_quota.map(CpuQuota::processors), Some(1));
        assert_eq!(limits.memory_limit, Some(268_435_456));
        let resolved = resolve(
            &Settings::default(),
            &Facts {
                available_parallelism: Some(8),
                cpu_quota: limits.cpu_quota,
                memory_limit: limits.memory_limit,
            },
        )
        .expect("limits");
        assert_eq!(resolved.requested_workers, 1);
        assert_eq!(resolved.memory_budget_bytes, 161_061_273);
        // A container's own namespace: the group's path is not visible, the mount's
        // root holds the limit.
        let namespaced = tree(&[
            ("proc/self/cgroup", "4:memory:/docker/abc\n"),
            ("sys/fs/cgroup/memory/memory.limit_in_bytes", "536870912\n"),
        ]);
        assert_eq!(
            cgroup::read(namespaced.path()).memory_limit,
            Some(536_870_912)
        );
    }
}
