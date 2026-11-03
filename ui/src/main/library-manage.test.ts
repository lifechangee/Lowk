// Library management over the mock backend: rename (calls, folders, documents), move a document, documents in the Trash.
import { describe, expect, it } from 'vitest';
import { en, ru } from '../lib/i18n';
import { api } from './api';
import { library, ui } from './state.svelte';

const ACME = '01JC8Z3K6Q9X2M4N7R5T0V0001';
const EMPTY = '01JC8Z3K6Q9X2M4N7R5T0V0005';
const CALL1 = '01JC8Z3K6Q9X2M4N7R5T0V0101';

describe('G1 rename', () => {
  it('renames a call and the list follows', async () => {
    await library.reload();
    expect(await library.rename('call', CALL1, '  Новое имя  ')).toBe(true);
    expect(library.call(CALL1)?.title).toBe('Новое имя');
  });
  it('refuses an empty name with a message and keeps the old one', async () => {
    await library.reload();
    const before = library.call(CALL1)!.title;
    ui.toast = null;
    expect(await library.rename('call', CALL1, '   ')).toBe(false);
    expect(library.call(CALL1)?.title).toBe(before);
    expect([ru['m.rename.empty'], en['m.rename.empty']]).toContain(ui.toast);
  });
  it('allows duplicate names', async () => {
    await library.reload();
    const other = library.calls.find((c) => c.id !== CALL1 && !c.deleted)!;
    await library.rename('call', CALL1, 'Дубль');
    await library.rename('call', other.id, 'Дубль');
    expect(library.calls.filter((c) => c.title === 'Дубль')).toHaveLength(2);
  });
  it('renames a folder (title only reaches folderUpdate)', async () => {
    await library.reload();
    expect(await library.rename('folder', ACME, ' Клиенты ')).toBe(true);
    expect(library.folder(ACME)?.title).toBe('Клиенты');
    expect(await library.rename('folder', ACME, '')).toBe(false);
    expect(library.folder(ACME)?.title).toBe('Клиенты');
  });
  it('renames a document without touching the file name', async () => {
    const [d] = await api.docList(ACME);
    const r = await api.docPatch(d!.id, { title: ' Регламент ' });
    expect(r.title).toBe('Регламент');
    expect(r.file_name).toBe(d!.file_name);
    await expect(api.docPatch(d!.id, { title: '  ' })).rejects.toBeTruthy();
    expect((await api.docGet(d!.id))?.title).toBe('Регламент');
  });
  it('has the strings in both languages', () => {
    for (const k of ['m.rename', 'm.rename.label', 'm.rename.hint', 'm.rename.empty', 'm.rename.folderRecording', 'm.trashDoc', 'm.movedToFolder'] as const) {
      expect(ru[k], k).toBeTruthy();
      expect(en[k], k).toBeTruthy();
    }
  });
});

describe('G8 move a document', () => {
  it('moves between folders and to Unsorted', async () => {
    const [d] = await api.docList(ACME);
    await api.docPatch(d!.id, { folder: { kind: 'user', id: EMPTY } });
    expect((await api.docList(ACME)).some((x) => x.id === d!.id)).toBe(false);
    expect((await api.docList(EMPTY)).some((x) => x.id === d!.id)).toBe(true);
    await api.docPatch(d!.id, { folder: { kind: 'unsorted' } });
    expect((await api.docList('sys:unsorted')).some((x) => x.id === d!.id)).toBe(true);
  });
  it('opens the move menu for a document with its current folder', () => {
    ui.openMoveDoc('d1', ACME, 10, 20);
    expect(ui.moveMenu).toEqual({ id: 'd1', x: 10, y: 20, doc: { folder: ACME } });
    ui.moveMenu = null;
  });
});

describe('G7 documents in the Trash', () => {
  it('a deleted document is listed in the Trash and counted; restore brings it back', async () => {
    const [d] = await api.docList(ACME);
    await library.reload();
    const before = library.trashCount;
    await api.docDelete(d!.id);
    await library.reload();
    expect(library.trashedDocs.map((x) => x.id)).toContain(d!.id);
    expect(library.trashCount).toBe(before + 1);
    expect((await api.docList(ACME)).some((x) => x.id === d!.id)).toBe(false);
    await library.restoreDoc(d!.id);
    expect(library.trashedDocs.some((x) => x.id === d!.id)).toBe(false);
    expect(library.trashCount).toBe(before);
    expect((await api.docList(ACME)).some((x) => x.id === d!.id)).toBe(true);
  });
  it('documents of a trashed folder are not listed on their own', async () => {
    await library.reload();
    const docs = await api.docList(ACME);
    await api.folderDelete(ACME);
    await library.reload();
    for (const d of docs) expect(library.trashedDocs.some((x) => x.id === d.id)).toBe(false);
    await api.folderRestore(ACME);
  });
});
