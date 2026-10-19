<script lang="ts">
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import Tag from '../../lib/components/Tag.svelte';
  import { api, type CallDetail } from '../api';
  import { clock, download, duration, fullDate, langTag } from '../fmt';
  import { tm } from '../i18n';
  import { library, ui } from '../state.svelte';

  let { id }: { id: string } = $props();

  let detail = $state<CallDetail | null>(null);
  let building = $state(false);
  let md = $state('');

  $effect(() => {
    const cid = id;
    detail = null;
    api.callGet(cid).then((d) => { if (cid === id) detail = d; });
  });
  $effect(() => {
    if (ui.callTab === 'md' && detail) api.exportMarkdown(detail.meta.id).then((t) => (md = t));
  });

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

  const hasSummary = $derived(!!detail?.summary);
  const tabs = $derived([
    { id: 'transcript', label: tm('main.transcript') },
    { id: 'summary', label: tm('main.summary') },
    { id: 'qa', label: `${tm('main.aiQuestions')} · ${detail?.qa.length ?? 0}` },
    { id: 'shots', label: `${tm('main.screenshots')} · ${detail?.shots.length ?? 0}` },
  ] as const);

  async function summarize() {
    if (!detail || building) return;
    ui.callTab = 'summary';
    building = true;
    const text = await api.summaryGenerate(detail.meta.id);
    detail.summary = text;
    building = false;
    await library.reload();
  }
  async function doExport() {
    if (detail) download(`${detail.meta.title}.md`, await api.exportMarkdown(detail.meta.id));
  }
  const bullets = (s: string) => s.split('\n').map((l) => l.replace(/^[-*]\s+/, '')).filter(Boolean);
  const bold = (s: string): { b: boolean; t: string }[] => s.split(/\*\*(.+?)\*\*/g).map((t, i) => ({ b: i % 2 === 1, t })).filter((x) => x.t);
</script>

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
            {#each detail.meta.tags as t (t)}<Tag text={'# ' + t} />{/each}
          </div>
        </div>
        <Button variant="primary" disabled={building} onclick={summarize}>{hasSummary ? tm('m.regenerate') : tm('main.summary')}</Button>
      </div>
    </div>

    <div class="tabsbar">
      <div class="cin tabrow">
        <div class="tabs" role="tablist">
          {#each tabs as tab (tab.id)}
            <button class="tab" class:on={ui.callTab === tab.id} role="tab" aria-selected={ui.callTab === tab.id} onclick={() => (ui.callTab = tab.id)}><span>{tab.label}</span></button>
          {/each}
        </div>
        <div class="tools">
          <Button size="sm" pressed={ui.callTab === 'md'} onclick={() => (ui.callTab = ui.callTab === 'md' ? 'transcript' : 'md')}>{tm('main.exportMd')}</Button>
          <Button size="sm" icon="download" title={tm('main.export')} onclick={doExport} />
        </div>
      </div>
    </div>

    <div class="doc">
      <div class="cin">
        {#if ui.callTab === 'transcript'}
          {#each timeline as it (it.kind + it.i)}
            {#if it.kind === 'line'}
              {@const u = detail.transcript[it.i]!}
              <div class="ln" class:sys={u.speaker === 'system'}><span class="ts">{clock(u.at_s)}</span><span class={u.speaker === 'me' ? 'sp-me' : 'sp-sys'}>{speaker(u.speaker)}</span><span class="tx">{u.text}</span></div>
            {:else if it.kind === 'qa'}
              {@const q = detail.qa[it.i]!}
              <div class="qa ind">
                <div class="q"><Icon name="sparkle" size={12} /><span class="lbl">{tm('m.aiHint')} · {q.template} · {clock(q.at_s)}</span></div>
                <p>{#each bold(q.answer) as p}{#if p.b}<b>{p.t}</b>{:else}{p.t}{/if}{/each}</p>
                {#if q.my_answer}<div class="mine"><div class="q"><span class="lbl">{tm('m.myAnswer')}{q.my_answer_at_s !== null ? ' · ' + clock(q.my_answer_at_s) : ''}</span></div><i>«{q.my_answer}»</i></div>{/if}
              </div>
            {:else}
              {@const s = detail.shots[it.i]!}
              <div class="shot ind"><div class="im">{s.title}</div><div class="cp"><Tag text={tm('m.screenshot')} /><span class="lbl">{clock(s.at_s)} · {s.caption}</span></div></div>
            {/if}
          {/each}
          {#if timeline.length === 0}<p class="empty">{tm('m.emptyTranscript')}</p>{/if}
        {:else if ui.callTab === 'summary'}
          <div class="sumbox">
            <div class="h6">{tm('main.summary')}</div>
            {#if building}<div class="muted">{tm('main.summaryBuilding')}</div>
            {:else if detail.summary}<ul>{#each bullets(detail.summary) as b}<li>{b}</li>{/each}</ul>
            {:else}<div class="muted">{tm('main.summaryEmpty')}</div>{/if}
          </div>
        {:else if ui.callTab === 'qa'}
          {#each detail.qa as q (q.id)}
            <div class="chain3">
              <div class="st q"><div class="lab"><span class="lbl">{tm('m.question')} · {clock(q.at_s)} · {speaker(q.question_stream)}</span></div><div class="box">{q.question}</div></div>
              <div class="st ai"><div class="lab"><span class="lbl">{tm('m.aiHint')}</span><Tag text={q.template} /></div><div class="box">{#each bold(q.answer) as p}{#if p.b}<b>{p.t}</b>{:else}{p.t}{/if}{/each}</div></div>
              <div class="st mine"><div class="lab"><span class="lbl">{tm('m.myAnswer')}{q.my_answer_at_s !== null ? ' · ' + clock(q.my_answer_at_s) : ''}</span></div><div class="box">{q.my_answer ? `«${q.my_answer}»` : tm('m.noMyAnswer')}</div></div>
            </div>
          {:else}
            <p class="empty">{tm('m.emptyQa')}</p>
          {/each}
        {:else if ui.callTab === 'shots'}
          <div class="shots2">
            {#each detail.shots as s (s.id)}<div class="sh"><div class="im">{s.title}</div><div class="cp lbl">{clock(s.at_s)} · {s.caption}</div></div>{:else}<p class="empty">{tm('m.emptyShots')}</p>{/each}
          </div>
        {:else}
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
  .tabsbar { border-bottom: 1px solid var(--line); flex: none; }
  .tabrow { display: flex; align-items: center; height: 44px; gap: var(--s4); }
  .tabs { display: flex; gap: var(--s1); height: 100%; min-width: 0; overflow-x: auto; scrollbar-width: none; }
  .tab { all: unset; box-sizing: border-box; cursor: pointer; display: inline-flex; align-items: center; height: 44px; padding: 0 var(--s3); color: var(--tx2); font-size: 13px; border-bottom: 2px solid transparent; margin-bottom: -1px; white-space: nowrap; flex: none; }
  .tab:hover { color: var(--tx); }
  .tab.on { color: var(--tx); border-color: var(--ac); font-weight: 550; }
  .tools { margin-left: auto; display: flex; gap: var(--s2); flex: none; }
  .doc { flex: 1; overflow: auto; padding: var(--s6) 0 64px; min-height: 0; }
  .ln { display: grid; grid-template-columns: 64px 72px minmax(0, 1fr); gap: var(--s3); padding: var(--s1) 0; }
  .ts { font: 500 12px/20px var(--mono); color: var(--tx3); }
  .sp-me { font-size: 12px; font-weight: 700; color: var(--tx); }
  .sp-sys { font-size: 12px; font-weight: 600; color: var(--tx2); }
  .sys .tx { color: var(--tx2); }
  .ind { margin-left: 148px; }
  .qa { margin-top: var(--s2); margin-bottom: var(--s2); border: 1px solid var(--line); border-left: 2px solid var(--ac); border-radius: 4px 12px 12px 4px; background: var(--panel); padding: var(--s3) var(--s4); }
  .qa .q { font: 600 var(--fs-xs) var(--font); letter-spacing: 0.03em; color: var(--ac); margin-bottom: var(--s2); display: flex; gap: var(--s2); align-items: center; }
  .qa p { margin: 0 0 var(--s1); }
  .mine { margin-top: var(--s3); padding-top: var(--s3); border-top: 1px dashed var(--line); font-size: 13px; }
  .mine .q { color: var(--tx2); margin-bottom: var(--s1); }
  .mine i { color: var(--tx); }
  .shot { margin-top: var(--s2); margin-bottom: var(--s2); max-width: 420px; border: 1px solid var(--line); border-radius: var(--r-lg); overflow: hidden; background: var(--panel); }
  .im { background: linear-gradient(135deg, #3a3d44, #1d1e22); color: #fff; font-weight: 650; height: 152px; padding: var(--s4); font-size: 20px; line-height: 24px; }
  .cp { padding: var(--s2) var(--s3); font-size: 12px; line-height: 16px; color: var(--tx2); display: flex; gap: var(--s2); align-items: center; min-width: 0; }
  .shots2 { display: grid; grid-template-columns: 1fr 1fr; gap: var(--s4); }
  .sh { border: 1px solid var(--line); border-radius: var(--r-lg); overflow: hidden; background: var(--panel); }
  .sumbox { border: 1px solid var(--line); background: var(--panel); border-radius: var(--r-lg); padding: var(--s4); }
  .sumbox ul { margin: var(--s2) 0 0; padding-left: var(--s4); }
  .sumbox li { margin: var(--s1) 0; }
  .h6 { font: 600 var(--fs-xs) var(--font); letter-spacing: 0.08em; text-transform: uppercase; color: var(--tx3); }
  .muted { color: var(--tx2); margin-top: var(--s2); }
  .chain3 { position: relative; margin: 0 0 var(--s6) var(--s1); padding-left: var(--s6); }
  .chain3::before { content: ''; position: absolute; left: 3px; top: 8px; bottom: 8px; width: 1px; background: var(--line); }
  .st { position: relative; margin-bottom: var(--s3); }
  .st::before { content: ''; position: absolute; left: -25px; top: 4px; width: 8px; height: 8px; border-radius: 50%; background: var(--bg); border: 1.5px solid var(--tx3); }
  .st.ai::before { background: var(--ac); border-color: var(--ac); }
  .st.mine::before { border-color: var(--tx); }
  .lab { font: 600 var(--fs-xs) var(--font); letter-spacing: 0.08em; text-transform: uppercase; color: var(--tx3); margin-bottom: var(--s1); display: flex; gap: var(--s2); align-items: center; min-width: 0; }
  .box { border: 1px solid var(--line); background: var(--panel); border-radius: var(--r); padding: var(--s2) var(--s3); }
  .st.mine .box { background: transparent; border-style: dashed; font-style: italic; }
  .st.q .box { background: transparent; border: none; padding: 0; color: var(--tx2); }
  pre.md { font: 12px/20px var(--mono); color: var(--tx2); white-space: pre-wrap; margin: 0; }
  .empty { color: var(--tx3); margin: var(--s6) 0; }
  .pad { padding: var(--s6); }
  @media (max-width: 900px) { .ind { margin-left: 0; } .shots2 { grid-template-columns: 1fr; } }
</style>
