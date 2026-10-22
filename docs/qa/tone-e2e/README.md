# Tone e2e (owner-run live check)

Proves on the real app that the overlay's colours follow the pixels behind each part of it. Agents never run this
(it needs a screen); the earlier gallery was invalid (the overlay was not captured) and has been removed.

## Run

```
just build-qa     # once: qa-feature bundle, same "Lowk Dev" signature and bundle id as `just build`
just qa-tone      # ~2 minutes;  --quick = 2 backdrops only
```

Same identity and bundle id means the Screen Recording grant you already gave Lowk applies: **no new permission
prompt is expected**. The app is started through LaunchServices (`open`), so macOS attributes capture to Lowk itself,
not to the terminal. It uses a throw-away data folder and `LOWK_NO_KEYCHAIN=1`: your keychain and your real settings are
not touched. If a prompt does appear anyway, press Cancel and tell the orchestrator; nothing else needs a grant.

## What you will see on the screen

1. A second Lowk icon appears in the Dock (the qa bundle) and the Lowk overlay shows with demo subtitles for ~6 s.
2. A borderless window covers the main display: the **backdrop**, painted by Lowk itself. It changes about every
   2.5 s through five patterns, laid out relative to the overlay so each one is hit at every position:
   - left half black / right half white
   - quadrants navy | red over yellow | teal
   - top yellow / bottom navy
   - left quarter green / rest light grey
   - right quarter black / rest white
3. The overlay (600x384 pt) sits on top and jumps through **5 positions**: centre, top-left edge, right edge,
   bottom edge, an odd offset (173, 97). It is not clickable and no key is needed.
4. Each ~2.5 s step the overlay's colours re-adapt (left of a black/white split dark, right light, and so on).
   Meanwhile the screen briefly flickers once or twice as the overlay text is hidden for a capture; that is the test
   reading the scrim without text. No sound, no dialogs.
5. At the end the backdrop closes, Lowk quits. Total 5 positions x 5 backdrops = 25 captures.

## Output (this folder)

- `gallery.png`: one row per backdrop, one column per position (the overlay exactly as a person sees it over the backdrop).
- `gallery-<backdrop>.png`: the five positions of one backdrop.
- `results.json`: per capture the overlay frame, the region the sampler grabbed, the 12 cell colours painted, the backend
  grid, glyph contrast; and the list of checks.

Per-cell assertions (each cell lying wholly inside one backdrop region):
1. sampled region == overlay frame (points, global top-left; catches y-flip / display-origin / stale-frame bugs);
2. the backend grid's cell has that region's polarity (black or navy gives a dark scrim, white or yellow a light one);
3. the painted scrim under the cell (text hidden) has the same polarity and keeps the hue (navy/red/teal stay tinted);
4. text pixels have WCAG contrast of at least 3:1 (p10) and 4.5:1 (median) against what is directly behind them.

Exit code is 0 only if every check passes. Failures print as `FAIL name detail`.
