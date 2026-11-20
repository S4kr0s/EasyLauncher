//! The player's side: subscribed hubs, installed projects, settings, and the
//! high-level "update" and "play" flows the app calls.

use crate::accounts;
use crate::download::{resolve, to_url, Http};
use crate::manifest::{Hub, Pack};
use crate::minecraft::{self, LaunchOptions};
use crate::paths::Paths;
use crate::progress::{log, stage, Reporter};
use crate::sync::{self, Plan, STATE_DIR};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub hubs: Vec<String>,
    #[serde(default = "default_memory")]
    pub default_memory_mb: u32,
    #[serde(default)]
    pub ms_client_id: Option<String>,
    #[serde(default)]
    pub jvm_args: Vec<String>,
}

fn default_memory() -> u32 {
    4096
}

impl Default for Settings {
    fn default() -> Self {
        Self { hubs: default_hubs(), default_memory_mb: default_memory(), ms_client_id: None, jvm_args: vec![] }
    }
}

/// The hub a fresh install starts with, so friends see the projects without
/// pasting anything. Builds can point elsewhere with `EASYLAUNCHER_DEFAULT_HUB`
/// (empty for none). Players can remove it or add their own hubs.
pub const DEFAULT_HUB: &str = "https://s4kr0s.github.io/EasyLauncher-manifests/hub.json";

fn default_hubs() -> Vec<String> {
    let hub = option_env!("EASYLAUNCHER_DEFAULT_HUB").unwrap_or(DEFAULT_HUB);
    if hub.is_empty() { vec![] } else { vec![hub.to_string()] }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSettings {
    #[serde(default)]
    pub memory_mb: Option<u32>,
    #[serde(default)]
    pub java_path: Option<String>,
    #[serde(default)]
    pub jvm_args: Vec<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    /// Join the pack's first server on launch.
    #[serde(default = "yes")]
    pub auto_join: bool,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub manifest_url: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LibraryFile {
    #[serde(default)]
    pub installed: BTreeMap<String, Installed>,
}

/// A project as the UI shows it: from a hub, installed, or both.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub manifest_url: String,
    pub hub: Option<String>,
    pub latest_version: Option<String>,
    pub installed_version: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HubStatus {
    pub url: String,
    pub name: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct Launcher {
    pub paths: Paths,
    pub http: Http,
}

fn read_json<T: serde::de::DeserializeOwned + Default>(p: &Path) -> Result<T> {
    match std::fs::read(p) {
        Ok(b) => serde_json::from_slice(&b).with_context(|| format!("reading {}", p.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e.into()),
    }
}

fn write_json<T: Serialize>(p: &Path, v: &T) -> Result<()> {
    std::fs::create_dir_all(p.parent().unwrap())?;
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(v)?)?;
    std::fs::rename(tmp, p)?;
    Ok(())
}

impl Launcher {
    pub fn new(paths: Paths) -> Self {
        Self { paths, http: Http::new() }
    }

    pub fn settings(&self) -> Result<Settings> {
        if !self.paths.settings_file().exists() {
            return Ok(Settings::default());
        }
        read_json(&self.paths.settings_file())
    }
    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        write_json(&self.paths.settings_file(), s)
    }
    pub fn library(&self) -> Result<LibraryFile> {
        read_json(&self.paths.library_file())
    }
    fn save_library(&self, l: &LibraryFile) -> Result<()> {
        write_json(&self.paths.library_file(), l)
    }
    pub fn instance_dir(&self, id: &str) -> PathBuf {
        self.paths.instance(id)
    }
    pub fn instance_settings(&self, id: &str) -> Result<InstanceSettings> {
        let p = self.instance_dir(id).join(STATE_DIR).join("settings.json");
        if !p.exists() {
            return Ok(InstanceSettings { auto_join: true, ..Default::default() });
        }
        read_json(&p)
    }
    pub fn save_instance_settings(&self, id: &str, s: &InstanceSettings) -> Result<()> {
        write_json(&self.instance_dir(id).join(STATE_DIR).join("settings.json"), s)
    }
    pub fn client_id(&self) -> Option<String> {
        self.settings().ok().and_then(|s| s.ms_client_id).filter(|s| !s.is_empty()).or(accounts::BUILTIN_CLIENT_ID.map(String::from))
    }

    pub async fn add_hub(&self, loc: &str) -> Result<Hub> {
        let url = to_url(loc.trim())?;
        let hub: Hub = self.http.json(&url).await.context("could not load that hub")?;
        let mut s = self.settings()?;
        if !s.hubs.contains(&url) {
            s.hubs.push(url);
            self.save_settings(&s)?;
        }
        Ok(hub)
    }

    pub fn remove_hub(&self, url: &str) -> Result<()> {
        let mut s = self.settings()?;
        s.hubs.retain(|h| h != url);
        self.save_settings(&s)
    }

    /// All projects from all hubs plus installed ones. Network failures for a
    /// hub are reported, not fatal.
    pub async fn projects(&self) -> Result<(Vec<Project>, Vec<HubStatus>)> {
        let settings = self.settings()?;
        let lib = self.library()?;
        let mut out: BTreeMap<String, Project> = BTreeMap::new();
        let mut statuses = Vec::new();
        let fetched = futures::future::join_all(settings.hubs.iter().map(|h| async move { (h.clone(), self.http.json::<Hub>(h).await) })).await;
        for (url, res) in fetched {
            match res {
                Ok(hub) => {
                    statuses.push(HubStatus { url: url.clone(), name: Some(hub.name.clone()), error: None });
                    for p in hub.projects {
                        let manifest_url = resolve(&url, &p.manifest)?;
                        let icon = p.icon.as_deref().and_then(|i| resolve(&url, i).ok());
                        out.insert(
                            p.id.clone(),
                            Project {
                                id: p.id,
                                name: p.name,
                                description: p.description,
                                icon,
                                manifest_url,
                                hub: Some(url.clone()),
                                latest_version: p.version,
                                installed_version: None,
                            },
                        );
                    }
                }
                Err(e) => statuses.push(HubStatus { url, name: None, error: Some(format!("{e:#}")) }),
            }
        }
        for (id, i) in lib.installed {
            let e = out.entry(id.clone()).or_insert_with(|| Project {
                id: id.clone(),
                name: i.name.clone(),
                description: None,
                icon: i.icon.clone(),
                manifest_url: i.manifest_url.clone(),
                hub: None,
                latest_version: None,
                installed_version: None,
            });
            e.installed_version = i.version;
        }
        Ok((out.into_values().collect(), statuses))
    }

    pub async fn fetch_pack(&self, manifest_url: &str) -> Result<Pack> {
        let p: Pack = self.http.json(manifest_url).await?;
        p.validate()?;
        Ok(p)
    }

    fn cached_pack(&self, id: &str) -> Result<(Pack, String)> {
        let lib = self.library()?;
        let i = lib.installed.get(id).ok_or_else(|| anyhow!("{id} is not installed"))?;
        let b = std::fs::read(self.instance_dir(id).join(STATE_DIR).join("pack.json"))?;
        Ok((serde_json::from_slice(&b)?, i.manifest_url.clone()))
    }

    /// What an update would change, for the UI ("3 mods updated, your
    /// keybinds stay").
    pub async fn check(&self, manifest_url: &str) -> Result<(Pack, Plan)> {
        let pack = self.fetch_pack(manifest_url).await?;
        let (paths, p2) = (self.paths.clone(), pack.clone());
        let inst = self.instance_dir(&pack.id);
        let plan = tokio::task::spawn_blocking(move || {
            let st = sync::load_state(&inst)?;
            sync::plan(&paths, &p2, &inst, &st)
        })
        .await??;
        Ok((pack, plan))
    }

    /// Installs or updates a project to the manifest's current version.
    pub async fn update(&self, manifest_url: &str, r: &Reporter) -> Result<Pack> {
        stage(r, "Checking for updates");
        let pack = self.fetch_pack(manifest_url).await?;
        let inst = self.instance_dir(&pack.id);
        std::fs::create_dir_all(&inst)?;
        let (plan, _) = sync::sync(&self.http, &self.paths, &pack, manifest_url, &inst, r).await?;
        let changed = plan.ops.iter().filter(|o| o.changes_disk()).count();
        log(r, format!("{} → {}: {changed} file changes", plan.from_version.as_deref().unwrap_or("new"), plan.to_version));
        if let Some(mc) = &pack.minecraft {
            let is = self.instance_settings(&pack.id)?;
            let java = is.java_path.as_ref().map(PathBuf::from);
            minecraft::prepare(&self.http, &self.paths, mc, java, r).await?;
            minecraft::servers::ensure_servers(&inst, &pack.servers)?;
        }
        write_json(&inst.join(STATE_DIR).join("pack.json"), &pack)?;
        let mut lib = self.library()?;
        lib.installed.insert(
            pack.id.clone(),
            Installed {
                id: pack.id.clone(),
                name: pack.name.clone(),
                manifest_url: manifest_url.to_string(),
                version: Some(pack.version.clone()),
                icon: pack.icon.as_deref().and_then(|i| resolve(manifest_url, i).ok()),
            },
        );
        self.save_library(&lib)?;
        stage(r, "Up to date");
        Ok(pack)
    }

    /// Updates (when the pack is reachable) and starts the game. Returns the
    /// game's exit code. Offline, the last installed version is launched.
    pub async fn play(&self, manifest_url: &str, id: &str, r: &Reporter) -> Result<i32> {
        let pack = match self.update(manifest_url, r).await {
            Ok(p) => p,
            Err(e) => {
                let installed = self.library()?.installed.contains_key(id);
                let offline = format!("{e:#}").contains("downloading") || format!("{e:#}").contains("error sending request");
                if installed && offline {
                    log(r, format!("Could not reach the pack server ({e:#}); starting the installed version"));
                    self.cached_pack(id)?.0
                } else {
                    return Err(e);
                }
            }
        };
        let Some(mc) = &pack.minecraft else { bail!("this project has nothing to launch yet") };
        let is = self.instance_settings(&pack.id)?;
        let settings = self.settings()?;
        let java = is.java_path.as_ref().map(PathBuf::from);
        let prepared = minecraft::prepare(&self.http, &self.paths, mc, java, r).await?;
        let account = accounts::launch_account(&self.http, &self.paths, self.client_id().as_deref()).await?;
        let mut jvm_args = mc.jvm_args.clone();
        jvm_args.extend(settings.jvm_args.clone());
        jvm_args.extend(is.jvm_args.clone());
        let opts = LaunchOptions {
            game_dir: self.instance_dir(&pack.id),
            memory_mb: is.memory_mb.or(mc.memory_mb).unwrap_or(settings.default_memory_mb),
            jvm_args,
            width: is.width,
            height: is.height,
            join: if is.auto_join { pack.servers.first().map(|s| s.address.clone()) } else { None },
            account,
        };
        stage(r, "Running");
        minecraft::launch::run(&self.paths, &prepared, &opts, r).await
    }

    /// Removes a project's instance (saves included) after moving it to the
    /// system trash is not available: the caller must confirm first.
    pub fn uninstall(&self, id: &str) -> Result<()> {
        let mut lib = self.library()?;
        lib.installed.remove(id);
        self.save_library(&lib)?;
        let dir = self.instance_dir(id);
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        Ok(())
    }
}
