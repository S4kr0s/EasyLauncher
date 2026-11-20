<script lang="ts">
  import { onMount } from "svelte";
  import { api, onTask, onTaskDone, type Account, type Overview, type Project } from "./lib/api";
  import { tasks, type Task } from "./lib/tasks.svelte";
  import ProjectView from "./ProjectView.svelte";
  import AccountsModal from "./AccountsModal.svelte";
  import SettingsModal from "./SettingsModal.svelte";
  import Icon from "./Icon.svelte";

  let overview = $state<Overview | null>(null);
  let selected = $state<string | null>(null);
  let loadError = $state<string | null>(null);
  let accounts = $state<Account[]>([]);
  let showAccounts = $state(false);
  let showSettings = $state(false);
  let hubInput = $state("");
  let hubError = $state<string | null>(null);
  let addingHub = $state(false);

  const project = $derived(overview?.projects.find((p) => p.id === selected) ?? null);
  const active = $derived(accounts.find((a) => a.active));

  async function refresh() {
    try {
      overview = await api.overview();
      loadError = null;
      for (const id of overview.running) tasks.get(id).running = true;
      if (!selected || !overview.projects.some((p) => p.id === selected)) selected = overview.projects[0]?.id ?? null;
    } catch (e) {
      loadError = String(e);
    }
  }

  async function refreshAccounts() {
    accounts = (await api.listAccounts()).accounts;
  }

  async function addHub() {
    addingHub = true;
    hubError = null;
    try {
      await api.addHub(hubInput);
      hubInput = "";
      await refresh();
    } catch (e) {
      hubError = String(e);
    } finally {
      addingHub = false;
    }
  }

  function status(p: Project, t: Task) {
    if (t.running) return { text: t.stage || "Working…", tone: "busy" };
    if (!p.installedVersion) return { text: "Not installed", tone: "muted" };
    if (p.latestVersion && p.latestVersion !== p.installedVersion) return { text: "Update available", tone: "warn" };
    return { text: "Ready", tone: "ok" };
  }

  onMount(() => {
    refresh();
    refreshAccounts();
    const un1 = onTask((id, e) => tasks.handle(id, e));
    const un2 = onTaskDone((d) => {
      tasks.done(d.project, d.error, d.exitCode, d.needsDownloads, d.kind);
      refresh();
    });
    return () => {
      un1.then((f) => f());
      un2.then((f) => f());
    };
  });
</script>

<div class="shell">
  <aside>
    <div class="brand">
      <div class="logo"><Icon name="cube" size={18} /></div>
      <span>EasyLauncher</span>
    </div>

    <nav>
      {#each overview?.projects ?? [] as p (p.id)}
        {@const s = status(p, tasks.get(p.id))}
        <button class="item" class:sel={p.id === selected} onclick={() => (selected = p.id)}>
          {#if p.icon}
            <img src={p.icon} alt="" />
          {:else}
            <div class="ph">{p.name.slice(0, 1)}</div>
          {/if}
          <div class="meta">
            <div class="name">{p.name}</div>
            <div class="st {s.tone}">{s.text}</div>
          </div>
        </button>
      {/each}
      {#if overview && overview.projects.length === 0 && overview.hubs.length > 0}
        <p class="hint pad">No projects in your hubs yet.</p>
      {/if}
    </nav>

    {#each overview?.hubs.filter((h) => h.error) ?? [] as h}
      <p class="hint pad error" title={h.error}>Can't reach a hub. Check your internet.</p>
    {/each}

    <div class="foot">
      <button class="ghost account" onclick={() => (showAccounts = true)}>
        <Icon name="user" size={16} />
        <span>{active ? active.name : "Add account"}</span>
      </button>
      <button class="ghost" title="Settings" aria-label="Settings" onclick={() => (showSettings = true)}>
        <Icon name="gear" size={16} />
      </button>
    </div>
  </aside>

  <main>
    {#if loadError}
      <div class="center"><p class="error">{loadError}</p></div>
    {:else if !overview}
      <div class="center"><p class="hint">Loading…</p></div>
    {:else if overview.hubs.length === 0 && overview.projects.length === 0}
      <div class="center welcome">
        <div class="logo big"><Icon name="cube" size={34} /></div>
        <h1>Welcome!</h1>
        <p class="hint">Paste the link your friend sent you. It adds their projects here.</p>
        <form class="row" onsubmit={(e) => { e.preventDefault(); addHub(); }}>
          <input placeholder="https://…/hub.json" bind:value={hubInput} style="width: 340px" />
          <button class="primary" disabled={!hubInput || addingHub}>{addingHub ? "Adding…" : "Add"}</button>
        </form>
        {#if hubError}<p class="error">{hubError}</p>{/if}
      </div>
    {:else if project}
      {#key project.id}
        <ProjectView {project} hasAccount={!!active} onAccount={() => (showAccounts = true)} onChanged={refresh} />
      {/key}
    {/if}
  </main>
</div>

{#if showAccounts}
  <AccountsModal onClose={() => { showAccounts = false; refreshAccounts(); }} />
{/if}
{#if showSettings}
  <SettingsModal hubs={overview?.hubs ?? []} onClose={() => { showSettings = false; refresh(); }} />
{/if}

<style>
  .shell { display: grid; grid-template-columns: 252px 1fr; height: 100vh; }
  aside {
    background: var(--panel); border-right: 1px solid var(--line);
    display: flex; flex-direction: column; min-height: 0;
  }
  .brand { display: flex; align-items: center; gap: 10px; padding: 18px 16px 14px; font-weight: 700; letter-spacing: 0.01em; }
  .logo {
    width: 30px; height: 30px; border-radius: 9px; display: grid; place-items: center;
    background: linear-gradient(145deg, #8be39d, #4fae67); color: var(--accent-ink);
  }
  .logo.big { width: 64px; height: 64px; border-radius: 18px; margin-bottom: 8px; }
  nav { flex: 1; overflow: auto; padding: 4px 8px; display: grid; align-content: start; gap: 2px; }
  .item {
    display: flex; align-items: center; gap: 10px; width: 100%; text-align: left;
    background: transparent; border: 1px solid transparent; padding: 8px; border-radius: 10px;
  }
  .item:hover { background: var(--panel-2); }
  .item.sel { background: var(--accent-soft); border-color: rgba(123, 216, 143, 0.25); }
  .item img, .ph { width: 40px; height: 40px; border-radius: 9px; object-fit: cover; flex: none; }
  .ph { display: grid; place-items: center; background: var(--panel-2); font-weight: 700; color: var(--muted); }
  .meta { min-width: 0; }
  .name { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .st { font-size: 12px; color: var(--muted); }
  .st.ok { color: var(--accent); }
  .st.warn { color: var(--warn); }
  .st.busy { color: var(--text); }
  .pad { padding: 0 16px; }
  .foot { display: flex; gap: 4px; padding: 10px 8px; border-top: 1px solid var(--line); }
  .account { flex: 1; display: flex; align-items: center; gap: 8px; text-align: left; overflow: hidden; }
  .account span { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  main { min-width: 0; min-height: 0; overflow: hidden; }
  .center { height: 100%; display: grid; place-content: center; justify-items: center; gap: 10px; text-align: center; padding: 24px; }
  .welcome h1 { margin: 0; font-size: 26px; }
</style>
