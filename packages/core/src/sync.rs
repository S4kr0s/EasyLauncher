//! Game-agnostic file sync: brings an instance folder in line with a pack
//! manifest without destroying the player's own changes.
//!
//! `plan` is a pure read of manifest + state + disk; `apply` downloads into
//! the shared object cache and then performs file operations. The policy table
//! lives in docs/DESIGN.md.

use crate::download::{resolve, run_jobs, Http, Job};
use crate::hash::sha1_file;
use crate::manifest::{ExtraFiles, FileEntry, Pack, Policy};
use crate::paths::{safe_join, Paths};
use crate::progress::{log, stage, Reporter};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

pub const STATE_DIR: &str = ".easylauncher";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    #[serde(default)]
    pub pack_id: String,
    #[serde(default)]
    pub version: Option<String>,
    /// What the launcher last installed, by path.
    #[serde(default)]
    pub files: BTreeMap<String, Installed>,
    /// Optional files the player switched off.
    #[serde(default)]
    pub disabled_optional: BTreeSet<String>,
    /// Player files the launcher renamed to `*.disabled`.
    #[serde(default)]
    pub disabled_extras: BTreeSet<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    pub sha1: Option<String>,
    pub policy: Policy,
}

pub fn state_path(inst: &Path) -> PathBuf {
    inst.join(STATE_DIR).join("state.json")
}

pub fn load_state(inst: &Path) -> Result<State> {
    match std::fs::read(state_path(inst)) {
        Ok(b) => Ok(serde_json::from_slice(&b).context("reading instance state")?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
        Err(e) => Err(e.into()),
    }
}

pub fn save_state(inst: &Path, s: &State) -> Result<()> {
    let p = state_path(inst);
    std::fs::create_dir_all(p.parent().unwrap())?;
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(s)?)?;
    std::fs::rename(&tmp, &p)?;
    Ok(())
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Op {
    /// File is missing: write it.
    Install { path: String },
    /// File differs and may be replaced. `backup` when the player changed it.
    Update { path: String, backup: bool },
    /// File differs, but it is the player's now (edited config, seed file).
    KeepPlayer { path: String },
    /// No longer in the pack: delete. `backup` when the player changed it.
    Remove { path: String, backup: bool },
    /// No longer in the pack, but left in place (player-edited or seed).
    Forget { path: String },
    /// Merge keys into a `keys` file.
    MergeKeys { path: String },
    /// Player file in a managed folder: rename to `*.disabled`.
    DisableExtra { path: String },
    /// Player file in a managed folder: move to backup.
    RemoveExtra { path: String },
    /// Previously disabled player file: rename back.
    RestoreExtra { path: String },
}

impl Op {
    pub fn path(&self) -> &str {
        match self {
            Op::Install { path }
            | Op::Update { path, .. }
            | Op::KeepPlayer { path }
            | Op::Remove { path, .. }
            | Op::Forget { path }
            | Op::MergeKeys { path }
            | Op::DisableExtra { path }
            | Op::RemoveExtra { path }
            | Op::RestoreExtra { path } => path,
        }
    }
    /// Whether this op changes anything on disk.
    pub fn changes_disk(&self) -> bool {
        !matches!(self, Op::KeepPlayer { .. } | Op::Forget { .. })
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub ops: Vec<Op>,
    /// Bytes to download (not yet in the cache).
    pub download_bytes: u64,
    pub from_version: Option<String>,
    pub to_version: String,
}

impl Plan {
    pub fn is_noop(&self) -> bool {
        self.ops.iter().all(|o| !o.changes_disk()) && self.from_version.as_deref() == Some(self.to_version.as_str())
    }
}

fn active_entries<'a>(pack: &'a Pack, state: &State) -> Vec<&'a FileEntry> {
    pack.files
        .iter()
        .filter(|f| !(f.optional && state.disabled_optional.contains(&f.path)))
        .collect()
}

/// Computes what a sync would do. Reads (hashes) local files; writes nothing.
pub fn plan(paths: &Paths, pack: &Pack, inst: &Path, state: &State) -> Result<Plan> {
    pack.validate()?;
    let mut ops = Vec::new();
    let mut download_bytes = 0u64;
    let active = active_entries(pack, state);
    let active_paths: HashSet<&str> = active.iter().map(|f| f.path.as_str()).collect();

    for e in &active {
        let target = safe_join(inst, &e.path)?;
        if e.policy == Policy::Keys {
            let old = std::fs::read_to_string(&target).unwrap_or_default();
            let new = crate::keys::merge(&old, e.separator.as_deref().unwrap_or(":"), &e.defaults, &e.force);
            if new != old || !state.files.contains_key(&e.path) {
                ops.push(Op::MergeKeys { path: e.path.clone() });
            }
            continue;
        }
        let want = e.sha1.as_deref().unwrap();
        let local = sha1_file(&target)?;
        let prev = state.files.get(&e.path).and_then(|i| i.sha1.clone());
        let op = match &local {
            Some(l) if l.eq_ignore_ascii_case(want) => None,
            None => Some(Op::Install { path: e.path.clone() }),
            Some(l) => {
                let untouched = prev.as_deref().map(|p| p.eq_ignore_ascii_case(l)).unwrap_or(false);
                Some(match e.policy {
                    Policy::Managed => Op::Update { path: e.path.clone(), backup: !untouched },
                    Policy::Config if untouched => Op::Update { path: e.path.clone(), backup: false },
                    Policy::Config | Policy::Seed => Op::KeepPlayer { path: e.path.clone() },
                    Policy::Keys => unreachable!(),
                })
            }
        };
        if let Some(op) = op {
            if matches!(op, Op::Install { .. } | Op::Update { .. }) && !paths.object(want).exists() {
                download_bytes += e.size.unwrap_or(0);
            }
            ops.push(op);
        }
    }

    for (path, prev) in &state.files {
        if active_paths.contains(path.as_str()) {
            continue;
        }
        let target = safe_join(inst, path)?;
        let local = sha1_file(&target)?;
        let untouched = match (&local, &prev.sha1) {
            (Some(l), Some(p)) => l.eq_ignore_ascii_case(p),
            _ => false,
        };
        ops.push(match (local.is_some(), prev.policy) {
            (false, _) => Op::Forget { path: path.clone() },
            (true, Policy::Managed) => Op::Remove { path: path.clone(), backup: !untouched },
            (true, Policy::Config) if untouched => Op::Remove { path: path.clone(), backup: false },
            _ => Op::Forget { path: path.clone() },
        });
    }

    let known = |p: &str| active_paths.contains(p) || state.files.contains_key(p);
    for dir in &pack.sync.managed_dirs {
        let root = safe_join(inst, dir)?;
        if !root.is_dir() {
            continue;
        }
        for ent in walkdir::WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
            if !ent.file_type().is_file() {
                continue;
            }
            let rel = rel_path(inst, ent.path());
            if known(&rel) {
                continue;
            }
            if let Some(base) = rel.strip_suffix(".disabled") {
                if state.disabled_extras.contains(base)
                    && pack.sync.extra_files == ExtraFiles::Keep
                    && !known(base)
                    && !safe_join(inst, base)?.exists()
                {
                    ops.push(Op::RestoreExtra { path: base.to_string() });
                }
                continue;
            }
            match pack.sync.extra_files {
                ExtraFiles::Keep => {}
                ExtraFiles::Disable => ops.push(Op::DisableExtra { path: rel }),
                ExtraFiles::Delete => ops.push(Op::RemoveExtra { path: rel }),
            }
        }
    }

    Ok(Plan { ops, download_bytes, from_version: state.version.clone(), to_version: pack.version.clone() })
}

fn rel_path(base: &Path, p: &Path) -> String {
    p.strip_prefix(base)
        .unwrap_or(p)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Executes a plan: downloads missing objects, applies file operations, and
/// records the new state.
pub async fn apply(
    http: &Http,
    paths: &Paths,
    pack: &Pack,
    manifest_url: &str,
    inst: &Path,
    plan: &Plan,
    r: &Reporter,
) -> Result<State> {
    let by_path: BTreeMap<&str, &FileEntry> = pack.files.iter().map(|f| (f.path.as_str(), f)).collect();

    let mut seen = HashSet::new();
    let mut jobs = Vec::new();
    for op in &plan.ops {
        if let Op::Install { path } | Op::Update { path, .. } = op {
            let e = by_path[path.as_str()];
            let sha = e.sha1.clone().unwrap();
            if !seen.insert(sha.clone()) {
                continue;
            }
            let urls = e.urls.iter().map(|u| resolve(manifest_url, u)).collect::<Result<Vec<_>>>()?;
            jobs.push(Job { urls, dest: paths.object(&sha), sha1: Some(sha), size: e.size });
        }
    }
    if !jobs.is_empty() {
        stage(r, format!("Downloading {} files", jobs.len()));
        run_jobs(http, jobs, r).await?;
    }

    stage(r, "Updating files");
    let paths = paths.clone();
    let pack = pack.clone();
    let inst = inst.to_path_buf();
    let plan = plan.clone();
    let r2 = r.clone();
    tokio::task::spawn_blocking(move || apply_ops(&paths, &pack, &inst, &plan, &r2)).await?
}

fn apply_ops(paths: &Paths, pack: &Pack, inst: &Path, plan: &Plan, r: &Reporter) -> Result<State> {
    let mut state = load_state(inst)?;
    let by_path: BTreeMap<&str, &FileEntry> = pack.files.iter().map(|f| (f.path.as_str(), f)).collect();
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let backup_root = inst.join(STATE_DIR).join("backup").join(stamp);
    let backup = |rel: &str| -> Result<()> {
        let src = safe_join(inst, rel)?;
        let dst = backup_root.join(rel);
        std::fs::create_dir_all(dst.parent().unwrap())?;
        std::fs::rename(&src, &dst).or_else(|_| std::fs::copy(&src, &dst).and_then(|_| std::fs::remove_file(&src)))?;
        Ok(())
    };

    for op in &plan.ops {
        let target = safe_join(inst, op.path())?;
        match op {
            Op::Install { path } | Op::Update { path, .. } => {
                if let Op::Update { backup: true, .. } = op {
                    backup(path)?;
                    log(r, format!("Backed up your changes to {path}"));
                }
                let e = by_path[path.as_str()];
                let sha = e.sha1.as_deref().unwrap();
                place(&paths.object(sha), &target)?;
                state.files.insert(path.clone(), Installed { sha1: Some(sha.to_lowercase()), policy: e.policy });
                state.disabled_extras.remove(path);
            }
            Op::KeepPlayer { path } => log(r, format!("Kept your version of {path}")),
            Op::Remove { path, backup: b } => {
                if *b {
                    backup(path)?;
                } else {
                    std::fs::remove_file(&target)?;
                }
                state.files.remove(path);
            }
            Op::Forget { path } => {
                state.files.remove(path);
            }
            Op::MergeKeys { path } => {
                let e = by_path[path.as_str()];
                let old = std::fs::read_to_string(&target).unwrap_or_default();
                let new = crate::keys::merge(&old, e.separator.as_deref().unwrap_or(":"), &e.defaults, &e.force);
                if new != old {
                    std::fs::create_dir_all(target.parent().unwrap())?;
                    std::fs::write(&target, new)?;
                }
                state.files.insert(path.clone(), Installed { sha1: None, policy: Policy::Keys });
            }
            Op::DisableExtra { path } => {
                let mut d = target.clone().into_os_string();
                d.push(".disabled");
                std::fs::rename(&target, PathBuf::from(d))?;
                state.disabled_extras.insert(path.clone());
                log(r, format!("Disabled {path} (not part of the pack)"));
            }
            Op::RemoveExtra { path } => {
                backup(path)?;
                log(r, format!("Moved {path} to backup (not part of the pack)"));
            }
            Op::RestoreExtra { path } => {
                let mut d = target.clone().into_os_string();
                d.push(".disabled");
                std::fs::rename(PathBuf::from(d), &target)?;
                state.disabled_extras.remove(path);
            }
        }
    }

    // Files that were already correct still need a state record.
    for e in active_entries(pack, &state.clone()) {
        if e.policy != Policy::Keys && !plan.ops.iter().any(|o| o.path() == e.path) {
            state.files.insert(e.path.clone(), Installed { sha1: e.sha1.as_ref().map(|s| s.to_lowercase()), policy: e.policy });
        }
    }
    state.pack_id = pack.id.clone();
    state.version = Some(pack.version.clone());
    save_state(inst, &state)?;
    Ok(state)
}

/// Copies a cached object into place via a temp file + rename.
fn place(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst.parent().unwrap())?;
    let mut tmp = dst.as_os_str().to_owned();
    tmp.push(".easylauncher-tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::copy(src, &tmp).with_context(|| format!("copying {}", src.display()))?;
    std::fs::rename(&tmp, dst)?;
    Ok(())
}

/// Plan + apply in one call.
pub async fn sync(http: &Http, paths: &Paths, pack: &Pack, manifest_url: &str, inst: &Path, r: &Reporter) -> Result<(Plan, State)> {
    stage(r, "Checking files");
    let (p2, pk, i2) = (paths.clone(), pack.clone(), inst.to_path_buf());
    let plan = tokio::task::spawn_blocking(move || {
        let st = load_state(&i2)?;
        plan(&p2, &pk, &i2, &st)
    })
    .await??;
    let need = crate::manual::missing(paths, pack, &plan);
    if !need.is_empty() {
        return Err(crate::manual::NeedsDownloads(need).into());
    }
    let state = apply(http, paths, pack, manifest_url, inst, &plan, r).await?;
    Ok((plan, state))
}
