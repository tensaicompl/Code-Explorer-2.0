//! A build's stage durations, timed from its progress events: each of the five stages
//! of 4.5 must start once and finish once, in order, and anything else is an error,
//! never a guessed duration. Times are integer nanoseconds from one monotonic origin.
//!
//! The timer is fed timestamps, so the same state machine times a build as it runs
//! (the worker, with [`MonotonicClock`]) and replays one from the events a stopped
//! worker left behind (the harness).

use std::time::Instant;

use pdx_core::index::{Progress, Stage};
use serde::{Deserialize, Serialize};

/// The stages in the order a build runs them.
pub const STAGES: [Stage; 5] = [
    Stage::Discover,
    Stage::Extract,
    Stage::Resolve,
    Stage::Derive,
    Stage::Write,
];

/// A monotonic time in nanoseconds since some fixed origin.
pub trait Clock {
    /// Now.
    fn now_ns(&self) -> u64;
}

/// [`Instant`] since the clock was made.
#[derive(Clone, Copy, Debug)]
pub struct MonotonicClock {
    origin: Instant,
}

impl MonotonicClock {
    /// A clock whose zero is now.
    pub fn start() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Clock for MonotonicClock {
    fn now_ns(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

/// One finished stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageRecord {
    /// The stage.
    #[serde(with = "stage_name")]
    pub stage: Stage,
    /// What it produced, as its `Finished` event says.
    pub items: u64,
    /// How long it ran.
    pub duration_ns: u64,
}

/// Why progress events time no build.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TimerError {
    /// A stage started before the one before it finished, or out of order.
    #[error("the {got} stage started where the {expected} stage was due")]
    OutOfOrder {
        /// The stage that started.
        got: &'static str,
        /// The stage due.
        expected: &'static str,
    },
    /// A stage started while another ran.
    #[error("the {got} stage started while the {running} stage was running")]
    Overlap {
        /// The stage that started.
        got: &'static str,
        /// The one running.
        running: &'static str,
    },
    /// A stage started after the last.
    #[error("the {0} stage started after the build's last stage")]
    Extra(&'static str),
    /// A stage finished that was not running.
    #[error("the {0} stage finished without having started")]
    NotStarted(&'static str),
    /// A later event is earlier than an earlier one.
    #[error("the {0} stage finished before it started")]
    Backwards(&'static str),
    /// The build ended before every stage finished.
    #[error("the build ended before the {0} stage finished")]
    Unfinished(&'static str),
}

/// Times a build from its progress events.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StageTimer {
    finished: Vec<StageRecord>,
    running: Option<(Stage, u64)>,
}

impl StageTimer {
    /// A timer that has seen nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes `event`, which happened at `at_ns`.
    ///
    /// # Errors
    ///
    /// [`TimerError`] for an event out of order; the timer is then unchanged.
    pub fn observe(&mut self, event: &Progress, at_ns: u64) -> Result<(), TimerError> {
        match *event {
            Progress::Started(stage) => {
                if let Some((running, _)) = self.running {
                    return Err(TimerError::Overlap {
                        got: stage.as_str(),
                        running: running.as_str(),
                    });
                }
                let Some(&due) = STAGES.get(self.finished.len()) else {
                    return Err(TimerError::Extra(stage.as_str()));
                };
                if stage != due {
                    return Err(TimerError::OutOfOrder {
                        got: stage.as_str(),
                        expected: due.as_str(),
                    });
                }
                self.running = Some((stage, at_ns));
                Ok(())
            }
            Progress::Finished { stage, items } => {
                let Some((running, started)) = self.running.filter(|(s, _)| *s == stage) else {
                    return Err(TimerError::NotStarted(stage.as_str()));
                };
                let duration_ns = at_ns
                    .checked_sub(started)
                    .ok_or(TimerError::Backwards(running.as_str()))?;
                self.finished.push(StageRecord {
                    stage,
                    items,
                    duration_ns,
                });
                self.running = None;
                Ok(())
            }
        }
    }

    /// The stages finished so far, in order.
    pub fn finished(&self) -> &[StageRecord] {
        &self.finished
    }

    /// The stage running now, if any.
    pub fn running(&self) -> Option<Stage> {
        self.running.map(|(stage, _)| stage)
    }

    /// The furthest stage the build reached: the one running, else the last finished.
    pub fn reached(&self) -> Option<Stage> {
        self.running()
            .or_else(|| self.finished.last().map(|r| r.stage))
    }

    /// The finished build's five stages.
    ///
    /// # Errors
    ///
    /// [`TimerError::Unfinished`] unless every stage finished.
    pub fn complete(self) -> Result<StageTimes, TimerError> {
        match STAGES.get(self.finished.len()) {
            Some(due) => Err(TimerError::Unfinished(due.as_str())),
            None => Ok(StageTimes {
                records: self.finished,
            }),
        }
    }
}

/// A finished build's five stages, in order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<StageRecord>", into = "Vec<StageRecord>")]
pub struct StageTimes {
    records: Vec<StageRecord>,
}

impl StageTimes {
    /// The record of `stage`.
    pub fn get(&self, stage: Stage) -> StageRecord {
        let index = STAGES.iter().position(|s| *s == stage).unwrap_or(0);
        self.records[index]
    }

    /// The five records, in order.
    pub fn records(&self) -> &[StageRecord] {
        &self.records
    }

    /// The duration of `stage`.
    pub fn duration_ns(&self, stage: Stage) -> u64 {
        self.get(stage).duration_ns
    }

    /// What `stage` produced.
    pub fn items(&self, stage: Stage) -> u64 {
        self.get(stage).items
    }
}

impl TryFrom<Vec<StageRecord>> for StageTimes {
    type Error = String;

    fn try_from(records: Vec<StageRecord>) -> Result<Self, Self::Error> {
        let stages: Vec<Stage> = records.iter().map(|r| r.stage).collect();
        if stages == STAGES {
            Ok(Self { records })
        } else {
            Err("the stages are not the five of a build, in order".to_owned())
        }
    }
}

impl From<StageTimes> for Vec<StageRecord> {
    fn from(times: StageTimes) -> Self {
        times.records
    }
}

/// A stage's name, as its span is named.
pub fn stage_by_name(name: &str) -> Option<Stage> {
    STAGES.into_iter().find(|s| s.as_str() == name)
}

/// [`Stage`] as its name.
pub mod stage_name {
    use pdx_core::index::Stage;
    use serde::{Deserialize, Deserializer, Serializer};

    /// Writes the name.
    ///
    /// # Errors
    ///
    /// The serializer's.
    pub fn serialize<S: Serializer>(stage: &Stage, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(stage.as_str())
    }

    /// Reads a name.
    ///
    /// # Errors
    ///
    /// For a name no stage has.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Stage, D::Error> {
        let name = String::deserialize(deserializer)?;
        super::stage_by_name(&name)
            .ok_or_else(|| serde::de::Error::custom(format!("no stage is named {name:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(timer: &mut StageTimer, events: &[(Progress, u64)]) -> Result<(), TimerError> {
        for (event, at) in events {
            timer.observe(event, *at)?;
        }
        Ok(())
    }

    fn whole_build() -> Vec<(Progress, u64)> {
        let mut events = Vec::new();
        let mut at = 0;
        for (i, stage) in STAGES.into_iter().enumerate() {
            events.push((Progress::Started(stage), at));
            at += 10 * (i as u64 + 1);
            events.push((
                Progress::Finished {
                    stage,
                    items: i as u64,
                },
                at,
            ));
            at += 1;
        }
        events
    }

    #[test]
    fn five_stages_in_order_are_timed() {
        let mut timer = StageTimer::new();
        run(&mut timer, &whole_build()).expect("in order");
        let stage_times = timer.complete().expect("complete");
        let durations: Vec<u64> = stage_times
            .records()
            .iter()
            .map(|r| r.duration_ns)
            .collect();
        assert_eq!(durations, [10, 20, 30, 40, 50]);
        assert_eq!(stage_times.items(Stage::Derive), 3);
        assert_eq!(stage_times.duration_ns(Stage::Write), 50);
    }

    #[test]
    fn anything_but_once_each_in_order_is_an_error() {
        let started = |s| Progress::Started(s);
        let finished = |stage| Progress::Finished { stage, items: 0 };
        let cases: Vec<(Vec<(Progress, u64)>, TimerError)> = vec![
            (
                vec![(started(Stage::Extract), 0)],
                TimerError::OutOfOrder {
                    got: "extract",
                    expected: "discover",
                },
            ),
            (
                vec![(started(Stage::Discover), 0), (started(Stage::Discover), 1)],
                TimerError::Overlap {
                    got: "discover",
                    running: "discover",
                },
            ),
            (
                vec![(finished(Stage::Discover), 0)],
                TimerError::NotStarted("discover"),
            ),
            (
                vec![
                    (started(Stage::Discover), 5),
                    (finished(Stage::Discover), 4),
                ],
                TimerError::Backwards("discover"),
            ),
            (
                vec![
                    (started(Stage::Discover), 0),
                    (finished(Stage::Discover), 1),
                    (finished(Stage::Discover), 2),
                ],
                TimerError::NotStarted("discover"),
            ),
        ];
        for (events, want) in cases {
            let mut timer = StageTimer::new();
            assert_eq!(run(&mut timer, &events), Err(want));
        }
        let mut timer = StageTimer::new();
        let mut events = whole_build();
        events.push((started(Stage::Discover), 1_000));
        assert_eq!(run(&mut timer, &events), Err(TimerError::Extra("discover")));
    }

    #[test]
    fn a_build_that_stops_early_is_unfinished_and_says_where() {
        let mut timer = StageTimer::new();
        let events = whole_build();
        run(&mut timer, &events[..3]).expect("in order");
        assert_eq!(timer.running(), Some(Stage::Extract));
        assert_eq!(timer.reached(), Some(Stage::Extract));
        assert_eq!(timer.finished().len(), 1);
        assert_eq!(timer.complete(), Err(TimerError::Unfinished("extract")));
        assert_eq!(StageTimer::new().reached(), None);
    }

    #[test]
    fn stage_times_round_trip_and_refuse_a_wrong_list() {
        let mut timer = StageTimer::new();
        run(&mut timer, &whole_build()).expect("in order");
        let stage_times = timer.complete().expect("complete");
        let json = serde_json::to_string(&stage_times).expect("serialises");
        assert!(json.contains("\"stage\":\"resolve\""), "{json}");
        let back: StageTimes = serde_json::from_str(&json).expect("parses");
        assert_eq!(back, stage_times);
        let short = r#"[{"stage":"discover","items":1,"duration_ns":2}]"#;
        assert!(serde_json::from_str::<StageTimes>(short).is_err());
    }

    #[test]
    fn the_monotonic_clock_never_goes_back() {
        let clock = MonotonicClock::start();
        let a = clock.now_ns();
        let b = clock.now_ns();
        assert!(b >= a);
    }
}
