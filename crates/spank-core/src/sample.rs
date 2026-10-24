//! Raw sensor samples (ADR-0010). Timestamps are monotonic milliseconds on a source-defined epoch;
//! every input fed to one [`crate::Orchestrator`] must share that epoch.

use serde::{Deserialize, Serialize};

/// One polled lid angle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LidSample {
    pub at_ms: u64,
    pub angle_deg: f32,
}

/// One accelerometer report, in g, gravity included.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AccelSample {
    pub at_ms: u64,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl AccelSample {
    pub fn magnitude(&self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
}

/// Whether a sensor can be used on this machine (ADR-0014 capability detection).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Capability {
    /// Not checked yet.
    Unknown,
    Available,
    Unsupported {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SensorCaps {
    pub lid: Capability,
    pub accel: Capability,
}

impl Default for SensorCaps {
    fn default() -> Self {
        Self {
            lid: Capability::Unknown,
            accel: Capability::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magnitude() {
        let s = AccelSample {
            at_ms: 0,
            x: 3.0,
            y: 0.0,
            z: 4.0,
        };
        assert_eq!(s.magnitude(), 5.0);
    }

    #[test]
    fn capability_serde() {
        let c = SensorCaps {
            lid: Capability::Available,
            accel: Capability::Unsupported {
                reason: "no driver".into(),
            },
        };
        let j = serde_json::to_string(&c).unwrap();
        assert!(
            j.contains(r#"{"state":"unsupported","reason":"no driver"}"#),
            "{j}"
        );
        assert_eq!(serde_json::from_str::<SensorCaps>(&j).unwrap(), c);
        assert_eq!(SensorCaps::default().lid, Capability::Unknown);
    }
}
