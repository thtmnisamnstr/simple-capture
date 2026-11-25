<script lang="ts">
  import { errorText } from '../lib/api.ts';
  import { appApi, type AboutInfo, type LicenseFile } from '../lib/appinfo.ts';
  import { editor } from '../lib/editor.ts';
  import { showWindow } from '../lib/windows.ts';
  import Logo from '../components/Logo.svelte';
  import Icon from '../components/Icon.svelte';

  let info: AboutInfo | null = $state(null);
  let error: string | null = $state(null);
  let viewing: { title: string; text: string } | null = $state(null);

  $effect(() => {
    appApi
      .about()
      .then((i) => (info = i))
      .catch((e) => (error = errorText(e)));
  });

  async function privacy() {
    error = null;
    try {
      await appApi.openPrivacyPolicy();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function source() {
    error = null;
    try {
      await appApi.openSourceRelease();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function view(title: string, f: LicenseFile) {
    error = null;
    try {
      if (f.html) {
        await editor.openFile(f.path);
      } else {
        viewing = { title: `${title}: ${f.name}`, text: await appApi.licenseText(f.path) };
      }
    } catch (e) {
      error = errorText(e);
    }
  }

  const OS: Record<string, string> = { macos: 'macOS', windows: 'Windows', linux: 'Linux' };
</script>

<div class="window about">
  {#if viewing}
    <header class="sheet-head">
      <button class="plain" onclick={() => (viewing = null)} aria-label="Back"><Icon name="chevron" size={14} /></button>
      <h2>{viewing.title}</h2>
    </header>
    <pre class="text group">{viewing.text}</pre>
  {:else}
    <header class="hero">
      <span class="logo"><Logo size={52} /></span>
      <div>
        <h1>Simple Capture</h1>
        <p class="caption tnum">
          Version {info?.version ?? '…'}{info ? ` · ${OS[info.os] ?? info.os} ${info.arch}` : ''}
        </p>
      </div>
    </header>
    <p class="caption lead">
      Record your screen, camera and sound, edit, and export. No account and no network: what you record stays on this
      computer.
    </p>

    <h2 class="section-title">Licenses</h2>
    <div class="list">
      {#each info?.licenses ?? [] as e (e.title)}
        <div class="group entry">
          <div class="row1">
            <span class="name">{e.title}</span>
            <span class="badge">{e.license}</span>
          </div>
          <p class="caption">{e.note}</p>
          {#if e.files.length}
            <div class="files">
              {#each e.files as f (f.path)}
                <button class="link" onclick={() => view(e.title, f)}>{f.html ? 'Open notices' : f.name}</button>
              {/each}
              {#if e.source}<button class="link" onclick={source}>Source code</button>{/if}
            </div>
          {:else}
            <p class="caption muted">Included in release builds.</p>
          {/if}
        </div>
      {/each}
    </div>
    {#if error}<p class="warn" role="alert"><Icon name="warning" size={14} /><span>{error}</span></p>{/if}
    <footer>
      <button class="link" onclick={privacy}>Privacy Policy</button>
      <button onclick={() => showWindow('diagnostics')}>Diagnostics…</button>
    </footer>
  {/if}
</div>

<style>
  .about {
    padding: 34px 18px 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    height: 100vh;
    box-sizing: border-box;
  }
  .hero {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .hero h1 {
    margin: 0;
  }
  .logo {
    display: grid;
    place-items: center;
    width: 52px;
    height: 52px;
    border-radius: 16px;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.25);
  }
  .lead {
    margin: 0;
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .entry {
    flex: none;
    padding: 8px 10px;
  }
  .row1 {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    align-items: baseline;
  }
  .name {
    font-weight: 600;
  }
  .entry p {
    margin: 4px 0 0;
  }
  .files {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 12px;
    margin-top: 4px;
  }
  .sheet-head {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .sheet-head h2 {
    margin: 0;
    font-size: 13px;
  }
  .sheet-head button :global(svg) {
    transform: rotate(90deg);
  }
  .text {
    flex: 1;
    min-height: 0;
    overflow: auto;
    margin: 0;
    padding: 10px;
    font: 11px/1.45 ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
    white-space: pre-wrap;
    user-select: text;
  }
  footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
</style>
