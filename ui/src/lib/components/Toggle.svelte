<script lang="ts">
  let { checked, label, disabled = false, onchange }: { checked: boolean; label: string; disabled?: boolean; onchange: (v: boolean) => void } = $props();
</script>

<button class="tg" class:on={checked} {disabled} role="switch" aria-checked={checked} aria-label={label} onclick={() => onchange(!checked)}></button>

<style>
  .tg { all: unset; box-sizing: border-box; width: 32px; height: 20px; border-radius: 99px; background: var(--hv); border: 1px solid var(--line); position: relative; cursor: pointer; flex: none; }
  .tg::after { content: ''; position: absolute; left: 3px; top: 3px; width: 12px; height: 12px; border-radius: 50%; background: var(--tx3); transition: 0.15s; }
  .on { background: var(--ac); border-color: var(--ac); }
  .on::after { left: 15px; background: var(--bg); }
  .tg:focus-visible { outline: 2px solid var(--ac); outline-offset: 2px; }
  .tg[disabled] { opacity: 0.4; pointer-events: none; }
</style>
