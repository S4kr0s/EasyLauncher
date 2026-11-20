//! Files the launcher may not download itself.
//!
//! The pack lists a page (a CurseForge download page, a playtest portal, …).
//! The player opens it in their own browser and goes through whatever the site
//! asks for; the launcher only watches the Downloads folder and takes a file
//! whose size and sha1 match exactly. Nothing is guessed from file names.

use crate::manifest::{FileEntry, Pack};
use crate::paths::Paths;
use crate::sync::{Op, Plan};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Missing {
    pub path: String,
    pub name: String,
    pub page: String,
    pub filename: Option<String>,
    pub sha1: String,
    pub size: u64,
}

/// Returned by sync when some files have to come through the browser first.
#[derive(Debug)]
pub struct NeedsDownloads(pub Vec<Missing>);

impl std::fmt::Display for NeedsDownloads {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} file(s) have to be fetched in your browser first", self.0.len())
    }
}

impl std::error::Error for NeedsDownloads {}

fn have(paths: &Paths, sha1: &str, size: u64) -> bool {
    std::fs::metadata(paths.object(sha1)).map(|m| m.len() == size).unwrap_or(false)
}

/// Browser-only files that the plan needs and the object cache doesn't have.
pub fn missing(paths: &Paths, pack: &Pack, plan: &Plan) -> Vec<Missing> {
    let mut out: Vec<Missing> = Vec::new();
    for op in &plan.ops {
        let (Op::Install { path } | Op::Update { path, .. }) = op else { continue };
        let Some(e) = pack.files.iter().find(|f| &f.path == path) else { continue };
        if let Some(m) = manual_of(e) {
            if !have(paths, &m.sha1, m.size) && !out.iter().any(|o| o.sha1 == m.sha1) {
                out.push(m);
            }
        }
    }
    out
}

fn manual_of(e: &FileEntry) -> Option<Missing> {
    let m = e.manual.as_ref()?;
    if !e.urls.is_empty() {
        return None;
    }
    Some(Missing {
        path: e.path.clone(),
        name: e.name.clone().unwrap_or_else(|| e.path.rsplit('/').next().unwrap_or(&e.path).to_string()),
        page: m.page.clone(),
        filename: m.filename.clone(),
        sha1: e.sha1.clone()?.to_lowercase(),
        size: e.size?,
    })
}

/// Folders to look in: the user's Downloads folder (or `EASYLAUNCHER_DOWNLOADS`).
pub fn download_dirs() -> Vec<PathBuf> {
    if let Some(d) = std::env::var_os("EASYLAUNCHER_DOWNLOADS") {
        return vec![PathBuf::from(d)];
    }
    dirs::download_dir().into_iter().collect()
}

/// Looks for the wanted files in `dirs` (not recursive) and copies every exact
/// match into the object cache. Returns the sha1s that are now available.
/// Only files whose size matches are hashed, so this is cheap to call often.
pub fn scan(paths: &Paths, dirs: &[PathBuf], wanted: &[Missing]) -> Result<Vec<String>> {
    let mut found = Vec::new();
    for w in wanted {
        if have(paths, &w.sha1, w.size) {
            found.push(w.sha1.clone());
        }
    }
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        for entry in rd.flatten() {
            if found.len() == wanted.len() {
                return Ok(found);
            }
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            if !wanted.iter().any(|w| w.size == meta.len() && !found.contains(&w.sha1)) {
                continue;
            }
            if let Some(sha) = take(paths, &entry.path(), wanted, &found)? {
                found.push(sha);
            }
        }
    }
    Ok(found)
}

/// Checks one file the player picked by hand. Returns its sha1 if it is one of `wanted`.
pub fn import(paths: &Paths, file: &Path, wanted: &[Missing]) -> Result<Option<String>> {
    take(paths, file, wanted, &[])
}

fn take(paths: &Paths, file: &Path, wanted: &[Missing], skip: &[String]) -> Result<Option<String>> {
    let Some(sha) = crate::hash::sha1_file(file)? else { return Ok(None) };
    let Some(w) = wanted.iter().find(|w| w.sha1 == sha && !skip.contains(&w.sha1)) else { return Ok(None) };
    let dest = paths.object(&w.sha1);
    std::fs::create_dir_all(dest.parent().unwrap())?;
    let tmp = dest.with_extension("part");
    std::fs::copy(file, &tmp)?;
    // The browser may still have been writing; trust only what we copied.
    if crate::hash::sha1_file(&tmp)?.as_deref() != Some(sha.as_str()) {
        let _ = std::fs::remove_file(&tmp);
        return Ok(None);
    }
    std::fs::rename(&tmp, &dest)?;
    Ok(Some(sha))
}
