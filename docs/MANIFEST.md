# Manifest format (schema 1)

Two JSON files make up everything EasyLauncher reads from the internet. Both are
static: any web host, a GitHub Pages site, an S3 bucket, or a `file://` folder works.
Relative URLs are resolved against the URL of the file they appear in.

`ezl build` and `ezl hub` write both files for you. You only need this page to
understand or hand-edit them.

## hub.json — the list of projects

```json
{
  "schema": 1,
  "name": "My Servers",
  "projects": [
    { "id": "atm10", "name": "All the Mods 10", "manifest": "atm10/pack.json",
      "description": "Survival server", "icon": "atm10/icon.png", "version": "2025.11.3" }
  ]
}
```

Fresh installs start with the built-in hub (`DEFAULT_HUB` in
`packages/core/src/library.rs`, overridable at build time with
`EASYLAUNCHER_DEFAULT_HUB`); players can add more by URL. `version` is informational; the app always reads the
pack itself to decide whether an update is needed.

## pack.json — one version of one project

```json
{
  "schema": 1,
  "id": "atm10",
  "name": "All the Mods 10",
  "version": "2025.11.3",
  "description": "…",
  "icon": "icon.png",
  "changelog": "Added Create 6, removed Mekanism Generators",
  "minecraft": {
    "version": "1.21.1",
    "loader": { "type": "neoforge", "version": "21.1.267" },
    "java": 21,
    "memoryMb": 8192,
    "jvmArgs": ["-XX:+UseZGC"]
  },
  "servers": [{ "name": "ATM10", "address": "mc.example.com" }],
  "sync": { "managedDirs": ["mods"], "extraFiles": "disable" },
  "files": [ … ]
}
```

| Field | Meaning |
| --- | --- |
| `id` | Folder name of the instance. Letters, digits, `-`, `_`, `.`. Never change it after release. |
| `version` | Any string. A different string from the installed one means "update available". |
| `minecraft.version` | A version id, or `latest-release` / `latest-snapshot` for rolling servers (resolved when the player syncs). |
| `minecraft.loader.type` | `vanilla`, `fabric`, `quilt`, `forge`, `neoforge`. `version` is optional; without it the newest stable/recommended loader is used. |
| `minecraft.java` | Java major version. Optional; the version JSON says what Mojang ships. |
| `minecraft.memoryMb` | Suggested RAM. The player can override it. |
| `servers` | Added to the top of the player's server list if missing. The first one is joined on Play when "auto join" is on. |
| `sync.managedDirs` | Folders the pack owns (default `["mods"]`). Files in them that the pack doesn't list are "extra files". |
| `sync.extraFiles` | What happens to extra files: `keep`, `disable` (rename to `.disabled`, undone when you go back to `keep`), `delete` (moved to the backup folder). Default `keep`. |

### files

```json
{ "path": "mods/create-6.0.jar", "policy": "managed", "sha1": "…", "size": 123,
  "urls": ["https://cdn.modrinth.com/…", "files/<sha1>"], "optional": false, "name": "Create" }
```

`urls` are tried in order. `optional: true` files show a toggle in the Mods tab.

A file the launcher may not download itself has no `urls` and a `manual` source
instead (`size` is required then):

```json
{ "path": "mods/foo-1.2.jar", "policy": "managed", "sha1": "…", "size": 123, "name": "Foo",
  "manual": { "page": "https://www.curseforge.com/minecraft/mc-mods/foo/download/123456", "filename": "foo-1.2.jar" } }
```

The app lists these, opens the pages in the player's browser (a few at a time),
and takes any file in the Downloads folder whose size and sha1 match. The player
can also pick files by hand. Once found, a file stays in the launcher's cache.

| Policy | On update |
| --- | --- |
| `managed` | Always matches the pack. If the player edited it, their copy is backed up first. Mods use this. |
| `config` | Updated only if the player hasn't touched it since the last install. Their edits win. |
| `seed` | Written once if missing. After that it belongs to the player. |
| `keys` | Line-based `key<sep>value` merge (`options.txt`). `defaults` are written only when the key is missing; `force` always wins. Every other key, including keybinds, stays. No `sha1`/`urls`. |

A file removed from the pack is deleted if it was `managed` and untouched, backed up
if it was edited, and simply forgotten if it was `config`/`seed`.

Backups go to `<instance>/.easylauncher/backup/<timestamp>/`. Per-instance state
lives in `<instance>/.easylauncher/state.json`.

Paths must be relative, use `/`, contain no `..`, and must not start with
`.easylauncher`. The app rejects a pack that breaks this.
