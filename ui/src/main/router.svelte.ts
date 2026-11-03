// Tiny hash router:
//   #/  #/folder/:id[/calls|documents|instructions]  #/call/:id[?t=SEC]  #/doc/:id[?p=PAGE]
//   #/search?q=..&folder=..&from=..&to=..&kind=..  #/settings/:page  #/onboarding/:step
export type SettingsPage = 'general' | 'hotkeys' | 'templates' | 'appearance' | 'providers' | 'network' | 'knowledge' | 'storage' | 'permissions' | 'about';
export const SETTINGS_PAGES: SettingsPage[] = ['general', 'hotkeys', 'templates', 'appearance', 'providers', 'network', 'knowledge', 'storage', 'permissions', 'about'];

export type FolderTab = 'calls' | 'documents' | 'instructions';
export const FOLDER_TABS: FolderTab[] = ['calls', 'documents', 'instructions'];
export type SearchKind = 'call' | 'doc' | 'image';
export const SEARCH_KINDS: SearchKind[] = ['call', 'doc', 'image'];

/** The full results page: its query and filters live in the URL so that back/forward and links work. */
export type SearchParams = { q: string; folder: string; from: string; to: string; kind: SearchKind | '' };
export const NO_SEARCH: SearchParams = { q: '', folder: '', from: '', to: '', kind: '' };

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
    return { name: 'search', q: query.get('q') ?? '', folder: query.get('folder') ?? '', from: query.get('from') ?? '', to: query.get('to') ?? '', kind: kind && SEARCH_KINDS.includes(kind) ? kind : '' };
  }
  if (a === 'settings') return { name: 'settings', page: SETTINGS_PAGES.includes(b as SettingsPage) ? (b as SettingsPage) : 'general' };
  if (a === 'onboarding') return { name: 'onboarding', step: Math.min(8, Math.max(0, Number(b) || 0)) };
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

/** A page with unsaved work registers one: leaving it (by any link, the palette, back/forward) asks first. */
export type NavGuard = { doc: string; dirty: () => boolean };

/** Would going to `nextHash` leave the guarded page with unsaved work? (`?p=` on the same document is not leaving.) */
export function blocksNav(guard: NavGuard | null, nextHash: string): boolean {
  if (!guard || !guard.dirty()) return false;
  const next = parse(nextHash);
  return !(next.name === 'doc' && next.id === guard.doc);
}

class Router {
  hash = $state(typeof location === 'undefined' ? '' : location.hash);
  route: Route = $derived(parse(this.hash));
  #guard: NavGuard | null = null;
  /** Where the user wanted to go while the guard held them back; the page shows Save / Discard / Stay for it. */
  pending = $state<string | null>(null);
  constructor() {
    if (typeof window !== 'undefined') {
      window.addEventListener('hashchange', () => {
        const next = location.hash;
        if (blocksNav(this.#guard, next)) {
          // the page stays (route unchanged) while the question is open; `stay` puts the address back
          this.pending = next;
          return;
        }
        this.hash = next;
      });
    }
  }
  /** Register a guard; returns its remover. */
  guard(g: NavGuard): () => void {
    this.#guard = g;
    return () => { if (this.#guard === g) { this.#guard = null; this.pending = null; } };
  }
  go(href: string): void {
    if (location.hash === href || (href === '#/' && location.hash === '')) return;
    if (blocksNav(this.#guard, href)) { this.pending = href; return; }
    location.hash = href;
  }
  /** The user chose to leave (after saving or discarding): follow the held-back navigation. */
  proceed(): void {
    const href = this.pending;
    this.pending = null;
    this.#guard = null;
    if (!href) return;
    if (location.hash === href) this.hash = href; // back/forward or a link: the address already moved
    else location.hash = href;
  }
  /** The user chose to stay. */
  stay(): void {
    this.pending = null;
    if (location.hash !== this.hash && !(this.hash === '' && location.hash === '#/')) history.replaceState(null, '', this.hash || '#/');
  }
  /** Same page, new address (filters, query): no history entry per keystroke. */
  replace(href: string): void {
    if (location.hash === href) return;
    history.replaceState(null, '', href);
    this.hash = href;
  }
}
export const router = new Router();
