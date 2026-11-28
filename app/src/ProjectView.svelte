<script lang="ts">
  import { onMount, tick } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { api, mb, type Details, type InstanceSettings, type Project } from "./lib/api";
  import { tasks } from "./lib/tasks.svelte";
  import Icon from "./Icon.svelte";
  import DownloadsModal from "./DownloadsModal.svelte";

  let { project, hasAccount, onAccount, onChanged }: { project: Project; hasAccount: boolean; onAccount: () => void; onChanged: () => void } = $props();

  const task = $derived(tasks.get(project.id));
  let details = $state<Details | null>(null);
  let detailsError = $state<string | null>(null);
  let tab = $state<"overview" | "mods" | "options" | "log">("overview");
  let inst = $state<InstanceSettings | null>(null);
  let savedNote = $state(false);
  let logEl: HTMLElement | undefined = $state();
  let confirmUninstall = $state(false);

  const installed = $derived(!!project.installedVersion);
  const needsUpdate = $derived(!!details && (details.changes > 0 || details.installedVersion !== details.pack.version));
  const mc = $derived(details?.pack.minecraft);
  const optional = $derived(details?.pack.files.filter((f) => f.optional) ?? []);
  const mods = $derived(details?.pack.files.filter((f) => f.path.startsWith("mods/")) ?? []);
  const pct = $derived(task.total > 0 ? Math.min(100, (task.done / task.total) * 100) : null);

  async function load() {
    detailsError = null;
    try {
      details = await api.details(project.manifestUrl);
    } catch (e) {
      detailsError = String(e);
    }
  }

  onMount(async () => {
    load();
    inst = await api.getInstanceSettings(project.id);
  });

  function resume() {
    const kind = task.resume;
    task.needs = null;
    kind === "update" ? update() : play();
  }

  async function play() {
    if (!hasAccount) return onAccount();
    tasks.begin(project.id);
    try {
      await api.play(project.manifestUrl, project.id);
    } catch (e) {
      tasks.done(project.id, String(e));
    }
  }

  async function update() {
    tasks.begin(project.id);
    try {
      await api.update(project.manifestUrl, project.id);
    } catch {
      /* reported via task-done */
    }
    await load();
    onChanged();
  }

  async function saveInst() {
    if (!inst) return;
    await api.saveInstanceSettings(project.id, $state.snapshot(inst));
    savedNote = true;
    setTimeout(() => (savedNote = false), 1500);
  }

  async function pickJava() {
    const p = await open({ multiple: false, directory: false, title: "Choose a java executable" });
    if (typeof p === "string" && inst) {
      inst.javaPath = p;
      saveInst();
    }
  }

  async function toggleOptional(path: string, enabled: boolean) {
    await api.setOptional(project.id, path, enabled);
    await load();
  }

  async function uninstall() {
    await api.uninstall(project.id);
    confirmUninstall = false;
    onChanged();
    load();
  }

  $effect(() => {
    if (tab === "log" && task.log.length && logEl) {
      tick().then(() => logEl && (logEl.scrollTop = logEl.scrollHeight));
    }
  });

  const loaderName: Record<string, string> = { vanilla: "Vanilla", fabric: "Fabric", quilt: "Quilt", forge: "Forge", neoforge: "NeoForge" };
  const memGb = $derived(((inst?.memoryMb ?? mc?.memoryMb ?? 4096) / 1024).toFixed(1));
</script>

{#if task.needs}
  <DownloadsModal items={task.needs} onDone={resume} onCancel={() => (task.needs = null)} />
{/if}

<div class="view">
  <header>
    {#if project.icon}<img class="icon" src={project.icon} alt="" />{:else}<div class="icon ph">{project.name.slice(0, 1)}</div>{/if}
    <div class="title">
      <h1>{project.name}</h1>
      <div class="sub">
        {#if mc}
          <span>Minecraft {mc.version.startsWith("latest") ? `${mc.version.replace("latest-", "latest ")}` : mc.version}</span>
          <span>·</span><span>{loaderName[mc.loader.type] ?? mc.loader.type}{mc.loader.version ? ` ${mc.loader.version}` : ""}</span>
        {/if}
        {#if details}<span>·</span><span>Pack {details.pack.version}</span>{/if}
      </div>
      {#if project.description}<p class="desc">{project.description}</p>{/if}
    </div>
  </header>

  <section class="action">
    {#if task.running}
      <div class="progress">
        <div class="stage">{task.stage}</div>
        <div class="bar"><div class="fill" class:indet={pct === null && !task.playing} style:width={pct === null ? (task.playing ? "100%" : "30%") : `${pct}%`}></div></div>
        {#if task.total > 0}<div class="hint">{mb(task.done)} of {mb(task.total)}</div>{/if}
      </div>
      <button class="primary big" disabled>{task.playing ? "Playing" : "Working…"}</button>
    {:else}
      <div class="progress">
        {#if task.error}
          <div class="error msg">{task.error}</div>
        {:else if detailsError}
          <div class="error msg">Can't reach this project right now. {installed ? "You can still play the installed version." : ""}</div>
        {:else if !installed}
          <div class="hint">{details ? `Download about ${mb(details.downloadBytes)} the first time.` : "Checking…"}</div>
        {:else if needsUpdate}
          <div class="warn msg">Update ready: {details?.changes} file{details?.changes === 1 ? "" : "s"} change. Your settings, keybinds and maps stay.</div>
        {:else}
          <div class="ok msg"><Icon name="check" size={15} /> Up to date</div>
        {/if}
        {#if details && details.kept.length > 0 && !task.error}
          <div class="hint">Keeping your own changes to {details.kept.length} config file{details.kept.length === 1 ? "" : "s"}.</div>
        {/if}
      </div>
      {#if installed && needsUpdate}<button onclick={update}>Update only</button>{/if}
      <button class="primary big" onclick={play}>
        <Icon name="play" size={18} />
        {!hasAccount ? "Add account to play" : !installed ? "Install & Play" : needsUpdate ? "Update & Play" : "Play"}
      </button>
    {/if}
  </section>

  <div class="tabs">
    {#each [["overview", "Overview"], ["mods", `Mods${mods.length ? ` (${mods.length})` : ""}`], ["options", "Options"], ["log", "Log"]] as [k, label]}
      <button class="tab" class:on={tab === k} onclick={() => (tab = k as typeof tab)}>{label}</button>
    {/each}
  </div>

  <div class="body">
    {#if tab === "overview"}
      {#if details?.pack.servers.length}
        <h3>Servers</h3>
        {#each details.pack.servers as s}
          <div class="card row"><Icon name="server" size={16} /><b>{s.name}</b><span class="hint">{s.address}</span></div>
        {/each}
        <p class="hint">Added to your server list. {inst?.autoJoin ? "Play joins the first one directly." : ""}</p>
      {/if}
      {#if details?.pack.changelog}
        <h3>What's new</h3>
        <pre class="changelog">{details.pack.changelog}</pre>
      {/if}
      {#if details && !details.pack.servers.length && !details.pack.changelog}
        <p class="hint">Nothing else to show here yet.</p>
      {/if}
    {:else if tab === "mods"}
      {#if optional.length}
        <h3>Optional</h3>
        {#each optional as f}
          {@const on = !details?.disabledOptional.includes(f.path)}
          <label class="card row toggle">
            <input type="checkbox" checked={on} onchange={(e) => toggleOptional(f.path, (e.target as HTMLInputElement).checked)} />
            <span>{f.name ?? f.path.split("/").pop()}</span>
          </label>
        {/each}
        <p class="hint">Changes apply on the next Play.</p>
      {/if}
      <h3>Included</h3>
      <div class="mods">
        {#each mods.filter((m) => !m.optional) as m}<div class="mod">{m.path.slice(5)}</div>{/each}
      </div>
    {:else if tab === "options" && inst}
      <div class="grid">
        <label class="field">
          <span>Memory: {memGb} GB</span>
          <input type="range" min="1024" max="32768" step="512" value={inst.memoryMb ?? mc?.memoryMb ?? 4096}
            oninput={(e) => (inst!.memoryMb = +(e.target as HTMLInputElement).value)} onchange={saveInst} />
          <span class="hint">{mc?.memoryMb ? `The pack suggests ${(mc.memoryMb / 1024).toFixed(1)} GB.` : "More isn't always faster."}
            {#if inst.memoryMb}<button class="ghost link" onclick={() => { inst!.memoryMb = null; saveInst(); }}>Reset</button>{/if}</span>
        </label>
        <label class="field">
          <span>Java</span>
          <div class="row">
            <input readonly value={inst.javaPath ?? "Automatic (recommended)"} style="flex: 1" />
            <button onclick={pickJava}>Choose…</button>
            {#if inst.javaPath}<button onclick={() => { inst!.javaPath = null; saveInst(); }}>Automatic</button>{/if}
          </div>
        </label>
        <label class="field">
          <span>Extra Java arguments</span>
          <textarea rows="2" value={inst.jvmArgs.join(" ")} placeholder="-XX:+UseZGC"
            onchange={(e) => { inst!.jvmArgs = (e.target as HTMLTextAreaElement).value.split(/\s+/).filter(Boolean); saveInst(); }}></textarea>
        </label>
        <label class="field">
          <span>Window size</span>
          <div class="row">
            <input type="number" placeholder="854" style="width: 90px" value={inst.width ?? ""} onchange={(e) => { inst!.width = +(e.target as HTMLInputElement).value || null; saveInst(); }} />
            ×
            <input type="number" placeholder="480" style="width: 90px" value={inst.height ?? ""} onchange={(e) => { inst!.height = +(e.target as HTMLInputElement).value || null; saveInst(); }} />
          </div>
        </label>
        <label class="row check">
          <input type="checkbox" bind:checked={inst.autoJoin} onchange={saveInst} />
          <span>Join the server when the game starts</span>
        </label>
        <div class="row">
          <button onclick={() => api.openFolder(project.id)}><Icon name="folder" size={14} /> Open game folder</button>
          {#if installed}
            {#if confirmUninstall}
              <span class="error">Deletes this project's folder, including singleplayer worlds.</span>
              <button class="danger" onclick={uninstall}>Delete</button>
              <button onclick={() => (confirmUninstall = false)}>Cancel</button>
            {:else}
              <button class="danger" onclick={() => (confirmUninstall = true)}>Uninstall…</button>
            {/if}
          {/if}
          {#if savedNote}<span class="hint">Saved</span>{/if}
        </div>
      </div>
    {:else if tab === "log"}
      <pre class="log" bind:this={logEl}>{task.log.join("\n") || "Nothing yet. Output appears here while installing and playing."}</pre>
    {/if}
  </div>
</div>

<style>
  .view { height: 100%; display: flex; flex-direction: column; min-height: 0; }
  header { display: flex; gap: 20px; padding: 30px 32px 18px; align-items: flex-start; }
  .icon { width: 92px; height: 92px; border-radius: var(--radius-card); object-fit: cover; flex: none; box-shadow: 0 8px 30px rgba(0, 0, 0, 0.35); }
  .ph { display: grid; place-items: center; background: var(--surface-tile); font-size: 36px; font-weight: 700; color: var(--ink-secondary); }
  h1 { margin: 2px 0 6px; font: var(--text-window-title); font-variant-numeric: tabular-nums; letter-spacing: -0.01em; }
  .sub { display: flex; gap: 6px; color: var(--ink-secondary); flex-wrap: wrap; }
  .desc { margin: 10px 0 0; color: var(--ink); max-width: 640px; line-height: 1.45; }
  .action {
    display: flex; gap: 10px; align-items: center; margin: 0 32px; padding: 16px 18px;
    background: var(--surface-card); border: 1px solid var(--hairline); border-radius: var(--radius-card);
  }
  .progress { flex: 1; display: grid; gap: 6px; min-width: 0; }
  .stage { font-weight: 600; }
  .msg { display: flex; gap: 6px; align-items: center; }
  .ok { color: var(--live); }
  .warn { color: var(--warn); }
  .bar { height: 6px; background: var(--track); border-radius: var(--radius-pill); overflow: hidden; }
  .fill { height: 100%; background: var(--series-1); border-radius: var(--radius-pill); transition: width 0.25s; }
  .fill.indet { animation: slide 1.2s ease-in-out infinite; }
  @keyframes slide { from { transform: translateX(-100%); } to { transform: translateX(340%); } }
  .big { display: flex; align-items: center; gap: 8px; padding: 12px 26px; font-size: 15px; border-radius: var(--radius-md); min-width: 170px; justify-content: center; }
  .tabs { display: flex; gap: 2px; padding: 18px 32px 0; border-bottom: 1px solid var(--hairline); }
  .tab { background: none; border: none; border-bottom: 2px solid transparent; border-radius: 0; padding: 10px 12px; color: var(--ink-secondary); }
  .tab.on { color: var(--ink); border-bottom-color: var(--accent); }
  .body { flex: 1; overflow: auto; padding: 18px 32px 32px; min-height: 0; }
  h3 { margin: 6px 0 10px; font: var(--text-section-label); font-variant-numeric: tabular-nums; color: var(--ink-secondary); }
  .card { background: var(--surface-card); border: 1px solid var(--hairline); border-radius: var(--radius-card); padding: 10px 12px; margin-bottom: 6px; }
  .card :global(svg) { color: var(--ink-secondary); }
  .toggle { cursor: pointer; }
  .changelog { white-space: pre-wrap; font-family: inherit; margin: 0; line-height: 1.5; color: var(--ink); }
  .mods { columns: 2 260px; column-gap: 24px; }
  .mod { padding: 3px 0; color: var(--ink); font-size: 13px; break-inside: avoid; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .grid { display: grid; gap: 18px; max-width: 560px; }
  .check { color: var(--ink); }
  .link { padding: 0 4px; color: var(--accent-text); }
  .log {
    margin: 0; height: 100%; min-height: 240px; overflow: auto; background: var(--surface-card); border: 1px solid var(--hairline);
    border-radius: var(--radius-md); padding: 12px; font-size: 11.5px; line-height: 1.45; white-space: pre-wrap; word-break: break-all;
    font-family: var(--font-mono); color: var(--ink); user-select: text;
  }
</style>
