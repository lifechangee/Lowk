<script lang="ts">
  // A finished call: tabs Transcript / Summary / Questions to AI / Screenshots / Sources; page actions in the ⋯ menu.
  import { onMount, tick } from 'svelte';
  import type { AppError, SourceRef, TagSuggestion, LlmProviderStatus } from '../../lib/ipc';
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import Menu from '../../lib/components/Menu.svelte';
  import SourceLabel from '../../lib/components/SourceLabel.svelte';
  import Tabs from '../../lib/components/Tabs.svelte';
  import TagEditor from '../../lib/components/TagEditor.svelte';
  import Tag from '../../lib/components/Tag.svelte';
  import { tPlural } from '../../lib/i18n/locale.svelte';
  import { api, folderKey, type CallDetail } from '../api';
  import { appErrorText, errText } from '../errors';
  import { clock, download, duration, fullDate, langTag } from '../fmt';
  import { tm } from '../i18n';
  import Md from '../Md.svelte';
  import { hrefs, router } from '../router.svelte';
  import { pickLine } from '../search/util';
  import { library, ui } from '../state.svelte';

  let { id, at = null, match = null }: { id: string; at?: number | null; match?: string | null } = $props();

  let detail = $state<CallDetail | null>(null);
  let md = $state('');
  let showProps = $state(false);
  let provider = $state<LlmProviderStatus | null>(null);
  let body: HTMLElement | undefined = $state();
  let hitIdx = $state<number | null>(null);

  // ---- summary ----
  let sum = $state<{ phase: 'idle' | 'streaming' | 'error'; text: string; error: AppError | null }>({ phase: 'idle', text: '', error: null });
  // ---- tag suggestions ----
  let sug = $state<{ phase: 'idle' | 'loading' | 'ready' | 'error'; list: TagSuggestion[]; error: string }>({ phase: 'idle', list: [], error: '' });

  $effect(() => {
    const cid = id;
    detail = null;
    sum = { phase: 'idle', text: '', error: null };
    sug = { phase: 'idle', list: [], error: '' };
    api.callGet(cid).then((d) => { if (cid === id) detail = d; });
    api.llmProviderStatus().then((p) => (provider = p)).catch(() => (provider = null));
  });
  $effect(() => {
    if (ui.callTab === 'md' && detail) api.exportMarkdown(detail.meta.id).then((t) => (md = t));
  });
  // A search hit / source label opens the call at a minute: scroll to the nearest preceding line and flash it.
  $effect(() => {
    const t = at;
    if (t === null || !detail) return;
    ui.callTab = 'transcript';
    void tick().then(() => {
      const best = pickLine(detail!.transcript, t, match);
      hitIdx = best;
      body?.querySelector<HTMLElement>(`[data-line="${best}"]`)?.scrollIntoView({ block: 'center' });
      setTimeout(() => { if (hitIdx === best) hitIdx = null; }, 2600);
    });
  });

  onMount(() =>
    api.onSummaryEvent((e) => {
      if (e.call !== id) return;
      if (e.type === 'start') sum = { phase: 'streaming', text: '', error: null };
      else if (e.type === 'delta') sum.text += e.text;
      else if (e.type === 'done') {
        sum = { phase: 'idle', text: '', error: null };
        if (detail) detail.summary = e.summary.md;
        void library.reload();
        void suggest();
      } else if (e.error.code === 'llm_cancelled') sum = { phase: 'idle', text: '', error: null };
      else sum = { phase: 'error', text: '', error: e.error };
    }),
  );

  async function summarize() {
    if (!detail || sum.phase === 'streaming') return;
    ui.callTab = 'summary';
    sum = { phase: 'streaming', text: '', error: null };
    try { await api.llmSummarize(detail.meta.id); } catch (e) { sum = { phase: 'error', text: '', error: e instanceof Error && 'app' in e ? (e as { app: AppError }).app : { code: 'unknown' } }; }
  }
  const cancelSummary = () => detail && void api.llmSummarizeCancel(detail.meta.id);

  async function suggest() {
    if (!detail) return;
    sug = { phase: 'loading', list: [], error: '' };
    try {
      const list = await api.llmSuggestTags(detail.meta.id);
      sug = { phase: 'ready', list, error: '' };
    } catch (e) { sug = { phase: 'error', list: [], error: errText(e) }; }
  }
  async function setTags(tags: string[]) {
    if (!detail) return;
    try {
      await api.callSetTags(detail.meta.id, tags);
      detail.meta.tags = tags;
      await library.reload();
    } catch (e) { ui.notify(errText(e)); }
  }
  async function accept(s: TagSuggestion) {
    if (!detail) return;
    sug.list = sug.list.filter((x) => x.tag !== s.tag);
    if (!detail.meta.tags.includes(s.tag)) await setTags([...detail.meta.tags, s.tag]);
  }
  const reject = (s: TagSuggestion) => (sug.list = sug.list.filter((x) => x.tag !== s.tag));

  async function doExport() {
    if (detail) download(`${detail.meta.title}.md`, await api.exportMarkdown(detail.meta.id));
  }
  async function trashCall() {
    if (!detail) return;
    await api.callTrash(detail.meta.id);
    await library.reload();
    ui.notify(tm('m.movedToTrash'));
    router.go(hrefs.folder(folderKey(detail.meta.folder)));
  }
  function moveCall() {
    const r = document.querySelector('[data-testid=call-menu]')?.getBoundingClientRect();
    if (detail) ui.openMove(detail.meta.id, r?.right ?? 600, r?.bottom ?? 120);
  }
  async function copySummary() {
    if (detail?.summary) { try { await navigator.clipboard.writeText(detail.summary); ui.notify(tm('summary.copied')); } catch { ui.notify(tm('summary.copyFailed')); } }
  }

  const speaker = (s: 'me' | 'system') => (s === 'me' ? tm('main.speakerMe') : tm('main.speakerSystem'));
  type Item = { at: number; kind: 'line'; i: number } | { at: number; kind: 'qa'; i: number } | { at: number; kind: 'shot'; i: number };
  const timeline = $derived.by<Item[]>(() => {
    if (!detail) return [];
    const items: Item[] = [
      ...detail.transcript.map((u, i): Item => ({ at: u.at_s, kind: 'line', i })),
      ...detail.qa.map((q, i): Item => ({ at: q.at_s + 0.1, kind: 'qa', i })),
      ...detail.shots.map((s, i): Item => ({ at: s.at_s + 0.2, kind: 'shot', i })),
    ];
    return items.sort((a, b) => a.at - b.at);
  });
  const sources = $derived(detail?.sources ?? []);
  const withheld = $derived((detail?.qa ?? []).reduce((n, q) => n + (q.withheld ?? 0), 0));
  const byKind = (k: SourceRef['kind']) => sources.filter((s) => s.kind === k);

  const hasSummary = $derived(!!detail?.summary);
  const tabs = $derived([
    { id: 'transcript' as const, label: tm('main.transcript') },
    { id: 'summary' as const, label: tm('main.summary') },
    { id: 'qa' as const, label: tm('main.aiQuestions'), count: detail?.qa.length ?? 0 },
    { id: 'shots' as const, label: tm('main.screenshots'), count: detail?.shots.length ?? 0 },
    { id: 'sources' as const, label: tm('main.sources'), count: sources.length },
  ]);
  const cloudFolder = $derived(detail && detail.meta.folder.kind === 'user' ? (library.folder(folderKey(detail.meta.folder))?.allow_cloud ?? true) : true);
  const bold = (s: string): { b: boolean; t: string }[] => s.split(/\*\*(.+?)\*\*/g).map((t, i) => ({ b: i % 2 === 1, t })).filter((x) => x.t);
</script>

{#snippet answer(text: string)}{#each bold(text) as p, i (i)}{#if p.b}<b>{p.t}</b>{:else}{p.t}{/if}{/each}{/snippet}
{#snippet srcs(list: SourceRef[] | undefined, held: number | undefined)}
  {#if (list?.length ?? 0) > 0 || (held ?? 0) > 0}
    <div class="srcs" data-testid="answer-sources">
      <span class="sl lbl">{tm('main.basedOn')}</span>
      {#each list ?? [] as s, i (i)}<SourceLabel source={s} />{/each}
      {#if (held ?? 0) > 0}<span class="held lbl" title={tm('source.withheldHint')}><Icon name="shield" size={12} />{tPlural('source.withheld', held ?? 0)}</span>{/if}
    </div>
  {/if}
{/snippet}

{#if detail}
  <div class="page">
    <div class="cin chead">
      <div class="row1">
        <div class="hd">
          <h1 class="lbl" title={detail.meta.title}>{detail.meta.title}</h1>
          <div class="meta">
            <span>{fullDate(detail.meta.started)}</span><i class="d"></i><span>{duration(detail.meta.duration_s)}</span>
            {#if detail.meta.sources.system}<i class="d"></i><span class="lbl mx">{detail.meta.sources.system}</span>{/if}
            <i class="d"></i><span>{langTag(detail.meta.languages)}</span>
            <TagEditor tags={detail.meta.tags} addLabel={tm('main.addTag')} newLabel={tm('main.newTag')} removeLabel={(t) => tm('main.removeTag', { tag: t })} onchange={setTags} />
          </div>
        </div>
        <Menu label={tm('main.callMenu')} testid="call-menu" items={[
          { id: 'props', label: showProps ? tm('main.hideProps') : tm('main.showProps'), icon: 'info', onselect: () => (showProps = !showProps) },
          { id: 'md', label: tm('main.exportMd'), icon: 'doc', onselect: () => (ui.callTab = 'md') },
          { id: 'export', label: tm('main.export'), icon: 'download', onselect: doExport },
          { sep: true },
          { id: 'move', label: tm('m.moveTo'), icon: 'folder', onselect: moveCall },
          { id: 'trash', label: tm('m.moveToTrash'), icon: 'trash', danger: true, onselect: trashCall },
        ]} />
      </div>
      {#if showProps}
        <dl class="props" data-testid="call-props">
          <div><dt>{tm('settings.source')}</dt><dd class="lbl">{detail.meta.sources.system ?? '—'}</dd></div>
          <div><dt>{tm('m.languages')}</dt><dd class="lbl">{langTag(detail.meta.languages)}</dd></div>
          <div><dt>{tm('settings.stt')}</dt><dd class="lbl">{detail.meta.stt_engine ?? '—'}</dd></div>
          <div><dt>{tm('settings.answers')}</dt><dd class="lbl">{detail.model}</dd></div>
        </dl>
      {/if}
    </div>

    <Tabs {tabs} value={ui.callTab === 'md' ? 'transcript' : ui.callTab} label={detail.meta.title} onselect={(t) => (ui.callTab = t)} />

    <div class="doc" bind:this={body}>
      <div class="cin">
        {#if ui.callTab === 'transcript'}
          {#each timeline as it (it.kind + it.i)}
            {#if it.kind === 'line'}
              {@const u = detail.transcript[it.i]!}
              <div class="ln" class:sys={u.speaker === 'system'} class:hit={hitIdx === it.i} data-line={it.i}><span class="ts">{clock(u.at_s)}</span><span class={u.speaker === 'me' ? 'sp-me' : 'sp-sys'}>{speaker(u.speaker)}</span><span class="tx">{u.text}</span></div>
            {:else if it.kind === 'qa'}
              {@const q = detail.qa[it.i]!}
              <div class="qa ind">
                <div class="q"><Icon name="sparkle" size={12} /><span class="lbl">{tm('m.aiHint')} · {q.template} · {clock(q.at_s)}</span></div>
                <p>{@render answer(q.answer)}</p>
                {@render srcs(q.sources, q.withheld)}
                {#if q.my_answer}<div class="mine"><div class="q"><span class="lbl">{tm('m.myAnswer')}{q.my_answer_at_s !== null ? ' · ' + clock(q.my_answer_at_s) : ''}</span></div><i>«{q.my_answer}»</i></div>{/if}
              </div>
            {:else}
              {@const s = detail.shots[it.i]!}
              <div class="shot ind"><div class="im">{s.title}</div><div class="cp"><Tag text={tm('m.screenshot')} /><span class="lbl">{clock(s.at_s)} · {s.caption}</span></div></div>
            {/if}
          {/each}
          {#if timeline.length === 0}<p class="empty">{tm('m.emptyTranscript')}</p>{/if}
        {:else if ui.callTab === 'summary'}
          <div class="sumbox" data-testid="summary-tab">
            {#if sum.phase === 'streaming'}
              <div class="sh" role="status"><span class="lbl"><i class="spin"></i>{tm('summary.building')}</span><Button size="sm" onclick={cancelSummary}>{tm('common.cancel')}</Button></div>
              {#if sum.text}<Md md={sum.text} cursor />{:else}<p class="muted">{tm('summary.waiting')}</p>{/if}
            {:else}
              {#if sum.phase === 'error' && sum.error}
                <div class="err" role="alert" data-testid="summary-error"><Icon name="alert" size={16} /><span>{appErrorText(sum.error)}</span>
                  {#if sum.error.code === 'api_key_missing' || sum.error.code === 'llm_provider'}<a class="lk" href={hrefs.settings('providers')}>{tm('summary.openSettings')}</a>{/if}
                  <Button size="sm" icon="refresh" onclick={summarize}>{tm('common.retry')}</Button></div>
              {/if}
              {#if detail.summary}
                <div class="sh"><span class="lbl muted">{tm('summary.by', { model: provider?.model ?? detail.model })}</span>
                  <span class="acts"><Button size="sm" icon="copy" onclick={copySummary}>{tm('common.copy')}</Button><Button size="sm" icon="refresh" onclick={summarize}>{tm('m.regenerate')}</Button></span></div>
                <Md md={detail.summary} />
                {#if sug.phase === 'idle'}<div class="sugcta"><Button size="sm" icon="tag" onclick={suggest}>{tm('summary.suggestTags')}</Button></div>{/if}
              {:else if sum.phase !== 'error'}
                <div class="start" data-testid="summary-empty">
                  <div class="eic"><Icon name="sparkle" size={20} /></div>
                  <h2>{tm('summary.empty.title')}</h2>
                  <p>{tm('summary.empty.body')}</p>
                  <small class="note">{tm('summary.cloudNote', { model: provider?.model ?? 'DeepSeek' })}</small>
                  {#if !cloudFolder}<small class="note warnl"><Icon name="alert" size={12} />{tm('summary.localFolderNote')}</small>{/if}
                  {#if provider && !provider.key_present}<small class="note warnl" data-testid="summary-nokey"><Icon name="key" size={12} />{tm('summary.noKey')} <a class="lk" href={hrefs.settings('providers')}>{tm('summary.openSettings')}</a></small>{/if}
                  <Button variant="primary" icon="sparkle" onclick={summarize}>{tm('summary.create')}</Button>
                </div>
              {/if}
              {#if sug.phase !== 'idle'}
                <section class="sug" data-testid="tag-suggestions">
                  <div class="sugh"><b>{tm('summary.tags.title')}</b><small class="lbl">{tm('summary.tags.hint')}</small></div>
                  {#if sug.phase === 'loading'}<p class="muted">{tm('summary.tags.loading')}</p>
                  {:else if sug.phase === 'error'}<p class="errl" role="alert">{sug.error} <Button size="sm" icon="refresh" onclick={suggest}>{tm('common.retry')}</Button></p>
                  {:else if sug.list.length === 0}<p class="muted">{tm('summary.tags.none')}</p>
                  {:else}
                    <div class="chips">
                      {#each sug.list as s (s.tag)}
                        <span class="sg" class:new={!s.existing} title={s.reason ?? ''}>
                          <span class="sgt lbl"># {s.tag}{#if s.existing}<em>{tm('summary.tags.inVault')}</em>{/if}</span>
                          <button class="ok" aria-label={tm('summary.tags.accept', { tag: s.tag })} title={tm('summary.tags.accept', { tag: s.tag })} onclick={() => accept(s)}><Icon name="check" size={12} /></button>
                          <button class="no" aria-label={tm('summary.tags.reject', { tag: s.tag })} title={tm('summary.tags.reject', { tag: s.tag })} onclick={() => reject(s)}><Icon name="close" size={12} /></button>
                        </span>
                      {/each}
                    </div>
                  {/if}
                </section>
              {/if}
            {/if}
          </div>
        {:else if ui.callTab === 'qa'}
          {#each detail.qa as q (q.id)}
            <div class="chain3">
              <div class="st q"><div class="lab"><span class="lbl">{tm('m.question')} · {clock(q.at_s)} · {speaker(q.question_stream)}</span></div><div class="box">{q.question}</div></div>
              <div class="st ai"><div class="lab"><span class="lbl">{tm('m.aiHint')}</span><Tag text={q.template} /></div><div class="box">{@render answer(q.answer)}{@render srcs(q.sources, q.withheld)}</div></div>
              <div class="st mine"><div class="lab"><span class="lbl">{tm('m.myAnswer')}{q.my_answer_at_s !== null ? ' · ' + clock(q.my_answer_at_s) : ''}</span></div><div class="box">{q.my_answer ? `«${q.my_answer}»` : tm('m.noMyAnswer')}</div></div>
            </div>
          {:else}
            <p class="empty">{tm('m.emptyQa')}</p>
          {/each}
        {:else if ui.callTab === 'shots'}
          <div class="shots2">
            {#each detail.shots as s (s.id)}<div class="sh2"><div class="im">{s.title}</div><div class="cp lbl">{clock(s.at_s)} · {s.caption}</div></div>{:else}<p class="empty">{tm('m.emptyShots')}</p>{/each}
          </div>
        {:else if ui.callTab === 'sources'}
          <div class="srctab" data-testid="sources-tab">
            {#if sources.length === 0 && withheld === 0}
              <div class="start"><div class="eic"><Icon name="layers" size={20} /></div><h2>{tm('source.empty.title')}</h2><p>{tm('source.empty.body')}</p></div>
            {:else}
              <p class="lead">{tm('source.intro')}</p>
              {#each [['instructions', 'source.group.instructions'], ['doc', 'source.group.docs'], ['call', 'source.group.calls'], ['image', 'source.group.images']] as const as [kind, label] (kind)}
                {#if byKind(kind).length}
                  <section class="grp"><h3 class="h6">{tm(label)}</h3><div class="chips">{#each byKind(kind) as s, i (i)}<SourceLabel source={s} />{/each}</div></section>
                {/if}
              {/each}
              {#if withheld > 0}<p class="held2"><Icon name="shield" size={14} />{tPlural('source.withheld', withheld)}. {tm('source.withheldHint')}</p>{/if}
            {/if}
          </div>
        {:else}
          <div class="mdh"><Button size="sm" icon="chevron-left" onclick={() => (ui.callTab = 'transcript')}>{tm('main.transcript')}</Button></div>
          <pre class="md">{md}</pre>
        {/if}
      </div>
    </div>
  </div>
{:else}
  <p class="empty pad">{tm('common.loading')}</p>
{/if}

<style>
  .page { display: flex; flex-direction: column; min-height: 0; flex: 1; }
  .cin { max-width: 768px; width: 100%; margin: 0 auto; padding: 0 var(--s8); }
  .chead { padding-top: var(--s6); padding-bottom: var(--s4); }
  .row1 { display: flex; justify-content: space-between; gap: var(--s4); align-items: flex-start; }
  .hd { min-width: 0; flex: 1; }
  h1 { font-size: 24px; line-height: 32px; letter-spacing: -0.025em; margin: 0; font-weight: 650; }
  .meta { display: flex; flex-wrap: wrap; gap: var(--s1) var(--s3); align-items: center; color: var(--tx2); font-size: 12px; line-height: 20px; margin-top: var(--s1); }
  .mx { max-width: 200px; }
  .d { width: 3px; height: 3px; border-radius: 50%; background: var(--tx3); }
  .props { display: grid; grid-template-columns: repeat(auto-fit, minmax(160px, 1fr)); gap: var(--s3) var(--s6); margin: var(--s4) 0 0; padding: var(--s3) var(--s4); border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--panel); }
  .props div { min-width: 0; }
  .props dt { font: 600 var(--fs-xs) var(--font); letter-spacing: 0.08em; text-transform: uppercase; color: var(--tx3); }
  .props dd { margin: var(--s0) 0 0; font-size: 13px; }
  .doc { flex: 1; overflow: auto; padding: var(--s6) 0 64px; min-height: 0; }
  .ln { display: grid; grid-template-columns: 64px 72px minmax(0, 1fr); gap: var(--s3); padding: var(--s1) 0; border-radius: var(--r-sm); }
  .ln.hit { background: var(--acbg); box-shadow: -8px 0 0 var(--acbg), 8px 0 0 var(--acbg); }
  .ts { font: 500 12px/20px var(--mono); color: var(--tx3); }
  .sp-me { font-size: 12px; font-weight: 700; color: var(--tx); }
  .sp-sys { font-size: 12px; font-weight: 600; color: var(--tx2); }
  .sys .tx { color: var(--tx2); }
  .ind { margin-left: 148px; }
  .qa { margin-top: var(--s2); margin-bottom: var(--s2); border: 1px solid var(--line); border-left: 2px solid var(--ac); border-radius: 4px 12px 12px 4px; background: var(--panel); padding: var(--s3) var(--s4); }
  .qa .q { font: 600 var(--fs-xs) var(--font); letter-spacing: 0.03em; color: var(--ac); margin-bottom: var(--s2); display: flex; gap: var(--s2); align-items: center; }
  .qa p { margin: 0 0 var(--s1); }
  .srcs { display: flex; flex-wrap: wrap; gap: var(--s1) var(--s2); align-items: center; margin-top: var(--s2); }
  .sl { font: 600 var(--fs-xs) var(--font); letter-spacing: 0.08em; text-transform: uppercase; color: var(--tx3); margin-right: var(--s1); }
  .held { display: inline-flex; align-items: center; gap: var(--s1); height: 24px; padding: 0 var(--s2); border-radius: var(--r-sm); border: 1px dashed var(--line-strong); color: var(--tx2); font-size: 12px; max-width: 280px; }
  .mine { margin-top: var(--s3); padding-top: var(--s3); border-top: 1px dashed var(--line); font-size: 13px; }
  .mine .q { color: var(--tx2); margin-bottom: var(--s1); }
  .mine i { color: var(--tx); }
  .shot { margin-top: var(--s2); margin-bottom: var(--s2); max-width: 420px; border: 1px solid var(--line); border-radius: var(--r-lg); overflow: hidden; background: var(--panel); }
  .im { background: linear-gradient(135deg, #3a3d44, #1d1e22); color: #fff; font-weight: 650; height: 152px; padding: var(--s4); font-size: 20px; line-height: 24px; }
  .cp { padding: var(--s2) var(--s3); font-size: 12px; line-height: 16px; color: var(--tx2); display: flex; gap: var(--s2); align-items: center; min-width: 0; }
  .shots2 { display: grid; grid-template-columns: 1fr 1fr; gap: var(--s4); }
  .sh2 { border: 1px solid var(--line); border-radius: var(--r-lg); overflow: hidden; background: var(--panel); }
  .sumbox { border: 1px solid var(--line); background: var(--panel); border-radius: var(--r-lg); padding: var(--s4); display: flex; flex-direction: column; gap: var(--s3); }
  .sh { display: flex; justify-content: space-between; align-items: center; gap: var(--s3); min-width: 0; }
  .sh .lbl { display: inline-flex; align-items: center; gap: var(--s2); font-size: 12px; }
  .acts { display: flex; gap: var(--s2); flex: none; }
  .spin { width: 10px; height: 10px; border-radius: 50%; border: 2px solid var(--ac); border-top-color: transparent; animation: sp 0.9s linear infinite; flex: none; }
  @keyframes sp { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .spin { animation: none; } }
  .muted { color: var(--tx2); margin: 0; }
  .start { display: flex; flex-direction: column; align-items: center; text-align: center; gap: var(--s2); padding: var(--s6) var(--s4); }
  .start h2 { margin: 0; font: 600 var(--fs-lg) var(--font); }
  .start p { margin: 0 0 var(--s1); max-width: 460px; color: var(--tx2); }
  .note { display: flex; align-items: center; gap: var(--s1); max-width: 460px; color: var(--tx3); font-size: 12px; line-height: 16px; text-align: center; }
  .warnl { color: var(--tx2); }
  .start :global(.btn) { margin-top: var(--s2); }
  .eic { width: 44px; height: 44px; border-radius: var(--r-lg); background: var(--hv); border: 1px solid var(--line); display: grid; place-items: center; color: var(--tx2); margin-bottom: var(--s1); }
  .lk { color: var(--ac); }
  .err { display: flex; align-items: center; gap: var(--s3); border: 1px solid rgba(255, 77, 79, 0.4); border-radius: var(--r); padding: var(--s3); color: #ff8f90; font-size: 13px; }
  .err span { flex: 1; min-width: 0; }
  .sugcta { margin-top: var(--s2); }
  .sug { border-top: 1px solid var(--line); padding-top: var(--s4); display: flex; flex-direction: column; gap: var(--s3); }
  .sugh { display: flex; align-items: baseline; justify-content: space-between; gap: var(--s3); }
  .sugh small { color: var(--tx3); font-size: 12px; }
  .chips { display: flex; flex-wrap: wrap; gap: var(--s2); }
  .sg { display: inline-flex; align-items: center; height: 28px; border: 1px solid var(--line); border-radius: 99px; background: var(--hv); padding: 0 var(--s1) 0 var(--s3); gap: var(--s1); max-width: 100%; }
  .sg.new { border-style: dashed; }
  .sgt { font-size: 12px; color: var(--tx); display: inline-flex; align-items: center; gap: var(--s2); }
  .sgt em { font-style: normal; color: var(--tx3); font-size: 11px; }
  .sg button { all: unset; cursor: pointer; width: 20px; height: 20px; display: grid; place-items: center; border-radius: 50%; color: var(--tx3); }
  .sg .ok:hover, .sg .ok:focus-visible { background: var(--acbg); color: var(--ac); }
  .sg .no:hover, .sg .no:focus-visible { background: var(--sel); color: var(--tx); }
  .errl { margin: 0; color: #ff8f90; font-size: 13px; display: flex; align-items: center; gap: var(--s3); }
  .srctab .lead { margin: 0 0 var(--s4); color: var(--tx2); }
  .grp { margin-bottom: var(--s4); }
  .h6 { margin: 0 0 var(--s2); font: 600 var(--fs-xs) var(--font); letter-spacing: 0.08em; text-transform: uppercase; color: var(--tx3); }
  .held2 { display: flex; gap: var(--s2); align-items: flex-start; color: var(--tx2); font-size: 13px; margin: 0; }
  .held2 :global(svg) { margin-top: 2px; flex: none; }
  .chain3 { position: relative; margin: 0 0 var(--s6) var(--s1); padding-left: var(--s6); }
  .chain3::before { content: ''; position: absolute; left: 3px; top: 8px; bottom: 8px; width: 1px; background: var(--line); }
  .st.q, .st.ai, .st.mine { position: relative; margin-bottom: var(--s3); display: block; }
  .st.q::before, .st.ai::before, .st.mine::before { content: ''; position: absolute; left: -25px; top: 4px; width: 8px; height: 8px; border-radius: 50%; background: var(--bg); border: 1.5px solid var(--tx3); }
  .st.ai::before { background: var(--ac); border-color: var(--ac); }
  .st.mine::before { border-color: var(--tx); }
  .lab { font: 600 var(--fs-xs) var(--font); letter-spacing: 0.08em; text-transform: uppercase; color: var(--tx3); margin-bottom: var(--s1); display: flex; gap: var(--s2); align-items: center; min-width: 0; }
  .box { border: 1px solid var(--line); background: var(--panel); border-radius: var(--r); padding: var(--s2) var(--s3); }
  .st.mine .box { background: transparent; border-style: dashed; font-style: italic; }
  .st.q .box { background: transparent; border: none; padding: 0; color: var(--tx2); }
  .mdh { margin-bottom: var(--s3); }
  pre.md { font: 12px/20px var(--mono); color: var(--tx2); white-space: pre-wrap; margin: 0; }
  .empty { color: var(--tx3); margin: var(--s6) 0; }
  .pad { padding: var(--s6); }
  @media (max-width: 900px) { .ind { margin-left: 0; } .shots2 { grid-template-columns: 1fr; } }
</style>
