//! App-wide state shared by Tauri commands, the tray and the engine.
//!
//! `config` is the source of truth for settings: a command changes it, saves `config.json`, sends
//! the new copy to the engine and announces it with the `config_changed` event. Profiles and sounds live in the store; built-ins come from the
//! bundled `.spankpack` files.

use crate::engine::{Engine, EngineCmd, Resolver, SoundData};
use spank_core::{AppConfig, Profile, ProfileId, SoundEntry, SoundId, SoundSource};
use spank_pack::BuiltinSet;
use spank_store::{Profiles, Store, StoreError};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

pub const CONFIG_CHANGED: &str = "config_changed";

pub struct AppState {
    pub app: AppHandle,
    pub store: Store,
    pub builtin: Arc<BuiltinSet>,
    pub profiles: Profiles,
    pub config: Mutex<AppConfig>,
    pub engine: Engine,
    pub sysmon: Mutex<Option<spank_sysmon::SysMonitor>>,
}

pub type CmdResult<T> = Result<T, String>;

pub fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl AppState {
    /// Resolver for the engine: built-in audio from memory, user audio from the store.
    pub fn resolver(store_sounds: std::path::PathBuf, builtin: Arc<BuiltinSet>) -> Resolver {
        Arc::new(move |id: &SoundId| {
            if let Some(b) = builtin.audio.get(id) {
                return Some(SoundData::Bytes(b.clone()));
            }
            spank_store::validate_id(&id.0).ok()?;
            Some(SoundData::File(store_sounds.join(format!("{}.wav", id.0))))
        })
    }

    pub fn config(&self) -> AppConfig {
        self.config.lock().map(|c| c.clone()).unwrap_or_default()
    }

    /// Change the config, save it and hand it to the engine.
    pub fn update_config(&self, f: impl FnOnce(&mut AppConfig)) -> CmdResult<AppConfig> {
        let mut guard = self.config.lock().map_err(err)?;
        let mut next = guard.clone();
        f(&mut next);
        crate::edition::restrict(&mut next);
        next.validate().map_err(err)?;
        self.store.save_config(&next).map_err(err)?;
        *guard = next.clone();
        drop(guard);
        self.engine.send(EngineCmd::Config(next.clone()));
        let _ = self.app.emit(CONFIG_CHANGED, &next);
        crate::tray::refresh(&self.app);
        Ok(next)
    }

    pub fn active_profile(&self) -> Profile {
        self.profiles.active(&self.store, &self.config())
    }

    /// Tell the engine to reload the active profile (after any profile change).
    pub fn refresh_active(&self) {
        self.engine.send(EngineCmd::Profile(self.active_profile()));
    }

    /// Built-in sounds followed by the user's library.
    pub fn sounds(&self) -> CmdResult<Vec<SoundEntry>> {
        let user = self.store.load_sound_index().map_err(err)?.value.sounds;
        Ok(self.builtin.sounds.iter().cloned().chain(user).collect())
    }

    /// Remove a deleted sound from every user profile that used it. Returns the changed profiles.
    pub fn unlink_sound(&self, id: &SoundId) -> Result<Vec<ProfileId>, StoreError> {
        let (profiles, _) = self.store.list_profiles()?;
        let mut changed = Vec::new();
        for mut p in profiles {
            let mut touched = false;
            for b in p.mapping.values_mut() {
                let before = b.sounds.len();
                b.sounds.retain(|s| s != id);
                touched |= b.sounds.len() != before;
            }
            if touched {
                self.profiles.save(&self.store, &p)?;
                changed.push(p.id);
            }
        }
        Ok(changed)
    }

    pub fn is_builtin_sound(&self, id: &SoundId) -> bool {
        self.builtin
            .sounds
            .iter()
            .any(|s| &s.id == id && s.source == SoundSource::Builtin)
    }
}
