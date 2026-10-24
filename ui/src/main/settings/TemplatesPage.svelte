<script lang="ts">
  // Settings → Templates: create / edit / duplicate / reorder / delete the prompt templates. The list order is the overlay's
  // cycle order; the default template is what the «Ask: brief answer» key (⌘↵) asks. Persisted through the settings store.
  import type { Template } from '../../lib/ipc';
  import Badge from '../../lib/components/Badge.svelte';
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import Kbd from '../../lib/components/Kbd.svelte';
  import Modal from '../../lib/components/Modal.svelte';
  import Select from '../../lib/components/Select.svelte';
  import SettingRow from '../../lib/components/SettingRow.svelte';
  import TextInput from '../../lib/components/TextInput.svelte';
  import Toggle from '../../lib/components/Toggle.svelte';
  import { locale } from '../../lib/i18n/locale.svelte';
  import { api } from '../api';
  import { bindFor, formatTrigger, withBind } from '../binds';
  import { tm } from '../i18n';
  import { mouseNames } from '../names';
  import { prefs } from '../state.svelte';
  import { applyDraft, bindSlot, defaultId, draftOf, duplicate, fillNames, isBuiltin, move, nameOf, newTemplate, restoreBuiltins, validate, visible, type Draft, type Lang } from '../templates';
  import BindEditor from './BindEditor.svelte';

  const os = api.platform();
  const all = $derived<Template[]>(prefs.value.templates ?? []);
  const list = $derived(visible(all));
  const def = $derived(defaultId(all, prefs.value.llm?.default_template));
  const lang = $derived<Lang>(locale.lang === 'ru' ? 'ru' : 'en');

  let openId = $state<string | null>(null);
  let isNew = $state(false);
  let draft = $state<Draft | null>(null);
  let tried = $state(false);
  let confirmDelete = $state<Template | null>(null);
  let confirmReset = $state(false);

  const errors = $derived(draft ? validate(draft, all, lang) : []);
  const shown = $derived(errors.filter((e) => e === 'nameTaken' || tried));
  const current = $derived(list.find((t) => t.id === openId));
  const reasoningOptions = $derived([
    { value: 'inherit', label: tm('m.tpl.reasoning.inherit') },
    { value: 'on', label: tm('m.tpl.reasoning.on') },
    { value: 'off', label: tm('m.tpl.reasoning.off') },
  ]);

  function open(t: Template, fresh = false) {
    openId = t.id; isNew = fresh; draft = draftOf(t); tried = false;
  }
  function close() { openId = null; isNew = false; draft = null; tried = false; }
  const toggleRow = (t: Template) => (openId === t.id ? close() : open(t));

  function create() { open(newTemplate(all), true); }

  async function save() {
    if (!draft) return;
    tried = true;
    if (errors.length) return;
    const d = fillNames(draft, lang);
    const base = isNew ? newTemplate(all) : all.find((t) => t.id === d.id);
    if (!base) return close();
    const next = applyDraft(isNew ? { ...base, id: d.id } : base, d);
    const out = isNew ? [...all, next] : all.map((t) => (t.id === next.id ? next : t));
    await prefs.patch({ templates: out });
    openId = next.id; isNew = false; draft = draftOf(next); tried = false;
  }

  async function dup(t: Template) {
    const id = newTemplate(all).id;
    await prefs.patch({ templates: duplicate(all, t.id, id) });
    const copy = (prefs.value.templates ?? []).find((x) => x.id === id);
    if (copy) open(copy);
  }

  async function remove(t: Template) {
    confirmDelete = null;
    if (isBuiltin(t.id)) return;
    const patch: Record<string, unknown> = { templates: all.filter((x) => x.id !== t.id) };
    if (def === t.id || prefs.value.llm?.default_template === t.id) patch.llm = { default_template: 'brief' };
    const bound = bindSlot(t.id, '');
    if (bindFor(prefs.value.binds, bound.action, bound.template)) patch.binds = withBind(prefs.value.binds ?? [], bound.action, null, bound.template);
    await prefs.patch(patch);
    if (openId === t.id) close();
  }

  const reorder = (t: Template, dir: -1 | 1) => prefs.patch({ templates: move(all, t.id, dir) });
  const makeDefault = (t: Template) => prefs.patch({ llm: { default_template: t.id } });
  async function reset() { confirmReset = false; await prefs.patch({ templates: restoreBuiltins(all) }); if (draft && !isNew) { const t = (prefs.value.templates ?? []).find((x) => x.id === draft!.id); if (t) draft = draftOf(t); } }

  const chip = (id: string): string[] => {
    const s = bindSlot(id, def);
    const b = bindFor(prefs.value.binds, s.action, s.template);
    return b ? formatTrigger(b.trigger, os, mouseNames()) : [];
  };
  const reasoningValue = (r: boolean | null): string => (r === true ? 'on' : r === false ? 'off' : 'inherit');
  const errText = (e: string) => tm(`m.tpl.err.${e}` as 'm.tpl.err.name');
</script>

<h2>{tm('m.tpl.title')}</h2>
<p class="lead">{tm('m.tpl.lead')}</p>

<ul class="list" data-testid="tpl-list">
  {#each list as t, i (t.id)}
    {@const keys = chip(t.id)}
    <li class="tp" class:open={openId === t.id && !isNew} data-tpl={t.id}>
      <div class="head">
        <div class="mv">
          <button class="ic" aria-label={tm('m.tpl.up')} title={tm('m.tpl.up')} disabled={i === 0} onclick={() => reorder(t, -1)}><Icon name="chevron-up" size={14} /></button>
          <button class="ic" aria-label={tm('m.tpl.down')} title={tm('m.tpl.down')} disabled={i === list.length - 1} onclick={() => reorder(t, 1)}><Icon name="chevron-down" size={14} /></button>
        </div>
        <button class="main" aria-expanded={openId === t.id && !isNew} aria-label={tm('m.tpl.edit') + ': ' + nameOf(t, lang)} onclick={() => toggleRow(t)}>
          <span class="nm"><span class="n">{nameOf(t, lang)}</span>
            {#if t.id === def}<Badge text={tm('m.tpl.default')} />{/if}
            {#if t.needs_my_answer}<Badge text={tm('m.tpl.myAnswer')} />{/if}
          </span>
          <span class="pv">{t.instruction}</span>
        </button>
        <span class="chip" data-testid="tpl-bind">
          {#if keys.length}{#each keys as k, j (j)}<Kbd text={k} />{/each}{:else}<span class="unset">{tm('m.notSet')}</span>{/if}
        </span>
      </div>
      {#if openId === t.id && !isNew && draft}
        {@render editor(t)}
      {/if}
    </li>
  {/each}
  {#if isNew && draft}
    <li class="tp open" data-tpl="new">
      <div class="head"><span class="main static"><span class="nm"><span class="n">{tm('m.tpl.newTitle')}</span></span></span></div>
      {@render editor(null)}
    </li>
  {/if}
</ul>
{#if !list.length && !isNew}<p class="lead">{tm('m.tpl.empty')}</p>{/if}

<div class="foot">
  <Button variant="primary" size="sm" icon="plus" onclick={create}>{tm('m.tpl.new')}</Button>
  <Button size="sm" icon="restore" onclick={() => (confirmReset = true)}>{tm('m.tpl.reset')}</Button>
</div>

{#snippet editor(t: Template | null)}
  {#if draft}
    <div class="ed" data-testid="tpl-editor">
      {#if (t ?? current)?.needs_my_answer}<p class="note acc" data-testid="tpl-myanswer">{tm('m.tpl.myAnswerNote')}</p>
      {:else if t && isBuiltin(t.id)}<p class="note" data-testid="tpl-builtin">{tm('m.tpl.builtinNote')}</p>{/if}
      <SettingRow label={tm('m.tpl.nameRu')} stack>
        <TextInput bind:value={draft.ru} label={tm('m.tpl.nameRu')} maxlength={60} autofocus={isNew} />
      </SettingRow>
      <SettingRow label={tm('m.tpl.nameEn')} hint={tm('m.tpl.nameHint')} stack>
        <TextInput bind:value={draft.en} label={tm('m.tpl.nameEn')} maxlength={60} />
      </SettingRow>
      <SettingRow label={tm('m.tpl.instruction')} hint={tm('m.tpl.instructionHint')} stack>
        <textarea bind:value={draft.instruction} aria-label={tm('m.tpl.instruction')} placeholder={tm('m.tpl.instructionPlaceholder')} rows="4"></textarea>
      </SettingRow>
      <SettingRow label={tm('m.tpl.vision')} hint={tm('m.tpl.visionHint')}>
        <Toggle checked={draft.vision} label={tm('m.tpl.vision')} onchange={(v) => draft && (draft.vision = v)} />
      </SettingRow>
      <SettingRow label={tm('m.tpl.reasoning')} hint={tm('m.tpl.reasoningHint')}>
        <Select label={tm('m.tpl.reasoning')} value={reasoningValue(draft.reasoning)} options={reasoningOptions} onchange={(v) => draft && (draft.reasoning = v === 'on' ? true : v === 'off' ? false : null)} />
      </SettingRow>
      {#if t}
        <div class="bind" data-testid="tpl-bindeditor">
          <BindEditor actions={[]} templates={[{ slot: bindSlot(t.id, def), label: tm('m.tpl.bind') }]} showReset={false} hints={false} />
          {#if t.id === def}<p class="note">{tm('m.tpl.bindDefaultHint')}</p>{/if}
        </div>
      {:else}
        <p class="note">{tm('m.tpl.bindNewHint')}</p>
      {/if}
      {#if shown.length}<div class="errs" role="alert">{#each shown as e (e)}<p class="wn">{errText(e)}</p>{/each}</div>{/if}
      <div class="acts">
        <Button variant="primary" size="sm" icon="check" onclick={save}>{tm('m.tpl.save')}</Button>
        <Button size="sm" onclick={close}>{tm('m.tpl.cancel')}</Button>
        {#if t}
          {#if t.id === def}<span class="isdef">{tm('m.tpl.defaultNote')}</span>
          {:else}<Button size="sm" icon="star" onclick={() => makeDefault(t)}>{tm('m.tpl.makeDefault')}</Button>{/if}
          <Button size="sm" icon="copy" onclick={() => dup(t)}>{tm('m.tpl.duplicate')}</Button>
          {#if !isBuiltin(t.id)}<Button size="sm" variant="danger" icon="trash" onclick={() => (confirmDelete = t)}>{tm('m.tpl.delete')}</Button>{/if}
        {/if}
      </div>
    </div>
  {/if}
{/snippet}

<Modal open={confirmDelete !== null} title={confirmDelete ? tm('m.tpl.deleteTitle', { name: nameOf(confirmDelete, lang) }) : ''} closeLabel={tm('m.tpl.cancel')} onclose={() => (confirmDelete = null)} width={420}>
  <p class="mp">{tm('m.tpl.deleteBody')}</p>
  {#snippet footer()}
    <Button size="sm" onclick={() => (confirmDelete = null)}>{tm('m.tpl.cancel')}</Button>
    <Button size="sm" variant="danger" icon="trash" onclick={() => confirmDelete && remove(confirmDelete)}>{tm('m.tpl.delete')}</Button>
  {/snippet}
</Modal>
<Modal open={confirmReset} title={tm('m.tpl.resetTitle')} closeLabel={tm('m.tpl.cancel')} onclose={() => (confirmReset = false)} width={420}>
  <p class="mp">{tm('m.tpl.resetBody')}</p>
  {#snippet footer()}
    <Button size="sm" onclick={() => (confirmReset = false)}>{tm('m.tpl.cancel')}</Button>
    <Button size="sm" variant="primary" icon="restore" onclick={reset}>{tm('m.tpl.resetDo')}</Button>
  {/snippet}
</Modal>

<style>
  h2 { font: 600 var(--fs-lg) var(--font); margin: 0 0 var(--s1); }
  .lead { color: var(--tx2); margin: 0 0 var(--s3); font-size: 13px; }
  .list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--s2); }
  .tp { border: 1px solid var(--line); background: var(--panel); border-radius: var(--r-lg); min-width: 0; }
  .tp.open { border-color: var(--ac); }
  .head { display: grid; grid-template-columns: 28px minmax(0, 1fr) auto; align-items: center; gap: var(--s3); padding: var(--s2) var(--s3); min-height: 56px; }
  .mv { display: flex; flex-direction: column; }
  .ic { all: unset; box-sizing: border-box; width: 28px; height: 20px; display: grid; place-items: center; cursor: pointer; border-radius: var(--r-sm); color: var(--tx3); }
  .ic:hover { background: var(--hv); color: var(--tx); }
  .ic:focus-visible { outline: 2px solid var(--ac); }
  .ic[disabled] { opacity: 0.3; pointer-events: none; }
  .main { all: unset; box-sizing: border-box; cursor: pointer; display: flex; flex-direction: column; gap: 2px; min-width: 0; padding: var(--s1) 0; font-size: 13px; }
  .main:focus-visible { outline: 2px solid var(--ac); outline-offset: 2px; border-radius: var(--r-sm); }
  .main.static { cursor: default; }
  .nm { display: flex; flex-wrap: wrap; align-items: center; gap: var(--s2); min-width: 0; }
  .n { font-weight: 600; color: var(--tx); overflow-wrap: anywhere; }
  .pv { color: var(--tx2); font-size: 12px; line-height: 16px; overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow-wrap: anywhere; }
  .chip { display: inline-flex; gap: var(--s1); align-items: center; justify-content: flex-end; min-width: 0; }
  .unset { font-size: 12px; color: var(--tx3); }
  .ed { border-top: 1px solid var(--line); padding: var(--s2) var(--s4) var(--s4); }
  .note { color: var(--tx2); font-size: 12px; line-height: 16px; margin: var(--s2) 0; }
  .note.acc { color: var(--tx); background: var(--acbg); border-radius: var(--r); padding: var(--s2) var(--s3); }
  textarea { width: 100%; box-sizing: border-box; min-height: 96px; resize: vertical; border: 1px solid var(--line); border-radius: var(--r); background: var(--bg); color: var(--tx); padding: var(--s2) var(--s3); font: 13px/20px var(--font); outline: none; }
  textarea:focus { border-color: var(--ac); }
  textarea::placeholder { color: var(--tx3); }
  .bind { margin-top: var(--s1); }
  .errs { margin-top: var(--s2); }
  .wn { margin: 0; font-size: 12px; line-height: 16px; color: var(--warn); }
  .acts { display: flex; flex-wrap: wrap; align-items: center; gap: var(--s2); margin-top: var(--s4); }
  .isdef { color: var(--tx3); font-size: 12px; line-height: 16px; flex: 1 1 220px; min-width: 0; }
  .foot { display: flex; flex-wrap: wrap; gap: var(--s2); padding: var(--s4) 0; }
  .mp { margin: 0; color: var(--tx2); font-size: 13px; line-height: 20px; }
</style>
