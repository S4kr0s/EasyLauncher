//! Resolves a pack's Minecraft spec into a merged version JSON and downloads
//! everything needed to launch it: client jar, libraries, assets, Java.

use super::loaders;
use super::version::*;
use crate::download::{run_jobs, Http, Job};
use crate::manifest::MinecraftSpec;
use crate::paths::Paths;
use crate::progress::{log, stage, Reporter};
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const LIBRARIES: &str = "https://libraries.minecraft.net/";
const RESOURCES: &str = "https://resources.download.minecraft.net/";

/// Everything `launch` needs, produced by `prepare`.
#[derive(Clone, Debug)]
pub struct Prepared {
    pub version: VersionJson,
    pub vanilla_id: String,
    pub java: PathBuf,
    pub java_major: u32,
    pub env: Env,
    pub classpath: Vec<PathBuf>,
    pub natives_dir: PathBuf,
    pub client_jar: PathBuf,
    pub assets_virtual: Option<PathBuf>,
    pub map_to_resources: bool,
}

pub async fn version_manifest(http: &Http, paths: &Paths) -> Result<VersionManifest> {
    let cache = paths.meta().join("version_manifest_v2.json");
    match http.json::<VersionManifest>(VERSION_MANIFEST).await {
        Ok(m) => {
            let _ = crate::download::write_atomic(&cache, &serde_json::to_vec(&m)?).await;
            Ok(m)
        }
        Err(e) => match std::fs::read(&cache) {
            Ok(b) => Ok(serde_json::from_slice(&b)?),
            Err(_) => Err(e),
        },
    }
}

/// `latest-release` / `latest-snapshot` → concrete id.
pub async fn resolve_version_id(http: &Http, paths: &Paths, want: &str) -> Result<String> {
    match want {
        "latest-release" | "latest-snapshot" => {
            let m = version_manifest(http, paths).await?;
            Ok(if want == "latest-release" { m.latest.release } else { m.latest.snapshot })
        }
        id => Ok(id.to_string()),
    }
}

pub fn version_json_path(paths: &Paths, id: &str) -> PathBuf {
    paths.versions().join(id).join(format!("{id}.json"))
}

pub fn read_version(paths: &Paths, id: &str) -> Result<VersionJson> {
    let p = version_json_path(paths, id);
    let b = std::fs::read(&p).with_context(|| format!("version {id} is not installed"))?;
    serde_json::from_slice(&b).with_context(|| format!("parsing {}", p.display()))
}

async fn ensure_vanilla_json(http: &Http, paths: &Paths, id: &str) -> Result<VersionJson> {
    let p = version_json_path(paths, id);
    if p.exists() {
        if let Ok(v) = read_version(paths, id) {
            return Ok(v);
        }
    }
    let m = version_manifest(http, paths).await?;
    let e = m.versions.iter().find(|v| v.id == id).ok_or_else(|| anyhow!("Minecraft {id} does not exist"))?;
    http.to_file(&e.url, &p, e.sha1.as_deref(), None).await?;
    read_version(paths, id)
}

/// Loads `id` and merges its whole `inheritsFrom` chain.
pub fn load_merged(paths: &Paths, id: &str) -> Result<VersionJson> {
    let mut v = read_version(paths, id)?;
    let mut depth = 0;
    while let Some(parent) = v.inherits_from.clone() {
        depth += 1;
        if depth > 8 {
            bail!("inheritsFrom chain too deep at {id}");
        }
        let p = read_version(paths, &parent)?;
        v = merge(v, p);
    }
    Ok(v)
}

fn has_arm_natives(v: &VersionJson) -> bool {
    v.libraries.iter().any(|l| l.name.contains("natives-macos-arm64"))
}

pub async fn prepare(http: &Http, paths: &Paths, spec: &MinecraftSpec, java_override: Option<PathBuf>, r: &Reporter) -> Result<Prepared> {
    stage(r, "Preparing Minecraft");
    let mc = resolve_version_id(http, paths, &spec.version).await?;
    log(r, format!("Minecraft {mc}, loader {:?} {}", spec.loader.kind, spec.loader.version.clone().unwrap_or_default()));
    let vanilla = ensure_vanilla_json(http, paths, &mc).await?;

    let mut env = Env::current();
    let x86_on_arm = env.os == "osx" && env.arch == "aarch64" && !has_arm_natives(&vanilla);
    if x86_on_arm {
        env.arch = "x86_64";
        log(r, "This Minecraft version has no Apple Silicon natives; using Intel Java via Rosetta");
    }

    let major = spec.java.or(vanilla.java_version.as_ref().map(|j| j.major_version)).unwrap_or(8);
    let component = if spec.java.is_some() { None } else { vanilla.java_version.as_ref().and_then(|j| j.component.clone()) };
    let java = match java_override {
        Some(j) => j,
        None => super::java::ensure(http, paths, component.as_deref(), major, x86_on_arm, r).await?,
    };

    // Vanilla files first: Forge/NeoForge installers expect them in place.
    download_version_files(http, paths, &vanilla, &mc, &env, r).await?;

    let id = loaders::install(http, paths, &mc, &spec.loader, &java, r).await?;
    let version = load_merged(paths, &id)?;
    if id != mc {
        stage(r, "Downloading loader libraries");
        download_libraries(http, paths, &version, &env, r).await?;
    }

    // The final version's jar must be named after it (Forge's ignoreList
    // looks for `${version_name}.jar`).
    let vanilla_jar = paths.versions().join(&mc).join(format!("{mc}.jar"));
    let client_jar = paths.versions().join(&id).join(format!("{id}.jar"));
    if client_jar != vanilla_jar {
        let stale = match (std::fs::metadata(&client_jar), std::fs::metadata(&vanilla_jar)) {
            (Ok(a), Ok(b)) => a.len() != b.len(),
            _ => true,
        };
        if stale {
            std::fs::create_dir_all(client_jar.parent().unwrap())?;
            std::fs::copy(&vanilla_jar, &client_jar)?;
        }
    }

    let (classpath, natives) = classpath(paths, &version, &env)?;
    let natives_dir = paths.meta().join("natives").join(&id);
    extract_natives(&natives, &natives_dir)?;
    let (assets_virtual, map_to_resources) = asset_layout(paths, &version)?;

    Ok(Prepared {
        version,
        vanilla_id: mc,
        java,
        java_major: major,
        env,
        classpath,
        natives_dir,
        client_jar,
        assets_virtual,
        map_to_resources,
    })
}

async fn download_version_files(http: &Http, paths: &Paths, v: &VersionJson, id: &str, env: &Env, r: &Reporter) -> Result<()> {
    stage(r, "Downloading Minecraft");
    let mut jobs = Vec::new();
    if let Some(c) = v.downloads.as_ref().and_then(|d| d.get("client")) {
        jobs.push(Job {
            urls: vec![c.url.clone()],
            dest: paths.versions().join(id).join(format!("{id}.jar")),
            sha1: c.sha1.clone(),
            size: c.size,
        });
    }
    if let Some(l) = v.logging.as_ref().and_then(|l| l.get("client")) {
        jobs.push(Job {
            urls: vec![l.file.url.clone()],
            dest: paths.assets().join("log_configs").join(&l.file.id),
            sha1: l.file.sha1.clone(),
            size: l.file.size,
        });
    }
    jobs.extend(library_jobs(paths, v, env)?);
    run_jobs(http, jobs, r).await?;

    if let Some(ai) = &v.asset_index {
        stage(r, "Downloading assets");
        let idx = paths.assets().join("indexes").join(format!("{}.json", ai.id));
        http_ensure(http, &ai.url, &idx, ai.sha1.as_deref(), ai.size).await?;
        let index: AssetIndex = serde_json::from_slice(&std::fs::read(&idx)?)?;
        let objects = paths.assets().join("objects");
        let mut seen = std::collections::HashSet::new();
        let jobs: Vec<Job> = index
            .objects
            .values()
            .filter(|o| seen.insert(o.hash.clone()))
            .map(|o| {
                let pre = &o.hash[..2];
                Job {
                    urls: vec![format!("{RESOURCES}{pre}/{}", o.hash)],
                    dest: objects.join(pre).join(&o.hash),
                    sha1: Some(o.hash.clone()),
                    size: Some(o.size),
                }
            })
            .collect();
        run_assets(http, jobs, r).await?;
    }
    Ok(())
}

/// Assets: checking 4000 hashes on every launch is slow, so an existing file
/// of the right size counts as present.
async fn run_assets(http: &Http, jobs: Vec<Job>, r: &Reporter) -> Result<()> {
    let missing: Vec<Job> = jobs
        .into_iter()
        .filter(|j| std::fs::metadata(&j.dest).map(|m| Some(m.len()) != j.size).unwrap_or(true))
        .collect();
    run_jobs(http, missing, r).await
}

async fn http_ensure(http: &Http, url: &str, dest: &Path, sha1: Option<&str>, size: Option<u64>) -> Result<()> {
    if let (Some(h), Ok(Some(got))) = (sha1, crate::hash::sha1_file(dest)) {
        if got.eq_ignore_ascii_case(h) {
            return Ok(());
        }
    } else if sha1.is_none() && dest.exists() {
        return Ok(());
    }
    http.to_file(url, dest, sha1, size).await
}

#[derive(Deserialize)]
struct AssetIndex {
    objects: HashMap<String, AssetObject>,
    #[serde(default, rename = "virtual")]
    is_virtual: bool,
    #[serde(default)]
    map_to_resources: bool,
}
#[derive(Deserialize)]
struct AssetObject {
    hash: String,
    size: u64,
}

async fn download_libraries(http: &Http, paths: &Paths, v: &VersionJson, env: &Env, r: &Reporter) -> Result<()> {
    run_jobs(http, library_jobs(paths, v, env)?, r).await
}

struct ResolvedLib {
    path: PathBuf,
    job: Job,
    native: bool,
}

fn resolve_libs(paths: &Paths, v: &VersionJson, env: &Env) -> Result<Vec<ResolvedLib>> {
    let libdir = paths.libraries();
    let mut out = Vec::new();
    for l in &v.libraries {
        if !rules_allow(l.rules.as_ref(), env) {
            continue;
        }
        if let Some(natives) = &l.natives {
            let Some(cls) = natives.get(env.os) else { continue };
            let cls = cls.replace("${arch}", if env.arch == "x86" { "32" } else { "64" });
            let art = l.downloads.as_ref().and_then(|d| d.classifiers.as_ref()).and_then(|c| c.get(&cls));
            let (rel, url, sha1, size) = match art {
                Some(a) => (a.path.clone().or_else(|| maven_path(&format!("{}:{cls}", l.name))), a.url.clone(), a.sha1.clone(), a.size),
                None => {
                    let rel = maven_path(&format!("{}:{cls}", l.name));
                    let base = l.url.clone().unwrap_or_else(|| LIBRARIES.into());
                    (rel.clone(), rel.map(|p| format!("{base}{p}")).unwrap_or_default(), None, None)
                }
            };
            let rel = rel.ok_or_else(|| anyhow!("bad library name {}", l.name))?;
            let path = libdir.join(&rel);
            out.push(ResolvedLib { path: path.clone(), job: job(url, path, sha1, size), native: true });
            if l.downloads.as_ref().and_then(|d| d.artifact.as_ref()).is_none() {
                continue;
            }
        }
        let (rel, url, sha1, size) = match l.downloads.as_ref().and_then(|d| d.artifact.as_ref()) {
            Some(a) => (a.path.clone().or_else(|| maven_path(&l.name)), a.url.clone(), a.sha1.clone(), a.size),
            None => {
                let rel = maven_path(&l.name);
                let mut base = l.url.clone().unwrap_or_else(|| LIBRARIES.into());
                if !base.ends_with('/') {
                    base.push('/');
                }
                (rel.clone(), rel.map(|p| format!("{base}{p}")).unwrap_or_default(), l.sha1.clone(), l.size)
            }
        };
        let rel = rel.ok_or_else(|| anyhow!("bad library name {}", l.name))?;
        let path = libdir.join(&rel);
        out.push(ResolvedLib { path: path.clone(), job: job(url, path, sha1, size), native: false });
    }
    Ok(out)
}

fn job(url: String, dest: PathBuf, sha1: Option<String>, size: Option<u64>) -> Job {
    Job { urls: if url.is_empty() { vec![] } else { vec![url] }, dest, sha1, size }
}

fn library_jobs(paths: &Paths, v: &VersionJson, env: &Env) -> Result<Vec<Job>> {
    let mut seen = std::collections::HashSet::new();
    Ok(resolve_libs(paths, v, env)?.into_iter().filter(|l| seen.insert(l.path.clone())).map(|l| l.job).collect())
}

/// Classpath entries (in order, de-duplicated) and legacy native jars.
fn classpath(paths: &Paths, v: &VersionJson, env: &Env) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let mut seen = std::collections::HashSet::new();
    let mut cp = Vec::new();
    let mut natives = Vec::new();
    for l in resolve_libs(paths, v, env)? {
        if l.native {
            natives.push(l.path);
        } else if seen.insert(l.path.clone()) {
            cp.push(l.path);
        }
    }
    Ok((cp, natives))
}

fn extract_natives(jars: &[PathBuf], dir: &Path) -> Result<()> {
    if jars.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    for jar in jars {
        let f = std::fs::File::open(jar).with_context(|| format!("opening {}", jar.display()))?;
        let mut z = zip::ZipArchive::new(f)?;
        for i in 0..z.len() {
            let mut e = z.by_index(i)?;
            let Some(name) = e.enclosed_name() else { continue };
            if e.is_dir() || name.starts_with("META-INF") {
                continue;
            }
            let dest = dir.join(name);
            if dest.exists() {
                continue;
            }
            if let Some(p) = dest.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut out = std::fs::File::create(&dest)?;
            std::io::copy(&mut e, &mut out)?;
        }
    }
    Ok(())
}

/// Pre-1.7 asset layouts: a "virtual" tree under assets/virtual/<id>, or
/// copies inside the game folder (`map_to_resources`, done at launch).
fn asset_layout(paths: &Paths, v: &VersionJson) -> Result<(Option<PathBuf>, bool)> {
    let Some(ai) = &v.asset_index else { return Ok((None, false)) };
    let idx = paths.assets().join("indexes").join(format!("{}.json", ai.id));
    let index: AssetIndex = serde_json::from_slice(&std::fs::read(&idx)?)?;
    if !index.is_virtual && !index.map_to_resources {
        return Ok((None, false));
    }
    let vdir = paths.assets().join("virtual").join(&ai.id);
    let objects = paths.assets().join("objects");
    for (name, o) in &index.objects {
        let dest = vdir.join(name);
        if !dest.exists() {
            std::fs::create_dir_all(dest.parent().unwrap())?;
            std::fs::copy(objects.join(&o.hash[..2]).join(&o.hash), &dest)?;
        }
    }
    Ok((Some(vdir), index.map_to_resources))
}
