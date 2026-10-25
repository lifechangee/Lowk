<script lang="ts">
  // Settings -> Network: the system proxy that Lowk found (read-only), a manual override and a connection check.
  import type { NetworkCheck, NetworkStatus } from '../../lib/ipc';
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import SettingRow from '../../lib/components/SettingRow.svelte';
  import TextInput from '../../lib/components/TextInput.svelte';
  import { api } from '../api';
  import { errText } from '../errors';
  import { tm } from '../i18n';
  import { prefs } from '../state.svelte';

  let status = $state<NetworkStatus | null>(null);
  $effect(() => { api.networkStatus().then((s) => (status = s)).catch(() => {}); });

  let draft = $state('');
  let seeded = false;
  $effect(() => {
    if (prefs.loaded && !seeded) { seeded = true; draft = prefs.value.network?.proxy ?? ''; }
  });
  const valid = $derived(/^\s*$|^\s*((https?|socks5h?|socks):\/\/)?(\[[0-9a-f:]+\]|[^\s/:@]+|[^\s/@]+@[^\s/:@]+)(:\d{1,5})?\s*$/i.test(draft) && !/^\s*(ftp|socks4):/i.test(draft));
  const saved = $derived((prefs.value.network?.proxy ?? '') === draft.trim());

  let saveError = $state('');
  async function save() {
    saveError = '';
    try {
      await prefs.patch({ network: { proxy: draft.trim() } });
      const got = prefs.value.network?.proxy ?? '';
      if (got !== draft.trim()) saveError = tm('net.bad');
    } catch (e) { saveError = errText(e); }
  }

  let checking = $state(false);
  let result = $state<NetworkCheck | null>(null);
  let failure = $state('');
  async function check() {
    checking = true; result = null; failure = '';
    try { result = await api.networkCheck(draft.trim()); } catch (e) { failure = errText(e); }
    checking = false;
  }
  const via = (c: NetworkCheck) => (c.via ? tm('net.via.proxy', { url: c.via }) : tm('net.via.direct'));
  const sourceName = (s: NetworkStatus) => tm(`net.src.${s.system_source ?? 'env'}` as const);
</script>

<h2>{tm('net.title')}</h2>
<SettingRow stack label={tm('net.system')} hint={tm('net.systemHint')}>
  <code class="v" data-testid="net-system">{status?.system_proxy ? tm('net.system.value', { url: status.system_proxy, source: sourceName(status) }) : status ? tm('net.system.none') : '…'}</code>
</SettingRow>
<SettingRow stack label={tm('net.manual')} hint={tm('net.manualHint')}>
  <div class="col">
  <div class="field"><TextInput bind:value={draft} label={tm('net.manual')} placeholder={tm('net.manualPlaceholder')} mono onenter={() => valid && !saved && save()} /></div>
  {#if !valid}<p class="warn" role="alert" data-testid="net-bad">{tm('net.bad')}</p>{/if}
  <div class="btns">
    <Button disabled={!valid || saved} onclick={save}>{saved && draft.trim() ? tm('net.saved') : tm('net.save')}</Button>
    <Button icon="refresh" disabled={!valid || checking} onclick={check}>{checking ? tm('net.checking') : tm('net.check')}</Button>
  </div>
  {#if saveError}<p class="warn" role="alert" data-testid="net-save-error">{saveError}</p>{/if}
  {#if result}
    <div class="res" class:bad={!result.ok} role="status" data-testid="net-result">
      <Icon name={result.ok ? 'check' : 'alert'} size={14} />
      <p>{result.ok ? tm('net.ok', { status: String(result.status ?? ''), ms: String(result.ms), via: via(result) }) : tm('net.fail', { via: via(result), error: result.error ?? '' })}{#if result.status === 403} {tm('net.blocked')}{/if}</p>
    </div>
  {/if}
  {#if failure}<div class="res bad" role="alert" data-testid="net-error"><Icon name="alert" size={14} /><p>{failure}</p></div>{/if}
  </div>
</SettingRow>

<style>
  h2 { font: 600 var(--fs-lg) var(--font); margin: 0 0 var(--s2); }
  .v { font: 12px var(--mono); color: var(--tx2); overflow-wrap: anywhere; }
  .col { display: flex; flex-direction: column; gap: var(--s2); width: 100%; min-width: 0; }
  .field { width: 100%; }
  .btns { display: flex; flex-wrap: wrap; gap: var(--s2); }
  .warn { margin: 0; font-size: 12px; color: var(--err, var(--tx)); overflow-wrap: anywhere; }
  .res { display: flex; align-items: center; gap: var(--s2); padding: var(--s3); border: 1px solid var(--line); border-radius: var(--r); font-size: 12px; width: 100%; box-sizing: border-box; }
  .res p { flex: 1; min-width: 0; margin: 0; overflow-wrap: anywhere; }
  .bad { color: var(--err, var(--tx)); }
</style>
