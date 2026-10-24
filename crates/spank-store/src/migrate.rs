//! Schema migrations for `config.json`.
//!
//! Each step upgrades the raw JSON from version N to N+1. To add one: bump
//! `AppConfig::SCHEMA_VERSION`, append a function to [`CONFIG_STEPS`], and add a test with a real
//! old file.

use serde_json::Value;
use spank_core::AppConfig;

pub type Step = fn(Value) -> Result<Value, String>;

/// `CONFIG_STEPS[i]` upgrades version `i + 1` to `i + 2`. Empty while we are on v1.
pub const CONFIG_STEPS: &[Step] = &[];

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum MigrateError {
    #[error("config root must be a JSON object")]
    NotAnObject,
    #[error("config schema_version {0} is newer than this app supports ({1})")]
    TooNew(u64, u32),
    #[error("config schema_version {0} is invalid")]
    Invalid(u64),
    #[error("migration from v{from} failed: {reason}")]
    Step { from: u64, reason: String },
}

pub fn migrate_config(v: Value) -> Result<Value, MigrateError> {
    migrate(v, AppConfig::SCHEMA_VERSION, CONFIG_STEPS)
}

/// Run `steps` until `schema_version == target`. A missing version is treated as 1.
pub fn migrate(mut v: Value, target: u32, steps: &[Step]) -> Result<Value, MigrateError> {
    let mut version = match v.as_object() {
        None => return Err(MigrateError::NotAnObject),
        Some(o) => o.get("schema_version").and_then(Value::as_u64).unwrap_or(1),
    };
    if version == 0 {
        return Err(MigrateError::Invalid(version));
    }
    if version > target as u64 {
        return Err(MigrateError::TooNew(version, target));
    }
    while version < target as u64 {
        let step = steps
            .get((version - 1) as usize)
            .ok_or(MigrateError::Invalid(version))?;
        v = step(v).map_err(|reason| MigrateError::Step {
            from: version,
            reason,
        })?;
        version += 1;
        if let Some(o) = v.as_object_mut() {
            o.insert("schema_version".into(), version.into());
        }
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rename_volume(mut v: Value) -> Result<Value, String> {
        let o = v.as_object_mut().ok_or("not an object")?;
        if let Some(vol) = o.remove("volume") {
            o.insert("app_volume".into(), vol);
        }
        Ok(v)
    }

    fn fail(_: Value) -> Result<Value, String> {
        Err("boom".into())
    }

    #[test]
    fn current_version_passes_through() {
        let v = json!({"schema_version": 1, "app_volume": 0.5});
        assert_eq!(migrate_config(v.clone()).unwrap(), v);
    }

    #[test]
    fn missing_version_is_v1() {
        let v = json!({"app_volume": 0.5});
        assert_eq!(migrate_config(v.clone()).unwrap(), v);
    }

    #[test]
    fn runs_steps_in_order_and_bumps_version() {
        let v = json!({"schema_version": 1, "volume": 0.3});
        let out = migrate(v, 2, &[rename_volume]).unwrap();
        assert_eq!(out, json!({"schema_version": 2, "app_volume": 0.3}));
    }

    #[test]
    fn rejects_bad_inputs() {
        assert_eq!(migrate_config(json!([1])), Err(MigrateError::NotAnObject));
        assert_eq!(
            migrate_config(json!({"schema_version": 0})),
            Err(MigrateError::Invalid(0))
        );
        assert_eq!(
            migrate_config(json!({"schema_version": 9})),
            Err(MigrateError::TooNew(9, 1))
        );
        assert_eq!(
            migrate(json!({"schema_version": 1}), 3, &[rename_volume]),
            Err(MigrateError::Invalid(2))
        );
        assert!(matches!(
            migrate(json!({}), 2, &[fail]),
            Err(MigrateError::Step { from: 1, .. })
        ));
    }
}
