//! A worker run from the harness: its request on standard input, its answer read
//! from standard output, its standard error kept to a bounded prefix, and its resident
//! memory sampled from this process.
//!
//! Sampling: once immediately after the worker starts, then every
//! [`SAMPLE_INTERVAL`], the last sample taken just before its exit is seen; the peak
//! is the largest. Growth between the last sample and the exit is not seen, so the
//! recorded peak is a lower bound, short by at most one interval's growth.
//!
//! The ceiling: a worker whose sampled memory exceeds [`MEMORY_CEILING_BYTES`] is
//! stopped at once, so that a repository too large for the machine ends as a recorded
//! outcome instead of exhausting the machine's memory.

use std::io::{self, Read, Write};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use super::measure::Peak;

/// How often a running worker's resident memory is sampled.
pub const SAMPLE_INTERVAL: Duration = Duration::from_millis(10);

/// The resident memory above which a worker is stopped: 6 GiB.
pub const MEMORY_CEILING_BYTES: u64 = 6 * 1024 * 1024 * 1024;

/// The most of a worker's standard error kept.
pub const STDERR_LIMIT: usize = 4096;

/// The most of a worker's standard output kept: one response is far smaller.
const STDOUT_LIMIT: usize = 1024 * 1024;

/// Reads a process's resident memory.
pub trait RssProbe {
    /// The resident memory of `pid` in bytes, if it can be read.
    fn sample(&mut self, pid: u32) -> Option<u64>;
}

/// The operating system's account of a process's resident set.
#[derive(Debug)]
pub struct SystemProbe {
    system: System,
}

impl SystemProbe {
    /// A probe that has read nothing yet.
    pub fn new() -> Self {
        Self {
            system: System::new(),
        }
    }
}

impl Default for SystemProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl RssProbe for SystemProbe {
    fn sample(&mut self, pid: u32) -> Option<u64> {
        let pid = Pid::from_u32(pid);
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing().with_memory(),
        );
        self.system.process(pid).map(sysinfo::Process::memory)
    }
}

/// How a worker ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ended {
    /// It exited, with this code if it had one.
    Exited(Option<i32>),
    /// It was stopped for exceeding the ceiling.
    OverCeiling,
}

/// A bounded prefix of a stream.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bounded {
    /// The first bytes.
    pub bytes: Vec<u8>,
    /// Whether more followed.
    pub truncated: bool,
}

impl Bounded {
    /// The prefix as text, marked when cut short.
    pub fn text(&self) -> String {
        let text = String::from_utf8_lossy(&self.bytes).trim().to_owned();
        if self.truncated {
            format!("{text} [cut short]")
        } else {
            text
        }
    }
}

/// What a monitored run did.
#[derive(Clone, Debug)]
pub struct Monitored {
    /// How it ended.
    pub ended: Ended,
    /// Its standard output.
    pub stdout: Bounded,
    /// Its standard error.
    pub stderr: Bounded,
    /// Its resident-memory samples.
    pub peak: Peak,
}

/// Why a worker could not be run.
#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    /// It could not be started.
    #[error("cannot start the benchmark worker: {0}")]
    Spawn(io::Error),
    /// Its request could not be sent.
    #[error("cannot send the benchmark worker its request: {0}")]
    Stdin(io::Error),
    /// It could not be waited for or stopped.
    #[error("cannot wait for the benchmark worker: {0}")]
    Wait(io::Error),
}

/// Reads `reader` to its end, keeping at most `limit` bytes: the rest is drained, so
/// the writer never blocks on a full pipe.
pub fn read_bounded(mut reader: impl Read, limit: usize) -> Bounded {
    let mut kept = Bounded::default();
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                let room = limit.saturating_sub(kept.bytes.len());
                kept.bytes.extend_from_slice(&buffer[..n.min(room)]);
                kept.truncated |= n > room;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    kept
}

/// Runs `command` with `input` on its standard input, sampling its memory with
/// `probe` every `interval` and stopping it above `ceiling` bytes.
///
/// # Errors
///
/// [`ProcessError`] when it cannot be started, sent its input, or waited for.
pub fn run_monitored(
    mut command: Command,
    input: &[u8],
    probe: &mut dyn RssProbe,
    ceiling: u64,
    interval: Duration,
) -> Result<Monitored, ProcessError> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(ProcessError::Spawn)?;
    let stdout = child
        .stdout
        .take()
        .map(|out| thread::spawn(move || read_bounded(out, STDOUT_LIMIT)));
    let stderr = child
        .stderr
        .take()
        .map(|err| thread::spawn(move || read_bounded(err, STDERR_LIMIT)));
    if let Some(mut stdin) = child.stdin.take() {
        match stdin.write_all(input) {
            // A worker that has already exited says why through its status.
            Err(e) if e.kind() != io::ErrorKind::BrokenPipe => {
                stop(&mut child);
                return Err(ProcessError::Stdin(e));
            }
            _ => {}
        }
    }
    let mut peak = Peak::default();
    let ended = loop {
        if let Some(rss) = probe.sample(child.id()) {
            peak.observe(rss);
            if rss > ceiling {
                stop(&mut child);
                break Ended::OverCeiling;
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ended::Exited(status.code()),
            Ok(None) => thread::sleep(interval),
            Err(e) => {
                stop(&mut child);
                return Err(ProcessError::Wait(e));
            }
        }
    };
    let join = |handle: Option<thread::JoinHandle<Bounded>>| {
        handle.and_then(|h| h.join().ok()).unwrap_or_default()
    };
    Ok(Monitored {
        ended,
        stdout: join(stdout),
        stderr: join(stderr),
        peak,
    })
}

/// Stops a child and reaps it.
fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bounded_read_keeps_a_prefix_and_drains_the_rest() {
        let input = vec![b'x'; 100_000];
        let kept = read_bounded(&input[..], STDERR_LIMIT);
        assert_eq!(kept.bytes.len(), STDERR_LIMIT);
        assert!(kept.truncated);
        assert!(kept.text().ends_with("[cut short]"));
        let short = read_bounded(&b"one line\n"[..], STDERR_LIMIT);
        assert_eq!(short.text(), "one line");
        assert!(!short.truncated);
    }

    /// A probe that answers from a script, then nothing.
    struct Scripted(Vec<u64>);

    impl RssProbe for Scripted {
        fn sample(&mut self, _pid: u32) -> Option<u64> {
            if self.0.is_empty() {
                None
            } else {
                Some(self.0.remove(0))
            }
        }
    }

    #[cfg(unix)]
    fn shell(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }

    #[cfg(unix)]
    #[test]
    fn the_peak_is_the_largest_sample() {
        let mut probe = Scripted(vec![10, 20, 15]);
        let run = run_monitored(
            shell("exec sleep 1"),
            b"",
            &mut probe,
            u64::MAX,
            Duration::from_millis(1),
        )
        .expect("runs");
        assert_eq!(run.ended, Ended::Exited(Some(0)));
        assert_eq!(run.peak.peak(), Some(20));
        assert_eq!(run.peak.samples(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn a_worker_over_the_ceiling_is_stopped() {
        let mut probe = Scripted(vec![10, 50]);
        let run = run_monitored(
            shell("exec sleep 30"),
            b"",
            &mut probe,
            40,
            Duration::from_millis(1),
        )
        .expect("runs");
        assert_eq!(run.ended, Ended::OverCeiling);
        assert_eq!(run.peak.peak(), Some(50));
    }

    #[cfg(unix)]
    #[test]
    fn a_worker_s_stderr_is_bounded_and_its_input_delivered() {
        let mut probe = SystemProbe::new();
        let run = run_monitored(
            shell("cat; i=0; while [ $i -lt 2000 ]; do echo 'a noisy line of error' >&2; i=$((i+1)); done; exit 3"),
            b"the request\n",
            &mut probe,
            u64::MAX,
            SAMPLE_INTERVAL,
        )
        .expect("runs");
        assert_eq!(run.ended, Ended::Exited(Some(3)));
        assert_eq!(run.stdout.text(), "the request");
        assert_eq!(run.stderr.bytes.len(), STDERR_LIMIT);
        assert!(run.stderr.truncated);
    }

    #[cfg(unix)]
    #[test]
    fn the_system_probe_reads_a_live_process() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exec sleep 5"])
            .spawn()
            .expect("spawns");
        let mut probe = SystemProbe::new();
        // A child sampled before its exec completes can read as empty, so the probe
        // gets a moment, as the harness's own repeated sampling does.
        let mut rss = None;
        for _ in 0..200 {
            rss = probe.sample(child.id());
            if rss.is_some_and(|bytes| bytes > 0) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        stop(&mut child);
        assert!(rss.is_some_and(|bytes| bytes > 0), "{rss:?}");
    }
}
