# Orchestrator smoke test of the bundled app — M2 (2026-10-22)

Bundle: target/release/bundle/macos/Lowk.app (13:01 build, ad-hoc pinned DR). Silent, computer use.

PASS: sidebar IA (variant A), new folder dialog, folder tabs Созвоны/Документы/Инструкции with helpful empty states; external files dropped into `<folder>/Docs` were adopted and ingested (DOCX, scanned PDF with OCR notice, XLSX, PPTX) → «Готово»; pin star; scanned-PDF viewer readable; search: RU stemming (неустойка → «неустойку»), finds yesterday's call («Петрович»), opens the call; summary tab explains cloud use and missing key with a settings link; tour (step 1/6, 2/6 with example, skip).

Bugs:
| ID | Problem |
|---|---|
| M2-S1 | Search page date fields are pre-filled with today–today and look like an active filter (results from yesterday still appear) — misleading; default should be empty «Любая дата». |
| M2-S2 | Opening a search hit highlights the first transcript line (00:00:00) instead of the matching line (00:00:05). |
| M2-S3 | OCR'd contract: every numbered clause rendered as «1.», sentences broken across list items/lines. |
| M2-S4 | Semantic search off until the embedding model is installed — fine, but home/search should offer a one-click install (currently only a small notice + Settings link). |

Not testable by orchestrator: native file dialog navigation (computer-use key injection under RU layout can't use ⌘⇧G), Ask/summary/tags/sources live (no OpenRouter key in the app keychain — owner must enter it; agents may not enter keys), overlay visuals (stealth hides them from my screenshots).
