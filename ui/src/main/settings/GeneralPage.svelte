<script lang="ts">
  import type { CaptureMonitors, UiLanguage } from '../../lib/ipc';
  import Button from '../../lib/components/Button.svelte';
  import SettingRow from '../../lib/components/SettingRow.svelte';
  import Select from '../../lib/components/Select.svelte';
  import Toggle from '../../lib/components/Toggle.svelte';
  import { when } from '../fmt';
  import { tm } from '../i18n';
  import { hrefs, router } from '../router.svelte';
  import { callLangs, prefs, sources } from '../state.svelte';
  import { tour } from '../tour/tour.svelte';
  import LangPicker from '../LangPicker.svelte';
  import { resolveSource } from '../source';
  import SourcePicker from '../SourcePicker.svelte';

  const s = $derived(prefs.value);
  const src = $derived(s.audio?.default_source);
  const srcValue = $derived(resolveSource(src, sources.running, s.capture?.default_source));
</script>

<h2>{tm('m.set.general')}</h2>
<SettingRow label={tm('settings.language')}>
  <Select label={tm('settings.language')} value={s.ui?.language ?? 'auto'} options={[{ value: 'auto', label: tm('settings.language.auto') }, { value: 'ru', label: tm('settings.language.ru') }, { value: 'en', label: tm('settings.language.en') }]} onchange={(v) => prefs.patch({ ui: { language: v as UiLanguage } })} />
</SettingRow>
<SettingRow stack label={tm('m.callLangs')} hint={`${tm('m.callLangsHint')} ${tm('m.callLangsAll')}`}>
  <LangPicker value={callLangs.default} supported={callLangs.supported} label={tm('m.callLangs')} onchange={(v) => prefs.patch({ stt: { languages: v, last_languages: [] } })} />
</SettingRow>
<SettingRow label={tm('m.defaultSource')} hint={tm('m.defaultSourceHint')}>
  <div class="srcw"><SourcePicker size="md" label={tm('m.defaultSource')} value={srcValue} onchange={(v) => prefs.patch({ capture: { default_source: v.startsWith('app:') ? 'one_app' : 'whole_system' }, audio: { default_source: v.startsWith('app:') ? { kind: 'app', id: v.slice(4) } : { kind: 'whole_system' } } })} /></div>
</SettingRow>
<SettingRow label={tm('settings.keepAudio')} hint={tm('m.keepAudioHint')}>
  <Toggle checked={s.audio?.keep_audio ?? false} label={tm('settings.keepAudio')} onchange={(v) => prefs.patch({ audio: { keep_audio: v } })} />
</SettingRow>
<SettingRow label={tm('m.captureMonitors')} hint={tm('m.captureMonitorsHint')}>
  <Select label={tm('m.captureMonitors')} value={s.capture?.monitors ?? 'all'} options={[{ value: 'all', label: tm('m.mon.all') }, { value: 'primary', label: tm('m.mon.primary') }, { value: 'active', label: tm('m.mon.active') }]} onchange={(v) => prefs.patch({ capture: { monitors: v as CaptureMonitors } })} />
</SettingRow>
<SettingRow label={tm('m.hideMain')} hint={tm('m.hideMainHint')}>
  <Toggle checked={s.stealth?.hide_main ?? false} label={tm('m.hideMain')} onchange={(v) => prefs.patch({ stealth: { hide_main: v } })} />
</SettingRow>

<h2 class="gap">{tm('m.set.updates')}</h2>
<SettingRow label={tm('m.checkUpdates')} hint={s.updates?.last_checked ? tm('m.lastChecked', { when: when(s.updates.last_checked) }) : tm('m.checkUpdatesHint')}>
  <Toggle checked={s.updates?.check_daily ?? true} label={tm('m.checkUpdates')} onchange={(v) => prefs.patch({ updates: { check_daily: v } })} />
</SettingRow>

<h2 class="gap">{tm('m.set.setup')}</h2>
<SettingRow label={tm('m.rerunOnboarding')} hint={tm('m.rerunOnboardingHint')}>
  <Button onclick={() => router.go(hrefs.onboarding(0))}>{tm('common.start')}</Button>
</SettingRow>
<SettingRow label={tm('tour.settings.label')} hint={tm('tour.settings.hint')}>
  <Button icon="play" onclick={() => { router.go(hrefs.home); tour.start(); }}>{tm('tour.settings.show')}</Button>
</SettingRow>

<style>
  h2 { font: 600 var(--fs-lg) var(--font); margin: 0 0 var(--s2); }
  .gap { margin-top: var(--s8); }
  .srcw { width: 280px; max-width: 100%; }
</style>
