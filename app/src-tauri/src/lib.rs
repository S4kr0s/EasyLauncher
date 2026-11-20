//! Tauri shell: exposes the core library to the UI as commands and streams
//! progress as `task` events.

use easylauncher_core::accounts::{self, AccountInfo, Accounts, DeviceCode};
use easylauncher_core::library::{HubStatus, InstanceSettings, Launcher, Project, Settings};
use easylauncher_core::manifest::Pack;
use easylauncher_core::manual::{self, Missing, NeedsDownloads};
use easylauncher_core::paths::Paths;
use easylauncher_core::progress::{Event, Reporter};
use easylauncher_core::sync::{self, Op};
use serde::Serialize;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

struct App {
    launcher: Launcher,
    /// Project ids with a running task (install/update/game).
    busy: Mutex<HashSet<String>>,
}

type Res<T> = Result<T, String>;

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskEvent {
    project: String,
    event: Event,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskDone {
    project: String,
    kind: String,
    error: Option<String>,
    exit_code: Option<i32>,
    /// Files the player has to fetch in the browser before this can continue.
    needs_downloads: Option<Vec<Missing>>,
}

/// Splits a task error into "needs browser downloads" and a plain message.
fn outcome(e: anyhow::Error) -> (Option<String>, Option<Vec<Missing>>) {
    match e.downcast_ref::<NeedsDownloads>() {
        Some(n) => (None, Some(n.0.clone())),
        None => (Some(err(e)), None),
    }
}

fn reporter(app: &AppHandle, project: &str) -> Reporter {
    let app = app.clone();
    let project = project.to_string();
    Arc::new(move |event| {
        let _ = app.emit("task", TaskEvent { project: project.clone(), event });
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Overview {
    projects: Vec<Project>,
    hubs: Vec<HubStatus>,
    running: Vec<String>,
}

#[tauri::command]
async fn overview(s: State<'_, App>) -> Res<Overview> {
    let (projects, hubs) = s.launcher.projects().await.map_err(err)?;
    let running = s.busy.lock().unwrap().iter().cloned().collect();
    Ok(Overview { projects, hubs, running })
}

#[tauri::command]
async fn add_hub(s: State<'_, App>, url: String) -> Res<String> {
    s.launcher.add_hub(&url).await.map(|h| h.name).map_err(err)
}

#[tauri::command]
fn remove_hub(s: State<'_, App>, url: String) -> Res<()> {
    s.launcher.remove_hub(&url).map_err(err)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Details {
    pack: Pack,
    /// Files that would change on update (0 = up to date).
    changes: usize,
    kept: Vec<String>,
    download_bytes: u64,
    installed_version: Option<String>,
    disabled_optional: Vec<String>,
}

#[tauri::command]
async fn details(s: State<'_, App>, manifest_url: String) -> Res<Details> {
    let (pack, plan) = s.launcher.check(&manifest_url).await.map_err(err)?;
    let st = sync::load_state(&s.launcher.instance_dir(&pack.id)).map_err(err)?;
    Ok(Details {
        changes: plan.ops.iter().filter(|o| o.changes_disk()).count(),
        kept: plan.ops.iter().filter_map(|o| if let Op::KeepPlayer { path } = o { Some(path.clone()) } else { None }).collect(),
        download_bytes: plan.download_bytes,
        installed_version: st.version,
        disabled_optional: st.disabled_optional.into_iter().collect(),
        pack,
    })
}

fn start(s: &State<'_, App>, id: &str) -> Res<()> {
    let mut b = s.busy.lock().unwrap();
    if !b.insert(id.to_string()) {
        return Err("already running".into());
    }
    Ok(())
}

#[tauri::command]
async fn update(app: AppHandle, s: State<'_, App>, manifest_url: String, id: String) -> Res<()> {
    start(&s, &id)?;
    let launcher = s.launcher.clone();
    let r = reporter(&app, &id);
    let res = launcher.update(&manifest_url, &r).await;
    s.busy.lock().unwrap().remove(&id);
    let (error, needs_downloads) = res.err().map(outcome).unwrap_or_default();
    let _ = app.emit("task-done", TaskDone { project: id, kind: "update".into(), error: error.clone(), exit_code: None, needs_downloads });
    error.map_or(Ok(()), Err)
}

/// Updates and starts the game in the background; completion arrives as a
/// `task-done` event.
#[tauri::command]
async fn play(app: AppHandle, s: State<'_, App>, manifest_url: String, id: String) -> Res<()> {
    start(&s, &id)?;
    let launcher = s.launcher.clone();
    let r = reporter(&app, &id);
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let res = launcher.play(&manifest_url, &id, &r).await;
        let state = app2.state::<App>();
        state.busy.lock().unwrap().remove(&id);
        let (error, exit_code, needs_downloads) = match res {
            Ok(c) => (None, Some(c), None),
            Err(e) => {
                let (error, needs) = outcome(e);
                (error, None, needs)
            }
        };
        let _ = app2.emit("task-done", TaskDone { project: id, kind: "play".into(), error, exit_code, needs_downloads });
    });
    Ok(())
}

/// Looks in the Downloads folder for the files; returns the sha1s now available.
#[tauri::command]
async fn scan_downloads(s: State<'_, App>, items: Vec<Missing>) -> Res<Vec<String>> {
    let paths = s.launcher.paths.clone();
    tauri::async_runtime::spawn_blocking(move || manual::scan(&paths, &manual::download_dirs(), &items))
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)
}

/// Takes a file the player picked by hand; returns its sha1 if it was one of `items`.
#[tauri::command]
async fn import_download(s: State<'_, App>, path: String, items: Vec<Missing>) -> Res<Option<String>> {
    let paths = s.launcher.paths.clone();
    tauri::async_runtime::spawn_blocking(move || manual::import(&paths, std::path::Path::new(&path), &items))
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)
}

#[tauri::command]
fn set_optional(s: State<'_, App>, id: String, path: String, enabled: bool) -> Res<()> {
    let inst = s.launcher.instance_dir(&id);
    let mut st = sync::load_state(&inst).map_err(err)?;
    if enabled {
        st.disabled_optional.remove(&path);
    } else {
        st.disabled_optional.insert(path);
    }
    sync::save_state(&inst, &st).map_err(err)
}

#[tauri::command]
fn get_settings(s: State<'_, App>) -> Res<Settings> {
    s.launcher.settings().map_err(err)
}

#[tauri::command]
fn save_settings(s: State<'_, App>, settings: Settings) -> Res<()> {
    s.launcher.save_settings(&settings).map_err(err)
}

#[tauri::command]
fn get_instance_settings(s: State<'_, App>, id: String) -> Res<InstanceSettings> {
    s.launcher.instance_settings(&id).map_err(err)
}

#[tauri::command]
fn save_instance_settings(s: State<'_, App>, id: String, settings: InstanceSettings) -> Res<()> {
    s.launcher.save_instance_settings(&id, &settings).map_err(err)
}

#[tauri::command]
fn open_folder(s: State<'_, App>, id: String) -> Res<()> {
    let dir = s.launcher.instance_dir(&id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    tauri_plugin_opener::open_path(dir, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_url(url: String) -> Res<()> {
    if !url.starts_with("https://") {
        return Err("only https links".into());
    }
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn uninstall(s: State<'_, App>, id: String) -> Res<()> {
    if s.busy.lock().unwrap().contains(&id) {
        return Err("stop the game first".into());
    }
    s.launcher.uninstall(&id).map_err(err)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountsView {
    accounts: Vec<AccountInfo>,
    microsoft_available: bool,
}

#[tauri::command]
fn list_accounts(s: State<'_, App>) -> Res<AccountsView> {
    let a = Accounts::load(&s.launcher.paths).map_err(err)?;
    Ok(AccountsView { accounts: a.info(), microsoft_available: s.launcher.client_id().is_some() })
}

#[tauri::command]
fn add_offline(s: State<'_, App>, name: String) -> Res<()> {
    let mut a = Accounts::load(&s.launcher.paths).map_err(err)?;
    a.upsert(accounts::offline(name.trim()).map_err(err)?);
    a.save(&s.launcher.paths).map_err(err)
}

#[tauri::command]
fn select_account(s: State<'_, App>, id: String) -> Res<()> {
    let mut a = Accounts::load(&s.launcher.paths).map_err(err)?;
    if a.accounts.iter().any(|x| x.id == id) {
        a.active = Some(id);
    }
    a.save(&s.launcher.paths).map_err(err)
}

#[tauri::command]
fn remove_account(s: State<'_, App>, id: String) -> Res<()> {
    let mut a = Accounts::load(&s.launcher.paths).map_err(err)?;
    a.remove(&id);
    a.save(&s.launcher.paths).map_err(err)
}

#[tauri::command]
async fn ms_login_start(s: State<'_, App>) -> Res<DeviceCode> {
    let cid = s.launcher.client_id().ok_or("Microsoft login is not configured in this build")?;
    accounts::ms_start(&s.launcher.http, &cid).await.map_err(err)
}

/// `true` once the account was added.
#[tauri::command]
async fn ms_login_poll(s: State<'_, App>, device_code: String) -> Res<bool> {
    let cid = s.launcher.client_id().ok_or("Microsoft login is not configured")?;
    match accounts::ms_poll(&s.launcher.http, &cid, &device_code).await.map_err(err)? {
        None => Ok(false),
        Some(acc) => {
            let mut a = Accounts::load(&s.launcher.paths).map_err(err)?;
            a.upsert(acc);
            a.save(&s.launcher.paths).map_err(err)?;
            Ok(true)
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let launcher = Launcher::new(Paths::new(Paths::default_root()));
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(App { launcher, busy: Mutex::new(HashSet::new()) })
        .invoke_handler(tauri::generate_handler![
            overview,
            add_hub,
            remove_hub,
            details,
            update,
            play,
            set_optional,
            scan_downloads,
            import_download,
            get_settings,
            save_settings,
            get_instance_settings,
            save_instance_settings,
            open_folder,
            open_url,
            uninstall,
            list_accounts,
            add_offline,
            select_account,
            remove_account,
            ms_login_start,
            ms_login_poll
        ])
        .run(tauri::generate_context!())
        .expect("error while running EasyLauncher");
}
