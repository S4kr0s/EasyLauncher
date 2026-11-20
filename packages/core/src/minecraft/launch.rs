//! Builds the Java command line from a prepared version and starts the game.

use super::install::Prepared;
use super::version::{rules_allow, Rule};
use crate::paths::Paths;
use crate::progress::{Event, Reporter};
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Clone, Debug)]
pub struct LaunchAccount {
    pub name: String,
    /// Without dashes.
    pub uuid: String,
    pub token: String,
    /// `msa` for Microsoft accounts, `legacy` for offline.
    pub user_type: String,
    pub xuid: Option<String>,
}

#[derive(Clone, Debug)]
pub struct LaunchOptions {
    pub game_dir: PathBuf,
    pub memory_mb: u32,
    pub jvm_args: Vec<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// `host[:port]` to join on start.
    pub join: Option<String>,
    pub account: LaunchAccount,
}

fn sep() -> &'static str {
    if cfg!(windows) {
        ";"
    } else {
        ":"
    }
}

fn substitute(s: &str, vars: &HashMap<&str, String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("${") {
        out.push_str(&rest[..i]);
        match rest[i..].find('}') {
            Some(j) => {
                let key = &rest[i + 2..i + j];
                match vars.get(key) {
                    Some(v) => out.push_str(v),
                    None => out.push_str(&rest[i..i + j + 1]),
                }
                rest = &rest[i + j + 1..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn collect(args: &[Value], env: &super::version::Env, vars: &HashMap<&str, String>, out: &mut Vec<String>) {
    for a in args {
        match a {
            Value::String(s) => out.push(substitute(s, vars)),
            Value::Object(o) => {
                let rules: Option<Vec<Rule>> = o.get("rules").and_then(|r| serde_json::from_value(r.clone()).ok());
                if !rules_allow(rules.as_ref(), env) {
                    continue;
                }
                match o.get("value") {
                    Some(Value::String(s)) => out.push(substitute(s, vars)),
                    Some(Value::Array(vs)) => {
                        for v in vs.iter().filter_map(|v| v.as_str()) {
                            out.push(substitute(v, vars));
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

/// Returns the program and its arguments.
pub fn command(paths: &Paths, p: &Prepared, o: &LaunchOptions) -> Result<(PathBuf, Vec<String>)> {
    let v = &p.version;
    let mut env = p.env.clone();
    let modern = v.arguments.is_some();
    let quick_play = serde_json::to_string(&v.arguments)?.contains("is_quick_play_multiplayer");
    if o.width.is_some() && o.height.is_some() {
        env.features.insert("has_custom_resolution".into());
    }
    if o.join.is_some() && quick_play {
        env.features.insert("is_quick_play_multiplayer".into());
    }

    let mut cp: Vec<String> = p.classpath.iter().map(|p| p.display().to_string()).collect();
    cp.push(p.client_jar.display().to_string());
    let assets_root = paths.assets();
    let game_assets = p.assets_virtual.clone().unwrap_or_else(|| assets_root.clone());
    let asset_index = v.asset_index.as_ref().map(|a| a.id.clone()).or(v.assets.clone()).unwrap_or_default();

    let mut vars: HashMap<&str, String> = HashMap::new();
    vars.insert("auth_player_name", o.account.name.clone());
    vars.insert("version_name", v.id.clone());
    vars.insert("game_directory", o.game_dir.display().to_string());
    vars.insert("assets_root", assets_root.display().to_string());
    vars.insert("game_assets", game_assets.display().to_string());
    vars.insert("assets_index_name", asset_index);
    vars.insert("auth_uuid", o.account.uuid.clone());
    vars.insert("auth_access_token", o.account.token.clone());
    vars.insert("auth_session", format!("token:{}:{}", o.account.token, o.account.uuid));
    vars.insert("auth_xuid", o.account.xuid.clone().unwrap_or_default());
    vars.insert("clientid", String::new());
    vars.insert("user_type", o.account.user_type.clone());
    vars.insert("user_properties", "{}".into());
    vars.insert("version_type", v.kind.clone().unwrap_or_else(|| "release".into()));
    vars.insert("natives_directory", p.natives_dir.display().to_string());
    vars.insert("launcher_name", "EasyLauncher".into());
    vars.insert("launcher_version", env!("CARGO_PKG_VERSION").into());
    vars.insert("classpath", cp.join(sep()));
    vars.insert("classpath_separator", sep().into());
    vars.insert("library_directory", paths.libraries().display().to_string());
    vars.insert("resolution_width", o.width.unwrap_or(854).to_string());
    vars.insert("resolution_height", o.height.unwrap_or(480).to_string());
    vars.insert("quickPlayMultiplayer", o.join.clone().unwrap_or_default());

    let mut args = vec![format!("-Xmx{}m", o.memory_mb), format!("-Xms{}m", (o.memory_mb / 2).max(512).min(o.memory_mb))];
    args.extend(o.jvm_args.iter().cloned());
    if modern {
        collect(&v.arguments.as_ref().unwrap().jvm, &env, &vars, &mut args);
    } else {
        if env.os == "osx" {
            args.push("-XstartOnFirstThread".into());
        }
        if env.os == "windows" {
            args.push("-XX:HeapDumpPath=MojangTricksIntelDriversForPerformance_javaw.exe_minecraft.exe.heapdump".into());
        }
        args.push(format!("-Djava.library.path={}", p.natives_dir.display()));
        args.push("-cp".into());
        args.push(cp.join(sep()));
    }
    if let Some(l) = v.logging.as_ref().and_then(|l| l.get("client")) {
        let path = paths.assets().join("log_configs").join(&l.file.id);
        let mut lv = HashMap::new();
        lv.insert("path", path.display().to_string());
        args.push(substitute(&l.argument, &lv));
    }
    args.push(v.main_class.clone().context("version has no main class")?);
    if let Some(a) = &v.arguments {
        collect(&a.game, &env, &vars, &mut args);
    } else if let Some(s) = &v.minecraft_arguments {
        args.extend(s.split_whitespace().map(|t| substitute(t, &vars)));
    }
    if let (Some(w), Some(h), false) = (o.width, o.height, modern) {
        args.extend(["--width".into(), w.to_string(), "--height".into(), h.to_string()]);
    }
    if let (Some(j), false) = (&o.join, quick_play) {
        let (host, port) = j.rsplit_once(':').filter(|(_, p)| p.parse::<u16>().is_ok()).unwrap_or((j.as_str(), "25565"));
        args.extend(["--server".into(), host.into(), "--port".into(), port.into()]);
    }
    Ok((p.java.clone(), args))
}

/// Starts the game and streams its output to the reporter. Resolves when the
/// game exits, with its exit code.
pub async fn run(paths: &Paths, p: &Prepared, o: &LaunchOptions, r: &Reporter) -> Result<i32> {
    std::fs::create_dir_all(&o.game_dir)?;
    if p.map_to_resources {
        if let Some(v) = &p.assets_virtual {
            copy_tree(v, &o.game_dir.join("resources"))?;
        }
    }
    let (java, args) = command(paths, p, o)?;
    r(Event::Log { text: format!("Starting {} ({} args, main class {})", java.display(), args.len(), p.version.main_class.clone().unwrap_or_default()) });
    let mut cmd = tokio::process::Command::new(&java);
    cmd.args(&args).current_dir(&o.game_dir).stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().with_context(|| format!("starting {}", java.display()))?;
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    let r1 = r.clone();
    let r2 = r.clone();
    let t1 = tokio::spawn(async move {
        let mut l = BufReader::new(out).lines();
        let mut x = Log4jText::default();
        while let Ok(Some(line)) = l.next_line().await {
            if let Some(line) = x.feed(line) {
                r1(Event::Game { line });
            }
        }
    });
    let t2 = tokio::spawn(async move {
        let mut l = BufReader::new(err).lines();
        while let Ok(Some(line)) = l.next_line().await {
            r2(Event::Game { line });
        }
    });
    let status = child.wait().await?;
    let _ = tokio::join!(t1, t2);
    Ok(status.code().unwrap_or(-1))
}

/// Minecraft's launcher log config prints log4j XML events to stdout; this
/// turns them back into plain lines and passes everything else through.
#[derive(Default)]
struct Log4jText {
    in_cdata: bool,
    level: String,
}

impl Log4jText {
    fn feed(&mut self, line: String) -> Option<String> {
        if self.in_cdata {
            match line.find("]]>") {
                Some(i) => {
                    self.in_cdata = false;
                    let t = &line[..i];
                    (!t.is_empty()).then(|| t.to_string())
                }
                None => Some(line),
            }
        } else if let Some(i) = line.find("<![CDATA[") {
            let rest = &line[i + 9..];
            let text = match rest.find("]]>") {
                Some(j) => &rest[..j],
                None => {
                    self.in_cdata = true;
                    rest
                }
            };
            Some(format!("[{}] {text}", self.level))
        } else if line.trim_start().starts_with("<log4j:Event") {
            self.level = line.split("level=\"").nth(1).and_then(|s| s.split('"').next()).unwrap_or("INFO").to_string();
            None
        } else if line.trim_start().starts_with("<log4j:") || line.trim_start().starts_with("</log4j:") {
            None
        } else {
            Some(line)
        }
    }
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) -> Result<()> {
    for e in walkdir::WalkDir::new(from).into_iter().filter_map(|e| e.ok()) {
        if e.file_type().is_file() {
            let dest = to.join(e.path().strip_prefix(from)?);
            if !dest.exists() {
                std::fs::create_dir_all(dest.parent().unwrap())?;
                std::fs::copy(e.path(), dest)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn log4j() {
        let mut x = Log4jText::default();
        let lines = [
            r#"<log4j:Event logger="a" timestamp="1" level="WARN" thread="main">"#,
            "  <log4j:Message><![CDATA[hello]]></log4j:Message>",
            "</log4j:Event>",
            r#"<log4j:Event logger="a" timestamp="1" level="ERROR" thread="main">"#,
            "  <log4j:Throwable><![CDATA[java.lang.X: boom",
            "\tat a.b(c)",
            "]]></log4j:Throwable>",
            "plain",
        ];
        let out: Vec<String> = lines.iter().filter_map(|l| x.feed(l.to_string())).collect();
        assert_eq!(out, vec!["[WARN] hello", "[ERROR] java.lang.X: boom", "\tat a.b(c)", "plain"]);
    }
    #[test]
    fn subst() {
        let mut v = HashMap::new();
        v.insert("a", "1".to_string());
        assert_eq!(substitute("x${a}y${b}z", &v), "x1y${b}z");
    }
}
