//! Acceptance test for the update model: install v1, let the "player" change
//! things, sync to v2, and check that every policy held.

use easylauncher_core::download::Http;
use easylauncher_core::hash::sha1_bytes;
use easylauncher_core::manifest::*;
use easylauncher_core::paths::Paths;
use easylauncher_core::progress::noop;
use easylauncher_core::sync::{load_state, plan, sync, Op};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

struct Site {
    dir: std::path::PathBuf,
}

impl Site {
    /// Stores `data` under files/<sha1> and returns a manifest entry for it.
    fn file(&self, path: &str, policy: Policy, data: &[u8]) -> FileEntry {
        let sha = sha1_bytes(data);
        fs::create_dir_all(self.dir.join("files")).unwrap();
        fs::write(self.dir.join("files").join(&sha), data).unwrap();
        FileEntry {
            manual: None,
            path: path.into(),
            policy,
            sha1: Some(sha),
            size: Some(data.len() as u64),
            urls: vec![format!("files/{}", sha1_bytes(data))],
            optional: false,
            name: None,
            defaults: BTreeMap::new(),
            force: BTreeMap::new(),
            separator: None,
        }
    }
    fn publish(&self, pack: &Pack) -> String {
        fs::write(self.dir.join("pack.json"), serde_json::to_vec(pack).unwrap()).unwrap();
        url::Url::from_file_path(self.dir.join("pack.json")).unwrap().to_string()
    }
}

fn keys(path: &str, defaults: &[(&str, &str)], force: &[(&str, &str)]) -> FileEntry {
    FileEntry {
            manual: None,
        path: path.into(),
        policy: Policy::Keys,
        sha1: None,
        size: None,
        urls: vec![],
        optional: false,
        name: None,
        defaults: defaults.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
        force: force.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
        separator: None,
    }
}

fn pack(version: &str, files: Vec<FileEntry>, extra: ExtraFiles) -> Pack {
    Pack {
        schema: SCHEMA,
        id: "test".into(),
        name: "Test".into(),
        version: version.into(),
        description: None,
        icon: None,
        changelog: None,
        minecraft: None,
        servers: vec![],
        sync: SyncRules { managed_dirs: vec!["mods".into()], extra_files: extra },
        files,
    }
}

fn read(p: &Path) -> String {
    fs::read_to_string(p).unwrap()
}

#[tokio::test]
async fn update_preserves_player_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let site = Site { dir: tmp.path().join("site") };
    let paths = Paths::new(tmp.path().join("data"));
    let inst = paths.instance("test");
    let http = Http::new();
    let r = noop();

    // v1
    let v1 = pack(
        "1",
        vec![
            site.file("mods/a.jar", Policy::Managed, b"mod a v1"),
            site.file("mods/b.jar", Policy::Managed, b"mod b v1"),
            site.file("config/a.toml", Policy::Config, b"a=1"),
            site.file("config/b.toml", Policy::Config, b"b=1"),
            site.file("config/server.toml", Policy::Managed, b"pvp=false"),
            site.file("config/seed.toml", Policy::Seed, b"seed=1"),
            keys("options.txt", &[("key_key.jump", "key.keyboard.space"), ("guiScale", "2")], &[]),
        ],
        ExtraFiles::Keep,
    );
    let url = site.publish(&v1);
    sync(&http, &paths, &v1, &url, &inst, &r).await.unwrap();
    assert_eq!(read(&inst.join("mods/a.jar")), "mod a v1");
    assert!(read(&inst.join("options.txt")).contains("guiScale:2"));

    // The player plays: changes keybinds and a config, edits the server
    // config, collects map data and adds a mod of their own.
    fs::write(inst.join("options.txt"), "guiScale:3\nkey_key.jump:key.keyboard.j\nfov:0.5\n").unwrap();
    fs::write(inst.join("config/a.toml"), "a=player").unwrap();
    fs::write(inst.join("config/server.toml"), "pvp=true").unwrap();
    fs::write(inst.join("config/seed.toml"), "seed=player").unwrap();
    fs::create_dir_all(inst.join("xaero/world-map")).unwrap();
    fs::write(inst.join("xaero/world-map/region.zip"), "explored").unwrap();
    fs::write(inst.join("mods/my-minimap.jar"), "player mod").unwrap();

    // v2: adds c, removes b, changes every config.
    let v2 = pack(
        "2",
        vec![
            site.file("mods/a.jar", Policy::Managed, b"mod a v2"),
            site.file("mods/c.jar", Policy::Managed, b"mod c v1"),
            site.file("config/a.toml", Policy::Config, b"a=2"),
            site.file("config/b.toml", Policy::Config, b"b=2"),
            site.file("config/server.toml", Policy::Managed, b"pvp=false\nnew=1"),
            site.file("config/seed.toml", Policy::Seed, b"seed=2"),
            keys("options.txt", &[("key_key.jump", "key.keyboard.space"), ("key_key.newmod", "key.keyboard.k")], &[("resourcePacks", "[\"vanilla\"]")]),
        ],
        ExtraFiles::Keep,
    );
    let url = site.publish(&v2);
    let (p, state) = sync(&http, &paths, &v2, &url, &inst, &r).await.unwrap();

    assert_eq!(read(&inst.join("mods/a.jar")), "mod a v2", "managed mod updated");
    assert_eq!(read(&inst.join("mods/c.jar")), "mod c v1", "new mod installed");
    assert!(!inst.join("mods/b.jar").exists(), "removed mod deleted");
    assert_eq!(read(&inst.join("mods/my-minimap.jar")), "player mod", "player mod kept");
    assert_eq!(read(&inst.join("config/a.toml")), "a=player", "edited config kept");
    assert_eq!(read(&inst.join("config/b.toml")), "b=2", "untouched config updated");
    assert_eq!(read(&inst.join("config/server.toml")), "pvp=false\nnew=1", "managed config enforced");
    assert_eq!(read(&inst.join("config/seed.toml")), "seed=player", "seed belongs to player");
    assert_eq!(read(&inst.join("xaero/world-map/region.zip")), "explored", "map data untouched");
    let opts = read(&inst.join("options.txt"));
    assert!(opts.contains("key_key.jump:key.keyboard.j"), "player keybind kept: {opts}");
    assert!(opts.contains("guiScale:3") && opts.contains("fov:0.5"), "player options kept");
    assert!(opts.contains("key_key.newmod:key.keyboard.k"), "new default added");
    assert!(opts.contains("resourcePacks:[\"vanilla\"]"), "forced key set");
    assert!(p.ops.contains(&Op::Update { path: "config/server.toml".into(), backup: true }));
    let backups: Vec<_> = walkdir::WalkDir::new(inst.join(".easylauncher/backup"))
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .collect();
    assert_eq!(backups.len(), 1, "player's server.toml backed up");
    assert_eq!(state.version.as_deref(), Some("2"));

    // A second sync is a no-op.
    let st = load_state(&inst).unwrap();
    let again = plan(&paths, &v2, &inst, &st).unwrap();
    assert!(again.is_noop(), "{:?}", again.ops);

    // v3 locks the mod folder: player mods get disabled, then restored when
    // the pack allows extra mods again.
    let mut v3 = v2.clone();
    v3.version = "3".into();
    v3.sync.extra_files = ExtraFiles::Disable;
    let url = site.publish(&v3);
    sync(&http, &paths, &v3, &url, &inst, &r).await.unwrap();
    assert!(!inst.join("mods/my-minimap.jar").exists());
    assert!(inst.join("mods/my-minimap.jar.disabled").exists());
    let mut v4 = v2.clone();
    v4.version = "4".into();
    let url = site.publish(&v4);
    sync(&http, &paths, &v4, &url, &inst, &r).await.unwrap();
    assert_eq!(read(&inst.join("mods/my-minimap.jar")), "player mod", "player mod restored");
}

#[test]
fn rejects_hostile_paths() {
    let mut p = pack("1", vec![], ExtraFiles::Keep);
    p.files.push(keys("../../evil.txt", &[], &[]));
    assert!(p.validate().is_err());
}

/// A browser-only file blocks the sync until it shows up in Downloads with the
/// right sha1; a same-size file with other content is ignored.
#[tokio::test]
async fn manual_files_come_from_downloads() {
    use easylauncher_core::manual::{scan, NeedsDownloads};
    let tmp = tempfile::tempdir().unwrap();
    let site = Site { dir: tmp.path().join("site") };
    let paths = Paths::new(tmp.path().join("home"));
    let inst = tmp.path().join("inst");
    fs::create_dir_all(&inst).unwrap();
    let http = Http::new();

    let jar = b"blocked mod contents";
    let mut entry = site.file("mods/blocked.jar", Policy::Managed, jar);
    entry.urls.clear();
    entry.manual = Some(ManualSource { page: "https://example.com/download/1".into(), filename: Some("blocked.jar".into()) });
    let p = pack("1", vec![entry, site.file("mods/normal.jar", Policy::Managed, b"normal")], ExtraFiles::Keep);
    p.validate().unwrap();
    let url = site.publish(&p);

    let err = sync(&http, &paths, &p, &url, &inst, &noop()).await.unwrap_err();
    let need = err.downcast_ref::<NeedsDownloads>().expect("needs downloads").0.clone();
    assert_eq!(need.len(), 1);
    assert!(!inst.join("mods/normal.jar").exists(), "nothing is installed while files are missing");

    let downloads = tmp.path().join("Downloads");
    fs::create_dir_all(&downloads).unwrap();
    fs::write(downloads.join("blocked.jar"), b"blocked mod CONTENTS").unwrap(); // same size, wrong file
    assert!(scan(&paths, &[downloads.clone()], &need).unwrap().is_empty());
    fs::write(downloads.join("blocked (1).jar"), jar).unwrap();
    assert_eq!(scan(&paths, &[downloads], &need).unwrap(), vec![need[0].sha1.clone()]);

    sync(&http, &paths, &p, &url, &inst, &noop()).await.unwrap();
    assert_eq!(fs::read(inst.join("mods/blocked.jar")).unwrap(), jar);
    assert!(inst.join("mods/normal.jar").exists());
}
