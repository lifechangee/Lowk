// A main window opened (or reopened after being closed) during a call must show that call as running.
import { afterEach, describe, expect, it } from 'vitest';
import { api } from './api';
import { library, session } from './state.svelte';

afterEach(async () => { await api.sessionStop(); });

describe('Session.hydrate', () => {
  it('stays idle when no call is running', async () => {
    await session.hydrate();
    expect(session.phase).toBe('idle');
  });
  it('takes a running call: phase, call id, folder and the real start time', async () => {
    const st = await api.sessionStart({ title: null, folder: null, system: null, mic: null, demo: false, languages: null });
    await library.reload();
    expect(session.phase).toBe('idle'); // this window never saw the start
    await session.hydrate();
    expect(session.phase).toBe('recording');
    expect(session.callId).toBe(st.call);
    const started = Date.parse(library.call(st.call!)!.started);
    expect(session.startedAt).toBe(started);
    expect(session.folder).toBe('sys:unsorted');
  });
});
