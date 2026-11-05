// Default bind labels. Display only; the real binds come from settings.
export interface KeyLabels {
  mark: string;
  recap: string;
  interactive: string;
  ask: string;
  /** G6: «Дополни мой ответ» bind. */
  complement: string;
  shot: string;
  move: string;
  /** G6: hold = overlay clickable without focus. */
  click: string;
  /** Start / stop recording bind (`toggle_record`). */
  record: string;
}

export function keyLabels(isMac: boolean): KeyLabels {
  return isMac
    ? { mark: '⌘⇧B', recap: '', interactive: '⌘⇧I', ask: '⌘↵', complement: '⌘⇧↵', shot: '⌘⇧S', move: '⌥', click: '⌃', record: '⌘⇧R' }
    : { mark: 'Ctrl+Shift+B', recap: '', interactive: 'Ctrl+Shift+I', ask: 'Ctrl+Enter', complement: 'Ctrl+Shift+Enter', shot: 'Ctrl+Shift+S', move: 'Alt', click: 'Ctrl', record: 'Ctrl+Shift+R' };
}
