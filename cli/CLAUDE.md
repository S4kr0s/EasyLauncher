# cli (`ezl`)

Pack author tool. `build` turns an instance folder + `pack.toml` into `pack.json` + `files/<sha1>`
(Modrinth lookup by sha1 for jars/zips), `hub` writes `hub.json`, `play`/`check` drive core for testing.

- `pack.toml` uses `deny_unknown_fields`; keep `docs/example-pack.toml` in sync when adding fields.
- `play --home <dir>` keeps test runs out of the real data folder.
