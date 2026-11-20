//! Mojang version JSON: types, rule evaluation, `inheritsFrom` merging.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub const VERSION_MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VersionManifest {
    pub latest: Latest,
    pub versions: Vec<ManifestVersion>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Latest {
    pub release: String,
    pub snapshot: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestVersion {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub sha1: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherits_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Arguments>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minecraft_arguments: Option<String>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_index: Option<AssetIndexRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<HashMap<String, Artifact>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_version: Option<JavaVersion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging: Option<HashMap<String, Logging>>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Value>,
    #[serde(default)]
    pub jvm: Vec<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexRef {
    pub id: String,
    pub url: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Artifact {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    #[serde(default)]
    pub component: Option<String>,
    pub major_version: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Logging {
    pub argument: String,
    pub file: LogFile,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LogFile {
    pub id: String,
    pub url: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Library {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<LibDownloads>,
    /// Maven repository base (Fabric/Quilt/legacy Forge style).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub natives: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Vec<Rule>>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct LibDownloads {
    #[serde(default)]
    pub artifact: Option<Artifact>,
    #[serde(default)]
    pub classifiers: Option<HashMap<String, Artifact>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Rule {
    pub action: String,
    #[serde(default)]
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: Option<HashMap<String, bool>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OsRule {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

/// The machine we evaluate rules for.
#[derive(Clone, Debug)]
pub struct Env {
    pub os: &'static str,
    pub arch: &'static str,
    pub features: HashSet<String>,
}

impl Env {
    pub fn current() -> Self {
        let os = match std::env::consts::OS {
            "macos" => "osx",
            "windows" => "windows",
            _ => "linux",
        };
        Self { os, arch: std::env::consts::ARCH, features: HashSet::new() }
    }
}

pub fn rules_allow(rules: Option<&Vec<Rule>>, env: &Env) -> bool {
    let Some(rules) = rules else { return true };
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for r in rules {
        let mut matches = true;
        if let Some(os) = &r.os {
            if let Some(n) = &os.name {
                matches &= n == env.os;
            }
            if let Some(a) = &os.arch {
                matches &= match a.as_str() {
                    "x86" => env.arch == "x86",
                    "arm64" | "aarch64" => env.arch == "aarch64",
                    other => other == env.arch,
                };
            }
            // OS version regexes target ancient or very specific releases;
            // treating them as non-matching is the safe choice.
            if os.version.is_some() {
                matches = false;
            }
        }
        if let Some(f) = &r.features {
            for (k, want) in f {
                matches &= env.features.contains(k) == *want;
            }
        }
        if matches {
            allowed = r.action == "allow";
        }
    }
    allowed
}

/// `group:artifact:version[:classifier][@ext]` → repository-relative path.
pub fn maven_path(name: &str) -> Option<String> {
    let (coords, ext) = match name.split_once('@') {
        Some((c, e)) => (c, e),
        None => (name, "jar"),
    };
    let parts: Vec<&str> = coords.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let (g, a, v) = (parts[0], parts[1], parts[2]);
    let file = match parts.get(3) {
        Some(c) => format!("{a}-{v}-{c}.{ext}"),
        None => format!("{a}-{v}.{ext}"),
    };
    Some(format!("{}/{a}/{v}/{file}", g.replace('.', "/")))
}

/// Identity used to de-duplicate libraries across an inheritance chain:
/// group, artifact and classifier, without the version.
fn lib_key(name: &str) -> String {
    let coords = name.split('@').next().unwrap_or(name);
    let p: Vec<&str> = coords.split(':').collect();
    match p.len() {
        0..=2 => coords.to_string(),
        3 => format!("{}:{}", p[0], p[1]),
        _ => format!("{}:{}:{}", p[0], p[1], p[3..].join(":")),
    }
}

/// Merges a child version (loader profile) onto its parent.
pub fn merge(child: VersionJson, parent: VersionJson) -> VersionJson {
    let mut seen = HashSet::new();
    let mut libraries = Vec::new();
    for l in child.libraries.into_iter().chain(parent.libraries) {
        if seen.insert(lib_key(&l.name)) {
            libraries.push(l);
        }
    }
    let arguments = match (parent.arguments, child.arguments) {
        (Some(mut p), Some(c)) => {
            p.game.extend(c.game);
            p.jvm.extend(c.jvm);
            Some(p)
        }
        (p, c) => c.or(p),
    };
    VersionJson {
        id: child.id,
        inherits_from: parent.inherits_from,
        main_class: child.main_class.or(parent.main_class),
        arguments,
        minecraft_arguments: child.minecraft_arguments.or(parent.minecraft_arguments),
        libraries,
        asset_index: child.asset_index.or(parent.asset_index),
        assets: child.assets.or(parent.assets),
        downloads: child.downloads.or(parent.downloads),
        java_version: child.java_version.or(parent.java_version),
        logging: child.logging.or(parent.logging),
        kind: child.kind.or(parent.kind),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maven() {
        assert_eq!(maven_path("net.fabricmc:fabric-loader:0.16.0").unwrap(), "net/fabricmc/fabric-loader/0.16.0/fabric-loader-0.16.0.jar");
        assert_eq!(maven_path("org.lwjgl:lwjgl:3.3.1:natives-macos").unwrap(), "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-macos.jar");
        assert_eq!(maven_path("de.oceanlabs.mcp:mcp_config:1.20.1@zip").unwrap(), "de/oceanlabs/mcp/mcp_config/1.20.1/mcp_config-1.20.1.zip");
        assert_eq!(lib_key("org.ow2.asm:asm:9.6"), "org.ow2.asm:asm");
        assert_eq!(lib_key("org.lwjgl:lwjgl:3.3.1:natives-macos"), "org.lwjgl:lwjgl:natives-macos");
    }
    #[test]
    fn rules() {
        let env = Env { os: "osx", arch: "aarch64", features: HashSet::new() };
        let r: Vec<Rule> = serde_json::from_str(r#"[{"action":"allow"},{"action":"disallow","os":{"name":"osx"}}]"#).unwrap();
        assert!(!rules_allow(Some(&r), &env));
        let r: Vec<Rule> = serde_json::from_str(r#"[{"action":"allow","os":{"name":"osx"}}]"#).unwrap();
        assert!(rules_allow(Some(&r), &env));
        let r: Vec<Rule> = serde_json::from_str(r#"[{"action":"allow","features":{"is_demo_user":true}}]"#).unwrap();
        assert!(!rules_allow(Some(&r), &env));
    }
}
