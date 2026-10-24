use serde::{Deserialize, Serialize};

/// Everything that can trigger a sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// System woke with the lid open (ADR-0005).
    LidOpened,
    /// Lid moved down past the close threshold while awake.
    LidClosing,
    /// Lid opened past the overbend threshold.
    LidOverbent,
    SlapLight,
    SlapMedium,
    SlapHard,
    /// Several slaps inside the combo window (ADR-0006).
    SlapCombo,
}

impl EventKind {
    pub const ALL: [EventKind; 7] = [
        EventKind::LidOpened,
        EventKind::LidClosing,
        EventKind::LidOverbent,
        EventKind::SlapLight,
        EventKind::SlapMedium,
        EventKind::SlapHard,
        EventKind::SlapCombo,
    ];

    pub fn is_slap(self) -> bool {
        matches!(
            self,
            Self::SlapLight | Self::SlapMedium | Self::SlapHard | Self::SlapCombo
        )
    }

    pub fn is_lid(self) -> bool {
        !self.is_slap()
    }

    /// Playback priority (ADR-0008): Combo > Hard > Overbent > Medium > Opened > Closing > Light.
    pub fn priority(self) -> Priority {
        Priority(match self {
            Self::SlapCombo => 7,
            Self::SlapHard => 6,
            Self::LidOverbent => 5,
            Self::SlapMedium => 4,
            Self::LidOpened => 3,
            Self::LidClosing => 2,
            Self::SlapLight => 1,
        })
    }
}

/// Higher wins. A new sound interrupts the current one only if its priority is strictly higher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Priority(pub u8);

impl Priority {
    pub fn preempts(self, current: Priority) -> bool {
        self > current
    }
}

/// Extra data that came with an event.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventMeta {
    None,
    Lid { angle_deg: f32 },
    Slap { peak_g: f32 },
    Combo { count: u8, peak_g: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TriggerEvent {
    pub kind: EventKind,
    /// Monotonic timestamp in milliseconds (source-defined epoch).
    pub at_ms: u64,
    pub meta: EventMeta,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_uses_snake_case() {
        assert_eq!(
            serde_json::to_string(&EventKind::LidOverbent).unwrap(),
            "\"lid_overbent\""
        );
        let k: EventKind = serde_json::from_str("\"slap_combo\"").unwrap();
        assert_eq!(k, EventKind::SlapCombo);
    }

    #[test]
    fn priority_order_matches_adr_0008() {
        let order = [
            EventKind::SlapCombo,
            EventKind::SlapHard,
            EventKind::LidOverbent,
            EventKind::SlapMedium,
            EventKind::LidOpened,
            EventKind::LidClosing,
            EventKind::SlapLight,
        ];
        for pair in order.windows(2) {
            assert!(
                pair[0].priority().preempts(pair[1].priority()),
                "{:?} should beat {:?}",
                pair[0],
                pair[1]
            );
        }
        assert!(
            !EventKind::SlapHard
                .priority()
                .preempts(EventKind::SlapHard.priority())
        );
    }

    #[test]
    fn all_contains_each_kind_once() {
        let mut v = EventKind::ALL.to_vec();
        v.sort();
        v.dedup();
        assert_eq!(v.len(), 7);
        assert_eq!(EventKind::ALL.iter().filter(|k| k.is_slap()).count(), 4);
        assert_eq!(EventKind::ALL.iter().filter(|k| k.is_lid()).count(), 3);
    }

    #[test]
    fn trigger_event_round_trip() {
        let e = TriggerEvent {
            kind: EventKind::SlapCombo,
            at_ms: 1234,
            meta: EventMeta::Combo {
                count: 3,
                peak_g: 0.4,
            },
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"type\":\"combo\""));
        assert_eq!(serde_json::from_str::<TriggerEvent>(&json).unwrap(), e);
    }
}
