<script lang="ts" module>
  export type IconName =
    | 'display'
    | 'window'
    | 'app'
    | 'region'
    | 'pointer'
    | 'picker'
    | 'camera'
    | 'mic'
    | 'speaker'
    | 'cursor'
    | 'stop'
    | 'switch'
    | 'close'
    | 'warning'
    | 'check'
    | 'folder'
    | 'export'
    | 'lock'
    | 'film'
    | 'frame'
    | 'chevron'
    | 'undo'
    | 'redo'
    | 'play'
    | 'pause'
    | 'start'
    | 'scissors'
    | 'trash';

  const PATHS: Record<IconName, string> = {
    display: 'M2.5 3.5h11a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1V4.5a1 1 0 0 1 1-1ZM6 14.5h4M8 12v2.5',
    window: 'M2.5 3h11a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1ZM1.5 5.8h13',
    app: 'M3 2.5h4v4H3zM9 2.5h4v4H9zM3 9.5h4v4H3zM9 9.5h4v4H9z',
    region: 'M2 5V3a1 1 0 0 1 1-1h2M11 2h2a1 1 0 0 1 1 1v2M14 11v2a1 1 0 0 1-1 1h-2M5 14H3a1 1 0 0 1-1-1v-2',
    pointer: 'M4 2.5 12.5 9 8.6 9.6 10.7 13.8 9 14.6 6.9 10.4 4 13Z',
    picker: 'M2.5 4.5h7a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-7a1 1 0 0 1-1-1v-6a1 1 0 0 1 1-1ZM5.5 2.5h8a1 1 0 0 1 1 1v6',
    camera: 'M2.5 4.5h7a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1h-7a1 1 0 0 1-1-1v-5a1 1 0 0 1 1-1ZM10.5 7l4-2.2v6.4l-4-2.2',
    mic: 'M8 1.8a2 2 0 0 1 2 2v3.7a2 2 0 0 1-4 0V3.8a2 2 0 0 1 2-2ZM4.3 7.2a3.7 3.7 0 0 0 7.4 0M8 11v3',
    speaker: 'M2.5 6h2.3L8 3.2v9.6L4.8 10H2.5zM10.6 5.6a3.2 3.2 0 0 1 0 4.8M12.3 3.8a5.6 5.6 0 0 1 0 8.4',
    cursor: 'M4 2.5 12.5 9 8.6 9.6 10.7 13.8 9 14.6 6.9 10.4 4 13Z',
    stop: 'M4.5 4.5h7v7h-7z',
    switch: 'M3 6.5a5 5 0 0 1 9-2.2M13 9.5a5 5 0 0 1-9 2.2M12.3 1.8v2.8H9.5M3.7 14.2v-2.8h2.8',
    close: 'm4.5 4.5 7 7m0-7-7 7',
    warning: 'M8 2.2 14.3 13.3H1.7ZM8 6.4v3.3M8 11.6v.2',
    check: 'm3.5 8.3 3 3 6-6.6',
    folder: 'M1.8 4a1 1 0 0 1 1-1h3.4l1.5 1.6h5.5a1 1 0 0 1 1 1v6.9a1 1 0 0 1-1 1H2.8a1 1 0 0 1-1-1Z',
    export: 'M8 1.8v8.4M5.2 4.4 8 1.6l2.8 2.8M4.8 6.8H3.6a1 1 0 0 0-1 1v5.6a1 1 0 0 0 1 1h8.8a1 1 0 0 0 1-1V7.8a1 1 0 0 0-1-1h-1.2',
    lock: 'M4 7.2h8a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1v-5a1 1 0 0 1 1-1ZM5.4 7.2V5a2.6 2.6 0 0 1 5.2 0v2.2',
    film: 'M2.5 2.5h11v11h-11zM5 2.5v11M11 2.5v11M2.5 6h2.5M2.5 10h2.5M11 6h2.5M11 10h2.5',
    frame: 'M2.5 3.5h11a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1v-7a1 1 0 0 1 1-1ZM5 6h6v4H5z',
    chevron: 'm6 3.5 4.5 4.5L6 12.5',
    undo: 'M5.5 3.5 2.5 6.5l3 3M2.8 6.5h6.7a3.8 3.8 0 0 1 0 7.6H7',
    redo: 'm10.5 3.5 3 3-3 3M13.2 6.5H6.5a3.8 3.8 0 0 0 0 7.6H9',
    play: 'M5 3.2v9.6L12.8 8Z',
    pause: 'M4.5 3h2.6v10H4.5zM8.9 3h2.6v10H8.9z',
    start: 'M4 3.5v9M12.5 3.8 6.5 8l6 4.2Z',
    scissors: 'M5.4 5a1.9 1.9 0 1 1-3.8 0 1.9 1.9 0 0 1 3.8 0ZM5.4 11a1.9 1.9 0 1 1-3.8 0 1.9 1.9 0 0 1 3.8 0ZM5 6.2l9 6M5 9.8l9-6',
    trash: 'M2.8 4.5h10.4M6.4 4.5V2.8h3.2v1.7M4.4 4.5l.6 8.6a1 1 0 0 0 1 .9h4a1 1 0 0 0 1-.9l.6-8.6',
  };

  const FILLED: Partial<Record<IconName, true>> = { stop: true, play: true, pause: true };
</script>

<script lang="ts">
  let { name, size = 16, title }: { name: IconName; size?: number; title?: string } = $props();
</script>

<svg
  class="icon"
  width={size}
  height={size}
  viewBox="0 0 16 16"
  role={title ? 'img' : undefined}
  aria-hidden={title ? undefined : 'true'}
  aria-label={title}
>
  <path
    d={PATHS[name]}
    fill={FILLED[name] ? 'currentColor' : 'none'}
    stroke="currentColor"
    stroke-width={FILLED[name] ? 0 : 1.35}
    stroke-linecap="round"
    stroke-linejoin="round"
  />
</svg>

<style>
  .icon {
    flex: none;
    display: block;
  }
</style>
