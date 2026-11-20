# EasyLauncher

A small launcher you hand to friends. They install it, see your projects, press
**Play**. When you change the modpack, they get the update on their next Play, and
their keybinds, map data, options and config tweaks stay where they were.

- **Platforms:** macOS and Windows (Tauri 2: a 5 MB download, no Electron).
- **Minecraft:** vanilla (also `latest-release` / `latest-snapshot` rolling), Fabric,
  Quilt, Forge, NeoForge. Java is downloaded per version from Mojang. Microsoft and
  offline accounts. Per-project RAM, Java, JVM arguments, window size.
- **No backend.** A project is a static folder (`pack.json` + `files/`), a hub is a
  `hub.json` listing projects. Host it anywhere.
- **Other games** are planned through a generic "files into a folder" adapter
  (see [docs/DESIGN.md](docs/DESIGN.md)).

## Layout

| Path | What |
| --- | --- |
| `packages/core` | Rust library: manifests, sync engine, downloads, Minecraft adapter, accounts. |
| `cli` | `ezl`, the pack author tool: `build`, `hub`, `play`, `check`. |
| `app` | The Tauri desktop app (Svelte 5 UI). |
| `docs` | [DESIGN.md](docs/DESIGN.md), [MANIFEST.md](docs/MANIFEST.md), [example-pack.toml](docs/example-pack.toml). |

## Publishing a pack

```bash
cargo run -p ezl -- build ~/path/to/instance/minecraft -c pack.toml -o site/mypack
cargo run -p ezl -- hub site --name "My Servers"
```

Upload `site/` to any static host and send the `hub.json` URL. To test locally,
`ezl play site/mypack/pack.json --offline Name` installs and starts it.

## Building the app

```bash
cd app && npm install && npx tauri dev
```

```bash
cd app && npx tauri build
```

Build-time settings (environment variables):

- `EASYLAUNCHER_MS_CLIENT_ID`: Azure app client id for Microsoft sign-in. The app
  must be registered as a public client and approved for the Minecraft API
  (https://aka.ms/mce-reviewappid). Without it, only offline accounts work.
- `EASYLAUNCHER_DEFAULT_HUB`: overrides the built-in hub (`DEFAULT_HUB` in
  `packages/core/src/library.rs`); set it to an empty string for none.

`EASYLAUNCHER_HOME` (runtime) moves the data folder, which defaults to
`~/Library/Application Support/EasyLauncher` or `%APPDATA%\EasyLauncher`.
