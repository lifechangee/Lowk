import type { Settings } from '../../lib/ipc';

export type SummaryRoute =
  | { kind: 'cloud'; model: string; provider: string }
  | { kind: 'local'; model: string }
  | { kind: 'blocked' };

function localUrl(value: string): boolean {
  try {
    const host = new URL(value).hostname.toLowerCase().replace(/^\[|\]$/g, '');
    if (host === 'localhost' || host.endsWith('.localhost') || host.endsWith('.local') || host === '::1') return true;
    const octets = host.split('.').map(Number);
    if (octets.length !== 4 || octets.some((n) => !Number.isInteger(n) || n < 0 || n > 255)) return false;
    return octets[0] === 127 || octets[0] === 10 || (octets[0] === 192 && octets[1] === 168) || (octets[0] === 172 && octets[1]! >= 16 && octets[1]! <= 31);
  } catch { return false; }
}

/** Mirrors route::provider_is_local + resolve for a summary request. */
export function summaryRoute(llm: Settings['llm'] | undefined, allowCloud: boolean, pinnedProvider?: string | null): SummaryRoute {
  const provider = llm?.provider ?? 'openrouter';
  const model = llm?.model ?? '';
  const baseUrl = llm?.base_url ?? '';
  const localModel = (llm?.local_model ?? '').trim();
  const localBase = llm?.local_base_url ?? '';
  const providerLocal = provider === 'ollama' || provider === 'lmstudio' || provider === 'llama_server'
    || (provider === 'custom' && localUrl(baseUrl));
  if (providerLocal) return { kind: 'local', model };
  if (!allowCloud) {
    if (!localModel || !localUrl(localBase)) return { kind: 'blocked' };
    return { kind: 'local', model: localModel };
  }
  return { kind: 'cloud', model: llm?.summary_model ?? model, provider: provider === 'openrouter' ? (pinnedProvider || 'OpenRouter') : 'custom' };
}
