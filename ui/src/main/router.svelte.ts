// Tiny hash router: #/  #/folder/:id  #/call/:id  #/settings/:page  #/onboarding/:step
export type SettingsPage = 'general' | 'hotkeys' | 'appearance' | 'providers' | 'storage' | 'permissions';
export const SETTINGS_PAGES: SettingsPage[] = ['general', 'hotkeys', 'appearance', 'providers', 'storage', 'permissions'];

export type Route =
  | { name: 'home' }
  | { name: 'folder'; id: string }
  | { name: 'call'; id: string }
  | { name: 'settings'; page: SettingsPage }
  | { name: 'onboarding'; step: number };

export function parse(hash: string): Route {
  const [, a, b] = hash.replace(/^#/, '').split('/');
  if (a === 'folder' && b) return { name: 'folder', id: decodeURIComponent(b) };
  if (a === 'call' && b) return { name: 'call', id: decodeURIComponent(b) };
  if (a === 'settings') return { name: 'settings', page: SETTINGS_PAGES.includes(b as SettingsPage) ? (b as SettingsPage) : 'general' };
  if (a === 'onboarding') return { name: 'onboarding', step: Math.min(7, Math.max(0, Number(b) || 0)) };
  return { name: 'home' };
}

export const hrefs = {
  home: '#/',
  folder: (id: string) => `#/folder/${encodeURIComponent(id)}`,
  call: (id: string) => `#/call/${encodeURIComponent(id)}`,
  settings: (page: SettingsPage = 'general') => `#/settings/${page}`,
  onboarding: (step = 0) => `#/onboarding/${step}`,
};

class Router {
  hash = $state(typeof location === 'undefined' ? '' : location.hash);
  route: Route = $derived(parse(this.hash));
  constructor() {
    if (typeof window !== 'undefined') window.addEventListener('hashchange', () => (this.hash = location.hash));
  }
  go(href: string): void {
    if (location.hash === href || (href === '#/' && location.hash === '')) return;
    location.hash = href;
  }
}
export const router = new Router();
