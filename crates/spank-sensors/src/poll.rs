//! Adaptive lid polling (ADR-0005): 5 Hz while still, 30 Hz from the first change until the
//! angle has been steady for 2 s.

pub const IDLE_INTERVAL_MS: u64 = 200;
pub const ACTIVE_INTERVAL_MS: u64 = 33;
pub const SETTLE_MS: u64 = 2000;

#[derive(Debug, Clone, Default)]
pub struct LidPollSchedule {
    last_angle: Option<f32>,
    last_change_ms: Option<u64>,
}

impl LidPollSchedule {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a sample and return how long to wait before the next poll.
    pub fn on_sample(&mut self, angle_deg: f32, at_ms: u64) -> u64 {
        if self.last_angle.is_some_and(|a| a != angle_deg) {
            self.last_change_ms = Some(at_ms);
        }
        self.last_angle = Some(angle_deg);
        if self.is_active(at_ms) {
            ACTIVE_INTERVAL_MS
        } else {
            IDLE_INTERVAL_MS
        }
    }

    pub fn is_active(&self, now_ms: u64) -> bool {
        self.last_change_ms
            .is_some_and(|t| now_ms.saturating_sub(t) < SETTLE_MS)
    }

    /// Forget history (after the sensor was stopped).
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds_up_on_change_and_settles() {
        let mut p = LidPollSchedule::new();
        assert_eq!(p.on_sample(100.0, 0), IDLE_INTERVAL_MS);
        assert_eq!(p.on_sample(100.0, 200), IDLE_INTERVAL_MS);
        assert_eq!(p.on_sample(98.0, 400), ACTIVE_INTERVAL_MS);
        assert_eq!(p.on_sample(98.0, 2000), ACTIVE_INTERVAL_MS);
        assert_eq!(p.on_sample(98.0, 2400), IDLE_INTERVAL_MS);
        assert!(!p.is_active(2400));
        p.on_sample(90.0, 3000);
        p.reset();
        assert_eq!(p.on_sample(80.0, 3100), IDLE_INTERVAL_MS);
    }
}
