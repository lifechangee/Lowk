<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type { HwInfo, UiLanguage } from '../../lib/ipc';
  import Mark from '../../lib/components/Mark.svelte';
  import Button from '../../lib/components/Button.svelte';
  import Icon, { type IconName } from '../../lib/components/Icon.svelte';
  import OptionCard from '../../lib/components/OptionCard.svelte';
  import { api } from '../api';
  import { bindFor, triggerText } from '../binds';
  import type { Key } from '../../lib/i18n';
  import { tm, type MKey } from '../i18n';
  import { mouseNames } from '../names';
  import { PERM_ORDER, perms } from '../permissions.svelte';
  import { hrefs, router } from '../router.svelte';
  import { callLangs, prefs, sources } from '../state.svelte';
  import { tour } from '../tour/tour.svelte';
  import LangPicker from '../LangPicker.svelte';
  import { resolve as resolveLangs, systemDefault } from '../langs';
  import { nameFromId, presetInfo, presetsFor, resolveSource } from '../source';
  import SourcePicker from '../SourcePicker.svelte';
  import KeyCard from '../settings/KeyCard.svelte';
  import BindEditor from '../settings/BindEditor.svelte';
  import PermRow from '../settings/PermRow.svelte';
  import EngineCard from '../settings/EngineCard.svelte';

  let { step }: { step: number } = $props();
  const TOTAL = 9;
  const names: Key[] = ['onboarding.step.language', 'onboarding.step.legal', 'onboarding.step.capture', 'onboarding.step.permissions', 'onboarding.step.hardware', 'onboarding.step.apiKey', 'onboarding.step.folder', 'onboarding.step.hotkeys', 'onboarding.step.done'];
  const os = api.platform();

  let skipped = $state<Set<number>>(new Set());
  let hw = $state<HwInfo | null>(null);
  let scanned = $state(false);
  let captureMode = $state<'app' | 'system'>('app');
  let appId = $state('');
  let engineMode = $state<'local' | 'cloud'>('local');
  /** Call languages, pre-filled: what is stored once onboarding was done before, else the system language + English. */
  let callLangsChoice = $state<string[]>(['ru', 'en']);
  let localTotalMb = $state(1400);
  let recId = $state('');
  let cloudName = $state('Soniox');

  onMount(async () => {
    await sources.refresh();
    // F12: preselect what will really be stored: an explicit «whole system» choice -> whole system;
    // a stored app -> that app; otherwise the first app playing audio; nothing playing -> whole system.
    const v = resolveSource(prefs.value.audio?.default_source, sources.running, prefs.value.capture?.default_source);
    appId = v.startsWith('app:') ? v.slice(4) : '';
    captureMode = appId ? 'app' : 'system';
    perms.refresh();
    await callLangs.load();
    const sys = systemDefault(await api.localeGet().catch(() => 'en'), callLangs.supported);
    callLangsChoice = prefs.value.onboarding?.done ? callLangs.default : resolveLangs(sys, ['en'], callLangs.supported);
    try {
      const l = (await api.engineStatus()).find((i) => i.recommended && i.descriptor.kind === 'local');
      if (l) recId = l.descriptor.id;
      if (l) localTotalMb = l.descriptor.size_mb + (prefs.value.stt?.background_correction === false ? 0 : (l.extra_mb ?? 0));
    } catch { /* keep the estimate */ }
    try {
      const v = prefs.value.stt?.cloud_vendor;
      const cs = (await api.engines()).filter((e) => e.kind === 'cloud');
      cloudName = (cs.find((e) => e.id === v) ?? cs[0])?.name ?? cloudName;
    } catch { /* keep the default */ }
  });
  // Live permission status while the permissions step is open (focus + light poll).
  $effect(() => (step === 3 ? perms.watch() : undefined));

  const ack = $derived(prefs.value.onboarding?.recording_notice_ack ?? false);
  const canNext = $derived(step !== 1 || ack);
  // A deep link or a stale stored step must not skip the notice: past step 1 it has to be acknowledged.
  $effect(() => { if (step > 1 && prefs.loaded && !ack) untrack(() => void go(1)); });
  const lang = $derived(prefs.value.ui?.language ?? 'auto');
  const recBind = $derived(bindFor(prefs.value.binds, 'toggle_record'));
  const recKey = $derived(recBind ? triggerText(recBind.trigger, os, mouseNames()) : '');

  $effect(() => {
    if (step === 4 && !hw) {
      scanned = false;
      api.hwProbe().then((h) => { hw = h; setTimeout(() => (scanned = true), 50); });
    }
  });

  async function go(n: number) {
    router.go(hrefs.onboarding(n));
    await prefs.patch({ onboarding: { step: n } });
  }
  async function next() {
    if (step === 2) await persistCapture();
    if (step === 4) await prefs.patch({ stt: await chosenEngine() });
    if (step < TOTAL - 1) await go(step + 1);
  }
  const persistCapture = () => prefs.patch({ stt: { languages: callLangsChoice, last_languages: [] }, capture: { default_source: captureMode === 'app' && appId ? 'one_app' : 'whole_system' }, audio: { default_source: captureMode === 'app' && appId ? { kind: 'app', id: appId } : { kind: 'whole_system' } } });
  /** The REAL registered engine id (an invented placeholder here meant no call ever found an engine). */
  async function chosenEngine(): Promise<{ engine: string; cloud_vendor?: string }> {
    const all = await api.engines();
    if (engineMode === 'cloud') {
      const c = all.find((e) => e.kind === 'cloud' && e.id === (prefs.value.stt?.cloud_vendor || 'soniox')) ?? all.find((e) => e.kind === 'cloud');
      return c ? { engine: c.id, cloud_vendor: c.id } : { engine: '' };
    }
    const rec = (await api.engineStatus()).find((i) => i.recommended && i.descriptor.kind === 'local');
    return { engine: rec?.descriptor.id ?? all.find((e) => e.kind === 'local')?.id ?? '' };
  }
  async function skip() {
    if (step === 2) await persistCapture();
    skipped = new Set([...skipped, step]);
    await go(step + 1);
  }
  async function finish() {
    await prefs.patch({ onboarding: { done: true, step: TOTAL } });
    router.go(hrefs.home);
    // «Готово» -> the tour is offered (skippable); `tour_done` is set when it is finished or skipped.
    if (!prefs.value.onboarding?.tour_done) tour.offer = true;
  }

  const vaultPath = $derived(prefs.value.vault?.path ?? '~/Documents/Lowk');
  async function pickVault() {
    const p = await api.pickVaultDir();
    if (p) await prefs.patch({ vault: { path: p } });
  }

  const shownPerms = $derived(PERM_ORDER.filter((k) => perms.state[k] !== 'not_applicable'));
  const langOptions: { v: UiLanguage; title: Key | MKey; hint: Key | MKey }[] = [
    { v: 'auto', title: 'settings.language.auto', hint: 'settings.language.autoHint' },
    { v: 'ru', title: 'settings.language.ru', hint: 'm.ob.langRuHint' },
    { v: 'en', title: 'settings.language.en', hint: 'm.ob.langEnHint' },
  ];
  const skippedLabel = (n: number) => (skipped.has(n) ? tm('onboarding.skipped') : null);
</script>

<div class="win">
  <aside class="rail">
    <div class="logo"><Mark size={26} />{tm('app.name')}</div>
    <div class="steps">
      {#each names as n, i (i)}
        <button class="st" class:on={i === step} class:done={i < step} disabled={i > step} onclick={() => go(i)}>
          <span class="d">{#if i < step}<Icon name="check" size={12} />{:else}{i + 1}{/if}</span><span class="lbl">{tm(n)}</span>
        </button>
      {/each}
    </div>
    <p class="fine">{tm('onboarding.welcomeHint')}</p>
  </aside>

  <div class="body">
    <section class="step">
      {#if step < 8}<div class="eyebrow lbl">{tm('onboarding.stepCounter', { n: step + 1, total: TOTAL })} · {tm(names[step]!)}</div>{/if}

      {#if step === 0}
        <h1>{tm('onboarding.welcome')}</h1>
        <p class="lead">{tm('m.ob.twoMinutes')}</p>
        {#each langOptions as o (o.v)}
          <OptionCard selected={lang === o.v} title={tm(o.title)} hint={tm(o.hint)} badge={o.v === 'auto' ? tm('m.auto') : undefined} onselect={() => prefs.patch({ ui: { language: o.v } })} />
        {/each}
        <p class="note">{tm('onboarding.language.note')}</p>

      {:else if step === 1}
        <h1>{tm('onboarding.legal.title')}</h1>
        <p class="lead">{tm('onboarding.legal.body1')}</p>
        <div class="warn"><Icon name="alert" size={22} /><div><p>{tm('onboarding.legal.body2')}</p><p class="sec">{tm('onboarding.legal.body3')}</p></div></div>
        <button class="chk" class:on={ack} role="checkbox" aria-checked={ack} onclick={() => prefs.patch({ onboarding: { recording_notice_ack: !ack } })}><span class="bx">{#if ack}<Icon name="check" size={12} />{/if}</span><span>{tm('onboarding.legal.ack')}</span></button>

      {:else if step === 2}
        <h1>{tm('onboarding.capture.title')}</h1>
        <p class="lead">{tm('onboarding.capture.hint')}</p>
        <OptionCard selected={captureMode === 'app'} title={tm('onboarding.capture.app')} hint={tm('onboarding.capture.appHint')} badge={tm('onboarding.capture.recommended')} onselect={() => { captureMode = 'app'; if (!appId) appId = presetInfo(presetsFor(os)[0]!, os, sources.pool).id; }} />
        <div class="apps"><SourcePicker allowSystem={false} label={tm('m.audioSource')} value={appId ? `app:${appId}` : 'app:'} onchange={(v) => { captureMode = 'app'; appId = v.slice(4); }} /></div>
        <OptionCard selected={captureMode === 'system'} title={tm('m.sourceSystem')} hint={tm('onboarding.capture.systemHint')} onselect={() => (captureMode = 'system')} />
        <p class="note">{tm('onboarding.capture.micNote')}</p>
        <div class="card langs"><b class="lbl">{tm('m.callLangsOnb')}</b><p>{tm('m.callLangsOnbHint')}</p><LangPicker value={callLangsChoice} supported={callLangs.supported} label={tm('m.callLangs')} onchange={(v) => (callLangsChoice = v)} /></div>

      {:else if step === 3}
        <h1>{tm('onboarding.permissions.title')}</h1>
        <p class="lead">{tm('onboarding.permissions.body')}</p>
        {#each shownPerms as k (k)}<PermRow kind={k} withOpen={os === 'macos'} />{/each}
        {#if perms.anyNeedsRestart}<div class="restart" role="status"><span class="lbl">{tm('m.perm.restartNeeded')}</span><Button size="sm" variant="primary" onclick={() => perms.relaunch()}>{tm('m.perm.relaunch')}</Button></div>{/if}
        <div class="all"><Button variant="primary" icon="shield" onclick={() => perms.requestAll()}>{tm('m.ob.allowAll')}</Button></div>
        {#if os === 'windows'}<p class="note">{tm('onboarding.permissions.windowsNote')}</p>{/if}

      {:else if step === 4}
        <h1>{tm('onboarding.hardware.title')}</h1>
        <p class="lead">{tm('onboarding.hardware.body')}</p>
        <div class="scan"><i style:width={scanned ? '100%' : '0'}></i></div>
        <div class="hw">
          <div><small>{tm('m.hw.chip')}</small><b class="lbl">{hw?.chip ?? '…'}</b></div>
          <div><small>{tm('m.hw.ram')}</small><b class="lbl">{hw?.ram_gb ? `${hw.ram_gb} GB` : '…'}</b></div>
          <div><small>{tm('m.hw.disk')}</small><b class="lbl">{hw?.free_disk_gb ? tm('m.hw.free', { n: hw.free_disk_gb }) : '…'}</b></div>
          <div><small>{tm('m.hw.os')}</small><b class="lbl">{hw?.os_version ?? '…'}</b></div>
        </div>
        <div class="two">
          <OptionCard selected={engineMode === 'local'} title={tm('onboarding.hardware.local')} hint={tm('onboarding.hardware.localDesc', { size: `${(localTotalMb / 1000).toFixed(1)} GB` })} badge={tm('onboarding.hardware.localFit')} onselect={() => (engineMode = 'local')} />
          <OptionCard selected={engineMode === 'cloud'} title={tm('onboarding.hardware.cloud')} hint={tm('onboarding.hardware.cloudDesc')} onselect={() => (engineMode = 'cloud')} />
        </div>
        {#if engineMode === 'local' && recId}<EngineCard engine={recId} />{/if}
        <p class="note">{tm('onboarding.hardware.later')}</p>
        <p class="note">{tm('onboarding.hardware.defaultModel', { model: 'DeepSeek V4.1 Flash' })}</p>

      {:else if step === 5}
        <h1>{tm('onboarding.apiKey.title')}</h1>
        <p class="lead">{tm('onboarding.apiKey.body')}</p>
        <KeyCard provider="openrouter" title={tm('settings.apiKey')} placeholder="sk-or-v1-…" />
        <p class="note">{tm('onboarding.apiKey.optional')}</p>
        <div class="card"><b>{tm('onboarding.apiKey.noKey')}</b><p>{tm('onboarding.apiKey.noKeyHint')}</p><Button icon="external" onclick={() => window.open('https://openrouter.ai/keys', '_blank', 'noopener')}>{tm('onboarding.apiKey.openSite')}</Button></div>

        <h2 class="sub">{tm('onboarding.apiKey.sttTitle')}</h2>
        <p class="note">{tm('onboarding.apiKey.sttHint', { vendor: cloudName })}</p>
        <KeyCard provider="stt-cloud" title={tm('m.sttKey', { vendor: cloudName })} placeholder={tm('m.sttKeyPlaceholder')} />

      {:else if step === 6}
        <h1>{tm('onboarding.folder.title')}</h1>
        <p class="lead">{tm('onboarding.folder.body')}</p>
        <div class="card vault" data-testid="onb-vault">
          <code class="path lbl" title={vaultPath}>{vaultPath}</code>
          <Button onclick={pickVault}>{tm('m.change')}</Button>
        </div>
        <p class="note">{tm('onboarding.folder.note')}</p>

      {:else if step === 7}
        <h1>{tm('onboarding.hotkeys.title')}</h1>
        <p class="lead">{tm('onboarding.hotkeys.hint')}</p>
        <BindEditor actions={['ask_brief', 'ask_complement', 'screenshot', 'screenshot_area', 'click_hold', 'toggle_interactive', 'toggle_record', 'move_hold']} showReset={false} />

      {:else}
        <div class="fin">
          <div class="big"><Icon name="check" size={28} /></div>
          <h1>{tm('onboarding.done.title')}</h1>
          <p class="lead centered">{tm('onboarding.done.body', { key: recKey })}</p>
          <div class="sum card">
            <div><span>{tm('settings.language')}</span><span class="lbl">{tm(lang === 'auto' ? 'settings.language.auto' : lang === 'ru' ? 'settings.language.ru' : 'settings.language.en')}</span></div>
            <div><span>{tm('m.ob.capture')}</span><span class="lbl">{skippedLabel(2) ?? (captureMode === 'app' ? (sources.find(appId)?.name ?? nameFromId(appId)) : tm('m.sourceSystemShort'))}</span></div>
            <div><span>{tm('settings.stt')}</span><span class="lbl">{skippedLabel(4) ?? (engineMode === 'local' ? tm('settings.stt.local') : 'Soniox')}</span></div>
            <div><span>{tm('settings.answers')}</span><span class="lbl">{skippedLabel(5) ?? 'DeepSeek V4.1 Flash'}</span></div>
            <div><span>{tm('settings.apiKey')}</span><span class="lbl">{skippedLabel(5) ?? tm('settings.apiKey.stored')}</span></div>
          </div>
        </div>
      {/if}
    </section>

    <div class="nav">
      <Button disabled={step === 0} onclick={() => go(step - 1)}>{tm('common.back')}</Button>
      <span class="sp"></span>
      {#if step >= 2 && step <= 7}<button class="skip" onclick={skip}>{tm('common.skip')}</button>{/if}
      {#if step < 8}<Button variant="primary" disabled={!canNext} onclick={next}>{step === 7 ? tm('common.done') : tm('common.next')}</Button>
      {:else}<Button variant="primary" onclick={finish}>{tm('onboarding.finish')}</Button>{/if}
    </div>
  </div>
</div>

<style>
  .win { width: 100%; height: calc(100vh / var(--z, 1)); background: var(--bg); overflow: hidden; display: grid; grid-template-columns: 256px minmax(0, 1fr); }
  .rail { background: var(--side); border-right: 1px solid var(--line); padding: var(--s8) var(--s4) var(--s6); display: flex; flex-direction: column; min-width: 0; }
  .logo { display: flex; gap: var(--s2); align-items: center; font-weight: 650; font-size: 16px; padding: 0 var(--s1) var(--s6); }
  .steps { flex: 1; }
  .st { all: unset; box-sizing: border-box; width: 100%; display: flex; gap: var(--s3); align-items: center; padding: var(--s2) var(--s2); border-radius: var(--r); color: var(--tx3); font-size: 13px; cursor: pointer; }
  .st:disabled { cursor: default; }
  .st:not(:disabled):hover { color: var(--tx2); }
  .d { width: 20px; height: 20px; border-radius: 50%; border: 1.5px solid var(--line); display: grid; place-items: center; font-size: 11px; font-weight: 600; flex: none; }
  .on { color: var(--tx); font-weight: 500; }
  .on .d { border-color: var(--ac); color: var(--ac); }
  .done { color: var(--tx2); }
  .done .d { background: var(--ac); border-color: var(--ac); color: var(--acfg); }
  .fine { color: var(--tx3); font-size: 11px; line-height: 16px; margin: 0; padding: 0 var(--s1); }
  .body { --gutter: max(var(--s12), calc((100% - 720px) / 2)); display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  .step { flex: 1; padding: var(--s10) var(--gutter) var(--s3); overflow: auto; animation: in 0.3s; }
  @keyframes in { from { opacity: 0; transform: translateY(6px); } }
  .eyebrow { font: 600 var(--fs-xs) var(--font); letter-spacing: 0.1em; text-transform: uppercase; color: var(--ac); margin-bottom: var(--s2); }
  h1 { font-size: 28px; line-height: 36px; letter-spacing: -0.03em; margin: 0 0 var(--s2); font-weight: 650; }
  .lead { color: var(--tx2); margin: 0 0 var(--s6); max-width: 560px; }
  .centered { margin-left: auto; margin-right: auto; }
  .note { font-size: 12px; line-height: 16px; color: var(--tx2); margin: var(--s4) 0 0; }
  .nav { display: flex; align-items: center; gap: var(--s3); padding: var(--s4) var(--gutter); border-top: 1px solid var(--line); }
  .sp { flex: 1; }
  .skip { all: unset; cursor: pointer; color: var(--tx2); font-size: 13px; padding: var(--s2) var(--s2); }
  .skip:hover { color: var(--tx); }
  .warn { display: flex; gap: var(--s4); padding: var(--s4); border-radius: var(--r-lg); background: var(--warnbg); border: 1px solid var(--line); margin-bottom: var(--s4); color: var(--warn); }
  .warn p { margin: 0 0 var(--s2); color: var(--tx); }
  .warn p.sec { margin: 0; color: var(--tx2); font-size: 13px; }
  .chk { all: unset; display: flex; gap: var(--s3); align-items: center; cursor: pointer; margin-top: var(--s2); }
  .bx { width: 20px; height: 20px; border-radius: var(--r-sm); border: 1.5px solid var(--tx3); display: grid; place-items: center; flex: none; color: var(--acfg); }
  .chk.on .bx { background: var(--ac); border-color: var(--ac); }
  h2.sub { font: 600 var(--fs-lg) var(--font); margin: var(--s8) 0 var(--s1); }
  .vault { display: flex; gap: var(--s3); align-items: center; }
  .path { flex: 1; min-width: 0; font: 12px var(--mono); background: var(--bg); border: 1px solid var(--line); border-radius: var(--r); padding: 0 var(--s3); height: 32px; line-height: 30px; }
  .apps { margin: var(--s1) 0 var(--s3); }
  .all { margin-top: var(--s3); }
  .restart { display: flex; align-items: center; justify-content: space-between; gap: var(--s3); padding: var(--s3) var(--s4); border: 1px solid var(--warn); border-radius: var(--r-lg); margin-top: var(--s3); font-size: 13px; }
  .scan { height: 4px; border-radius: 99px; background: var(--line); overflow: hidden; margin-bottom: var(--s4); }
  .scan i { display: block; height: 100%; background: var(--ac); transition: width 1.4s; }
  .hw { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: var(--s2); margin-bottom: var(--s4); }
  .hw div { border: 1px solid var(--line); border-radius: var(--r-lg); padding: var(--s3); background: var(--panel); min-width: 0; }
  .hw small { display: block; color: var(--tx3); font: 600 var(--fs-xs) var(--font); text-transform: uppercase; letter-spacing: 0.06em; margin-bottom: var(--s1); }
  .hw b { font-weight: 550; font-size: 13px; display: block; }
  .two { display: grid; grid-template-columns: 1fr 1fr; gap: var(--s3); }
  .card { border: 1px solid var(--line); background: var(--panel); border-radius: var(--r-lg); padding: var(--s4); margin-top: var(--s4); }
  .card p { margin: var(--s1) 0 var(--s3); color: var(--tx2); font-size: 13px; }
  .fin { text-align: center; padding-top: var(--s10); }
  .big { width: 56px; height: 56px; border: 2px solid var(--tx2); border-radius: var(--r-xl); margin: 0 auto var(--s5); display: grid; place-items: center; color: var(--ac); }
  .sum { display: inline-block; text-align: left; min-width: 360px; max-width: 100%; }
  .sum div { display: flex; justify-content: space-between; gap: var(--s6); padding: var(--s2) 0; border-bottom: 1px solid var(--line); font-size: 13px; min-width: 0; }
  .sum div:last-child { border-bottom: none; }
  .sum div span:first-child { color: var(--tx2); flex: none; }
  @media (max-width: 860px) { .win { grid-template-columns: 1fr; height: auto; } .rail { display: none; } .step { padding: var(--s6) var(--s5); } .hw, .two { grid-template-columns: 1fr 1fr; } .nav { padding: var(--s4) var(--s5); } }
</style>
