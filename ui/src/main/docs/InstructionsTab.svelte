<script lang="ts">
  // Folder tab «Инструкции»: free text that is ALWAYS sent with every question of a call in this folder.
  // Stored in `_folder.md` under `## Instructions`; autosaved (typing pause and blur).
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import { api } from '../api';
  import { errText } from '../errors';
  import { tm } from '../i18n';
  import { estimateTokens } from './util';

  let { folder }: { folder: string } = $props();

  const SOFT_LIMIT = 500;
  let text = $state('');
  let saved = $state('');
  let loaded = $state(false);
  let phase = $state<'idle' | 'saving' | 'saved' | 'error'>('idle');
  let error = $state('');
  let timer: ReturnType<typeof setTimeout> | undefined;

  const tokens = $derived(estimateTokens(text));
  const over = $derived(tokens > SOFT_LIMIT);

  $effect(() => {
    const k = folder;
    loaded = false;
    void api.folderGet(k).then((d) => { if (k === folder) { text = saved = d?.instructions ?? ''; loaded = true; phase = 'idle'; } });
  });

  async function save() {
    clearTimeout(timer);
    if (text === saved) return;
    phase = 'saving';
    try {
      await api.folderUpdate(folder, { instructions: text });
      saved = text;
      phase = 'saved';
    } catch (e) { phase = 'error'; error = errText(e); }
  }
  const soon = () => { clearTimeout(timer); phase = 'idle'; timer = setTimeout(() => void save(), 900); };
  function example() {
    text = text.trim() ? `${text.trimEnd()}\n${tm('docs.instr.exampleText')}` : tm('docs.instr.exampleText');
    void save();
  }
</script>

<div class="tab" data-testid="instructions-tab">
  <div class="card">
    <div class="h"><b>{tm('docs.instr.title')}</b><Button size="sm" onclick={example}>{tm('docs.instr.insertExample')}</Button></div>
    <p class="sub">{tm('docs.instr.hint')}</p>
    <textarea bind:value={text} disabled={!loaded} placeholder={tm('docs.instr.placeholder')} aria-label={tm('docs.instr.title')} oninput={soon} onblur={save}></textarea>
    <div class="cnt">
      <span class="st lbl" class:bad={phase === 'error'} role="status">
        {#if phase === 'saving'}{tm('docs.instr.saving')}{:else if phase === 'saved'}<Icon name="check" size={12} />{tm('docs.instr.saved')}{:else if phase === 'error'}{error}{:else if text !== saved}{tm('docs.instr.unsaved')}{/if}
      </span>
      <span class="tok" class:over data-testid="instr-counter">{tm('docs.instr.counter', { n: tokens, max: SOFT_LIMIT })}</span>
    </div>
    {#if over}<p class="warn" role="status"><Icon name="alert" size={14} /><span>{tm('docs.instr.long')}</span></p>{/if}
  </div>
  {#if !text.trim() && loaded}
    <section class="ex">
      <small class="lbl">{tm('docs.instr.exampleLabel')}</small>
      <p>{tm('docs.instr.exampleText')}</p>
    </section>
  {/if}
</div>

<style>
  .tab { display: flex; flex-direction: column; gap: var(--s4); }
  .card { border: 1px solid var(--line); background: var(--panel); border-radius: var(--r-lg); padding: var(--s4); display: flex; flex-direction: column; gap: var(--s3); }
  .h { display: flex; justify-content: space-between; align-items: center; gap: var(--s3); }
  .h b { font-weight: 600; }
  .sub { margin: 0; color: var(--tx2); font-size: 13px; }
  textarea { width: 100%; min-height: 176px; resize: vertical; background: var(--bg); border: 1px solid var(--line); border-radius: var(--r); padding: var(--s3); color: var(--tx); outline: none; font: 13px/20px var(--font); }
  textarea:focus { border-color: var(--ac); }
  textarea::placeholder { color: var(--tx3); }
  .cnt { display: flex; justify-content: space-between; gap: var(--s3); align-items: center; font-size: 12px; line-height: 16px; color: var(--tx3); min-height: 16px; }
  .st { display: inline-flex; align-items: center; gap: var(--s1); }
  .st.bad { color: #ff8f90; }
  .tok.over { color: var(--tx); font-weight: 600; }
  .warn { display: flex; gap: var(--s2); align-items: flex-start; margin: 0; color: var(--tx2); font-size: 12px; line-height: 16px; }
  .warn :global(svg) { color: var(--move); flex: none; margin-top: 1px; }
  .ex { border: 1px dashed var(--line-strong); border-radius: var(--r-lg); padding: var(--s3) var(--s4); }
  .ex small { display: block; color: var(--tx3); font-size: 11px; letter-spacing: 0.08em; text-transform: uppercase; font-weight: 600; margin-bottom: var(--s1); }
  .ex p { margin: 0; color: var(--tx2); font-size: 13px; }
</style>
