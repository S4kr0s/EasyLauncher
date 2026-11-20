//! Java runtimes from Mojang's own runtime manifests (the same builds the
//! official launcher uses), installed per component and platform.

use crate::download::{run_jobs, Http, Job};
use crate::paths::Paths;
use crate::progress::{stage, Reporter};
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const ALL: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

#[derive(Deserialize)]
struct Entry {
    manifest: Ref,
    version: Ver,
}
#[derive(Deserialize)]
struct Ref {
    url: String,
}
#[derive(Deserialize)]
struct Ver {
    name: String,
}
#[derive(Deserialize)]
struct RuntimeManifest {
    files: HashMap<String, RtFile>,
}
#[derive(Deserialize)]
struct RtFile {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    executable: bool,
    #[serde(default)]
    downloads: Option<HashMap<String, RtDl>>,
    #[serde(default)]
    target: Option<String>,
}
#[derive(Deserialize)]
struct RtDl {
    url: String,
    sha1: String,
    size: u64,
}

/// Mojang's platform key for this machine. `x86_on_arm` asks for the Intel
/// build on Apple Silicon (for old versions whose natives are x86-only).
pub fn platform(x86_on_arm: bool) -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") if !x86_on_arm => "mac-os-arm64",
        ("macos", _) => "mac-os",
        ("windows", "aarch64") => "windows-arm64",
        ("windows", "x86") => "windows-x86",
        ("windows", _) => "windows-x64",
        (_, "x86") => "linux-i386",
        _ => "linux",
    }
}

fn java_bin(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("jre.bundle/Contents/Home/bin/java")
    } else if cfg!(windows) {
        home.join("bin/javaw.exe")
    } else {
        home.join("bin/java")
    }
}

fn major_of(name: &str) -> Option<u32> {
    let n = name.strip_prefix("1.").unwrap_or(name);
    n.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

/// Ensures a runtime is installed and returns the `java` executable.
/// `component` comes from the version JSON; `major` is used when the
/// component is unknown or the pack overrides the Java version.
pub async fn ensure(http: &Http, paths: &Paths, component: Option<&str>, major: u32, x86_on_arm: bool, r: &Reporter) -> Result<PathBuf> {
    let plat = platform(x86_on_arm);
    let marker_dir = paths.runtimes();
    // Fast path: an installed runtime for this component/major.
    let installed_name = component.map(|c| c.to_string()).unwrap_or_else(|| format!("java-{major}"));
    let home = marker_dir.join(&installed_name).join(plat);
    if home.join(".installed").exists() && java_bin(&home).exists() {
        return Ok(java_bin(&home));
    }

    stage(r, format!("Installing Java {major}"));
    let all: HashMap<String, HashMap<String, Vec<Entry>>> = http.json(ALL).await?;
    let comps = all.get(plat).ok_or_else(|| anyhow!("no Java builds for platform {plat}"))?;
    let entry = component
        .and_then(|c| comps.get(c).and_then(|v| v.first()))
        .or_else(|| {
            comps
                .values()
                .filter_map(|v| v.first())
                .find(|e| major_of(&e.version.name) == Some(major))
        });
    let Some(entry) = entry else {
        if plat == "mac-os-arm64" {
            // Mojang ships some old runtimes for Intel only; Rosetta runs them.
            return Box::pin(ensure(http, paths, component, major, true, r)).await;
        }
        bail!("Mojang has no Java {major} for {plat}; set a custom Java path in the instance settings");
    };
    let manifest: RuntimeManifest = http.json(&entry.manifest.url).await?;

    let mut jobs = Vec::new();
    let mut links = Vec::new();
    let mut execs = Vec::new();
    for (p, f) in &manifest.files {
        let dest = home.join(p);
        match f.kind.as_str() {
            "directory" => std::fs::create_dir_all(&dest)?,
            "file" => {
                let raw = f.downloads.as_ref().and_then(|d| d.get("raw")).ok_or_else(|| anyhow!("runtime file {p} has no download"))?;
                jobs.push(Job { urls: vec![raw.url.clone()], dest: dest.clone(), sha1: Some(raw.sha1.clone()), size: Some(raw.size) });
                if f.executable {
                    execs.push(dest);
                }
            }
            "link" => {
                if let Some(t) = &f.target {
                    links.push((dest, t.clone()));
                }
            }
            _ => {}
        }
    }
    run_jobs(http, jobs, r).await.context("downloading Java")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for e in execs {
            std::fs::set_permissions(&e, std::fs::Permissions::from_mode(0o755))?;
        }
        for (dest, target) in links {
            let _ = std::fs::remove_file(&dest);
            if let Some(p) = dest.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::os::unix::fs::symlink(target, &dest)?;
        }
    }
    #[cfg(not(unix))]
    let _ = (execs, links);
    std::fs::write(home.join(".installed"), &entry.version.name)?;
    let bin = java_bin(&home);
    if !bin.exists() {
        bail!("Java installed but {} is missing", bin.display());
    }
    Ok(bin)
}
