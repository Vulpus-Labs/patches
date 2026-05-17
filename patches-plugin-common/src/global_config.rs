//! Host-scoped global configuration (ADR 0075, ticket 0896).
//!
//! v1 is a TOML file shared by both hosts (`patch_player` and the CLAP
//! plugin). The schema currently carries only the list of directories
//! the scanner should consult for `.pxm` bundles, but is designed to
//! grow into other host-scoped preferences (default device, default
//! oversampling, monitor tab default, etc.) without breaking
//! backwards-compatibility — readers refuse on `schema_version`
//! mismatch rather than silently misinterpreting fields.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Resolve the OS-native location of `settings.toml` via the
/// `directories` crate. Returns `None` when no home directory can be
/// determined — extremely rare; callers fall back to in-memory defaults
/// rather than crashing.
pub fn default_global_config_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "Patches")
        .map(|p| p.config_dir().join("settings.toml"))
}

/// Schema version embedded in every `settings.toml`. Bump when the
/// shape of [`GlobalConfig`] changes incompatibly so older readers can
/// refuse to load rather than silently misinterpret bytes.
pub const GLOBAL_CONFIG_SCHEMA_VERSION: u32 = 1;

/// Host-scoped settings serialised at the OS-native config location.
/// See ADR 0075 for the resolution table and the rationale for using
/// the `directories` crate to pick a path per OS.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalConfig {
    /// Schema version. Mismatches surface a clear error from
    /// [`GlobalConfig::from_toml_str`] rather than panicking.
    pub schema_version: u32,
    /// Directories scanned for `.pxm` bundles, in priority order. An
    /// empty list falls through to the scanner's default tier.
    #[serde(default)]
    pub bundle_dirs: Vec<PathBuf>,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            schema_version: GLOBAL_CONFIG_SCHEMA_VERSION,
            bundle_dirs: Vec::new(),
        }
    }
}

impl GlobalConfig {
    /// Parse `settings.toml` text, rejecting unknown schema versions.
    pub fn from_toml_str(s: &str) -> Result<Self, GlobalConfigError> {
        let cfg: GlobalConfig = toml::from_str(s).map_err(GlobalConfigError::Parse)?;
        if cfg.schema_version != GLOBAL_CONFIG_SCHEMA_VERSION {
            return Err(GlobalConfigError::SchemaVersion {
                found: cfg.schema_version,
                expected: GLOBAL_CONFIG_SCHEMA_VERSION,
            });
        }
        Ok(cfg)
    }

    /// Serialise to `settings.toml` text.
    pub fn to_toml_string(&self) -> Result<String, GlobalConfigError> {
        toml::to_string_pretty(self).map_err(GlobalConfigError::Serialize)
    }
}

/// Errors surfaced by [`GlobalConfig::from_toml_str`] /
/// [`GlobalConfig::to_toml_string`]. Mapped to `io::Error` by the env
/// trait wrappers so callers see a single error type.
#[derive(Debug)]
pub enum GlobalConfigError {
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
    SchemaVersion { found: u32, expected: u32 },
}

impl std::fmt::Display for GlobalConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "global config TOML parse: {e}"),
            Self::Serialize(e) => write!(f, "global config TOML serialize: {e}"),
            Self::SchemaVersion { found, expected } => write!(
                f,
                "global config schema v{found} (expected v{expected})"
            ),
        }
    }
}

impl std::error::Error for GlobalConfigError {}

impl From<GlobalConfigError> for std::io::Error {
    fn from(e: GlobalConfigError) -> Self {
        std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_empty() {
        let cfg = GlobalConfig::default();
        let s = cfg.to_toml_string().unwrap();
        let back = GlobalConfig::from_toml_str(&s).unwrap();
        assert_eq!(cfg, back);
        assert!(back.bundle_dirs.is_empty());
    }

    #[test]
    fn round_trip_one_path() {
        let cfg = GlobalConfig {
            schema_version: GLOBAL_CONFIG_SCHEMA_VERSION,
            bundle_dirs: vec![PathBuf::from("/home/me/audio/patches-bundles")],
        };
        let s = cfg.to_toml_string().unwrap();
        let back = GlobalConfig::from_toml_str(&s).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn round_trip_many_paths() {
        let cfg = GlobalConfig {
            schema_version: GLOBAL_CONFIG_SCHEMA_VERSION,
            bundle_dirs: vec![
                PathBuf::from("/a"),
                PathBuf::from("/b"),
                PathBuf::from("/c"),
            ],
        };
        let s = cfg.to_toml_string().unwrap();
        let back = GlobalConfig::from_toml_str(&s).unwrap();
        assert_eq!(cfg.bundle_dirs, back.bundle_dirs);
    }

    #[test]
    fn schema_version_mismatch_rejected() {
        let s = "schema_version = 999\nbundle_dirs = []\n";
        let err = GlobalConfig::from_toml_str(s).unwrap_err();
        match err {
            GlobalConfigError::SchemaVersion { found, expected } => {
                assert_eq!(found, 999);
                assert_eq!(expected, GLOBAL_CONFIG_SCHEMA_VERSION);
            }
            other => panic!("expected SchemaVersion, got {other:?}"),
        }
    }

    #[test]
    fn missing_bundle_dirs_defaults_to_empty() {
        let s = format!("schema_version = {GLOBAL_CONFIG_SCHEMA_VERSION}\n");
        let cfg = GlobalConfig::from_toml_str(&s).unwrap();
        assert!(cfg.bundle_dirs.is_empty());
    }
}
