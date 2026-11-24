<script lang="ts">
  import { api, errorText } from '../lib/api.ts';
  import type { PermissionKind, PermissionStatus, PermissionsDto } from '../lib/types.ts';
  import Icon, { type IconName } from '../components/Icon.svelte';

  let perms = $state<PermissionsDto | null>(null);
  let busy = $state<PermissionKind | null>(null);
  let error = $state<string | null>(null);

  const ROWS: { kind: PermissionKind; key: keyof PermissionsDto; title: string; why: string }[] = [
    {
      kind: 'screen',
      key: 'screen',
      title: 'Screen Recording',
      why: 'Lists your windows and apps, and records the screen and system audio. Without it you can still record through the System Picker.',
    },
    { kind: 'camera', key: 'camera', title: 'Camera', why: 'Only needed when you choose a camera to show yourself in the video.' },
    { kind: 'microphone', key: 'mic', title: 'Microphone', why: 'Only needed when you choose a microphone to narrate.' },
  ];

  const LOOK: Record<PermissionKind, { icon: IconName; tint: string }> = {
    screen: { icon: 'display', tint: '#5e5ce6' },
    camera: { icon: 'camera', tint: '#30b0c7' },
    microphone: { icon: 'mic', tint: '#ff9500' },
  };
  const TONE: Record<PermissionStatus, string> = {
    granted: 'ok',
    denied: 'bad',
    restricted: 'bad',
    not_determined: '',
    unknown: '',
  };

  const LABEL: Record<PermissionStatus, string> = {
    granted: 'Granted',
    denied: 'Denied',
    not_determined: 'Not asked yet',
    restricted: 'Restricted',
    unknown: 'Unknown',
  };

  async function refresh() {
    try {
      perms = await api.getPermissions();
    } catch (e) {
      error = errorText(e);
    }
  }

  $effect(() => {
    refresh();
    const timer = setInterval(() => {
      if (document.visibilityState === 'visible') refresh();
    }, 2000);
    window.addEventListener('focus', refresh);
    return () => {
      clearInterval(timer);
      window.removeEventListener('focus', refresh);
    };
  });

  const ASKED_KEY = 'sc.screen-asked';
  let screenAsked = $state(readAsked());

  function readAsked(): boolean {
    try {
      return localStorage.getItem(ASKED_KEY) === '1';
    } catch {
      return false;
    }
  }

  const badge = (kind: PermissionKind, status: keyof typeof LABEL) =>
    kind === 'screen' && screenAsked && status === 'not_determined' ? 'Off' : LABEL[status];

  async function request(kind: PermissionKind) {
    busy = kind;
    error = null;
    try {
      const s = await api.requestPermission(kind);
      await refresh();
      if (kind === 'screen' && s !== 'granted') {
        screenAsked = true;
        try {
          localStorage.setItem(ASKED_KEY, '1');
        } catch {
          /* only a convenience */
        }
        await api.openPermissionSettings('screen');
      }
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = null;
    }
  }

  async function openSettings(kind: PermissionKind) {
    error = null;
    try {
      if (kind === 'screen') await api.requestPermission('screen');
      await api.openPermissionSettings(kind);
    } catch (e) {
      error = errorText(e);
    }
  }
</script>

<div class="window onboarding">
  <header>
    <span class="app-tile"><Icon name="lock" size={22} /></span>
    <div>
      <h1>Permissions</h1>
      <p class="caption">macOS asks once for each. Simple Capture checks without asking, and only asks when you press Allow.</p>
    </div>
  </header>

  <ul class="group">
    {#each ROWS as row (row.kind)}
      {@const status = perms?.[row.key] ?? 'unknown'}
      <li class="grow perm" aria-labelledby="perm-{row.kind}">
        <span class="tile" style:background={LOOK[row.kind].tint}><Icon name={LOOK[row.kind].icon} size={16} /></span>
        <div class="glabel text">
          <div class="title-line">
            <h3 id="perm-{row.kind}">{row.title}</h3>
            <span class="badge {TONE[status]}" aria-label="{row.title}: {badge(row.kind, status)}">{badge(row.kind, status)}</span>
          </div>
          <p class="caption">{row.why}</p>
          {#if row.kind === 'screen' && screenAsked && status !== 'granted'}
            <p class="caption next">
              In System Settings → Screen Recording, switch on <strong>Simple Capture</strong>. macOS then asks to quit and reopen it;
              choose Quit &amp; Reopen.
            </p>
          {/if}
        </div>
        <div class="gcontrol actions">
          {#if status === 'not_determined' && !(row.kind === 'screen' && screenAsked)}
            <button class="primary" onclick={() => request(row.kind)} disabled={busy !== null}>
              {busy === row.kind ? 'Waiting…' : 'Allow…'}
            </button>
          {:else if status !== 'granted'}
            <button class:primary={row.kind === 'screen'} onclick={() => openSettings(row.kind)}>Open Settings…</button>
          {/if}
        </div>
      </li>
    {/each}
  </ul>

  {#if error}<p class="warn" role="alert"><Icon name="warning" size={14} /><span>{error}</span></p>{/if}

  <details>
    <summary>Why macOS may ask again</summary>
    <div class="more caption">
      <p>
        This build of Simple Capture is signed ad hoc, so macOS treats every update as a new app and asks for Screen Recording,
        Camera and Microphone again. If a permission looks stuck, remove Simple Capture from the list in System Settings and allow it
        again.
      </p>
      <p>
        On macOS 15 and later, macOS may also remind you from time to time that Simple Capture can see your screen, or ask to let it
        bypass the private window picker. The <strong>System Picker</strong> source avoids that: macOS shows its own chooser, and what
        you pick there counts as your consent for that recording.
      </p>
    </div>
  </details>
</div>

<style>
  .onboarding {
    padding: 18px 20px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  header {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  header h1 {
    font-size: 17px;
    margin-bottom: 2px;
  }
  .app-tile {
    flex: none;
    display: grid;
    place-items: center;
    width: 46px;
    height: 46px;
    border-radius: 11px;
    color: #fff;
    background: linear-gradient(135deg, #257a59, #0d4b34);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.25);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .perm {
    align-items: flex-start;
    gap: 12px;
    padding: 12px;
  }
  .next {
    margin-top: 4px;
  }
  .tile {
    flex: none;
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 7px;
    color: #fff;
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .title-line {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  h3 {
    font-size: 13px;
    font-weight: 600;
  }
  .actions {
    align-self: center;
  }
  .actions:empty {
    display: none;
  }
  details {
    padding: 0 4px;
  }
  summary {
    font-size: 12px;
    color: var(--label-2);
    cursor: default;
  }
  .more {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 8px;
  }
</style>
