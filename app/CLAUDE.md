# app (macos, windows)

Tauri 2 shell (`src-tauri/src/lib.rs`, thin commands over core) and Svelte 5 UI (`src/`).

- Run: `npx tauri dev` (set `EASYLAUNCHER_HOME` to a scratch folder for testing).
- Check: `npx svelte-check` and `npm run build`.
- Command names/args in `src/lib/api.ts` must match `lib.rs` (Tauri maps snake_case args to camelCase).
- Long work runs in core and reports through `task` / `task-done` events; `src/lib/tasks.svelte.ts`
  holds per-project state. Don't write `$state` from code that runs during rendering.
- Audience is non-technical friends: plain words, no jargon in the UI.
- Icons: edit `icon.svg`, then `npx tauri icon icon.svg -o src-tauri/icons` (delete android/ios output).
