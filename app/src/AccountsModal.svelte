<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api, type Account, type DeviceCode } from "./lib/api";
  import Icon from "./Icon.svelte";

  let { onClose }: { onClose: () => void } = $props();
  let accounts = $state<Account[]>([]);
  let msAvailable = $state(false);
  let offlineName = $state("");
  let error = $state<string | null>(null);
  let code = $state<DeviceCode | null>(null);
  let timer: ReturnType<typeof setTimeout> | null = null;

  async function load() {
    const r = await api.listAccounts();
    accounts = r.accounts;
    msAvailable = r.microsoftAvailable;
  }
  onMount(load);
  onDestroy(() => timer && clearTimeout(timer));

  async function signIn() {
    error = null;
    try {
      code = await api.msLoginStart();
      api.openUrl(code.verificationUri);
      const deadline = Date.now() + code.expiresIn * 1000;
      const poll = async () => {
        if (!code) return;
        if (Date.now() > deadline) {
          error = "The code expired. Try again.";
          code = null;
          return;
        }
        try {
          if (await api.msLoginPoll(code.deviceCode)) {
            code = null;
            await load();
            return;
          }
        } catch (e) {
          error = String(e);
          code = null;
          return;
        }
        timer = setTimeout(poll, code.interval * 1000);
      };
      timer = setTimeout(poll, code.interval * 1000);
    } catch (e) {
      error = String(e);
    }
  }

  async function addOffline() {
    error = null;
    try {
      await api.addOffline(offlineName);
      offlineName = "";
      await load();
    } catch (e) {
      error = String(e);
    }
  }
</script>

<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && onClose()}>
  <div class="modal" role="dialog" aria-label="Accounts">
    <h2>Accounts</h2>

    {#each accounts as a (a.id)}
      <div class="acc" class:on={a.active}>
        <div class="avatar">{a.name.slice(0, 1).toUpperCase()}</div>
        <div class="who">
          <b>{a.name}</b>
          <span class="hint">{a.kind === "microsoft" ? "Microsoft" : "Offline (singleplayer and offline servers only)"}</span>
        </div>
        {#if a.active}
          <span class="badge"><Icon name="check" size={13} /> Active</span>
        {:else}
          <button onclick={async () => { await api.selectAccount(a.id); load(); }}>Use</button>
        {/if}
        <button class="ghost" title="Remove" aria-label="Remove {a.name}" onclick={async () => { await api.removeAccount(a.id); load(); }}><Icon name="x" size={14} /></button>
      </div>
    {/each}

    {#if code}
      <div class="code">
        <p>On the page that just opened, enter this code and sign in with the Microsoft account you play Minecraft with:</p>
        <div class="big">{code.userCode}</div>
        <div class="row">
          <button onclick={() => navigator.clipboard.writeText(code!.userCode)}>Copy code</button>
          <button onclick={() => api.openUrl(code!.verificationUri)}>Open page again</button>
          <span class="hint">Waiting for you to finish…</span>
        </div>
      </div>
    {:else}
      <div class="add">
        {#if msAvailable}
          <button class="primary" onclick={signIn}>Sign in with Microsoft</button>
        {:else}
          <p class="hint">Microsoft sign-in isn't set up in this build yet. Offline players work for testing.</p>
        {/if}
        <form class="row" onsubmit={(e) => { e.preventDefault(); addOffline(); }}>
          <input placeholder="Offline player name" bind:value={offlineName} maxlength="16" />
          <button disabled={!offlineName}>Add offline</button>
        </form>
      </div>
    {/if}
    {#if error}<p class="error">{error}</p>{/if}

    <div class="row end"><button onclick={onClose}>Done</button></div>
  </div>
</div>

<style>
  .acc { display: flex; gap: 10px; align-items: center; padding: 10px; border-bottom: 1px solid var(--hairline); margin-bottom: 6px; }
  .acc.on { background: var(--surface-tile-hover); }
  .avatar { width: 32px; height: 32px; border-radius: 6px; display: grid; place-items: center; background: var(--surface-tile); font-weight: 700; color: var(--ink-secondary); }
  .who { flex: 1; display: grid; }
  .badge { display: flex; gap: 4px; align-items: center; color: var(--live); font-size: 12.5px; }
  .add { display: grid; gap: 12px; margin-top: 14px; }
  .code { margin-top: 14px; padding: 14px; border: 1px solid var(--hairline); border-radius: var(--radius-md); background: var(--surface-tile); }
  .code p { margin: 0 0 10px; line-height: 1.45; }
  .code .big { font-size: 30px; font-weight: 800; letter-spacing: 0.18em; font-family: var(--font-mono); margin-bottom: 10px; user-select: text; }
  .end { justify-content: flex-end; margin-top: 18px; }
</style>
