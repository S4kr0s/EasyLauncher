//! `ezl` — the pack author's tool.
//!
//!   ezl build <instance dir> -c pack.toml -o site/<id>   build a pack folder
//!   ezl hub site --name "My Servers"                     write site/hub.json
//!   ezl play <pack.json url or path> --offline Name      sync + launch (testing)

mod build;

use anyhow::Result;
use clap::{Parser, Subcommand};
use easylauncher_core::accounts::{self, Accounts};
use easylauncher_core::download::to_url;
use easylauncher_core::library::Launcher;
use easylauncher_core::paths::Paths;
use easylauncher_core::progress::{Event, Reporter};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "ezl", version, about = "EasyLauncher author tool")]
struct Cli {
    /// Launcher data folder (default: the app's own; env EASYLAUNCHER_HOME).
    #[arg(long, global = true)]
    home: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Build a pack folder (pack.json + self-hosted files) from an instance folder.
    Build {
        /// The game folder of a working instance (e.g. Prism's `minecraft/`).
        instance: PathBuf,
        #[arg(short, long, default_value = "pack.toml")]
        config: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
        /// Don't look files up on Modrinth; self-host everything.
        #[arg(long)]
        no_modrinth: bool,
        /// Override the version label from pack.toml.
        #[arg(long)]
        version: Option<String>,
    },
    /// Write hub.json listing every <site>/<id>/pack.json.
    Hub {
        site: PathBuf,
        #[arg(long)]
        name: String,
    },
    /// Install/update a pack and launch it (for testing a pack).
    Play {
        manifest: String,
        /// Use an offline account with this name.
        #[arg(long)]
        offline: Option<String>,
        /// Only sync and prepare; don't start the game.
        #[arg(long)]
        no_launch: bool,
    },
    /// Show what an update would change.
    Check { manifest: String },
}

fn reporter() -> Reporter {
    Arc::new(|e| match e {
        Event::Stage { text } => eprintln!("==> {text}"),
        Event::Log { text } => eprintln!("    {text}"),
        Event::Game { line } => println!("{line}"),
        Event::Progress { .. } => {}
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let paths = Paths::new(cli.home.clone().unwrap_or_else(Paths::default_root));
    match cli.cmd {
        Cmd::Build { instance, config, out, no_modrinth, version } => build::build(&instance, &config, &out, !no_modrinth, version).await,
        Cmd::Hub { site, name } => build::hub(&site, &name),
        Cmd::Check { manifest } => {
            let l = Launcher::new(paths);
            let (pack, plan) = l.check(&to_url(&manifest)?).await?;
            println!("{} {} → {}", pack.name, plan.from_version.as_deref().unwrap_or("(not installed)"), plan.to_version);
            for op in &plan.ops {
                println!("  {}", serde_json::to_string(op)?);
            }
            println!("download: {} MB", plan.download_bytes / 1_000_000);
            Ok(())
        }
        Cmd::Play { manifest, offline, no_launch } => {
            let l = Launcher::new(paths.clone());
            let url = to_url(&manifest)?;
            let r = reporter();
            if let Some(name) = offline {
                let mut a = Accounts::load(&paths)?;
                a.upsert(accounts::offline(&name)?);
                a.save(&paths)?;
            }
            // Browser-only files: list them, wait for them in Downloads, retry.
            loop {
                let res = if no_launch { l.update(&url, &r).await.map(|_| None) } else {
                    let pack = l.fetch_pack(&url).await?;
                    l.play(&url, &pack.id, &r).await.map(Some)
                };
                match res {
                    Ok(None) => return Ok(()),
                    Ok(Some(code)) => {
                        eprintln!("==> game exited with {code}");
                        return Ok(());
                    }
                    Err(e) => {
                        let Some(need) = e.downcast_ref::<easylauncher_core::manual::NeedsDownloads>() else { return Err(e) };
                        wait_for_downloads(&paths, &need.0)?;
                    }
                }
            }
        }
    }
}

fn wait_for_downloads(paths: &Paths, need: &[easylauncher_core::manual::Missing]) -> anyhow::Result<()> {
    use easylauncher_core::manual::{download_dirs, scan};
    let dirs = download_dirs();
    eprintln!("==> {} file(s) must be downloaded in a browser. Open these pages; waiting for the files in {:?}:", need.len(), dirs);
    for m in need {
        eprintln!("    {}  {}", m.name, m.page);
    }
    loop {
        let found = scan(paths, &dirs, need)?;
        if found.len() >= need.len() {
            eprintln!("==> all {} found", need.len());
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

