import { describe, expect, it } from 'vitest';
import type { Settings } from '../../lib/ipc';
import { summaryRoute } from './summary-route';

const cloud = {
  provider: 'openrouter', base_url: 'https://openrouter.ai/api/v1', summary_model: 'model-x',
  model: 'ask-model', local_base_url: '', local_model: '',
} as Settings['llm'];

describe('summaryRoute', () => {
  it('names the cloud model and selected provider for cloud folders', () => {
    expect(summaryRoute(cloud, true, 'DeepInfra')).toEqual({ kind: 'cloud', model: 'model-x', provider: 'DeepInfra' });
  });

  it('reports local when the local-only route is configured', () => {
    expect(summaryRoute({ ...cloud, local_base_url: 'http://localhost:11434/v1', local_model: 'qwen' }, false)).toEqual({ kind: 'local', model: 'qwen' });
  });

  it('reports blocked when a local-only folder has no usable local route', () => {
    expect(summaryRoute(cloud, false)).toEqual({ kind: 'blocked' });
    expect(summaryRoute({ ...cloud, local_base_url: 'https://openrouter.ai', local_model: 'qwen' }, false)).toEqual({ kind: 'blocked' });
  });

  it('keeps a configured local provider local even for cloud-enabled folders', () => {
    expect(summaryRoute({ ...cloud, provider: 'ollama', model: 'llama' }, true)).toEqual({ kind: 'local', model: 'llama' });
  });
});
