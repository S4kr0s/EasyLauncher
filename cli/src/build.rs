//! Builds a pack folder from an instance folder and a `pack.toml`.

use anyhow::{bail, Context, Result};
use easylauncher_core::hash::sha1_file;
use easylauncher_core::manifest::*;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    id: String,
    name: String,
    version: String,
    description: Option<String>,
    /// Image inside the instance folder (or next to pack.toml).
    icon: Option<String>,
    changelog: Option<String>,
    minecraft: Option<McConfig>,
    #[serde(default)]
    servers: Vec<Server>,
    #[serde(default)]
    sync: Option<SyncRules>,
    /// Ordered rules; the first matching glob decides a file's policy.
    #[serde(default)]
    files: Vec<Rule>,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    keys: BTreeMap<String, KeysConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct McConfig {
    version: String,
    #[serde(default)]
    loader: Option<LoaderKind>,
    loader_version: Option<String>,
    java: Option<u32>,
    memory_mb: Option<u32>,
    #[serde(default)]
    jvm_args: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    glob: String,
    policy: Option<Policy>,
    /// `false` excludes matching files.
    #[serde(default = "yes")]
    include: bool,
    #[serde(default)]
    optional: bool,
    /// Browser download page for matching files that the launcher may not
    /// fetch itself (CurseForge mods without third-party downloads, gated builds).
    page: Option<String>,
    /// Ship matching jars/zips from your own host. Only for files you may
    /// redistribute, such as your own mods.
    #[serde(default)]
    self_host: bool,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeysConfig {
    #[serde(default)]
    defaults: BTreeMap<String, String>,
    #[serde(default)]
    force: BTreeMap<String, String>,
    separator: Option<String>,
    /// Take defaults for these keys from the instance's own copy of the file.
    #[serde(default)]
    defaults_from_instance: Vec<String>,
}

pub async fn build(instance: &Path, config: &Path, out: &Path, modrinth: bool, version: Option<String>) -> Result<()> {
    let cfg: Config = toml::from_str(&std::fs::read_to_string(config).with_context(|| format!("reading {}", config.display()))?)
        .with_context(|| format!("parsing {}", config.display()))?;
    if !instance.is_dir() {
        bail!("{} is not a folder", instance.display());
    }
    let exclude = globs(cfg.exclude.iter().map(String::as_str))?;
    let rules: Vec<(GlobSet, &Rule)> = cfg.files.iter().map(|r| Ok((globs([r.glob.as_str()])?, r))).collect::<Result<_>>()?;

    let prism = prism_index(instance);
    let mut entries = Vec::new();
    let mut entry_rules: Vec<&Rule> = Vec::new();
    let mut sources: HashMap<String, PathBuf> = HashMap::new();
    for e in walkdir::WalkDir::new(instance).sort_by_file_name().into_iter().filter_map(|e| e.ok()) {
        if !e.file_type().is_file() {
            continue;
        }
        let rel = e.path().strip_prefix(instance)?.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
        if rel.starts_with(".easylauncher/") || rel.contains("/.index/") || exclude.is_match(&rel) || cfg.keys.contains_key(&rel) {
            continue;
        }
        let Some((_, rule)) = rules.iter().find(|(g, _)| g.is_match(&rel)) else { continue };
        if !rule.include {
            continue;
        }
        let policy = rule.policy.unwrap_or(Policy::Managed);
        if policy == Policy::Keys {
            bail!("{rel}: use a [keys.\"{rel}\"] table for keys files");
        }
        let sha = sha1_file(e.path())?.unwrap();
        let size = e.metadata()?.len();
        sources.insert(sha.clone(), e.path().to_path_buf());
        entry_rules.push(rule);
        entries.push(FileEntry {
            manual: None,
            path: rel.clone(),
            policy,
            sha1: Some(sha),
            size: Some(size),
            urls: vec![],
            optional: rule.optional,
            name: if rule.optional { Some(e.file_name().to_string_lossy().into()) } else { None },
            defaults: BTreeMap::new(),
            force: BTreeMap::new(),
            separator: None,
        });
    }
    for (path, k) in &cfg.keys {
        let mut defaults = k.defaults.clone();
        let sep = k.separator.clone().unwrap_or_else(|| ":".into());
        if !k.defaults_from_instance.is_empty() {
            let text = std::fs::read_to_string(instance.join(path)).unwrap_or_default();
            let pats = globs(k.defaults_from_instance.iter().map(String::as_str))?;
            for line in text.lines() {
                if let Some((key, v)) = line.split_once(sep.as_str()) {
                    if pats.is_match(key) {
                        defaults.entry(key.to_string()).or_insert_with(|| v.to_string());
                    }
                }
            }
        }
        entries.push(FileEntry {
            manual: None,
            path: path.clone(),
            policy: Policy::Keys,
            sha1: None,
            size: None,
            urls: vec![],
            optional: false,
            name: None,
            defaults,
            force: k.force.clone(),
            separator: k.separator.clone(),
        });
    }

    // Prefer public CDNs; self-host the rest.
    let mut remote: HashMap<String, String> = HashMap::new();
    if modrinth {
        let hashes: Vec<String> = entries.iter().filter(|e| e.path.ends_with(".jar") || e.path.ends_with(".zip")).filter_map(|e| e.sha1.clone()).collect();
        remote = modrinth_lookup(&hashes).await.context("looking up mods on Modrinth (try again, or --no-modrinth for a local test)")?;
    }
    std::fs::create_dir_all(out.join("files"))?;
    let (mut hosted, mut hosted_bytes, mut from_modrinth, mut browser) = (0, 0u64, 0, 0);
    let mut unknown = Vec::new();
    for (i, e) in entries.iter_mut().enumerate() {
        let Some(sha) = e.sha1.clone() else { continue };
        let rule = entry_rules.get(i).copied();
        let archive = e.path.ends_with(".jar") || e.path.ends_with(".zip");
        if let Some(u) = remote.get(&sha) {
            e.urls.push(u.clone());
            from_modrinth += 1;
        } else if let Some(page) = rule.and_then(|r| r.page.clone()) {
            if !page.starts_with("https://") {
                bail!("{}: page must be an https link", e.path);
            }
            e.manual = Some(ManualSource { page, filename: e.path.rsplit('/').next().map(String::from) });
            browser += 1;
        } else if let Some(cf) = prism.get(&sha).filter(|_| !rule.is_some_and(|r| r.self_host)) {
            e.manual = Some(ManualSource { page: cf.page(), filename: Some(cf.filename.clone()) });
            e.name.get_or_insert(cf.name.clone());
            browser += 1;
        } else if archive && modrinth && !rule.is_some_and(|r| r.self_host) {
            unknown.push(e.path.clone());
        } else {
            let dest = out.join("files").join(&sha);
            if !dest.exists() {
                std::fs::copy(&sources[&sha], &dest)?;
            }
            hosted += 1;
            hosted_bytes += e.size.unwrap_or(0);
            e.urls.push(format!("files/{sha}"));
        }
    }

    if !unknown.is_empty() {
        bail!(
            "{} file(s) are not on Modrinth and have no CurseForge record:\n  {}\n\
             For each, add a [[files]] rule above the general one with either\n  \
             page = \"https://…\"  (players download it in their browser), or\n  \
             self_host = true      (only for files you may share, like your own mods)",
            unknown.len(),
            unknown.join("\n  ")
        );
    }

    let icon = match &cfg.icon {
        Some(i) => {
            let src = [instance.join(i), config.parent().unwrap_or(Path::new(".")).join(i)].into_iter().find(|p| p.exists());
            match src {
                Some(src) => {
                    let name = format!("icon.{}", src.extension().and_then(|e| e.to_str()).unwrap_or("png"));
                    std::fs::copy(src, out.join(&name))?;
                    Some(name)
                }
                None => bail!("icon {i} not found"),
            }
        }
        None => None,
    };

    let pack = Pack {
        schema: SCHEMA,
        id: cfg.id,
        name: cfg.name,
        version: version.unwrap_or(cfg.version),
        description: cfg.description,
        icon,
        changelog: cfg.changelog,
        minecraft: cfg.minecraft.map(|m| MinecraftSpec {
            version: m.version,
            loader: Loader { kind: m.loader.unwrap_or_default(), version: m.loader_version },
            java: m.java,
            memory_mb: m.memory_mb,
            jvm_args: m.jvm_args,
        }),
        servers: cfg.servers,
        sync: cfg.sync.unwrap_or_default(),
        files: entries,
    };
    pack.validate()?;
    std::fs::write(out.join("pack.json"), serde_json::to_vec_pretty(&pack)?)?;
    prune(out, &pack)?;
    let total: u64 = pack.files.iter().filter_map(|f| f.size).sum();
    eprintln!(
        "{} {}: {} files ({} MB), {} from Modrinth, {} via browser, {} self-hosted ({} MB) → {}",
        pack.id,
        pack.version,
        pack.files.len(),
        total / 1_000_000,
        from_modrinth,
        browser,
        hosted,
        hosted_bytes / 1_000_000,
        out.join("pack.json").display()
    );
    Ok(())
}

/// Removes self-hosted files no longer referenced.
fn prune(out: &Path, pack: &Pack) -> Result<()> {
    let keep: std::collections::HashSet<&str> = pack.files.iter().filter_map(|f| f.sha1.as_deref()).collect();
    for e in std::fs::read_dir(out.join("files"))? {
        let e = e?;
        if !keep.contains(e.file_name().to_string_lossy().as_ref()) {
            std::fs::remove_file(e.path())?;
        }
    }
    Ok(())
}

fn globs<'a>(pats: impl IntoIterator<Item = &'a str>) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in pats {
        b.add(Glob::new(p).with_context(|| format!("bad glob {p}"))?);
    }
    Ok(b.build()?)
}

async fn modrinth_lookup(hashes: &[String]) -> Result<HashMap<String, String>> {
    let client = reqwest::Client::builder().user_agent(easylauncher_core::USER_AGENT).build()?;
    let mut out = HashMap::new();
    for chunk in hashes.chunks(500) {
        let v: HashMap<String, Value> = client
            .post("https://api.modrinth.com/v2/version_files")
            .json(&json!({"hashes": chunk, "algorithm": "sha1"}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        for (sha, ver) in v {
            let url = ver["files"].as_array().into_iter().flatten().find(|f| f["hashes"]["sha1"].as_str() == Some(sha.as_str())).and_then(|f| f["url"].as_str());
            if let Some(u) = url {
                out.insert(sha, u.to_string());
            }
        }
    }
    Ok(out)
}

pub fn hub(site: &Path, name: &str) -> Result<()> {
    let mut projects = Vec::new();
    for e in std::fs::read_dir(site)? {
        let e = e?;
        let pj = e.path().join("pack.json");
        if !pj.exists() {
            continue;
        }
        let p: Pack = serde_json::from_slice(&std::fs::read(&pj)?)?;
        let dir = e.file_name().to_string_lossy().into_owned();
        projects.push(HubProject {
            id: p.id,
            name: p.name,
            manifest: format!("{dir}/pack.json"),
            description: p.description,
            icon: p.icon.map(|i| format!("{dir}/{i}")),
            version: Some(p.version),
        });
    }
    projects.sort_by(|a, b| a.name.cmp(&b.name));
    let hub = Hub { schema: SCHEMA, name: name.into(), projects };
    std::fs::write(site.join("hub.json"), serde_json::to_vec_pretty(&hub)?)?;
    eprintln!("hub.json: {} projects", hub.projects.len());
    Ok(())
}

/// A file Prism installed from CurseForge, from its `.index/<slug>.pw.toml`.
struct CurseForgeFile {
    category: &'static str,
    slug: String,
    file_id: u64,
    name: String,
    filename: String,
}

impl CurseForgeFile {
    /// The page that starts the download in the player's browser (what Prism opens too).
    fn page(&self) -> String {
        format!("https://www.curseforge.com/minecraft/{}/{}/download/{}", self.category, self.slug, self.file_id)
    }
}

/// Reads Prism's packwiz-style metadata so CurseForge files get a download page
/// instead of being re-hosted. Keyed by sha1, so a match is always the exact file.
fn prism_index(instance: &Path) -> HashMap<String, CurseForgeFile> {
    let mut out = HashMap::new();
    for (dir, category) in [("mods", "mc-mods"), ("resourcepacks", "texture-packs"), ("shaderpacks", "shaders")] {
        let Ok(rd) = std::fs::read_dir(instance.join(dir).join(".index")) else { continue };
        for f in rd.flatten() {
            let p = f.path();
            let Some(slug) = p.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".pw.toml")) else { continue };
            let Ok(v) = std::fs::read_to_string(&p).map_err(anyhow::Error::from).and_then(|t| Ok(t.parse::<toml::Table>()?)) else { continue };
            let dl = v.get("download");
            let sha = dl.and_then(|d| d.get("hash")).and_then(|h| h.as_str());
            let fmt = dl.and_then(|d| d.get("hash-format")).and_then(|h| h.as_str());
            let file_id = v.get("update").and_then(|u| u.get("curseforge")).and_then(|c| c.get("file-id")).and_then(|i| i.as_integer());
            let (Some(sha), Some("sha1"), Some(file_id)) = (sha, fmt, file_id) else { continue };
            out.insert(
                sha.to_lowercase(),
                CurseForgeFile {
                    category,
                    slug: slug.to_string(),
                    file_id: file_id as u64,
                    name: v.get("name").and_then(|n| n.as_str()).unwrap_or(slug).to_string(),
                    filename: v.get("filename").and_then(|n| n.as_str()).unwrap_or_default().to_string(),
                },
            );
        }
    }
    out
}
