# core (lib)

Everything the app and CLI share. No UI, no Tauri.

- `manifest.rs` pack/hub types (camelCase JSON). `sync.rs` plan → apply with per-file policies,
  state in `<instance>/.easylauncher/state.json`. `keys.rs` options.txt merge.
- `library.rs` `Launcher`: settings, hubs, check/update/play. `accounts.rs` MS device code + offline.
- `minecraft/`: version JSON merge + rules, Mojang Java runtimes, loaders (Fabric/Quilt meta,
  Forge/NeoForge official installer run headless into `meta/`), launch, servers.dat.
- All paths from a pack go through `paths::safe_join`.
- Progress goes through `progress::Reporter` events; don't print.
