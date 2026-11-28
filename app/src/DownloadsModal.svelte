<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { api, mb, type Missing } from "./lib/api";
  import Icon from "./Icon.svelte";

  let { items, onDone, onCancel }: { items: Missing[]; onDone: () => void; onCancel: () => void } = $props();

  const BATCH = 8;
  let found = $state<string[]>([]);
  let opened = $state<string[]>([]);
  let note = $state<string | null>(null);
  let timer: ReturnType<typeof setInterval> | null = null;
  let finished = false;

  const left = $derived(items.filter((i) => !found.includes(i.sha1)));
  const toOpen = $derived(left.filter((i) => !opened.includes(i.sha1)));

  function check(nowFound: string[]) {
    found = [...new Set([...found, ...nowFound])];
    if (!finished && found.length >= new Set(items.map((i) => i.sha1)).size) {
      finished = true;
      if (timer) clearInterval(timer);
      setTimeout(onDone, 600);
    }
  }

  async function scan() {
    try {
      check(await api.scanDownloads(left));
    } catch (e) {
      note = String(e);
    }
  }

  function openOne(i: Missing) {
    api.openUrl(i.page);
    if (!opened.includes(i.sha1)) opened = [...opened, i.sha1];
  }

  function openBatch() {
    for (const i of toOpen.slice(0, BATCH)) openOne(i);
  }

  async function choose() {
    const picked = await open({ multiple: true, directory: false, title: "Choose the downloaded files" });
    const files = Array.isArray(picked) ? picked : picked ? [picked] : [];
    const got: string[] = [];
    for (const f of files) {
      const sha = await api.importDownload(f, left);
      if (sha) got.push(sha);
    }
    note = files.length && !got.length ? "Those aren't the files this pack needs." : null;
    check(got);
  }

  onMount(() => {
    scan();
    timer = setInterval(scan, 2000);
  });
  onDestroy(() => timer && clearInterval(timer));
</script>

<div class="scrim" role="presentation">
  <div class="modal wide" role="dialog" aria-label="Downloads needed">
    <h2>{items.length === 1 ? "One file" : `${items.length} files`} to download</h2>
    <p class="lead">
      The makers of these only allow downloads from their own website. Press <b>Open</b>: your browser downloads the file
      by itself after a few seconds. You can close each tab once its download is done. EasyLauncher finds the files in your
      Downloads folder on its own.
    </p>

    <div class="progress">
      <div class="bar"><div class="fill" style:width={`${(found.length / items.length) * 100}%`}></div></div>
      <span class="hint">{found.length} of {items.length} found</span>
    </div>

    <div class="list">
      {#each items as i (i.sha1 + i.path)}
        {@const ok = found.includes(i.sha1)}
        <div class="item">
          <div class="who">
            <b>{i.name}</b>
            <span class="hint">{i.filename ?? i.path.split("/").pop()} · {mb(i.size)}</span>
          </div>
          {#if ok}
            <span class="found"><Icon name="check" size={14} /> Found</span>
          {:else}
            <button onclick={() => openOne(i)}>{opened.includes(i.sha1) ? "Open again" : "Open"}</button>
          {/if}
        </div>
      {/each}
    </div>

    {#if note}<p class="error">{note}</p>{/if}
    <div class="row end">
      <button class="ghost" onclick={onCancel}>Cancel</button>
      <button onclick={choose}>Choose files…</button>
      {#if toOpen.length}
        <button class="primary" onclick={openBatch}>
          Open {Math.min(BATCH, toOpen.length)}{toOpen.length > BATCH ? ` of ${toOpen.length}` : ""} in browser
        </button>
      {:else if left.length}
        <span class="hint">Waiting for downloads…</span>
      {/if}
    </div>
  </div>
</div>

<style>
  .wide { width: min(640px, 92vw); }
  .lead { line-height: 1.5; margin: 0 0 14px; color: var(--ink); }
  .progress { display: flex; gap: 10px; align-items: center; margin-bottom: 10px; }
  .bar { flex: 1; height: 6px; background: var(--track); border-radius: var(--radius-pill); overflow: hidden; }
  .fill { height: 100%; background: var(--series-1); transition: width 0.3s; }
  .list { max-height: 46vh; overflow: auto; display: grid; gap: 6px; padding-right: 4px; }
  .item { display: flex; gap: 10px; align-items: center; padding: 8px 10px; border-bottom: 1px solid var(--hairline); }
  .who { flex: 1; display: grid; min-width: 0; }
  .who .hint { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .found { display: flex; gap: 4px; align-items: center; color: var(--live); font-size: 12.5px; }
  .end { justify-content: flex-end; margin-top: 16px; align-items: center; }
</style>
