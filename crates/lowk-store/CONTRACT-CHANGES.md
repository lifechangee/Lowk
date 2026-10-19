# lowk-store: contract notes and proposals (WP4)

Nothing in `lowk-types` was modified. Proposals for the architect:

1. **`default_templates()` should include `summary`** («Резюме»). `lowk-store` ships it
   (`TemplateSet::defaults()`, id `summary`) and writes it into a fresh `settings.toml`.
2. **`NewCall.clock: Option<SessionClock>`**: `CallEntry::Final` carries monotonic times,
   so the writer needs the session clock. Today `Vault::begin_call` assumes t0 = now;
   `Vault::begin_call_with_clock(NewCall, SessionClock)` is the precise variant that
   `lowk-session` should call.
3. **`Vault` API beyond section 10** (needed by the commands list): `calls_in(&FolderRef)`,
   `trash_items()`, `empty_trash()`, `update_folder`, `folder_doc`, `move_call`,
   `set_call_tags`, `set_call_summary`, `update_call`, `export_vault`, `import_vault`,
   `rescan()` (reconcile, returns `StoreChange`s), `watch(debounce)`,
   `recoverable_calls()` / `recover_call()` / `discard_recovery()`.
   `Vault` is `Clone` (shared `Arc`), `Send + Sync`.
4. `CallWriter::finish()` returns `CallMeta`; `finish_at(rfc3339)` for deterministic callers;
   `take_notice()` reports `ExternalEditMerged`.
5. `TRASH_DIR` is `.lowk/trash` (as in `lowk-types`): `.lowk/trash/<id>/` holds the moved
   files, `.lowk/trash/<id>.trash.json` the sidecar (orig path, deleted_at).
   `empty_trash` is the only hard delete.
6. Calls live in `<Folder>/Calls/`, system folder on disk is `Unsorted/Calls/`.
   Plain directories (e.g. an existing Obsidian vault) are adopted: `_folder.md` is
   created on scan, other files are never touched.
