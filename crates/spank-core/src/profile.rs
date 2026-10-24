use crate::config::ConfigError;
use crate::event::EventKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(transparent)]
pub struct SoundId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(transparent)]
pub struct ProfileId(pub String);

impl From<&str> for ProfileId {
    fn from(s: &str) -> Self {
        ProfileId(s.to_owned())
    }
}

impl From<&str> for SoundId {
    fn from(s: &str) -> Self {
        SoundId(s.to_owned())
    }
}

/// Sounds bound to one event. One is picked at random on each trigger.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(default)]
pub struct EventBinding {
    pub enabled: bool,
    pub sounds: Vec<SoundId>,
}

impl Default for EventBinding {
    fn default() -> Self {
        Self {
            enabled: true,
            sounds: Vec::new(),
        }
    }
}

impl EventBinding {
    /// True when this binding would actually play something.
    pub fn is_playable(&self) -> bool {
        self.enabled && !self.sounds.is_empty()
    }
}

/// A full event → sounds mapping (ADR-0007).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Profile {
    pub id: ProfileId,
    pub name: String,
    /// Built-in profiles ship with the app and are read-only.
    #[serde(default)]
    pub builtin: bool,
    /// Profile volume multiplier, 0.0–1.0.
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default)]
    pub mapping: BTreeMap<EventKind, EventBinding>,
}

fn default_volume() -> f32 {
    1.0
}

impl Profile {
    /// New empty profile with every event enabled and no sounds.
    pub fn new(id: impl Into<ProfileId>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            builtin: false,
            volume: 1.0,
            mapping: EventKind::ALL
                .iter()
                .map(|&k| (k, EventBinding::default()))
                .collect(),
        }
    }

    /// The binding for `kind`, if it would play something.
    pub fn playable(&self, kind: EventKind) -> Option<&EventBinding> {
        self.mapping.get(&kind).filter(|b| b.is_playable())
    }

    /// Writable copy of a (possibly built-in) profile.
    pub fn duplicate(&self, id: impl Into<ProfileId>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            builtin: false,
            ..self.clone()
        }
    }

    /// Every sound referenced by this profile, deduplicated, in stable order.
    pub fn referenced_sounds(&self) -> Vec<&SoundId> {
        let mut v: Vec<&SoundId> = self.mapping.values().flat_map(|b| b.sounds.iter()).collect();
        v.sort();
        v.dedup();
        v
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.id.0.trim().is_empty() {
            return Err(ConfigError::invalid("profile id must not be empty"));
        }
        if self.name.trim().is_empty() {
            return Err(ConfigError::invalid("profile name must not be empty"));
        }
        if !(0.0..=1.0).contains(&self.volume) {
            return Err(ConfigError::invalid(format!(
                "profile volume {} out of 0..=1",
                self.volume
            )));
        }
        Ok(())
    }
}

impl From<String> for ProfileId {
    fn from(s: String) -> Self {
        ProfileId(s)
    }
}

/// Picks a random sound per event, never the same one twice in a row when there is a choice
/// (ADR-0007).
#[derive(Debug, Clone)]
pub struct SoundPicker {
    rng: fastrand::Rng,
    last: BTreeMap<EventKind, SoundId>,
}

impl Default for SoundPicker {
    fn default() -> Self {
        Self::new()
    }
}

impl SoundPicker {
    pub fn new() -> Self {
        Self::with_rng(fastrand::Rng::new())
    }

    /// Deterministic picker for tests.
    pub fn with_seed(seed: u64) -> Self {
        Self::with_rng(fastrand::Rng::with_seed(seed))
    }

    fn with_rng(rng: fastrand::Rng) -> Self {
        Self {
            rng,
            last: BTreeMap::new(),
        }
    }

    /// A sound for `kind`, or `None` if the binding is disabled or empty.
    pub fn pick(&mut self, kind: EventKind, binding: &EventBinding) -> Option<SoundId> {
        if !binding.is_playable() {
            return None;
        }
        let last = self.last.get(&kind);
        let choices: Vec<&SoundId> = binding.sounds.iter().filter(|s| Some(*s) != last).collect();
        let pool = if choices.is_empty() {
            binding.sounds.iter().collect()
        } else {
            choices
        };
        let chosen = pool[self.rng.usize(..pool.len())].clone();
        self.last.insert(kind, chosen.clone());
        Some(chosen)
    }

    /// Forget the history (e.g. after switching profiles).
    pub fn reset(&mut self) {
        self.last.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Profile {
        let mut p = Profile::new("office", "辦公室安靜版");
        p.mapping.get_mut(&EventKind::LidOpened).unwrap().sounds = vec!["glass-01".into(), "glass-02".into()];
        p.mapping.get_mut(&EventKind::SlapHard).unwrap().sounds = vec!["glass-01".into()];
        p.mapping.get_mut(&EventKind::SlapLight).unwrap().enabled = false;
        p
    }

    #[test]
    fn new_profile_has_all_events_enabled_and_empty() {
        let p = Profile::new("x", "X");
        assert_eq!(p.mapping.len(), 7);
        assert!(p.mapping.values().all(|b| b.enabled && b.sounds.is_empty()));
        assert!(EventKind::ALL.iter().all(|&k| p.playable(k).is_none()));
    }

    #[test]
    fn playable_requires_enabled_and_sounds() {
        let mut p = sample();
        assert!(p.playable(EventKind::LidOpened).is_some());
        assert!(p.playable(EventKind::LidClosing).is_none()); // no sounds
        p.mapping.get_mut(&EventKind::SlapLight).unwrap().sounds = vec!["ow".into()];
        assert!(p.playable(EventKind::SlapLight).is_none()); // disabled
    }

    #[test]
    fn duplicate_is_writable() {
        let mut p = sample();
        p.builtin = true;
        let d = p.duplicate("copy", "Copy");
        assert!(!d.builtin);
        assert_eq!(d.mapping, p.mapping);
        assert_eq!(d.id, ProfileId::from("copy"));
    }

    #[test]
    fn referenced_sounds_dedupes() {
        let p = sample();
        let ids: Vec<_> = p.referenced_sounds().into_iter().map(|s| s.0.as_str()).collect();
        assert_eq!(ids, vec!["glass-01", "glass-02"]);
    }

    #[test]
    fn validate_rejects_bad_values() {
        assert!(sample().validate().is_ok());
        let mut p = sample();
        p.volume = 1.5;
        assert!(p.validate().is_err());
        let mut p = sample();
        p.name = "  ".into();
        assert!(p.validate().is_err());
        let mut p = sample();
        p.id = "".into();
        assert!(p.validate().is_err());
    }

    #[test]
    fn picker_never_repeats_when_it_can() {
        let mut picker = SoundPicker::with_seed(7);
        let p = sample();
        let b = p.playable(EventKind::LidOpened).unwrap();
        let mut prev = None;
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..50 {
            let s = picker.pick(EventKind::LidOpened, b).unwrap();
            assert_ne!(Some(&s), prev.as_ref());
            seen.insert(s.clone());
            prev = Some(s);
        }
        assert_eq!(seen.len(), 2);
    }

    #[test]
    fn picker_single_sound_and_unplayable() {
        let mut picker = SoundPicker::default();
        let p = sample();
        let hard = &p.mapping[&EventKind::SlapHard];
        assert_eq!(picker.pick(EventKind::SlapHard, hard), Some("glass-01".into()));
        assert_eq!(picker.pick(EventKind::SlapHard, hard), Some("glass-01".into()));
        assert_eq!(
            picker.pick(EventKind::SlapLight, &p.mapping[&EventKind::SlapLight]),
            None
        );
        assert_eq!(
            picker.pick(EventKind::LidClosing, &p.mapping[&EventKind::LidClosing]),
            None
        );
        picker.reset();
        assert_eq!(
            SoundPicker::new().pick(EventKind::SlapHard, hard),
            Some("glass-01".into())
        );
    }

    #[test]
    fn serde_round_trip_and_defaults() {
        let p = sample();
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<Profile>(&json).unwrap(), p);

        let minimal: Profile = serde_json::from_str(r#"{"id":"m","name":"M"}"#).unwrap();
        assert_eq!(minimal.volume, 1.0);
        assert!(!minimal.builtin);
        assert!(minimal.mapping.is_empty());
    }
}
