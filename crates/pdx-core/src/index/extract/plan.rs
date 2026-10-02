//! How Stage 2 divides its files between workers within the memory budget (4.5,
//! "Worker and memory limits").
//!
//! The budget bounds the source buffers Stage 2 holds at once: a worker reads the
//! files of one batch, normalises and extracts them, and drops them before it takes
//! the next, and no more batches run at once than there are workers. What the engine
//! allocates while it parses and extracts cannot be known before it does so and is not
//! bounded here; the process's whole footprint is what the performance gate measures.

use crate::consts::EXTRACT_BATCH_MAX_FILES;
use crate::index::discover::{DiscoveredFile, Disposition};

use super::ExtractError;

/// The limits Stage 2 runs within, already resolved by its caller. Deployment
/// configuration (`PDX_WORKERS`, `PDX_MEM_BUDGET_MB`, a cgroup's quota and limit) is
/// read by whoever builds the segment, never here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtractLimits {
    /// The most workers to run at once. At least 1.
    pub requested_workers: usize,
    /// The most bytes of source the workers may hold at once. At least 1.
    pub memory_budget_bytes: u64,
}

/// One batch: files a worker reads and extracts together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Batch {
    /// The files, as indices into the list Stage 2 was given, in its order.
    pub files: Vec<usize>,
    /// Their sizes, summed: never more than the plan's per-worker budget.
    pub bytes: u64,
}

/// How the candidates are divided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchPlan {
    /// The workers the budget allows: at most the number requested, and few enough
    /// that each can hold the largest file the plan extracts.
    pub effective_workers: usize,
    /// The bytes of source each worker may hold at once: the budget divided by
    /// [`Self::effective_workers`].
    pub per_worker_budget: u64,
    /// The batches, in the order of the files they hold.
    pub batches: Vec<Batch>,
    /// Candidates larger than the whole budget, which no worker can hold: skipped for
    /// memory, as indices into the list Stage 2 was given.
    pub memory_skipped: Vec<usize>,
}

impl BatchPlan {
    /// The workers that run: as many as the budget allows, but never more than there
    /// are batches.
    pub fn threads(&self) -> usize {
        self.effective_workers.min(self.batches.len())
    }
}

/// Whether Stage 2 extracts a discovered file: a candidate with a language.
pub(super) fn is_extracted(file: &DiscoveredFile) -> bool {
    file.disposition == Disposition::Candidate && file.language.is_some()
}

/// Divides the files Stage 2 extracts into batches, in the order given.
///
/// A file larger than the whole budget is skipped for memory. The rest fix the worker
/// count: as many as requested, but no more than can each hold the largest of them at
/// once. Each worker's share is the budget divided by that count, and batches are
/// filled greedily, in order, never past the share and never past
/// [`EXTRACT_BATCH_MAX_FILES`].
///
/// # Errors
///
/// [`ExtractError::InvalidLimits`] for no workers or no budget.
pub fn plan_batches(
    files: &[DiscoveredFile],
    limits: ExtractLimits,
) -> Result<BatchPlan, ExtractError> {
    if limits.requested_workers == 0 {
        return Err(ExtractError::InvalidLimits("no workers"));
    }
    let budget = limits.memory_budget_bytes;
    if budget == 0 {
        return Err(ExtractError::InvalidLimits("a memory budget of zero"));
    }

    let mut memory_skipped = Vec::new();
    let mut fitting = Vec::new();
    for (index, file) in files.iter().enumerate() {
        if !is_extracted(file) {
            continue;
        }
        if file.size_bytes > budget {
            memory_skipped.push(index);
        } else {
            fitting.push(index);
        }
    }

    let max_files = usize::try_from(EXTRACT_BATCH_MAX_FILES).unwrap_or(usize::MAX);
    let largest = fitting
        .iter()
        .map(|&i| files[i].size_bytes)
        .max()
        .unwrap_or(0);
    // As many workers as requested, but no more than can each hold the largest file.
    let requested = u64::try_from(limits.requested_workers).unwrap_or(u64::MAX);
    let effective = match budget.checked_div(largest) {
        Some(fit) => requested.min(fit),
        None => requested,
    }
    .max(1);
    let effective_workers = usize::try_from(effective).unwrap_or(usize::MAX);
    let per_worker_budget = budget / effective;

    let mut batches: Vec<Batch> = Vec::new();
    let mut current = Batch {
        files: Vec::new(),
        bytes: 0,
    };
    for index in fitting {
        let size = files[index].size_bytes;
        let fits = current
            .bytes
            .checked_add(size)
            .is_some_and(|total| total <= per_worker_budget);
        if !current.files.is_empty() && (!fits || current.files.len() == max_files) {
            batches.push(std::mem::replace(
                &mut current,
                Batch {
                    files: Vec::new(),
                    bytes: 0,
                },
            ));
        }
        current.files.push(index);
        current.bytes += size;
    }
    if !current.files.is_empty() {
        batches.push(current);
    }

    Ok(BatchPlan {
        effective_workers,
        per_worker_budget,
        batches,
        memory_skipped,
    })
}
