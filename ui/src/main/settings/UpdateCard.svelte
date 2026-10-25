<script lang="ts">
  // Settings > General > Updates.
  import Button from '../../lib/components/Button.svelte';
  import SettingRow from '../../lib/components/SettingRow.svelte';
  import Toggle from '../../lib/components/Toggle.svelte';
  import { when } from '../fmt';
  import { tm } from '../i18n';
  import { prefs } from '../state.svelte';
  import { updates } from '../updates.svelte';

  const s = $derived(prefs.value);
  const st = $derived(updates.status);
  const last = $derived(st?.last_checked ?? s.updates?.last_checked ?? null);
  const note = $derived.by(() => {
    if (!st) return '';
    if (updates.checking || st.phase === 'checking') return tm('upd.checking');
    if (st.blocked_by_recording) return tm('upd.blockedRec');
    if (st.phase === 'error') return tm(st.error === 'network' ? 'upd.err.network' : st.error === 'signature' ? 'upd.err.signature' : 'upd.err.generic');
    if (st.available) return tm('upd.found', { v: st.available.version });
    return last ? tm('upd.latest') : '';
  });
</script>

<SettingRow label={tm('upd.version')} hint={st?.enabled === false ? tm('upd.unavailable') : undefined}>
  <span data-testid="update-version">{st?.current ?? ''}</span>
</SettingRow>
{#if st?.enabled === false}
  <p class="off" role="status" data-testid="update-unavailable">{tm('upd.unavailable')}</p>
{:else if st}
  <SettingRow label={tm('m.checkUpdates')} hint={tm('m.checkUpdatesHint')}>
    <Toggle checked={s.updates?.check_daily ?? true} label={tm('m.checkUpdates')} onchange={(v) => prefs.patch({ updates: { check_daily: v } })} />
  </SettingRow>
  <SettingRow label={tm('upd.installAuto')} hint={tm('upd.installAutoHint')}>
    <Toggle checked={s.updates?.auto_install ?? false} label={tm('upd.installAuto')} onchange={(v) => prefs.patch({ updates: { auto_install: v } })} />
  </SettingRow>
  <SettingRow label={tm('upd.checkNow')} hint={last ? tm('m.lastChecked', { when: when(last) }) : undefined}>
    {#if note}<span class="note" role="status" data-testid="update-note">{note}</span>{/if}
    <Button disabled={updates.checking || st.phase === 'downloading'} onclick={() => void updates.check()}>{tm('upd.checkNow')}</Button>
  </SettingRow>
{/if}

<style>
  .off { margin: var(--s2) 0 0; color: var(--tx3); font-size: 12px; line-height: 16px; }
  .note { color: var(--tx2); font-size: 12px; line-height: 16px; min-width: 0; overflow-wrap: anywhere; text-align: right; }
</style>
