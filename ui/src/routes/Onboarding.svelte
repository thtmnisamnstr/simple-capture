<script lang="ts">
  import { api, errorText } from '../lib/api.ts';
  import type { PermissionKind, PermissionStatus, PermissionsDto } from '../lib/types.ts';

  let perms = $state<PermissionsDto | null>(null);
  let busy = $state<PermissionKind | null>(null);
  let error = $state<string | null>(null);

  const ROWS: { kind: PermissionKind; key: keyof PermissionsDto; title: string; why: string }[] = [
    {
      kind: 'screen',
      key: 'screen',
      title: 'Screen Recording',
      why: 'Lets Simple Capture list your windows and apps and record the screen and system audio. Without it you can still record through the system picker.',
    },
    { kind: 'camera', key: 'camera', title: 'Camera', why: 'Only needed when you choose a camera. Camera video is recorded now and shown in the video from M2.' },
    { kind: 'microphone', key: 'mic', title: 'Microphone', why: 'Only needed when you choose a microphone to narrate.' },
  ];

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

  async function request(kind: PermissionKind) {
    busy = kind;
    error = null;
    try {
      await api.requestPermission(kind);
      await refresh();
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = null;
    }
  }

  async function openSettings(kind: PermissionKind) {
    error = null;
    try {
      await api.openPermissionSettings(kind);
    } catch (e) {
      error = errorText(e);
    }
  }
</script>

<div class="window onboarding">
  <h1>Permissions</h1>
  <p class="muted">macOS asks you once for each. Simple Capture checks the status without asking; it only asks when you press Request.</p>

  <ul>
    {#each ROWS as row (row.kind)}
      {@const status = perms?.[row.key] ?? 'unknown'}
      <li aria-labelledby="perm-{row.kind}">
        <div class="head">
          <h3 id="perm-{row.kind}">{row.title}</h3>
          <span class="badge {status}" aria-label="{row.title}: {LABEL[status]}">{LABEL[status]}</span>
        </div>
        <p class="muted">{row.why}</p>
        <div class="actions">
          {#if status === 'not_determined'}
            <button class="primary" onclick={() => request(row.kind)} disabled={busy !== null}>
              {busy === row.kind ? 'Waiting for macOS…' : 'Request'}
            </button>
          {/if}
          {#if status !== 'granted'}
            <button onclick={() => openSettings(row.kind)}>Open System Settings</button>
          {/if}
        </div>
      </li>
    {/each}
  </ul>

  {#if error}<p class="warn" role="alert">{error}</p>{/if}

  <aside>
    <h2>Why macOS may ask again</h2>
    <p>
      This build of Simple Capture is signed ad hoc, so macOS treats every update as a new app and asks for Screen Recording, Camera
      and Microphone again. If a permission looks stuck, remove Simple Capture from the list in System Settings and grant it
      again.
    </p>
    <p>
      On macOS 15 and later, macOS may also show a reminder from time to time that Simple Capture can see your screen, or ask you to
      allow it to bypass the private window picker. The <strong>System picker…</strong> source avoids that: macOS shows its own
      chooser, and what you pick there counts as your consent for that recording.
    </p>
  </aside>
</div>

<style>
  .onboarding {
    padding: 16px 18px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  p {
    margin: 0;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  li {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  h3 {
    font-size: 13px;
  }
  .badge {
    font-size: 11px;
    border-radius: 10px;
    padding: 1px 8px;
    background: var(--surface-2);
    color: var(--muted);
  }
  .badge.granted {
    color: var(--ok);
  }
  .badge.denied,
  .badge.restricted {
    color: var(--danger);
  }
  .actions {
    display: flex;
    gap: 6px;
  }
  .actions:empty {
    display: none;
  }
  aside {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12px;
    color: var(--muted);
  }
</style>
