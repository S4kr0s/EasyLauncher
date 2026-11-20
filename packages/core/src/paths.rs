//! On-disk layout of the launcher's data directory.
//!
//! `meta/` mimics a vanilla `.minecraft` (versions/, libraries/, assets/) so
//! official Forge/NeoForge installers can install straight into it.

use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    /// Default location: `~/Library/Application Support/EasyLauncher` on macOS,
    /// `%APPDATA%\EasyLauncher` on Windows. `EASYLAUNCHER_HOME` overrides it.
    pub fn default_root() -> PathBuf {
        if let Some(p) = std::env::var_os("EASYLAUNCHER_HOME") {
            return PathBuf::from(p);
        }
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("EasyLauncher")
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn cache_objects(&self) -> PathBuf {
        self.root.join("cache").join("objects")
    }
    pub fn object(&self, sha1: &str) -> PathBuf {
        self.cache_objects().join(&sha1[..2]).join(sha1)
    }
    pub fn instances(&self) -> PathBuf {
        self.root.join("instances")
    }
    pub fn instance(&self, id: &str) -> PathBuf {
        self.instances().join(id)
    }
    pub fn meta(&self) -> PathBuf {
        self.root.join("meta")
    }
    pub fn versions(&self) -> PathBuf {
        self.meta().join("versions")
    }
    pub fn libraries(&self) -> PathBuf {
        self.meta().join("libraries")
    }
    pub fn assets(&self) -> PathBuf {
        self.meta().join("assets")
    }
    pub fn runtimes(&self) -> PathBuf {
        self.root.join("runtimes")
    }
    pub fn installers(&self) -> PathBuf {
        self.meta().join("installers")
    }
    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }
    pub fn accounts_file(&self) -> PathBuf {
        self.root.join("accounts.json")
    }
    pub fn library_file(&self) -> PathBuf {
        self.root.join("library.json")
    }
}

/// Validates a manifest-supplied relative path and joins it onto `base`.
/// Rejects absolute paths, `..`, and the launcher's own `.easylauncher` folder,
/// so a hostile manifest cannot write outside the instance.
pub fn safe_join(base: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    let p = Path::new(rel);
    if rel.is_empty() || rel.contains('\\') {
        anyhow::bail!("invalid path {rel:?} (use forward slashes)");
    }
    let mut out = base.to_path_buf();
    for (i, c) in p.components().enumerate() {
        match c {
            Component::Normal(s) => {
                if i == 0 && s == ".easylauncher" {
                    anyhow::bail!("path {rel:?} targets the launcher's state folder");
                }
                out.push(s)
            }
            _ => anyhow::bail!("path {rel:?} must be relative and stay inside the instance"),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_escapes() {
        let b = Path::new("/x");
        assert!(safe_join(b, "../a").is_err());
        assert!(safe_join(b, "/etc/passwd").is_err());
        assert!(safe_join(b, "a/../../b").is_err());
        assert!(safe_join(b, ".easylauncher/state.json").is_err());
        assert!(safe_join(b, "a\\b").is_err());
        assert_eq!(safe_join(b, "mods/a.jar").unwrap(), Path::new("/x/mods/a.jar"));
    }
}
