use super::{Failure, Metrics, pacing::mix};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering::Relaxed},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fault {
    pub tool: String,
    pub first_stage: usize,
    pub last_stage: usize,
    pub percent: u8,
    pub effect: Effect,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Effect {
    Latency { delay_ms: u64 },
    Error,
    Timeout,
}

impl Fault {
    pub fn check(&self) -> Result<(), String> {
        let valid = (1..=100).contains(&self.percent)
            && (1..=self.last_stage).contains(&self.first_stage)
            && self.effect != Effect::Latency { delay_ms: 0 };
        valid.then_some(()).ok_or_else(|| {
            "each fault needs percent from 1 to 100, 1 <= first_stage <= last_stage, and a positive delay_ms for latency".into()
        })
    }

    pub fn fits(&self, tools: &[String], timeout: Duration) -> Result<(), String> {
        if !tools.contains(&self.tool) {
            return Err(format!(
                "fault tool {:?} is not one of the target's tools",
                self.tool
            ));
        }
        match self.effect {
            Effect::Latency { delay_ms } if u128::from(delay_ms) >= timeout.as_millis() => {
                Err(format!(
                    "fault delay_ms on {:?} must be below the {} ms target timeout (TARGET_TIMEOUT_SECONDS)",
                    self.tool,
                    timeout.as_millis()
                ))
            }
            _ => Ok(()),
        }
    }

    fn active(&self, stage: usize) -> bool {
        (self.first_stage..=self.last_stage).contains(&stage)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageFaults {
    seed: u64,
    faults: Vec<Fault>,
}

impl StageFaults {
    pub fn new(run: u64, stage: usize, faults: &[Fault]) -> Self {
        Self {
            seed: mix(stage, run),
            faults: (faults.iter())
                .filter(|fault| fault.active(stage))
                .cloned()
                .collect(),
        }
    }

    pub fn faults(&self) -> &[Fault] {
        &self.faults
    }
}

pub(super) struct Injector {
    stage: StageFaults,
    hits: Box<[AtomicU64]>,
}

impl Injector {
    pub(super) fn new(stage: StageFaults) -> Self {
        let hits = stage.faults.iter().map(|_| AtomicU64::new(0)).collect();
        Self { stage, hits }
    }

    pub(super) fn draw(&self, tool: &str, call: u64) -> Option<Effect> {
        let roll = |index: usize| mix(index, self.stage.seed ^ call) % 100;
        let (index, fault) = (self.stage.faults.iter().enumerate())
            .find(|(index, fault)| fault.tool == tool && roll(*index) < u64::from(fault.percent))?;
        self.hits[index].fetch_add(1, Relaxed);
        Some(fault.effect)
    }

    pub(super) fn counts(&self) -> BTreeMap<String, Injected> {
        let mut counts = BTreeMap::<String, Injected>::new();
        for (fault, hits) in self.stage.faults.iter().zip(&self.hits) {
            let hits = hits.load(Relaxed);
            let tool = counts.entry(fault.tool.clone()).or_default();
            *tool = tool.merge(Injected::of(fault.effect, hits));
        }
        counts
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Injected {
    delayed: u64,
    errors: u64,
    timeouts: u64,
}

impl Injected {
    fn of(effect: Effect, hits: u64) -> Self {
        match effect {
            Effect::Latency { .. } => Self {
                delayed: hits,
                ..Self::default()
            },
            Effect::Error => Self {
                errors: hits,
                ..Self::default()
            },
            Effect::Timeout => Self {
                timeouts: hits,
                ..Self::default()
            },
        }
    }

    fn merge(self, other: Self) -> Self {
        Self {
            delayed: self.delayed + other.delayed,
            errors: self.errors + other.errors,
            timeouts: self.timeouts + other.timeouts,
        }
    }
}

pub struct SessionFaults {
    seed: u64,
    calls: u64,
    timeout: Duration,
}

impl SessionFaults {
    pub fn new(agent: usize, round: u64, timeout: Duration) -> Self {
        Self {
            seed: mix(agent, round),
            calls: 0,
            timeout,
        }
    }

    pub(crate) async fn inject<'n>(
        &mut self,
        metrics: &Metrics,
        tools: impl Iterator<Item = &'n str>,
    ) -> Result<(), Failure> {
        let mut delay = Duration::ZERO;
        let mut failed = false;
        for tool in tools {
            self.calls += 1;
            let call = mix(self.calls as usize, self.seed);
            match metrics.inject(tool, call) {
                Some(Effect::Latency { delay_ms }) => {
                    delay = delay.max(Duration::from_millis(delay_ms));
                }
                Some(Effect::Timeout) => {
                    delay = delay.max(self.timeout);
                    failed = true;
                }
                Some(Effect::Error) => failed = true,
                None => {}
            }
        }
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        if failed {
            return Err(Failure::Injected);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
