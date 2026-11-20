//! Mod loaders. Fabric/Quilt publish ready launcher profiles; Forge/NeoForge
//! are installed by running their official installer headless into the
//! shared meta folder (which is laid out like `.minecraft`).

use super::install::version_json_path;
use crate::download::{write_atomic, Http};
use crate::manifest::{Loader, LoaderKind};
use crate::paths::Paths;
use crate::progress::{log, stage, Reporter};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::io::Read;
use std::path::Path;

/// Installs the loader (if any) and returns the version id to launch.
pub async fn install(http: &Http, paths: &Paths, mc: &str, loader: &Loader, java: &Path, r: &Reporter) -> Result<String> {
    match loader.kind {
        LoaderKind::Vanilla => Ok(mc.to_string()),
        LoaderKind::Fabric => profile_loader(http, paths, mc, loader.version.as_deref(), "https://meta.fabricmc.net/v2", r).await,
        LoaderKind::Quilt => profile_loader(http, paths, mc, loader.version.as_deref(), "https://meta.quiltmc.org/v3", r).await,
        LoaderKind::Forge | LoaderKind::Neoforge => forge_like(http, paths, mc, loader, java, r).await,
    }
}

async fn profile_loader(http: &Http, paths: &Paths, mc: &str, ver: Option<&str>, meta: &str, r: &Reporter) -> Result<String> {
    let ver = match ver {
        Some(v) => v.to_string(),
        None => {
            let list: Vec<Value> = http.json(&format!("{meta}/versions/loader/{mc}")).await?;
            let pick = list
                .iter()
                .find(|e| e["loader"]["stable"].as_bool().unwrap_or(true))
                .or(list.first())
                .ok_or_else(|| anyhow!("no loader builds for Minecraft {mc}"))?;
            pick["loader"]["version"].as_str().unwrap_or_default().to_string()
        }
    };
    let url = format!("{meta}/versions/loader/{mc}/{ver}/profile/json");
    // The id is decided by the meta server; cache by (mc, loader) to work offline.
    let cache = paths.meta().join("loader-profiles").join(format!("{}-{mc}-{ver}.json", meta.contains("quilt") as u8));
    let body = match http.bytes(&url).await {
        Ok(b) => {
            write_atomic(&cache, &b).await?;
            b
        }
        Err(e) => std::fs::read(&cache).map_err(|_| e)?,
    };
    let v: Value = serde_json::from_slice(&body)?;
    let id = v["id"].as_str().ok_or_else(|| anyhow!("loader profile has no id"))?.to_string();
    write_atomic(&version_json_path(paths, &id), &body).await?;
    log(r, format!("Loader profile {id}"));
    Ok(id)
}

fn installer_url(mc: &str, kind: LoaderKind, v: &str) -> String {
    match kind {
        LoaderKind::Forge => format!("https://maven.minecraftforge.net/net/minecraftforge/forge/{mc}-{v}/forge-{mc}-{v}-installer.jar"),
        // NeoForge's only 1.20.1 line was published under the old coordinates.
        _ if mc == "1.20.1" => format!("https://maven.neoforged.net/releases/net/neoforged/forge/{mc}-{v}/forge-{mc}-{v}-installer.jar"),
        _ => format!("https://maven.neoforged.net/releases/net/neoforged/neoforge/{v}/neoforge-{v}-installer.jar"),
    }
}

async fn latest_loader(http: &Http, mc: &str, kind: LoaderKind) -> Result<String> {
    if kind == LoaderKind::Forge {
        let p: Value = http.json("https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json").await?;
        let promos = &p["promos"];
        return promos[format!("{mc}-recommended")]
            .as_str()
            .or(promos[format!("{mc}-latest")].as_str())
            .map(String::from)
            .ok_or_else(|| anyhow!("no Forge build for Minecraft {mc}"));
    }
    let list: Value = http.json("https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge").await?;
    let prefix = match mc.strip_prefix("1.") {
        Some(rest) if rest.contains('.') => format!("{rest}."),
        Some(rest) => format!("{rest}.0."),
        None => format!("{mc}."),
    };
    list["versions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str())
        .rfind(|v| v.starts_with(&prefix) && !v.contains("beta"))
        .map(String::from)
        .ok_or_else(|| anyhow!("no NeoForge build for Minecraft {mc}"))
}

async fn forge_like(http: &Http, paths: &Paths, mc: &str, loader: &Loader, java: &Path, r: &Reporter) -> Result<String> {
    let kind = loader.kind;
    let ver = match &loader.version {
        Some(v) => v.clone(),
        None => latest_loader(http, mc, kind).await?,
    };
    let url = installer_url(mc, kind, &ver);
    let file = paths.installers().join(url.rsplit('/').next().unwrap());
    if !file.exists() {
        stage(r, format!("Downloading {:?} {ver}", kind));
        http.to_file(&url, &file, None, None).await?;
    }
    let profile = read_zip_json(&file, "install_profile.json")?;
    let id = match profile["version"].as_str() {
        Some(id) => id.to_string(),
        None if profile.get("versionInfo").is_some() => {
            bail!("Forge for Minecraft {mc} uses the legacy installer format, which EasyLauncher does not support yet")
        }
        None => bail!("unrecognised installer {}", file.display()),
    };
    let done = file.with_extension("installed");
    if done.exists() && version_json_path(paths, &id).exists() {
        return Ok(id);
    }

    stage(r, format!("Installing {:?} {ver} (this takes a minute the first time)", kind));
    let meta = paths.meta();
    let profiles = meta.join("launcher_profiles.json");
    if !profiles.exists() {
        std::fs::create_dir_all(&meta)?;
        std::fs::write(&profiles, r#"{"profiles":{}}"#)?;
    }
    let flag = if kind == LoaderKind::Forge { "--installClient" } else { "--install-client" };
    let out = tokio::process::Command::new(java)
        .arg("-Djava.awt.headless=true")
        .arg("-jar")
        .arg(&file)
        .arg(flag)
        .arg(&meta)
        .current_dir(paths.installers())
        .output()
        .await
        .with_context(|| format!("running {}", java.display()))?;
    let logf = file.with_extension("log");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    std::fs::write(&logf, &text)?;
    if !out.status.success() || !version_json_path(paths, &id).exists() {
        let tail: Vec<&str> = text.lines().rev().take(15).collect();
        bail!(
            "the {:?} installer failed (log: {}):\n{}",
            kind,
            logf.display(),
            tail.into_iter().rev().collect::<Vec<_>>().join("\n")
        );
    }
    std::fs::write(&done, &ver)?;
    Ok(id)
}

fn read_zip_json(jar: &Path, name: &str) -> Result<Value> {
    let f = std::fs::File::open(jar)?;
    let mut z = zip::ZipArchive::new(f).with_context(|| format!("{} is not a valid installer", jar.display()))?;
    let mut e = z.by_name(name).with_context(|| format!("{name} missing in {}", jar.display()))?;
    let mut s = String::new();
    e.read_to_string(&mut s)?;
    Ok(serde_json::from_str(&s)?)
}
