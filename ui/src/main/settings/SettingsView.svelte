<script lang="ts">
  import Icon, { type IconName } from '../../lib/components/Icon.svelte';
  import { tm, type MKey } from '../i18n';
  import { hrefs, SETTINGS_PAGES, type SettingsPage } from '../router.svelte';
  import AppearancePage from './AppearancePage.svelte';
  import GeneralPage from './GeneralPage.svelte';
  import HotkeysPage from './HotkeysPage.svelte';
  import PermissionsPage from './PermissionsPage.svelte';
  import ProvidersPage from './ProvidersPage.svelte';
  import StoragePage from './StoragePage.svelte';

  let { page }: { page: SettingsPage } = $props();
  const icons: Record<SettingsPage, IconName> = { general: 'settings', hotkeys: 'keyboard', appearance: 'layers', providers: 'key', storage: 'drive', permissions: 'shield' };
</script>

<div class="set">
  <aside>
    <h1 class="lbl">{tm('settings.title')}</h1>
    <nav>
      {#each SETTINGS_PAGES as p (p)}
        <a href={hrefs.settings(p)} class:on={page === p}><Icon name={icons[p]} /><span class="lbl">{tm(`m.nav.${p}` as MKey)}</span></a>
      {/each}
    </nav>
  </aside>
  <main>
    <div class="in">
      {#if page === 'general'}<GeneralPage />
      {:else if page === 'hotkeys'}<HotkeysPage />
      {:else if page === 'appearance'}<AppearancePage />
      {:else if page === 'providers'}<ProvidersPage />
      {:else if page === 'storage'}<StoragePage />
      {:else}<PermissionsPage />{/if}
    </div>
  </main>
</div>

<style>
  .set { display: grid; grid-template-columns: 240px minmax(0, 1fr); height: 100%; min-height: 0; }
  aside { background: var(--side); border-right: 1px solid var(--line); padding: var(--s6) var(--s3); min-width: 0; }
  h1 { font: 650 var(--fs-xl) var(--font); margin: 0 var(--s2) var(--s4); letter-spacing: -0.02em; }
  nav { display: flex; flex-direction: column; gap: var(--s1); }
  nav a { display: flex; align-items: center; gap: var(--s3); height: 32px; padding: 0 var(--s3); border-radius: var(--r); color: var(--tx2); text-decoration: none; font-size: 13px; min-width: 0; }
  nav a:hover { background: var(--hv); color: var(--tx); }
  nav a.on { background: var(--sel); color: var(--tx); font-weight: 500; }
  main { overflow: auto; min-width: 0; }
  .in { max-width: 680px; margin: 0 auto; padding: var(--s8) var(--s8) 64px; }
  @media (max-width: 760px) { .set { grid-template-columns: 1fr; } aside { display: none; } }
</style>
