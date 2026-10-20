# M0 owner feedback (2026-10-20) — bug & polish list

Source: owner's hands-on test of `just dev` + screenshots. IDs are referenced by fix agents.

| ID | Area | Problem | Expected |
|---|---|---|---|
| F1 | Onboarding | Renders as a card inside the main window ("window in a window") with large outer margins | Onboarding fills the window edge-to-edge as its own screen (or a dedicated window); no nested frame |
| F2 | Layout | Alignment issues in several places, e.g. header button «●Новый созвон⇧⌘R» has no spacing between dot/text/kbd chip | Consistent spacing on the 4/8 grid; enforced by automated layout checks |
| F3 | Overlay move | During hold-to-move the yellow «Перемещение» toast at the bottom is clipped/unreadable (screenshot) | Move feedback fully visible inside the overlay bounds |
| F4 | Overlay modes | Switching Interactive → Passive makes the overlay disappear and reappear (flicker) | Mode switch never hides/re-shows the window; only changes ignoresMouseEvents/focus; zero blank frames |
| F5 | Adaptive tone | Barely helps; text over busy backgrounds unreadable | Split overlay into regions and adapt each region (gradient scrim), not one global tone |
| F6 | Settings | Text size control does nothing | Changes overlay + main text size live |
| F7 | Binds | ⌘\ (show/hide overlay) vs ⌘⇧H (panic) are confusing | DECIDED: remove panic from default binds (action stays, unbound, assignable in settings); ⌘\ = show/hide overlay |
| F8 | Recording | Only a hotkey stops recording | Stop button in the overlay header; after stop → open main window on library home |
| F9 | Library | Can't move calls out of Unsorted | Drag & drop calls from Unsorted (and anywhere) into folders on the home screen and in the tree |
| F10 | Overlay text | Streaming answer text is DUPLICATED: «Это Это подсказка-заглушка: подсказка-заглушка: подключите подключите…» | Each delta appended once (likely double event subscription or delta+snapshot both applied) |
| F11 | Overlay legibility | At the tested transparency the content is unreadable over Discord | Text/answer area keeps a minimum legibility scrim regardless of opacity |
| F12 | Defaults | Onboarding recommends «Одно приложение» but home screen defaults to «Весь звук системы» | Home default = setting chosen in onboarding |
| F13 | Overlay | «Мой ответ» block/hint row overlap the bottom area; yellow toast bar clipped at the bottom edge | No overlap/clipping |

Works well (keep): stealth is excellent; ⌥-hold magnet move works.
| F14 | «Мой ответ» | Auto-closes on timeouts/System speech (lowk-session) | Closes ONLY on the next Ask by the user; remove auto-close rules |

## Round 2 (owner hands-on, 2026-10-21)

| ID | Area | Problem | Expected |
|---|---|---|---|
| G1 | Settings / text size | Overlay text size setting has no effect; main-window size slider "jumps" (CSS zoom rescales the slider under the cursor while dragging) | Replace slider with preset steps (e.g. S / M / L / XL segmented control with live preview), applied on click; separate controls for main and overlay; both actually work in the real app |
| G2 | Pill | Pill is fixed in place and gets in the way | Pill movable (hold-⌥ magnet like the overlay + drag), position remembered per display |
| G3 | Source picker | Custom source asks for an app identifier — friction, people don't know bundle ids | Picker = list of running apps currently producing audio (icon + name) + presets (Zoom, Google Meet/Chrome, Teams, Discord, Telegram, Slack, FaceTime) + «Другое…» chooses from installed apps; never type ids |
| G4 | Permissions | «System audio» grant button does nothing (also in previous demo) | Pressing it triggers the real macOS prompt (start a short process tap), status updates; if denied, deep link to the right Settings pane with clear instructions |
| G5 | Home | Header «Новый созвон» and card «Начать запись» duplicate; header button only highlights the card | On home: no duplicate (header button hidden or becomes the single primary). Elsewhere: header button opens a compact popover (folder + source + Start) |
| G6 | Overlay interaction | Mode switching by key combo is too slow for changing template | DECIDED: (1) direct binds per template — ⌘↵ = default «Кратко», ⌘⇧↵ = «Дополни мой ответ», any template assignable in settings; (2) hold ⌃ (Ctrl on Windows) = overlay clickable while held (no focus steal), release = click-through again; (3) ⌘⇧I toggle interactive = only for typing text; (4) hold ⌥ = magnet move (unchanged) |
| G7 | Screenshots | Screenshot bind does nothing | Bind captures all monitors into the queue, overlay shows count + thumbnails, queue sent with next Ask (M3 wires LLM; capture+queue now) |
