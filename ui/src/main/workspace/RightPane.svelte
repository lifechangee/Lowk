<script lang="ts">
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import Section from '../../lib/components/Section.svelte';
  import Select from '../../lib/components/Select.svelte';
  import Tag from '../../lib/components/Tag.svelte';
  import TextInput from '../../lib/components/TextInput.svelte';
  import Toggle from '../../lib/components/Toggle.svelte';
  import { api, folderKey, isSystemKey, type CallDetail, type FolderDetail } from '../api';
  import { langTag } from '../fmt';
  import { tm } from '../i18n';
  import { hrefs, router } from '../router.svelte';
  import { library, ui } from '../state.svelte';

  let { folder, callId }: { folder: string; callId: string | null } = $props();

  let pane = $state<'folder' | 'call'>('folder');
  let fd = $state<FolderDetail | null>(null);
  let cd = $state<CallDetail | null>(null);
  let instr = $state('');
  let addingTag = $state(false);
  let tagText = $state('');
  let fileInput: HTMLInputElement | undefined = $state();

  const userFolder = $derived(!isSystemKey(folder));

  $effect(() => {
    const k = folder;
    fd = null;
    if (!isSystemKey(k)) api.folderGet(k).then((d) => { if (k === folder) { fd = d; instr = d?.instructions ?? ''; } });
  });
  $effect(() => {
    const id = callId;
    cd = null;
    if (id) { pane = 'call'; api.callGet(id).then((d) => { if (id === callId) cd = d; }); }
    else pane = 'folder';
  });

  const MAX = 500;
  async function saveInstr() { if (fd && instr !== fd.instructions) { await api.folderUpdate(folder, { instructions: instr }); fd.instructions = instr; } }
  async function addTag() {
    const t = tagText.trim().replace(/^#\s*/, '');
    addingTag = false; tagText = '';
    if (!t || !fd || fd.meta.tags.includes(t)) return;
    const tags = [...fd.meta.tags, t];
    await api.folderUpdate(folder, { tags });
    fd.meta.tags = tags;
    await library.reload();
  }
  async function addFiles(files: FileList | null) {
    if (!files || !fd) return;
    for (const f of files) fd.docs = [...fd.docs, await api.docAdd(folder, f.name)];
  }
  async function trashCall() {
    if (!cd) return;
    await api.callTrash(cd.meta.id);
    await library.reload();
    ui.notify(tm('m.movedToTrash'));
    router.go(hrefs.folder(folderKey(cd.meta.folder)));
  }
  async function move(to: string) { if (cd && to) { await api.callMove(cd.meta.id, to); await library.reload(); router.go(hrefs.call(cd.meta.id)); } }
  const tabLinks = $derived([
    { id: 'transcript', label: tm('main.transcript') },
    { id: 'qa', label: `${tm('main.aiQuestions')} · ${cd?.qa.length ?? 0}` },
    { id: 'shots', label: `${tm('main.screenshots')} · ${cd?.shots.length ?? 0}` },
    { id: 'summary', label: tm('main.summary') },
  ] as const);
  const moveOptions = $derived(library.folders.map((f) => ({ value: folderKey(f.id), label: f.title })));
</script>

<aside class="right">
  <div class="rh">
    {#if userFolder}<button class:on={pane === 'folder'} onclick={() => (pane = 'folder')}><span class="lbl">{tm('m.paneFolder')}</span></button>{/if}
    {#if callId}<button class:on={pane === 'call'} onclick={() => (pane = 'call')}><span class="lbl">{tm('m.paneCall')}</span></button>{/if}
  </div>

  {#if pane === 'folder' && fd}
    <div class="pane">
      <h4 class="lbl">{fd.meta.title}</h4>
      <div class="sub">{tm('settings.folder.instructionsHint')}</div>
      <div class="blk">
        <Section title={tm('settings.folder.instructions')} />
        <textarea class="inst" bind:value={instr} maxlength={MAX} placeholder={tm('settings.folder.instructionsPlaceholder')} aria-label={tm('settings.folder.instructions')} onblur={saveInstr}></textarea>
        <div class="cnt"><span class="lbl">{tm('settings.folder.documentsHint')}</span><span>{instr.length} / {MAX}</span></div>
      </div>
      <div class="blk">
        <Section title={`${tm('settings.folder.documents')} · ${fd.docs.length}`} />
        {#each fd.docs as d (d.id)}
          <div class="doc-i">
            <span class="ext">{d.ext.toUpperCase()}</span>
            <div><b class="lbl">{d.name}</b><small class="lbl">{d.state === 'indexing' ? tm('main.documentIndexing') : d.state === 'converted' ? `${d.pages} ${tm('m.pagesShort')} · ${tm('m.converted')}` : d.pages ? `${d.pages} ${tm('m.pagesShort')}` : ''}</small></div>
            {#if d.state === 'indexing'}<span class="dim">◌</span>{:else}<span class="ok"><Icon name="check" size={14} /></span>{/if}
          </div>
        {/each}
        <button class="drop" onclick={() => fileInput?.click()} ondragover={(e) => e.preventDefault()} ondrop={(e) => { e.preventDefault(); addFiles(e.dataTransfer?.files ?? null); }}>{tm('main.dropFiles')}</button>
        <input bind:this={fileInput} type="file" multiple accept=".pdf,.docx,.txt,.md" hidden onchange={(e) => addFiles(e.currentTarget.files)} />
      </div>
      <div class="blk">
        <Section title={tm('settings.folder.tags')} />
        <div class="tags">
          {#each fd.meta.tags as t, i (t)}<Tag accent={i === 0} text={'# ' + t} />{/each}
          {#if addingTag}<div class="tin"><TextInput bind:value={tagText} label={tm('main.newTag')} placeholder={tm('main.newTag')} autofocus onenter={addTag} /></div>
          {:else}<Tag dashed text={tm('main.addTag')} onclick={() => (addingTag = true)} />{/if}
        </div>
      </div>
      <div class="blk">
        <Section title={tm('settings.storage')} />
        <div class="row"><span class="k lbl">{tm('settings.folderOnDisk')}</span><button class="path lbl" title={fd.meta.path} onclick={() => api.revealPath(fd!.meta.path)}>~/Lowk/{fd.meta.path}</button></div>
      </div>
    </div>
  {:else if pane === 'call' && cd}
    <div class="pane">
      <h4 class="lbl">{tm('settings.folder.title')}</h4>
      <div class="sub lbl">{cd.meta.title}</div>
      <div class="blk outl">
        <Section title={tm('m.structure')} />
        {#each tabLinks as l (l.id)}<button class="ol lbl" onclick={() => (ui.callTab = l.id)}>{l.label}</button>{/each}
      </div>
      <div class="blk">
        <div class="row"><span class="k lbl">{tm('settings.source')}</span><span class="v lbl">{cd.meta.sources.system ?? '—'}</span></div>
        <div class="row"><span class="k lbl">{tm('m.languages')}</span><span class="v lbl">{langTag(cd.meta.languages)}</span></div>
        <div class="row"><span class="k lbl">{tm('settings.stt')}</span><span class="v lbl">{cd.meta.stt_engine ?? '—'}</span></div>
        <div class="row"><span class="k lbl">{tm('settings.answers')}</span><span class="v lbl">{cd.model}</span></div>
        <div class="row"><span class="k lbl">{tm('settings.keepAudio')}</span><Toggle checked={cd.keep_audio} label={tm('settings.keepAudio')} onchange={(v) => (cd!.keep_audio = v)} /></div>
      </div>
      {#if cd.meta.ended !== null}
        <div class="blk">
          <Section title={tm('m.moveTo')} />
          <Select label={tm('m.moveTo')} value={folderKey(cd.meta.folder)} options={cd.meta.folder.kind === 'user' ? moveOptions : [{ value: '', label: tm('main.unsorted') }, ...moveOptions]} onchange={move} />
        </div>
        <Button variant="danger" block icon="trash" onclick={trashCall}>{tm('m.moveToTrash')}</Button>
      {/if}
    </div>
  {/if}
</aside>

<style>
  .right { background: var(--side); border-left: 1px solid var(--line); overflow: auto; min-height: 0; min-width: 0; }
  .rh { display: flex; padding: var(--s2) var(--s3); gap: var(--s1); border-bottom: 1px solid var(--line); min-height: 45px; }
  .rh button { all: unset; cursor: pointer; height: 28px; padding: 0 var(--s3); border-radius: var(--r); color: var(--tx2); font-size: 13px; display: inline-flex; align-items: center; max-width: 50%; min-width: 0; }
  .rh button.on { background: var(--sel); color: var(--tx); }
  .pane { padding: var(--s4) var(--s4) var(--s8); }
  h4 { margin: 0; font: 600 var(--fs-lg) var(--font); }
  .sub { color: var(--tx2); font: var(--fs-sm) var(--font); margin: 0 0 var(--s4); }
  .blk { margin-bottom: var(--s6); }
  .inst { width: 100%; min-height: 112px; background: var(--bg); border: 1px solid var(--line); border-radius: var(--r); padding: var(--s2) var(--s3); color: var(--tx); resize: none; outline: none; font: 13px/20px var(--font); }
  .inst:focus { border-color: var(--ac); }
  .cnt { display: flex; justify-content: space-between; gap: var(--s2); color: var(--tx3); font: var(--fs-xs) var(--font); margin-top: var(--s1); }
  .doc-i { display: flex; gap: var(--s3); align-items: center; padding: var(--s2) var(--s3); border: 1px solid var(--line); border-radius: var(--r); background: var(--bg); margin-bottom: var(--s2); }
  .ext { font: 700 10px/16px sans-serif; padding: 0 var(--s1); border-radius: var(--s1); background: var(--hv); color: var(--tx2); letter-spacing: 0.04em; width: 40px; text-align: center; flex: none; }
  .doc-i div { flex: 1; min-width: 0; }
  .doc-i b { font-weight: 500; display: block; font-size: 13px; line-height: 20px; }
  small { display: block; color: var(--tx3); font: var(--fs-xs) var(--font); }
  .ok { color: var(--ac); display: grid; }
  .dim { color: var(--tx3); }
  .drop { all: unset; box-sizing: border-box; width: 100%; border: 1px dashed var(--line); border-radius: var(--r); padding: var(--s3); text-align: center; color: var(--tx3); font-size: 12px; cursor: pointer; }
  .drop:hover { border-color: var(--tx3); color: var(--tx2); }
  .tags { display: flex; gap: var(--s1); flex-wrap: wrap; align-items: center; }
  .tin { width: 120px; }
  .row { display: flex; justify-content: space-between; align-items: center; min-height: 40px; border-bottom: 1px solid var(--line); font-size: 13px; gap: var(--s4); }
  .k { color: var(--tx2); flex: none; max-width: 50%; }
  .v { text-align: right; }
  .path { all: unset; cursor: pointer; font: 12px var(--mono); min-width: 0; text-align: right; }
  .path:hover { color: var(--ac); }
  .outl .ol { all: unset; display: block; cursor: pointer; padding: var(--s1) 0; color: var(--tx2); font-size: 13px; max-width: 100%; box-sizing: border-box; }
  .outl .ol:hover { color: var(--tx); }
</style>
