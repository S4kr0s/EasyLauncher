//! The published formats: `hub.json` (a list of projects) and `pack.json`
//! (one version of one project). See docs/MANIFEST.md.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hub {
    pub schema: u32,
    pub name: String,
    #[serde(default)]
    pub projects: Vec<HubProject>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubProject {
    pub id: String,
    pub name: String,
    /// URL of the project's `pack.json`, relative to the hub URL or absolute.
    pub manifest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pack {
    pub schema: u32,
    pub id: String,
    pub name: String,
    /// Free-form version label; any change means "update available".
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changelog: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minecraft: Option<MinecraftSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub servers: Vec<Server>,
    #[serde(default)]
    pub sync: SyncRules,
    #[serde(default)]
    pub files: Vec<FileEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinecraftSpec {
    /// Exact version id, or `latest-release` / `latest-snapshot`.
    pub version: String,
    #[serde(default)]
    pub loader: Loader,
    /// Java major version override (default: what the version JSON asks for).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java: Option<u32>,
    /// Suggested RAM in MiB; the player can change it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mb: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jvm_args: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Loader {
    #[serde(rename = "type")]
    pub kind: LoaderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LoaderKind {
    #[default]
    Vanilla,
    Fabric,
    Quilt,
    Forge,
    Neoforge,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    pub name: String,
    /// `host` or `host:port`.
    pub address: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRules {
    /// Folders the pack fully controls; files there that the launcher did not
    /// install are handled by `extra_files`.
    #[serde(default = "default_managed_dirs")]
    pub managed_dirs: Vec<String>,
    #[serde(default)]
    pub extra_files: ExtraFiles,
}

impl Default for SyncRules {
    fn default() -> Self {
        Self { managed_dirs: default_managed_dirs(), extra_files: ExtraFiles::default() }
    }
}

fn default_managed_dirs() -> Vec<String> {
    vec!["mods".into()]
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExtraFiles {
    /// Players may add their own files.
    #[default]
    Keep,
    /// Rename unknown files to `*.disabled` (reversible).
    Disable,
    /// Move unknown files to the backup folder.
    Delete,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Policy {
    /// Always matches the pack; player edits are backed up and overwritten.
    Managed,
    /// Updated only while the player has not edited it.
    Config,
    /// Written once if missing, then belongs to the player.
    Seed,
    /// `key:value` file merged key by key (`defaults`, `force`).
    Keys,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    /// Relative to the instance (game) folder, forward slashes.
    pub path: String,
    pub policy: Policy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Download sources tried in order; relative URLs resolve against the
    /// manifest URL.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub urls: Vec<String>,
    /// The file may not be downloaded by the launcher (for example a CurseForge
    /// mod whose author disallows third-party downloads, or a gated playtest
    /// build). The player opens `page` in their browser and the launcher picks
    /// the file up from their Downloads folder by sha1. Only used when `urls`
    /// is empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual: Option<ManualSource>,
    /// The player may switch this file off.
    #[serde(default, skip_serializing_if = "is_false")]
    pub optional: bool,
    /// Display name for optional files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `keys` policy: set only when missing.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub defaults: BTreeMap<String, String>,
    /// `keys` policy: always set.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub force: BTreeMap<String, String>,
    /// `keys` policy: separator between key and value (default `:`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub separator: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManualSource {
    /// https page that starts or offers the download.
    pub page: String,
    /// File name the site usually saves, shown to the player.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Pack {
    /// Structural checks a launcher must run before trusting a manifest.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.schema > SCHEMA {
            anyhow::bail!(
                "pack {} needs a newer EasyLauncher (schema {}, this launcher supports {})",
                self.id, self.schema, SCHEMA
            );
        }
        if self.id.is_empty()
            || !self.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            anyhow::bail!("pack id {:?} may only contain letters, digits, - and _", self.id);
        }
        let mut seen = std::collections::HashSet::new();
        for f in &self.files {
            crate::paths::safe_join(std::path::Path::new("/"), &f.path)?;
            if !seen.insert(f.path.to_lowercase()) {
                anyhow::bail!("duplicate file {}", f.path);
            }
            match f.policy {
                Policy::Keys => {}
                _ => {
                    let sha = f.sha1.as_deref().unwrap_or("");
                    if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
                        anyhow::bail!("file {} needs a sha1", f.path);
                    }
                    match (&f.manual, f.urls.is_empty()) {
                        (_, false) => {}
                        (Some(m), true) if m.page.starts_with("https://") => {
                            if f.size.is_none() {
                                anyhow::bail!("file {} needs a size to be found in Downloads", f.path);
                            }
                        }
                        (Some(_), true) => anyhow::bail!("file {}: download page must be https", f.path),
                        (None, true) => anyhow::bail!("file {} has no download url", f.path),
                    }
                }
            }
        }
        for d in &self.sync.managed_dirs {
            crate::paths::safe_join(std::path::Path::new("/"), d)?;
        }
        Ok(())
    }
}
