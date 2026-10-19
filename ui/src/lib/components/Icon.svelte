<script lang="ts" module>
  // Inline stroke icons on a 16px grid; `currentColor`, no dependencies.
  const paths = {
    search: '<circle cx="7" cy="7" r="4.5"/><path d="m10.5 10.5 3 3"/>',
    folder: '<path d="M2 4.5A1.5 1.5 0 0 1 3.5 3H6l1.5 1.5h5A1.5 1.5 0 0 1 14 6v5.5A1.5 1.5 0 0 1 12.5 13h-9A1.5 1.5 0 0 1 2 11.5z"/>',
    plus: '<path d="M8 3v10M3 8h10"/>',
    'chevron-right': '<path d="m6 3.5 4.5 4.5L6 12.5"/>',
    'chevron-down': '<path d="m3.5 6 4.5 4.5L12.5 6"/>',
    home: '<path d="M2.5 7.5 8 3l5.5 4.5V13a.5.5 0 0 1-.5.5H3a.5.5 0 0 1-.5-.5z"/>',
    settings: '<path d="M3 5h6M12 5h1M3 11h1M7 11h6"/><circle cx="10.5" cy="5" r="1.5"/><circle cx="5.5" cy="11" r="1.5"/>',
    trash: '<path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.5 8.5h6l.5-8.5M7 7v4M9 7v4"/>',
    restore: '<path d="M3 8a5 5 0 1 0 1.6-3.7M3 2.5v3h3"/>',
    mic: '<rect x="6" y="2" width="4" height="7" rx="2"/><path d="M3.5 7.5a4.5 4.5 0 0 0 9 0M8 12v2"/>',
    monitor: '<rect x="2" y="3" width="12" height="8.5" rx="1.5"/><path d="M5.5 14h5"/>',
    keyboard: '<rect x="1.5" y="4" width="13" height="8" rx="1.5"/><path d="M4.5 7h.01M7.5 7h.01M10.5 7h.01M5 9.5h6"/>',
    check: '<path d="m3 8.5 3.2 3.2L13 4.8"/>',
    alert: '<path d="M8 2.5 1.8 13h12.4z"/><path d="M8 6.5v3M8 11.2v.01"/>',
    sparkle: '<path d="M8 1l1.8 4.2L14 7l-4.2 1.8L8 13l-1.8-4.2L2 7l4.2-1.8z"/>',
    doc: '<path d="M4 2h5l3 3v9H4z"/><path d="M9 2v3h3"/>',
    stop: '<rect x="4" y="4" width="8" height="8" rx="1.5"/>',
    download: '<path d="M8 2.5v8M4.5 7.5 8 11l3.5-3.5M3 13.5h10"/>',
    'arrow-left': '<path d="M13 8H3M7 4 3 8l4 4"/>',
    mouse: '<rect x="4.5" y="2" width="7" height="12" rx="3.5"/><path d="M8 2v4"/>',
    close: '<path d="m4 4 8 8M12 4l-8 8"/>',
    external: '<path d="M9 2.5h4.5V7M13.5 2.5 7.5 8.5M12 9.5V13a.5.5 0 0 1-.5.5h-8A.5.5 0 0 1 3 13V5a.5.5 0 0 1 .5-.5H7"/>',
    key: '<circle cx="5.5" cy="10.5" r="2.5"/><path d="m7.5 8.5 6-6M11 5l1.5 1.5"/>',
    refresh: '<path d="M13 8a5 5 0 1 1-1.6-3.7M13 2.5v3h-3"/>',
    globe: '<circle cx="8" cy="8" r="6"/><path d="M2 8h12M8 2c2 2 2 10 0 12M8 2c-2 2-2 10 0 12"/>',
    eye: '<path d="M1.5 8S4 3.5 8 3.5 14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8z"/><circle cx="8" cy="8" r="2"/>',
    layers: '<path d="m8 2 6 3.5L8 9 2 5.5z"/><path d="m2 8.5 6 3.5 6-3.5"/>',
    shield: '<path d="M8 2 3 4v4c0 3 2.2 5 5 6 2.8-1 5-3 5-6V4z"/>',
    inbox: '<path d="M2 9.5h3.5l1 2h3l1-2H14M2 9.5l1.5-6h9l1.5 6V13a.5.5 0 0 1-.5.5h-11A.5.5 0 0 1 2 13z"/>',
    drive: '<rect x="2" y="9" width="12" height="4.5" rx="1.5"/><path d="M3.5 9 5 3h6l1.5 6M11 11.2h.01"/>',
    info: '<circle cx="8" cy="8" r="6"/><path d="M8 7.5V11M8 5.2v.01"/>',
    pencil: '<path d="m10.5 3 2.5 2.5L5.5 13H3v-2.5z"/>',
    cpu: '<rect x="4" y="4" width="8" height="8" rx="1.5"/><path d="M6.5 1.5v2M9.5 1.5v2M6.5 12.5v2M9.5 12.5v2M1.5 6.5h2M1.5 9.5h2M12.5 6.5h2M12.5 9.5h2"/>',
  } as const;

  export type IconName = keyof typeof paths;
</script>

<script lang="ts">
  let { name, size = 16, label }: { name: IconName; size?: number; label?: string } = $props();
</script>

<svg
  class="icon"
  width={size}
  height={size}
  viewBox="0 0 16 16"
  fill="none"
  stroke="currentColor"
  stroke-width="1.5"
  stroke-linecap="round"
  stroke-linejoin="round"
  role={label ? 'img' : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : 'true'}
>
  {@html paths[name]}
</svg>

<style>
  .icon { flex: none; display: block; }
</style>
