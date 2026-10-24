# Tone e2e (owner-run live check)

Proves on the real app that the overlay's colours follow the pixels behind each part of it. Agents never run this
(it needs a screen); the earlier gallery was invalid (the overlay was not captured) and has been removed.

## Run

```
just build-qa     # once: qa-feature bundle, same "Lowk Dev" signature and bundle id as `just build`
just qa-tone      # ~3 minutes;  --quick = 2 backdrops only;  --grid = draw the 12x8 grid lines on the gallery
```

Same identity and bundle id means the Screen Recording grant you already gave Lowk applies: **no new permission
prompt is expected**. The app is started through LaunchServices (`open`), so macOS attributes capture to Lowk itself,
not to the terminal. It uses a throw-away data folder and `LOWK_NO_KEYCHAIN=1`: your keychain and your real settings are
not touched. If a prompt does appear anyway, press Cancel and tell the orchestrator; nothing else needs a grant.

## What you will see on the screen

1. A second Lowk icon appears in the Dock (the qa bundle) and the Lowk overlay shows with demo subtitles for ~6 s.
2. Your default browser opens a page (`tools/qa/tone-backdrop.html`, served from 127.0.0.1) that paints the **backdrop**
   in another process, like real use. Press Ctrl+Cmd+F to make it fill the screen (the run waits up to 90 s; a smaller
   window is accepted, cells outside it are then not asserted). `--backdrop=app` (or no browser reply) falls back to a
   borderless Lowk window.  The backdrop It changes about every
   2.5 s through five patterns, laid out relative to the overlay so each one is hit at every position:
   - left half black / right half white
   - quadrants navy | red over yellow | teal
   - top yellow / bottom navy
   - left quarter green / rest light grey
   - right quarter black / rest white
   - quadrants again with the borders in the MIDDLE of cells (43 % / 41 %), to check the majority rule
3. The overlay (600x384 pt) sits on top and jumps through **5 positions**: centre, top-left edge, right edge,
   bottom edge, an odd offset (173, 97). It is not clickable and no key is needed.
4. Each ~2.5 s step the overlay's colours re-adapt (left of a black/white split dark, right light, and so on).
   Meanwhile the screen briefly flickers once or twice as the overlay text is hidden for a capture; that is the test
   reading the scrim without text. No sound, no dialogs.
5. At the end the backdrop closes, Lowk quits. Total 5 positions x 6 backdrops = 30 captures.

## Known failure, fixed: lower rows dimmed / not following

Run 3 showed only row 1 following the backdrop and rows 2-3 dimmed by exactly 0.5625. Cause: the sampler asked
ScreenCaptureKit for 8x the 24x18 output (4:3) for a 600x384 pt (1.56:1) window; SCK keeps the source aspect and
letterboxes with black below, so ~15% of the image was black bars and the grid rows were also squeezed. The request now
has the source's aspect (`geometry::request_px`, unit-tested with a letterboxing SCK model). The harness also waits for
`tone-diag.sampler.frames` to advance twice after each layout ack/move before comparing, lists the mismatching cells
(index, row, expected, got) on failure, and retries a lost browser ack up to 4 times. Not yet re-run live.

## Known failure, fixed: grey/white haze at borders, wide blurry bands, 0.8 s latency

Run 4 (4x3 grid, smooth gradients, median-of-3, 250 ms poll) passed 222/225 cell-centre checks yet looked wrong: the middle row
straddled the navy/yellow border and averaged to light grey (white haze over navy), yellow ended ~70 pt early with a blurry band.
The grid is now 12x8 with majority voting in border cells, hard-ish 8 px transitions, 100 ms polling with a 2-sample
confirmation and a 120 ms fade; the harness gained checks 5 and 6 and the off-grid backdrop above.

## Continuous capture + edge cells (not yet run live)

The sampler now uses one SCStream (`tone-diag.sampler.mode` = `stream`; `frames_per_s`, `processed_per_s` give the CPU load). A sixth position `off-screen-corner` hangs the overlay over the left/bottom display edge when the app allows it; cells are then asserted only on their on-screen part, fully off-screen cells are not asserted.

## Output (this folder)

- `gallery.png`: one row per backdrop, one column per position (the overlay exactly as a person sees it over the backdrop).
- `gallery-<backdrop>.png`: the five positions of one backdrop.
- `results.json` (rewritten after every capture): per capture sampler diagnostics (`diag`: grab path, source rect in points,
  requested/received pixels, own windows excluded/kept, raw mean colour per cell, grab error counters), plus per capture the overlay frame, the region the sampler grabbed, the 96 cell colours painted, the backend
  grid, glyph contrast, `latency` (ms from layout ack to the painted scrim matching, witnesses = cells that changed polarity) and `bands` (every border band measured: side, backdrop colour, mean colour, p02/p98 luminance, ok); top level `latency: {n, p50, max, samples_ms}`; and the list of checks.

Per-cell assertions (each cell lying wholly inside one backdrop region):
1. sampled region == overlay frame (points, global top-left; catches y-flip / display-origin / stale-frame bugs);
2. the backend grid's cell has that region's polarity (black or navy gives a dark scrim, white or yellow a light one);
3. the painted scrim under the cell (text hidden) has the same polarity and keeps the hue (navy/red/teal stay tinted);
4. text pixels have WCAG contrast of at least 3:1 (p10) and 4.5:1 (median) against what is directly behind them.

Strict checks (grid 12x8, sharp transitions, low latency; run live with the owner, not yet run):
5. **Border bands**: for every backdrop border inside the overlay, the painted scrim (text hidden) in bands 8-20 pt on
   BOTH sides of the border must follow its side: a dark backdrop gives a dark scrim with NO light haze (98th-percentile
   luminance <= 0.25 over the whole band), light/bright gives a light scrim (2nd percentile >= 0.45), saturated colours
   keep their hue (navy/red/teal blue/red/blue-green, yellow warm). Bands stop 4 pt short of the ends of a border
   segment so corners are not mixed in. The off-grid backdrop has borders in the middle of cells, so its bands start half a
   cell (+6 pt) from the border instead of 8 pt (the grid can only be right to half a cell there).
6. **Latency**: every backdrop change is timed from the layout acknowledgement of the backdrop page to the first capture in
   which every cell that changed polarity is painted with the new one (dark < 0.2, light > 0.5 luminance, text hidden).
   Each must be <= 250 ms and p50 <= 150 ms (stream budget about 0.1 s, see docs/ARCHITECTURE.md 7.4; the time includes ~50 ms for the
   probe capture itself). p50 and max are printed and stored in `results.json` (`latency`).

Failures name the border and side (`v@300 left (navy): light haze over a dark region ...`).

Exit code is 0 only if every check passes. Failures print as `FAIL name detail`.
