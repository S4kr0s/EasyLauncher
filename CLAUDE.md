# EasyLauncher

Game launcher for friends: hubs of projects, update-without-losing-settings sync, Minecraft first.
Design: `docs/DESIGN.md`. Wire format: `docs/MANIFEST.md`.

Parts: `packages/core` (lib), `cli` (`ezl`), `app` (Tauri + Svelte). Cargo workspace at the root.

- Rust comes from Homebrew rustup, which is keg-only: start cargo commands with
  `export PATH=/opt/homebrew/opt/rustup/bin:$PATH`.
- Tests: `cargo test -p easylauncher-core`. The sync acceptance test is `packages/core/tests/sync_policies.rs`;
  any change to sync policies must keep it green and extend it.
- Never log full java command lines or classpaths (huge). Never read other launchers' account files.
- The pack format is a public contract (schema 1). Additive changes only; bump `SCHEMA` otherwise.
