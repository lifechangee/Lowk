<script lang="ts">
  import type { OverlayStore } from './store.svelte';
  import type { OverlaySource } from './source';
  import { ot } from './i18n';

  let { store, source }: { store: OverlayStore; source: OverlaySource } = $props();
  let value = $state('');
  let el: HTMLInputElement | undefined = $state();

  // The text field is focused once the interactive mode is shown (docs 7.3).
  $effect(() => {
    el?.focus();
  });

  function key(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.isComposing && value.trim()) {
      source.submitText(value);
      value = '';
    }
  }
</script>

<div class="inp">
  <input bind:this={el} bind:value type="text" placeholder={ot('overlay.input.placeholder', store.lang)} autocomplete="off" spellcheck="false" onkeydown={key} />
  <kbd>↵</kbd><kbd>Esc</kbd>
</div>
