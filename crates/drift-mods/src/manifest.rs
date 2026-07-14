//! Mod manifest — `manifest.toml` at the root of each mod directory.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::LoadError;

/// Parsed `manifest.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Unique mod id (namespace prefix for this mod's content ids, by convention).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Version string (unvalidated in M1; semver enforcement is future work).
    pub version: String,
    /// Ids of mods that must load before this one.
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Content ids this mod intentionally replaces. A cross-mod id collision is
    /// an error unless the overriding mod lists the id here — making every
    /// override a deliberate, auditable act.
    #[serde(default)]
    pub overrides: Vec<String>,
}

impl Manifest {
    pub fn from_path(path: &Path) -> Result<Self, LoadError> {
        let text = std::fs::read_to_string(path).map_err(|source| LoadError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        toml::from_str(&text).map_err(|source| LoadError::ManifestParse {
            path: path.to_path_buf(),
            source,
        })
    }

    pub fn overrides(&self, id: &str) -> bool {
        self.overrides.iter().any(|o| o == id)
    }
}

/// A discovered mod directory: its manifest plus where it lives on disk.
#[derive(Debug, Clone)]
pub struct ModDir {
    pub manifest: Manifest,
    pub dir: PathBuf,
}
