# EasyLauncher — Design

A small desktop launcher (Windows + macOS) that friends install once, then pick
projects (modpacks, later other modded games), keep them updated and play.
The pack author changes mods and configs at any time; players must
sync before they can play, and the sync never destroys their own settings.

## Goals and non-goals

- **Lightweight.** Tauri (system webview) + Rust. No Electron, no CurseForge
  app, no Overwolf. The launcher downloads only what a pack needs.
- **No backend.** A pack is a static folder (JSON manifest + optional files)
  that can live on any static host: GitHub Pages / Releases, Cloudflare R2,
  nginx on the game server, a local folder for testing. Relative URLs in a
  manifest resolve against the manifest's own URL.
- **Non-technical players.** One "Play" button. Updates are automatic and
  required; options (Java, RAM, JVM args, account) exist but have defaults.
- **Game-agnostic sync, game-specific launch.** The sync engine knows only
  files and policies. A game adapter (Minecraft now; generic "drop files into
  a game folder" later for Cyberpunk/BG3/Palworld) resolves and launches.

## Repository layout

| Path              | Part   | What                                                     |
|-------------------|--------|----------------------------------------------------------|
| `packages/core`   | lib    | Rust: manifest format, sync engine, Minecraft adapter, Java runtimes, auth |
| `cli`             | cli    | `ezl` — author tool: build a pack from an instance folder, build a hub |
| `app`             | app    | Tauri desktop app (Rust shell + Svelte UI)               |
| `docs`            |        | this file, manifest reference                            |

## Concepts

- **Hub**: `hub.json`, a list of projects. Players add a hub URL once (the
  installer can ship with a default hub baked in).
- **Pack**: `pack.json`, one project version. Has a game section and a file list.
- **Instance**: the local copy of a pack, `<data>/instances/<pack id>/`.
  `<instance>/.easylauncher/state.json` records what the launcher last
  installed (path → sha1 + policy). This record is what makes safe updates
  possible.
- **Object cache**: `<data>/cache/objects/<sha1[0..2]>/<sha1>`. Every
  downloaded pack file lands here first, verified by hash, then is copied into
  instances. Shared across instances and pack versions.

Data dir: macOS `~/Library/Application Support/EasyLauncher`, Windows
`%APPDATA%\EasyLauncher`.

## File policies (the core of the update model)

Every manifest file has a `policy`:

| Policy    | Absent locally | Present, untouched since last install | Present, changed by player | Removed from manifest |
|-----------|----------------|---------------------------------------|----------------------------|-----------------------|
| `managed` | install        | update                                | **overwrite** (player copy backed up) | delete        |
| `config`  | install        | update                                | keep player's file         | delete if untouched, else keep |
| `seed`    | install        | keep                                  | keep                       | keep (forget)         |
| `keys`    | create         | merge keys                            | merge keys                 | forget                |

- "Untouched" = local sha1 equals the sha1 recorded in `state.json`.
- `managed` is for mods, loader files and server-critical configs (a config a
  mod rewrites at runtime but the server needs to match gets `managed`).
- `keys` is for `key:value` / `key=value` files such as `options.txt`: the
  manifest gives `defaults` (only set when the key is missing — first-run
  keybinds, GUI scale) and `force` (always set — e.g. a resource pack order the
  pack depends on). Every other key (the player's keybinds, FOV, volume) stays.
- Files the launcher never installed are never touched — saves, Xaero/
  JourneyMap data, screenshots, `servers.dat` extras, shader settings — with
  one exception: **managed directories**.
- `managedDirs` (default `["mods"]`): unknown files there are handled by
  `extraFiles`: `"keep"` (players may add own mods), `"disable"` (renamed to
  `*.disabled` so the server modlist matches; reversible), or `"delete"`.
- `optional: true` files can be switched off by the player (stored in
  state, honoured on every sync).
- Backups of overwritten player files go to
  `<instance>/.easylauncher/backup/<timestamp>/`.

Sync is two-phase: plan (pure function of manifest + state + disk → list of
actions, shown in UI) then apply (downloads into cache, then file operations,
then writes state). A failed apply leaves state describing the previous
version for files not yet touched, so a re-run converges.

## Minecraft adapter

- **Version resolution**: `minecraft.version` is an exact id or
  `latest-release` / `latest-snapshot`, resolved against Mojang's
  `version_manifest_v2.json` at sync time (rolling vanilla servers).
- **Loaders**: `vanilla`, `fabric`, `quilt` (meta profile JSON merged via
  `inheritsFrom`), `forge`, `neoforge` (official installer run headless by the
  launcher's Java into the shared meta dir, then the produced version JSON is
  used). Legacy Forge (1.7.10 / GTNH) and lwjgl3ify are later work.
- **Java**: Mojang's own runtime manifests (`java-runtime-*` components named
  in the version JSON), downloaded per platform into `<data>/runtimes`. A
  player may override with a custom `java` path per instance.
- **Launch**: classpath from libraries (OS/arch rules), natives extraction
  for old versions, asset index + objects, argument templating for both
  `arguments` (1.13+) and `minecraftArguments` (legacy). RAM (`-Xmx/-Xms`),
  extra JVM args, window size, and `--quickPlayMultiplayer` to join the
  pack's server directly. The pack's `servers` are added to `servers.dat`.
- **Accounts**: Microsoft device-code flow → Xbox Live → XSTS → Minecraft
  services. Requires an Azure app registration with Minecraft API access
  (client id is a build/config setting). Offline accounts for testing.

## Author workflow (`ezl`)

```
ezl build <instance dir> --config pack.toml --out site/gango
ezl hub site --name "My Servers"
```

`pack.toml` names the Minecraft version/loader and glob rules → policy
(e.g. `mods/** = managed`, `config/** = config`, `options.txt = keys`).
Jars are looked up on Modrinth by sha1 and referenced by CDN URL; anything
unknown is copied into `<out>/files/<sha1>` and referenced relatively.
Bumping `version` and re-uploading the folder is the whole release process.

Where each jar comes from (`ezl build` decides, by exact sha1 only):

1. **Modrinth**: the file's sha1 is on Modrinth → the pack links the Modrinth CDN.
2. **CurseForge** (Prism's `mods/.index/*.pw.toml` records it) → a `manual`
   entry with the CurseForge download page. The player's browser downloads it,
   the launcher picks it up from Downloads by size + sha1 (`core/src/manual.rs`).
   Same idea as Prism's "blocked mods" dialog; nothing is re-hosted.
3. **`page = …` rule**: same browser flow for any site (gated playtests too).
4. **`self_host = true` rule**: copied to `files/<sha1>`. Only for files the
   author may share (own mods). Unknown jars without a rule fail the build, so
   nothing is re-hosted by accident.

Later: with a CurseForge API key (used only by `ezl` on the author's machine),
files whose authors allow third-party downloads can get direct CDN links, so
only truly blocked ones need the browser. ATM10 has 153 CurseForge-only files.

## Other games (later)

A `files` adapter: target folder = detected game install (Steam library
scan) or a player-chosen folder, same policies, launch = run the game exe /
`steam://run/<appid>`. BG3's `modsettings.lsx` is a natural `managed`/`keys`
candidate; Cyberpunk mods are plain files under the game folder.

## Distribution

Tauri bundler: `.dmg`/`.app` on macOS, NSIS `.exe` installer on Windows
(built in CI on a Windows runner). Tauri updater for launcher self-updates.
Unsigned builds trigger Gatekeeper / SmartScreen warnings; signing
(Apple Developer ID, Windows code-signing cert) is a cost decision.
