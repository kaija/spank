//! `manifest.json` schema v1 (ADR-0009).
//!
//! Event names follow the ADR example (`"LidOpened"`); snake_case (`"lid_opened"`, as in
//! config files) is accepted too.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use spank_core::EventKind;
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: u32 = 1;

/// Event key as written in packs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackEvent(pub EventKind);

pub fn pascal_name(k: EventKind) -> &'static str {
    match k {
        EventKind::LidOpened => "LidOpened",
        EventKind::LidClosing => "LidClosing",
        EventKind::LidOverbent => "LidOverbent",
        EventKind::SlapLight => "SlapLight",
        EventKind::SlapMedium => "SlapMedium",
        EventKind::SlapHard => "SlapHard",
        EventKind::SlapCombo => "SlapCombo",
    }
}

impl Serialize for PackEvent {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(pascal_name(self.0))
    }
}

impl<'de> Deserialize<'de> for PackEvent {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        EventKind::ALL
            .into_iter()
            .find(|&k| pascal_name(k) == s)
            .or_else(|| serde_json::from_value(serde_json::Value::String(s.clone())).ok())
            .map(PackEvent)
            .ok_or_else(|| D::Error::custom(format!("unknown event {s:?}")))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestSound {
    /// Path inside the zip, e.g. `sounds/shatter1.wav`.
    pub file: String,
    /// Hex sha256 of the file.
    pub sha256: String,
    pub duration_ms: u32,
    #[serde(default)]
    pub suggested_events: Vec<PackEvent>,
    /// SPDX identifier, overrides the pack license.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    /// Reverse-DNS style, e.g. `com.example.glass-pack`.
    pub id: String,
    /// Localized names: `{"en": "...", "zh-Hant": "..."}`.
    pub name: BTreeMap<String, String>,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// SPDX identifier.
    pub license: String,
    pub sounds: Vec<ManifestSound>,
    /// Optional event → files mapping; importing creates a profile from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<BTreeMap<PackEvent, Vec<String>>>,
}

impl Manifest {
    /// Name for display: zh-Hant, then en, then any, then the id.
    pub fn display_name(&self) -> &str {
        ["zh-Hant", "en"]
            .iter()
            .find_map(|l| self.name.get(*l))
            .or_else(|| self.name.values().next())
            .map(String::as_str)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADR_EXAMPLE: &str = r#"{
      "schema_version": 1,
      "id": "com.example.glass-pack",
      "name": { "en": "Glass Pack", "zh-Hant": "玻璃包" },
      "version": "1.0.0",
      "author": "someone",
      "license": "CC0-1.0",
      "sounds": [
        { "file": "sounds/shatter1.wav", "sha256": "ab", "duration_ms": 1200,
          "suggested_events": ["LidOpened", "slap_hard"], "license": "CC0-1.0", "source_url": "https://x" }
      ],
      "profile": { "LidOpened": ["sounds/shatter1.wav"] }
    }"#;

    #[test]
    fn parses_adr_example() {
        let m: Manifest = serde_json::from_str(ADR_EXAMPLE).unwrap();
        assert_eq!(m.display_name(), "玻璃包");
        assert_eq!(
            m.sounds[0].suggested_events,
            vec![PackEvent(EventKind::LidOpened), PackEvent(EventKind::SlapHard)]
        );
        assert!(
            m.profile
                .as_ref()
                .unwrap()
                .contains_key(&PackEvent(EventKind::LidOpened))
        );
        let back = serde_json::to_string(&m).unwrap();
        assert!(back.contains(r#""SlapHard""#));
        assert_eq!(serde_json::from_str::<Manifest>(&back).unwrap(), m);
    }

    #[test]
    fn unknown_event_and_names() {
        assert!(serde_json::from_str::<PackEvent>(r#""Explode""#).is_err());
        let mut m: Manifest = serde_json::from_str(ADR_EXAMPLE).unwrap();
        m.name.remove("zh-Hant");
        assert_eq!(m.display_name(), "Glass Pack");
        m.name = BTreeMap::from([("ja".into(), "ガラス".into())]);
        assert_eq!(m.display_name(), "ガラス");
        m.name.clear();
        assert_eq!(m.display_name(), "com.example.glass-pack");
        for k in EventKind::ALL {
            let j = serde_json::to_string(&PackEvent(k)).unwrap();
            assert_eq!(serde_json::from_str::<PackEvent>(&j).unwrap().0, k);
        }
    }
}
