// Tiny hash router:
//   #/  #/folder/:id[/calls|documents|instructions]  #/call/:id[?t=SEC]  #/doc/:id[?p=PAGE]
//   #/search?q=..&folder=..&tag=..&from=..&to=..&kind=..  #/settings/:page  #/onboarding/:step
export type SettingsPage = 'general' | 'hotkeys' | 'templates' | 'appearance' | 'providers' | 'knowledge' | 'storage' | 'permissions';
export const SETTINGS_PAGES: SettingsPage[] = ['general', 'hotkeys', 'templates', 'appearance', 'providers', 'knowledge', 'storage', 'permissions'];

export type FolderTab = 'calls' | 'documents' | 'instructions';
export const FOLDER_TABS: FolderTab[] = ['calls', 'documents', 'instructions'];
export type SearchKind = 'call' | 'doc' | 'image';
export const SEARCH_KINDS: SearchKind[] = ['call', 'doc', 'image'];

/** The full results page: its query and filters live in the URL so that back/forward and links work. */
export type SearchParams = { q: string; folder: string; tag: string; from: string; to: string; kind: SearchKind | '' };
export const NO_SEARCH: SearchParams = { q: '', folder: '', tag: '', from: '', to: '', kind: '' };

export type Route =
  | { name: 'home' }
  | { name: 'folder'; id: string; tab: FolderTab }
  | { name: 'call'; id: string; at: number | null; match?: string }
  | { name: 'doc'; id: string; page: number | null }
  | ({ name: 'search' } & SearchParams)
  | { name: 'settings'; page: SettingsPage }
  | { name: 'onboarding'; step: number };

const num = (s: string | null): number | null => {
  if (s === null || s === '') return null;
  const n = Number(s);
  return Number.isFinite(n) && n >= 0 ? n : null;
};

export function parse(hash: string): Route {
  const raw = hash.replace(/^#/, '');
  const qi = raw.indexOf('?');
  const path = qi < 0 ? raw : raw.slice(0, qi);
  const query = new URLSearchParams(qi < 0 ? '' : raw.slice(qi + 1));
  const [, a, b, c] = path.split('/');
  if (a === 'folder' && b) return { name: 'folder', id: decodeURIComponent(b), tab: FOLDER_TABS.includes(c as FolderTab) ? (c as FolderTab) : 'calls' };
  if (a === 'call' && b) return { name: 'call', id: decodeURIComponent(b), at: num(query.get('t')), ...(query.get('m') ? { match: query.get('m')! } : {}) };
  if (a === 'doc' && b) return { name: 'doc', id: decodeURIComponent(b), page: num(query.get('p')) };
  if (a === 'search') {
    const kind = query.get('kind') as SearchKind | null;
    return { name: 'search', q: query.get('q') ?? '', folder: query.get('folder') ?? '', tag: query.get('tag') ?? '', from: query.get('from') ?? '', to: query.get('to') ?? '', kind: kind && SEARCH_KINDS.includes(kind) ? kind : '' };
  }
  if (a === 'settings') return { name: 'settings', page: SETTINGS_PAGES.includes(b as SettingsPage) ? (b as SettingsPage) : 'general' };
  if (a === 'onboarding') return { name: 'onboarding', step: Math.min(7, Math.max(0, Number(b) || 0)) };
  return { name: 'home' };
}

const withQuery = (path: string, q: Record<string, string | number | null | undefined>): string => {
  const sp = new URLSearchParams();
  for (const [k, v] of Object.entries(q)) if (v !== null && v !== undefined && v !== '') sp.set(k, String(v));
  const s = sp.toString();
  return s ? `${path}?${s}` : path;
};

export const hrefs = {
  home: '#/',
  folder: (id: string, tab: FolderTab = 'calls') => `#/folder/${encodeURIComponent(id)}${tab === 'calls' ? '' : '/' + tab}`,
  call: (id: string, at: number | null = null, match: string | null = null) => withQuery(`#/call/${encodeURIComponent(id)}`, { t: at === null ? null : Math.floor(at), m: match }),
  doc: (id: string, page: number | null = null) => withQuery(`#/doc/${encodeURIComponent(id)}`, { p: page }),
  search: (p: Partial<SearchParams> = {}) => withQuery('#/search', { ...NO_SEARCH, ...p }),
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
  /** Same page, new address (filters, query): no history entry per keystroke. */
  replace(href: string): void {
    if (location.hash === href) return;
    history.replaceState(null, '', href);
    this.hash = href;
  }
}
export const router = new Router();
