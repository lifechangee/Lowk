<script lang="ts">
  import { onMount } from 'svelte';
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import TextInput from '../../lib/components/TextInput.svelte';
  import { api, type SecretProvider } from '../api';
  import { tm } from '../i18n';

  let { provider, title, placeholder, note }: { provider: SecretProvider; title: string; placeholder: string; note?: string } = $props();

  let present = $state(false);
  let value = $state('');
  let msg = $state<{ ok: boolean; text: string } | null>(null);
  let busy = $state(false);

  onMount(async () => { present = await api.secretStatus(provider); });

  async function save() {
    if (!value.trim()) return;
    await api.secretSet(provider, value.trim());
    value = '';
    present = true;
    msg = null;
  }
  async function check() {
    busy = true;
    msg = null;
    const r = await api.secretCheck(provider);
    busy = false;
    msg = r.ok ? { ok: true, text: r.balance ? tm('m.keyWorksBalance', { amount: r.balance }) : tm('onboarding.apiKey.ok') } : { ok: false, text: tm('m.keyNotWorking') };
  }
  async function remove() { await api.secretDelete(provider); present = false; msg = null; }
</script>

<div class="card">
  <div class="top"><b class="lbl">{title}</b><span class="pill lbl" class:on={present}>{#if present}<Icon name="check" size={12} />{tm('settings.apiKey.stored')}{:else}{tm('settings.apiKey.missing')}{/if}</span></div>
  <div class="in">
    <TextInput bind:value type="password" mono label={title} placeholder={present ? '••••••••••••' : placeholder} onenter={save} />
    <Button disabled={!value.trim()} onclick={save}>{tm('common.save')}</Button>
  </div>
  <div class="acts">
    <Button size="sm" disabled={!present || busy} onclick={check}>{busy ? tm('common.loading') : tm('onboarding.apiKey.verify')}</Button>
    {#if present}<Button size="sm" variant="danger" onclick={remove}>{tm('m.removeKey')}</Button>{/if}
    {#if msg}<span class="msg lbl" class:ok={msg.ok}>{msg.text}</span>{/if}
  </div>
  <p class="note">{note ?? tm('onboarding.apiKey.storage')}</p>
</div>

<style>
  .card { border: 1px solid var(--line); background: var(--panel); border-radius: var(--r-lg); padding: var(--s4); margin: var(--s3) 0; display: flex; flex-direction: column; gap: var(--s3); }
  .top { display: flex; justify-content: space-between; align-items: center; gap: var(--s3); min-width: 0; }
  .pill { display: inline-flex; align-items: center; gap: var(--s1); font-size: 12px; padding: 0 var(--s2); height: 20px; border-radius: 99px; background: var(--hv); color: var(--tx3); max-width: 60%; }
  .pill.on { background: var(--acbg); color: var(--ac); }
  .in { display: flex; gap: var(--s2); }
  .acts { display: flex; gap: var(--s2); align-items: center; min-width: 0; }
  .msg { font-size: 12px; color: var(--warn); }
  .msg.ok { color: var(--ac); }
  .note { margin: 0; color: var(--tx3); font-size: 12px; line-height: 16px; }
</style>
