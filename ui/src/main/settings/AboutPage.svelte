<script lang="ts">
  // Settings -> About: version and build, licence, source, third-party licences, privacy, diagnostics export.
  import type { AboutInfo, DiagnosticsSaved } from '../../lib/ipc';
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import Modal from '../../lib/components/Modal.svelte';
  import SettingRow from '../../lib/components/SettingRow.svelte';
  import { api } from '../api';
  import { errText } from '../errors';
  import { tm } from '../i18n';
  import { ui } from '../state.svelte';

  let info = $state<AboutInfo | null>(null);
  $effect(() => { api.appAbout().then((a) => (info = a)).catch(() => {}); });

  let licensesOpen = $state(false);
  let licenses = $state('');
  async function showLicenses() {
    licensesOpen = true;
    if (!licenses) licenses = await api.licensesText().catch((e) => errText(e));
  }
  const openSource = () => info && window.open(info.source_url, '_blank', 'noopener');

  let saving = $state(false);
  let saved = $state<DiagnosticsSaved | null>(null);
  let failure = $state('');
  const size = (b: number) => (b < 1024 * 1024 ? `${Math.max(1, Math.round(b / 1024))} KB` : `${(b / 1024 / 1024).toFixed(1)} MB`);
  async function saveDiagnostics() {
    saving = true; saved = null; failure = '';
    try {
      const dest = await api.pickSavePath(tm('about.diagFile'));
      if (dest) saved = await api.diagnosticsSave(dest);
    } catch (e) { failure = errText(e); }
    saving = false;
  }
</script>

<h2>{tm('about.title')}</h2>
<div data-testid="about-info">
  <SettingRow label={tm('about.version')}><code class="v" data-testid="about-version">{info?.version ?? '…'}</code></SettingRow>
  <SettingRow label={tm('about.build')} hint={tm('about.buildHint')}>
    <code class="v" data-testid="about-build">{info ? `${info.commit} · ${info.build_date}` : '…'}</code>
  </SettingRow>
  <SettingRow stack label={`${tm('about.license')}: ${info?.license ?? 'AGPL-3.0-or-later'}`}>
    <p class="note" data-testid="about-license">{tm('about.licenseNotice')}</p>
  </SettingRow>
  <SettingRow label={tm('about.source')} hint={tm('about.sourceHint')}>
    <Button icon="external" onclick={openSource}>{tm('about.sourceOpen')}</Button>
  </SettingRow>
  <SettingRow label={tm('about.third')} hint={tm('about.thirdHint')}>
    <Button onclick={showLicenses}>{tm('about.thirdShow')}</Button>
  </SettingRow>
</div>

<h2 class="gap">{tm('about.privacy')}</h2>
<div class="priv" data-testid="about-privacy">
  <p>{tm('about.privacyLead')}</p>
  <ul>
    <li>{tm('about.net.models')}</li>
    <li>{tm('about.net.cloud')}</li>
    <li>{tm('about.net.local')}</li>
    <li>{tm('about.net.updates')}</li>
  </ul>
</div>

<h2 class="gap">{tm('about.diag')}</h2>
<div data-testid="about-diagnostics">
  <SettingRow stack label={tm('about.diag')} hint={tm('about.diagHint')}>
    <Button icon="download" disabled={saving} onclick={saveDiagnostics}>{saving ? tm('about.diagSaving') : tm('about.diagSave')}</Button>
  </SettingRow>
  {#if saved}
    <div class="res" role="status" data-testid="diag-result">
      <Icon name="check" size={14} />
      <p>{tm('about.diagSaved', { path: saved.path, size: size(saved.bytes) })}</p>
      <Button icon="external" onclick={() => saved && api.revealPath(saved.path).catch((e) => ui.notify(errText(e)))}>{tm('about.diagReveal')}</Button>
    </div>
  {/if}
  {#if failure}
    <div class="res bad" role="alert" data-testid="diag-error"><Icon name="alert" size={14} /><p>{failure}</p></div>
  {/if}
</div>

<Modal open={licensesOpen} title={tm('about.third')} width={720} closeLabel={tm('about.thirdClose')} onclose={() => (licensesOpen = false)}>
  <pre class="lic" data-testid="about-licenses">{licenses || tm('about.thirdLoading')}</pre>
</Modal>

<style>
  h2 { font: 600 var(--fs-lg) var(--font); margin: 0 0 var(--s2); }
  .gap { margin-top: var(--s8); }
  .v { font: 12px var(--mono); color: var(--tx2); overflow-wrap: anywhere; text-align: right; }
  .note { color: var(--tx2); font-size: 12px; line-height: 18px; margin: 0; overflow-wrap: anywhere; }
  .priv { font-size: 13px; line-height: 20px; color: var(--tx2); }
  .priv p { margin: 0 0 var(--s2); }
  .priv ul { margin: 0; padding-left: var(--s6); display: grid; gap: var(--s2); }
  .priv li { overflow-wrap: anywhere; }
  .res { display: flex; align-items: center; gap: var(--s2); margin-top: var(--s3); padding: var(--s3); border: 1px solid var(--line); border-radius: var(--r); font-size: 12px; }
  .res p { flex: 1; min-width: 0; margin: 0; overflow-wrap: anywhere; }
  .bad { color: var(--err, var(--tx)); }
  .lic { margin: 0; font: 11px/16px var(--mono); white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
