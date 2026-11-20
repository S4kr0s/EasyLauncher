<script lang="ts">
  import { onMount } from "svelte";
  import { api, type HubStatus, type Settings } from "./lib/api";
  import Icon from "./Icon.svelte";

  let { hubs, onClose }: { hubs: HubStatus[]; onClose: () => void } = $props();
  let settings = $state<Settings | null>(null);
  let hubInput = $state("");
  let error = $state<string | null>(null);
  let busy = $state(false);
  let showAdvanced = $state(false);

  onMount(async () => (settings = await api.getSettings()));

  async function save() {
    if (settings) await api.saveSettings($state.snapshot(settings));
  }

  async function addHub() {
    busy = true;
    error = null;
    try {
      await api.addHub(hubInput);
      hubInput = "";
      settings = await api.getSettings();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function removeHub(url: string) {
    await api.removeHub(url);
    settings = await api.getSettings();
  }

  const name = (url: string) => hubs.find((h) => h.url === url)?.name ?? url;
</script>

<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && onClose()}>
  <div class="modal" role="dialog" aria-label="Settings">
    <h2>Settings</h2>
    {#if settings}
      <h3>Hubs</h3>
      <p class="hint">A hub is a link to a list of projects, usually from a friend.</p>
      {#each settings.hubs as h}
        <div class="hub">
          <div class="who"><b>{name(h)}</b><span class="hint url">{h}</span></div>
          <button class="ghost" aria-label="Remove hub" onclick={() => removeHub(h)}><Icon name="x" size={14} /></button>
        </div>
      {/each}
      <form class="row" onsubmit={(e) => { e.preventDefault(); addHub(); }}>
        <input placeholder="https://…/hub.json" bind:value={hubInput} style="flex: 1" />
        <button disabled={!hubInput || busy}>Add hub</button>
      </form>
      {#if error}<p class="error">{error}</p>{/if}

      <h3>Defaults</h3>
      <label class="field">
        <span>Memory for new projects: {(settings.defaultMemoryMb / 1024).toFixed(1)} GB</span>
        <input type="range" min="1024" max="32768" step="512" bind:value={settings.defaultMemoryMb} onchange={save} />
      </label>

      <button class="ghost link" onclick={() => (showAdvanced = !showAdvanced)}>{showAdvanced ? "Hide" : "Show"} advanced</button>
      {#if showAdvanced}
        <div class="adv">
          <label class="field">
            <span>Java arguments for every project</span>
            <textarea rows="2" value={settings.jvmArgs.join(" ")}
              onchange={(e) => { settings!.jvmArgs = (e.target as HTMLTextAreaElement).value.split(/\s+/).filter(Boolean); save(); }}></textarea>
          </label>
          <label class="field">
            <span>Microsoft app client id</span>
            <input value={settings.msClientId ?? ""} placeholder="Built-in" onchange={(e) => { settings!.msClientId = (e.target as HTMLInputElement).value.trim() || null; save(); }} />
            <span class="hint">Only needed if this build has no Microsoft sign-in.</span>
          </label>
        </div>
      {/if}
    {/if}
    <div class="row end"><button onclick={onClose}>Done</button></div>
  </div>
</div>

<style>
  h3 { margin: 18px 0 6px; font-size: 13px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted); }
  h3:first-of-type { margin-top: 0; }
  .hub { display: flex; gap: 8px; align-items: center; padding: 8px 10px; border: 1px solid var(--line); border-radius: 10px; margin: 8px 0; }
  .who { flex: 1; display: grid; min-width: 0; }
  .url { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .adv { display: grid; gap: 14px; margin-top: 8px; }
  .link { color: var(--accent); padding: 8px 0; margin-top: 10px; }
  .end { justify-content: flex-end; margin-top: 18px; }
</style>
