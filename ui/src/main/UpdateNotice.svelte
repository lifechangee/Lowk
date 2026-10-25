<script lang="ts">
  // Non-intrusive update notice (bottom right of the main window, never the overlay).
  import Button from '../lib/components/Button.svelte';
  import { tm } from './i18n';
  import { session } from './state.svelte';
  import { updates } from './updates.svelte';

  const st = $derived(updates.status);
  const v = $derived(st?.available?.version ?? '');
  const recording = $derived(session.phase !== 'idle');
  const pct = $derived(st?.total ? Math.min(100, Math.round((st.downloaded / st.total) * 100)) : null);
  const show = $derived(!!st?.enabled && (
    st.phase === 'downloading' || st.phase === 'ready' || (st.phase === 'error' && updates.dismissed !== 'err') || (st.phase === 'available' && updates.dismissed !== v)
  ));
  const errKey = $derived(st?.error === 'network' ? 'upd.err.network' : st?.error === 'signature' ? 'upd.err.signature' : 'upd.err.generic');
</script>

{#if show && st}
  <aside class="un" role="status" aria-label={tm('upd.notice')} data-testid="update-notice" data-phase={st.phase}>
    {#if st.phase === 'available'}
      <b class="t lbl">{tm('upd.title', { v })}</b>
      {#if st.available?.notes}<p class="notes" data-testid="update-notes">{st.available.notes}</p>{/if}
      <div class="act">
        <Button variant="primary" size="sm" onclick={() => void updates.install()}>{tm('upd.update')}</Button>
        <Button size="sm" onclick={() => updates.later(v)}>{tm('upd.later')}</Button>
        <Button variant="ghost" size="sm" onclick={() => void updates.skip(v)}>{tm('upd.skip')}</Button>
      </div>
    {:else if st.phase === 'downloading'}
      <b class="t lbl">{pct === null ? tm('upd.downloading', { v }) : tm('upd.downloadingPct', { v, pct })}</b>
      <div class="bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={pct ?? undefined} aria-label={tm('upd.downloading', { v })}><i style:width="{pct ?? 0}%"></i></div>
    {:else if st.phase === 'ready'}
      <b class="t lbl">{tm('upd.ready', { v })}</b>
      {#if st.restart_pending}
        <p class="notes" data-testid="update-waiting">{tm('upd.restartWaiting')}</p>
      {:else}
        <div class="act">
          <Button variant="primary" size="sm" onclick={() => void updates.restart()}>{recording ? tm('upd.restartAfterRec') : tm('upd.restart')}</Button>
          <Button size="sm" onclick={() => updates.later(v)}>{tm('upd.later')}</Button>
        </div>
      {/if}
    {:else}
      <p class="err" role="alert" data-testid="update-error" data-kind={st.error}>{tm(errKey)}</p>
      <div class="act">
        {#if st.error !== 'signature'}<Button size="sm" onclick={() => void updates.check()}>{tm('upd.retry')}</Button>{/if}
        <Button variant="ghost" size="sm" onclick={() => (updates.dismissed = 'err')}>{tm('upd.dismiss')}</Button>
      </div>
    {/if}
  </aside>
{/if}

<style>
  .un { position: fixed; right: var(--s6); bottom: var(--s6); z-index: 55; width: 360px; max-width: calc(100% - 32px); display: flex; flex-direction: column; gap: var(--s2); padding: var(--s3) var(--s4); background: var(--panel); border: 1px solid var(--line-strong); border-radius: var(--r); box-shadow: var(--sh); font-size: 13px; color: var(--tx); }
  .t { display: block; font-weight: 600; line-height: 20px; }
  .notes { margin: 0; color: var(--tx2); font-size: 12px; line-height: 16px; white-space: pre-line; overflow-wrap: anywhere; display: -webkit-box; -webkit-line-clamp: 4; line-clamp: 4; -webkit-box-orient: vertical; overflow: hidden; }
  .err { margin: 0; color: var(--tx); line-height: 18px; overflow-wrap: anywhere; }
  .act { display: flex; flex-wrap: wrap; gap: var(--s2); }
  .bar { height: 8px; border-radius: 99px; background: var(--hv); overflow: hidden; }
  .bar i { display: block; height: 100%; background: var(--ac); }
</style>
