<script lang="ts">
  // Tags of a folder or a call: chips with a remove button and an inline «+ тег» input. Manual tags always stay.
  import Icon from './Icon.svelte';
  import Tag from './Tag.svelte';
  import TextInput from './TextInput.svelte';

  let { tags, addLabel, newLabel, removeLabel, onchange }: { tags: string[]; addLabel: string; newLabel: string; removeLabel: (tag: string) => string; onchange: (tags: string[]) => void } = $props();

  let adding = $state(false);
  let text = $state('');
  function add() {
    const t = text.trim().replace(/^#\s*/, '');
    adding = false;
    text = '';
    if (t && !tags.includes(t)) onchange([...tags, t]);
  }
</script>

<span class="tags" data-testid="tag-editor">
  {#each tags as t, i (t)}
    <span class="tg"><Tag accent={i === 0} text={'# ' + t} /><button class="x" aria-label={removeLabel(t)} title={removeLabel(t)} onclick={() => onchange(tags.filter((x) => x !== t))}><Icon name="close" size={10} /></button></span>
  {/each}
  {#if adding}
    <span class="tin"><TextInput bind:value={text} label={newLabel} placeholder={newLabel} autofocus onenter={add} /></span>
  {:else}
    <Tag dashed text={addLabel} onclick={() => (adding = true)} />
  {/if}
</span>

<style>
  .tags { display: inline-flex; flex-wrap: wrap; gap: var(--s1) var(--s2); align-items: center; min-width: 0; }
  .tg { display: inline-flex; align-items: center; gap: var(--s0); min-width: 0; }
  .x { all: unset; cursor: pointer; width: 16px; height: 16px; display: grid; place-items: center; border-radius: 50%; color: var(--tx3); }
  .x:hover, .x:focus-visible { background: var(--sel); color: var(--tx); }
  .tin { width: 144px; }
</style>
