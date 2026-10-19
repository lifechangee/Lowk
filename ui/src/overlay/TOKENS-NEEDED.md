# Tokens needed by the overlay (for WP10)

The overlay defines its adaptive tokens locally in `overlay.css` (`--bgc --bd --ln --chip --chipbd --hov --tx --tx2 --solid --on --ts --shd --menu --menu-shd --intc --mvc --mvon --recc --toast`),
all `calc()` over the five tone inputs plus `--k` (`--rgb --fg --inv --int --mv --mrgb`), set inline by `varsToStyle()`.

If WP10 wants these in `tokens.css`, move the two blocks from `overlay.css` verbatim. No other global tokens are required;
the overlay only reads `--font` and `--mono` from `tokens.css`.

Notes
- `--mrgb` (menu surface) is not in the `overlay://tone` payload; the UI derives it (`menuRgb`: scrim colour nudged 6% towards `--fg`).
- `settings.overlay.opacity` is treated as the 0..90 transparency slider: `--k = 1 - opacity/100`.
