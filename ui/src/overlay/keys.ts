// Default bind labels (docs/BRIEF.md round 3). Display only; the real binds come from settings later.
export interface KeyLabels {
  interactive: string;
  ask: string;
  /** G6: «Дополни мой ответ» bind. */
  complement: string;
  shot: string;
  move: string;
  /** G6: hold = overlay clickable without focus. */
  click: string;
}

export function keyLabels(isMac: boolean): KeyLabels {
  return isMac
    ? { interactive: '⌘⇧I', ask: '⌘↵', complement: '⌘⇧↵', shot: '⌘⇧S', move: '⌥', click: '⌃' }
    : { interactive: 'Ctrl+Shift+I', ask: 'Ctrl+Enter', complement: 'Ctrl+Shift+Enter', shot: 'Ctrl+Shift+S', move: 'Alt', click: 'Ctrl' };
}
