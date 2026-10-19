// Default bind labels (docs/BRIEF.md round 3). Display only; the real binds come from settings later.
export interface KeyLabels {
  interactive: string;
  ask: string;
  shot: string;
  move: string;
}

export function keyLabels(isMac: boolean): KeyLabels {
  return isMac
    ? { interactive: '⌘⇧I', ask: '⌘↵', shot: '⌘⇧S', move: '⌥' }
    : { interactive: 'Ctrl+Shift+I', ask: 'Ctrl+Enter', shot: 'Ctrl+Shift+S', move: 'Alt' };
}
