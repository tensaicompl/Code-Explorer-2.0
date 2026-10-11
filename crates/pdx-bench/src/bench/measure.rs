//! The numbers a performance baseline is made of, computed one way only.
//!
//! - **Throughput** is the files given to extraction (`Finished(Extract)`'s count) per
//!   second of Stage 2 and Stage 3 together: the stages that scale with the files.
//!   The nanoseconds are summed exactly, in integers, and divided once; a zero
//!   duration is an error, never an infinite rate, and the result must be finite and
//!   non-negative.
//! - **Medians** are of an odd number of samples, so a median is always one of them.
//! - **Peak resident memory** is the maximum of every sample, never a median: the
//!   peak is what a machine must hold.

/// Why a measure cannot be taken.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MeasureError {
    /// Stages 2 and 3 took no measurable time.
    #[error("extraction and resolution took no measurable time, so there is no throughput")]
    ZeroDuration,
    /// The arithmetic gave no finite, non-negative rate.
    #[error("the throughput is not a finite, non-negative number")]
    NotFinite,
    /// A median of no samples, or of an even number of them.
    #[error("a median needs an odd number of samples, not {0}")]
    Median(usize),
    /// No sample at all.
    #[error("there are no samples")]
    Empty,
}

/// Files per second: `files` over `extract_ns + resolve_ns`.
///
/// # Errors
///
/// [`MeasureError::ZeroDuration`] or [`MeasureError::NotFinite`].
#[allow(clippy::cast_precision_loss)] // exact below 2^53, far beyond any repository
pub fn files_per_second(files: u64, extract_ns: u64, resolve_ns: u64) -> Result<f64, MeasureError> {
    let nanoseconds = u128::from(extract_ns) + u128::from(resolve_ns);
    if nanoseconds == 0 {
        return Err(MeasureError::ZeroDuration);
    }
    let numerator = u128::from(files) * 1_000_000_000;
    let rate = numerator as f64 / nanoseconds as f64;
    if rate.is_finite() && rate >= 0.0 {
        Ok(rate)
    } else {
        Err(MeasureError::NotFinite)
    }
}

/// The middle of an odd number of values.
///
/// # Errors
///
/// [`MeasureError::Median`] for none or an even number.
pub fn median_u64(values: &[u64]) -> Result<u64, MeasureError> {
    if values.len().is_multiple_of(2) {
        return Err(MeasureError::Median(values.len()));
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    Ok(sorted[sorted.len() / 2])
}

/// The middle of an odd number of rates.
///
/// # Errors
///
/// [`MeasureError::Median`] for none or an even number; [`MeasureError::NotFinite`]
/// for a value that is not a finite, non-negative rate.
pub fn median_f64(values: &[f64]) -> Result<f64, MeasureError> {
    if values.len().is_multiple_of(2) {
        return Err(MeasureError::Median(values.len()));
    }
    if values.iter().any(|v| !v.is_finite() || *v < 0.0) {
        return Err(MeasureError::NotFinite);
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    Ok(sorted[sorted.len() / 2])
}

/// The largest value: the peak of peaks.
///
/// # Errors
///
/// [`MeasureError::Empty`] for none.
pub fn maximum(values: &[u64]) -> Result<u64, MeasureError> {
    values.iter().copied().max().ok_or(MeasureError::Empty)
}

/// The peak of a series of resident-memory samples.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Peak {
    peak: Option<u64>,
    samples: u64,
}

impl Peak {
    /// Takes one sample.
    pub fn observe(&mut self, bytes: u64) {
        self.peak = Some(self.peak.map_or(bytes, |p| p.max(bytes)));
        self.samples += 1;
    }

    /// The largest sample, if any was taken.
    pub fn peak(&self) -> Option<u64> {
        self.peak
    }

    /// How many samples were taken.
    pub fn samples(&self) -> u64 {
        self.samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    #[test]
    fn throughput_is_extracted_files_over_extract_and_resolve() {
        // 100 files in 100 ms of extraction and 100 ms of resolution: 500 a second.
        assert_eq!(files_per_second(100, 100 * MS, 100 * MS), Ok(500.0));
        assert_eq!(files_per_second(0, 1, 0), Ok(0.0));
        assert_eq!(files_per_second(3, 0, 1_000_000_000), Ok(3.0));
    }

    #[test]
    fn a_zero_duration_is_an_error_not_an_infinite_rate() {
        assert_eq!(files_per_second(100, 0, 0), Err(MeasureError::ZeroDuration));
        assert_eq!(files_per_second(0, 0, 0), Err(MeasureError::ZeroDuration));
    }

    #[test]
    fn extreme_durations_never_overflow() {
        let rate = files_per_second(u64::MAX, u64::MAX, u64::MAX).expect("a rate");
        assert!(rate.is_finite() && rate > 0.0);
    }

    #[test]
    fn medians_are_of_odd_counts_and_are_a_sample() {
        assert_eq!(median_f64(&[100.0, 300.0, 200.0]), Ok(200.0));
        assert_eq!(median_u64(&[30, 10, 20]), Ok(20));
        assert_eq!(median_u64(&[7]), Ok(7));
        assert_eq!(median_u64(&[]), Err(MeasureError::Median(0)));
        assert_eq!(median_u64(&[1, 2]), Err(MeasureError::Median(2)));
        assert_eq!(
            median_f64(&[1.0, f64::NAN, 2.0]),
            Err(MeasureError::NotFinite)
        );
        assert_eq!(median_f64(&[1.0, -1.0, 2.0]), Err(MeasureError::NotFinite));
    }

    #[test]
    fn the_peak_is_the_maximum_never_the_median() {
        assert_eq!(maximum(&[10, 40, 20]), Ok(40));
        assert_eq!(maximum(&[]), Err(MeasureError::Empty));
        let mut peak = Peak::default();
        assert_eq!(peak.peak(), None);
        for rss in [10, 20, 15] {
            peak.observe(rss);
        }
        assert_eq!(peak.peak(), Some(20));
        assert_eq!(peak.samples(), 3);
    }
}
