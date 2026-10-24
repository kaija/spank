//! Profile management (T21, ADR-0007): built-in profiles (read-only, shipped in the app bundle)
//! plus user profiles in the store, and the active profile in `config.json`.

use crate::{Result, Store, StoreError, now_ms};
use spank_core::{AppConfig, Profile, ProfileId};
use std::path::Path;

/// Built-in profile ids start with this; user profiles may not use it.
pub const BUILTIN_PREFIX: &str = "builtin.";

#[derive(Debug, Clone, Default)]
pub struct Profiles {
    builtin: Vec<Profile>,
}

impl Profiles {
    pub fn new(builtin: Vec<Profile>) -> Self {
        let mut builtin: Vec<Profile> = builtin
            .into_iter()
            .map(|mut p| {
                p.builtin = true;
                p
            })
            .collect();
        builtin.sort_by(|a, b| a.id.cmp(&b.id));
        Self { builtin }
    }

    /// Load `*.json` built-in profiles from the bundle's resource directory. Files that fail to
    /// parse or validate are skipped and reported (a broken bundle must not stop the app).
    pub fn load_builtin(dir: &Path) -> (Self, Vec<String>) {
        let mut profiles = Vec::new();
        let mut errors = Vec::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) => return (Self::default(), vec![format!("{}: {e}", dir.display())]),
        };
        let mut paths: Vec<_> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        paths.sort();
        for path in paths {
            let parsed = std::fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|b| serde_json::from_slice::<Profile>(&b).map_err(|e| e.to_string()))
                .and_then(|p| {
                    p.validate().map_err(|e| e.to_string())?;
                    if !p.id.0.starts_with(BUILTIN_PREFIX) {
                        return Err(format!("built-in id must start with {BUILTIN_PREFIX:?}"));
                    }
                    Ok(p)
                });
            match parsed {
                Ok(p) => profiles.push(p),
                Err(e) => errors.push(format!("{}: {e}", path.display())),
            }
        }
        (Self::new(profiles), errors)
    }

    pub fn builtin(&self) -> &[Profile] {
        &self.builtin
    }

    fn builtin_by_id(&self, id: &ProfileId) -> Option<&Profile> {
        self.builtin.iter().find(|p| &p.id == id)
    }

    /// Built-in profiles first (by id), then user profiles (by name).
    pub fn list(&self, store: &Store) -> Result<Vec<Profile>> {
        let (user, _) = store.list_profiles()?;
        Ok(self.builtin.iter().cloned().chain(user).collect())
    }

    pub fn get(&self, store: &Store, id: &ProfileId) -> Result<Profile> {
        match self.builtin_by_id(id) {
            Some(p) => Ok(p.clone()),
            None if id.0.starts_with(BUILTIN_PREFIX) => {
                Err(StoreError::NotFound(format!("profile {}", id.0)))
            }
            None => store.load_profile(id),
        }
    }

    fn new_id() -> ProfileId {
        ProfileId(format!("user-{}-{:04x}", now_ms(), fastrand::u16(..)))
    }

    /// A new empty user profile, saved.
    pub fn create(&self, store: &Store, name: &str) -> Result<Profile> {
        let p = Profile::new(Self::new_id(), name.trim());
        self.save(store, &p)?;
        Ok(p)
    }

    /// Writable copy of any profile (built-in or user), saved.
    pub fn duplicate(&self, store: &Store, id: &ProfileId, name: &str) -> Result<Profile> {
        let p = self.get(store, id)?.duplicate(Self::new_id(), name.trim());
        self.save(store, &p)?;
        Ok(p)
    }

    /// Save a user profile. Built-in profiles (and ids in the built-in namespace) are read-only.
    pub fn save(&self, store: &Store, p: &Profile) -> Result<()> {
        if p.builtin || p.id.0.starts_with(BUILTIN_PREFIX) {
            return Err(StoreError::ReadOnly(format!("profile {}", p.id.0)));
        }
        store.save_profile(p)
    }

    pub fn rename(&self, store: &Store, id: &ProfileId, name: &str) -> Result<Profile> {
        if self.builtin_by_id(id).is_some() {
            return Err(StoreError::ReadOnly(format!("profile {}", id.0)));
        }
        let mut p = store.load_profile(id)?;
        p.name = name.trim().to_owned();
        self.save(store, &p)?;
        Ok(p)
    }

    /// Delete a user profile. If it was active, the default built-in becomes active.
    pub fn delete(&self, store: &Store, cfg: &mut AppConfig, id: &ProfileId) -> Result<()> {
        if self.builtin_by_id(id).is_some() || id.0.starts_with(BUILTIN_PREFIX) {
            return Err(StoreError::ReadOnly(format!("profile {}", id.0)));
        }
        store.delete_profile(id)?;
        if &cfg.active_profile == id {
            cfg.active_profile = self.fallback_id();
            store.save_config(cfg)?;
        }
        Ok(())
    }

    pub fn set_active(&self, store: &Store, cfg: &mut AppConfig, id: &ProfileId) -> Result<Profile> {
        let p = self.get(store, id)?;
        cfg.active_profile = id.clone();
        store.save_config(cfg)?;
        Ok(p)
    }

    /// The active profile. A missing or broken one falls back to the default built-in (or the
    /// first built-in, or an empty profile when the bundle has none).
    pub fn active(&self, store: &Store, cfg: &AppConfig) -> Profile {
        self.get(store, &cfg.active_profile).unwrap_or_else(|_| {
            self.builtin_by_id(&self.fallback_id())
                .cloned()
                .unwrap_or_else(|| Profile::new(AppConfig::DEFAULT_PROFILE, "Default"))
        })
    }

    fn fallback_id(&self) -> ProfileId {
        let default = ProfileId::from(AppConfig::DEFAULT_PROFILE);
        if self.builtin_by_id(&default).is_some() {
            default
        } else {
            self.builtin.first().map(|p| p.id.clone()).unwrap_or(default)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spank_core::EventKind;

    fn setup() -> (tempfile::TempDir, Store, Profiles) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("root")).unwrap();
        let mut destruction = Profile::new("builtin.destruction", "毀滅");
        destruction.mapping.get_mut(&EventKind::LidOpened).unwrap().sounds = vec!["glass".into()];
        let classic = Profile::new("builtin.classic", "經典");
        (dir, store, Profiles::new(vec![destruction, classic]))
    }

    #[test]
    fn builtin_profiles_are_read_only() {
        let (_d, s, lib) = setup();
        let b = lib.get(&s, &"builtin.destruction".into()).unwrap();
        assert!(b.builtin);
        assert!(matches!(lib.save(&s, &b), Err(StoreError::ReadOnly(_))));
        let mut sneaky = b.clone();
        sneaky.builtin = false;
        assert!(matches!(lib.save(&s, &sneaky), Err(StoreError::ReadOnly(_))));
        assert!(matches!(lib.rename(&s, &b.id, "x"), Err(StoreError::ReadOnly(_))));
        let mut cfg = AppConfig::default();
        assert!(matches!(
            lib.delete(&s, &mut cfg, &b.id),
            Err(StoreError::ReadOnly(_))
        ));
        assert!(matches!(
            lib.get(&s, &"builtin.nope".into()),
            Err(StoreError::NotFound(_))
        ));
    }

    #[test]
    fn crud_and_listing_order() {
        let (_d, s, lib) = setup();
        let a = lib.create(&s, " 辦公室 ").unwrap();
        assert_eq!(a.name, "辦公室");
        assert!(a.id.0.starts_with("user-"));
        let copy = lib
            .duplicate(&s, &"builtin.destruction".into(), "我的毀滅")
            .unwrap();
        assert!(!copy.builtin);
        assert_eq!(copy.mapping[&EventKind::LidOpened].sounds, vec!["glass".into()]);
        assert_ne!(copy.id, a.id);

        let ids: Vec<_> = lib.list(&s).unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(ids, vec!["經典", "毀滅", "我的毀滅", "辦公室"]);

        let renamed = lib.rename(&s, &a.id, "Home").unwrap();
        assert_eq!(lib.get(&s, &a.id).unwrap().name, "Home");
        assert_eq!(renamed.name, "Home");
        assert!(lib.create(&s, "  ").is_err(), "empty name fails validation");
    }

    #[test]
    fn active_profile_and_delete_fallback() {
        let (_d, s, lib) = setup();
        let mut cfg = AppConfig::default();
        assert_eq!(lib.active(&s, &cfg).id.0, "builtin.destruction");

        let p = lib.create(&s, "Mine").unwrap();
        lib.set_active(&s, &mut cfg, &p.id).unwrap();
        assert_eq!(s.load_config().unwrap().value.active_profile, p.id);
        assert_eq!(lib.active(&s, &cfg).id, p.id);

        lib.delete(&s, &mut cfg, &p.id).unwrap();
        assert_eq!(cfg.active_profile.0, "builtin.destruction");
        assert_eq!(
            s.load_config().unwrap().value.active_profile.0,
            "builtin.destruction"
        );
        assert!(matches!(lib.get(&s, &p.id), Err(StoreError::NotFound(_))));
        assert!(lib.set_active(&s, &mut cfg, &"user-missing".into()).is_err());

        // Missing active profile → default; no built-ins at all → empty default.
        cfg.active_profile = "user-gone".into();
        assert_eq!(lib.active(&s, &cfg).id.0, "builtin.destruction");
        let empty = Profiles::default();
        assert_eq!(empty.active(&s, &cfg).id.0, AppConfig::DEFAULT_PROFILE);
        let only_classic = Profiles::new(vec![Profile::new("builtin.classic", "C")]);
        assert_eq!(only_classic.active(&s, &cfg).id.0, "builtin.classic");
    }

    #[test]
    fn load_builtin_from_dir() {
        let dir = tempfile::tempdir().unwrap();
        let write = |n: &str, body: &str| std::fs::write(dir.path().join(n), body).unwrap();
        write("a.json", r#"{"id":"builtin.a","name":"A"}"#);
        write("b.json", r#"{"id":"user-b","name":"B"}"#);
        write("c.json", "not json");
        write("d.json", r#"{"id":"builtin.d","name":"D","volume":3}"#);
        write("readme.txt", "ignored");
        let (lib, errors) = Profiles::load_builtin(dir.path());
        assert_eq!(lib.builtin().len(), 1);
        assert!(lib.builtin()[0].builtin);
        assert_eq!(errors.len(), 3, "{errors:?}");
        let (lib, errors) = Profiles::load_builtin(&dir.path().join("missing"));
        assert!(lib.builtin().is_empty());
        assert_eq!(errors.len(), 1);
    }
}
