<script lang="ts">
  // Settings is a sidebar section; its pages are tabs on top (not a second sidebar).
  import Tabs from '../../lib/components/Tabs.svelte';
  import { tm, type MKey } from '../i18n';
  import { hrefs, router, SETTINGS_PAGES, type SettingsPage } from '../router.svelte';
  import AppearancePage from './AppearancePage.svelte';
  import GeneralPage from './GeneralPage.svelte';
  import HotkeysPage from './HotkeysPage.svelte';
  import KnowledgePage from './KnowledgePage.svelte';
  import PermissionsPage from './PermissionsPage.svelte';
  import ProvidersPage from './ProvidersPage.svelte';
  import TemplatesPage from './TemplatesPage.svelte';
  import StoragePage from './StoragePage.svelte';

  let { page }: { page: SettingsPage } = $props();
  const tabs = $derived(SETTINGS_PAGES.map((p) => ({ id: p, label: tm(`m.nav.${p}` as MKey) })));
</script>

<div class="set">
  <div class="cin head"><h1 class="lbl">{tm('settings.title')}</h1></div>
  <Tabs {tabs} value={page} label={tm('settings.title')} onselect={(p) => router.go(hrefs.settings(p))} />
  <main>
    <div class="in">
      {#if page === 'general'}<GeneralPage />
      {:else if page === 'hotkeys'}<HotkeysPage />
      {:else if page === 'templates'}<TemplatesPage />
      {:else if page === 'appearance'}<AppearancePage />
      {:else if page === 'providers'}<ProvidersPage />
      {:else if page === 'knowledge'}<KnowledgePage />
      {:else if page === 'storage'}<StoragePage />
      {:else}<PermissionsPage />{/if}
    </div>
  </main>
</div>

<style>
  .set { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .cin { max-width: 768px; width: 100%; margin: 0 auto; padding: 0 var(--s8); }
  .head { padding-top: var(--s6); padding-bottom: var(--s2); flex: none; }
  h1 { font: 650 24px/32px var(--font); letter-spacing: -0.025em; margin: 0; }
  main { flex: 1; overflow: auto; min-width: 0; min-height: 0; }
  .in { max-width: 768px; margin: 0 auto; padding: var(--s6) var(--s8) 64px; }
</style>
