<script lang="ts">
  import { api, errorText } from '../lib/api.ts';
  import { appApi, lineKind, type DiagnosticsDone } from '../lib/appinfo.ts';
  import { onEvent } from '../lib/events.ts';
  import { isObject } from '../lib/guards.ts';
  import { revealLabel } from '../lib/format.ts';
  import Icon from '../components/Icon.svelte';

  let lines: string[] = $state([]);
  let running = $state(false);
  let done: DiagnosticsDone | null = $state(null);
  let error: string | null = $state(null);
  let copied = $state(false);
  let log: HTMLElement | undefined = $state();

  const isLine = (v: unknown): v is { line: string } => isObject(v) && typeof v.line === 'string';
  const isDone = (v: unknown): v is DiagnosticsDone => isObject(v) && typeof v.passed === 'number';

  $effect(() => {
    const offs = [
      onEvent('diagnostics-line', isLine, (p) => {
        lines = [...lines, p.line];
        queueMicrotask(() => log?.scrollTo({ top: log.scrollHeight }));
      }),
      onEvent('diagnostics-done', isDone, (d) => {
        done = d;
        running = false;
      }),
    ];
    return () => offs.forEach((off) => off());
  });

  async function start() {
    error = null;
    done = null;
    copied = false;
    lines = [];
    running = true;
    try {
      await appApi.runDiagnostics();
    } catch (e) {
      running = false;
      error = errorText(e);
    }
  }

  async function copy() {
    if (!done) return;
    try {
      await navigator.clipboard.writeText(done.text);
      copied = true;
    } catch (e) {
      error = errorText(e);
    }
  }
</script>

<div class="window diag">
  <header>
    <div>
      <h1>Diagnostics</h1>
      <p class="caption">
        Checks screen capture, devices, encoders and the helper programs, then records a few seconds from a test pattern and a
        2-second sample of your screen, exports them and deletes them. Nothing is sent anywhere.
      </p>
    </div>
  </header>

  <div class="log group" bind:this={log} role="log" aria-live="polite" aria-label="Self-test output">
    {#each lines as l, i (i)}
      <div class="line {lineKind(l)}">{l}</div>
    {:else}
      <p class="muted empty">{running ? 'Starting…' : 'Run the self-test to see what this computer can do.'}</p>
    {/each}
  </div>

  {#if done}
    <p class={done.failed ? 'warn' : 'ok'} role="status">
      <Icon name={done.failed ? 'warning' : 'check'} size={14} />
      <span>
        {done.passed} passed, {done.failed} failed{done.skipped ? `, ${done.skipped} skipped` : ''}.
        {done.report ? 'The report is saved; send it with any bug report. It holds no recordings.' : (done.error ?? '')}
      </span>
    </p>
  {/if}
  {#if error}<p class="warn" role="alert"><Icon name="warning" size={14} /><span>{error}</span></p>{/if}

  <footer>
    {#if done?.report}
      <button onclick={() => done?.report && api.reveal(done.report)}>{revealLabel()}</button>
      <button onclick={copy}>{copied ? 'Copied' : 'Copy Report'}</button>
    {/if}
    <button class="primary" disabled={running} onclick={start}>{running ? 'Running…' : done ? 'Run Again' : 'Run Self-Test'}</button>
  </footer>
</div>

<style>
  .diag {
    padding: 34px 18px 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    height: 100vh;
    box-sizing: border-box;
  }
  h1 {
    margin: 0 0 4px;
  }
  .log {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 8px 10px;
    font: 11.5px/1.5 ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
    user-select: text;
  }
  .line {
    white-space: pre-wrap;
    word-break: break-word;
  }
  .line.head {
    font-weight: 600;
    margin-top: 6px;
  }
  .line.pass {
    color: var(--green);
  }
  .line.fail {
    color: var(--red, #d93025);
  }
  .line.skip,
  .line.info {
    color: var(--label-2);
  }
  .empty {
    margin: 0;
  }
  .ok {
    display: flex;
    gap: 6px;
    align-items: center;
    color: var(--green);
    font-size: 12px;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  footer button {
    min-width: 92px;
  }
</style>
