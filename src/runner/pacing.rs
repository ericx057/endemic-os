use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Pacing {
    pub start_spread_ms: u64,
    pub think_min_ms: u64,
    pub think_max_ms: u64,
    pub sync_sessions: bool,
}

impl Pacing {
    pub fn validate(&self, hold: Duration) -> Result<(), String> {
        let hold_ms = hold.as_millis();
        let fits = u128::from(self.start_spread_ms) < hold_ms
            && self.think_min_ms <= self.think_max_ms
            && u128::from(self.think_max_ms) < hold_ms;
        fits.then_some(()).ok_or(format!(
            "pacing needs start_spread_ms and think_max_ms below the {hold_ms} ms stage hold, and think_min_ms <= think_max_ms"
        ))
    }

    pub(super) fn start_offset(&self, agent: usize) -> Duration {
        random_ms(agent, 0, 0, self.start_spread_ms.saturating_sub(1))
    }

    pub(super) fn think(&self, agent: usize, round: u64) -> Duration {
        random_ms(agent, round + 1, self.think_min_ms, self.think_max_ms)
    }
}

fn random_ms(agent: usize, round: u64, low: u64, high: u64) -> Duration {
    if high <= low {
        return Duration::from_millis(low);
    }
    let mut z = (agent as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ round.wrapping_mul(0xD1B5_4A32_D192_ED03);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    Duration::from_millis(low + (z ^ (z >> 31)) % (high - low + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pacing(start_spread_ms: u64, think_min_ms: u64, think_max_ms: u64) -> Pacing {
        Pacing {
            start_spread_ms,
            think_min_ms,
            think_max_ms,
            sync_sessions: false,
        }
    }

    #[test]
    fn zero_pacing_never_waits() {
        assert_eq!(Pacing::default().start_offset(7), Duration::ZERO);
        assert_eq!(Pacing::default().think(7, 3), Duration::ZERO);
    }

    #[test]
    fn waits_stay_in_range_and_vary_across_agents() {
        let pacing = pacing(1000, 200, 400);
        let starts: Vec<_> = (0..64).map(|agent| pacing.start_offset(agent)).collect();
        assert!(
            starts
                .iter()
                .all(|start| *start < Duration::from_millis(1000))
        );
        assert!(starts.windows(2).any(|pair| pair[0] != pair[1]));
        assert!((0..64).all(|round| (200..=400).contains(&pacing.think(3, round).as_millis())));
    }

    #[test]
    fn rejects_waits_that_do_not_fit_the_hold() {
        let hold = Duration::from_secs(1);
        assert!(pacing(999, 0, 999).validate(hold).is_ok());
        assert!(pacing(1000, 0, 0).validate(hold).is_err());
        assert!(pacing(0, 300, 200).validate(hold).is_err());
        assert!(pacing(0, 0, 1000).validate(hold).is_err());
    }
}
