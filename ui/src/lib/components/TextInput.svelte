<script lang="ts">
  type Props = { value: string; label: string; placeholder?: string; type?: 'text' | 'password'; mono?: boolean; maxlength?: number; suggestions?: string[]; oninput?: (v: string) => void; onenter?: () => void; autofocus?: boolean };
  let { value = $bindable(), label, placeholder = '', type = 'text', mono = false, maxlength, suggestions, oninput, onenter, autofocus = false }: Props = $props();
  const listId = `suggestions-${Math.random().toString(36).slice(2)}`;
  function focusOnMount(node: HTMLInputElement) { if (autofocus) node.focus(); }
</script>

<input class="in" class:mono {type} {placeholder} {maxlength} aria-label={label} list={suggestions?.length ? listId : undefined} bind:value use:focusOnMount oninput={() => oninput?.(value)} onkeydown={(e) => e.key === 'Enter' && onenter?.()} autocomplete="off" spellcheck="false" />
{#if suggestions?.length}<datalist id={listId}>{#each suggestions as suggestion}<option value={suggestion} />{/each}</datalist>{/if}

<style>
  .in { height: 32px; width: 100%; min-width: 0; border-radius: var(--r); border: 1px solid var(--line); background: var(--bg); color: var(--tx); padding: 0 var(--s3); font: 13px/20px var(--font); outline: none; }
  .mono { font-family: var(--mono); font-size: 12px; }
  .in:focus { border-color: var(--ac); }
  .in::placeholder { color: var(--tx3); }
</style>
