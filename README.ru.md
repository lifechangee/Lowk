<p align="center"><picture><source media="(prefers-color-scheme: dark)" srcset="docs/design/logo/final/lowk-lockup-dark.svg"><img src="docs/design/logo/final/lowk-lockup-light.svg" alt="Lowk" width="300"></picture></p>

# Lowk

Lowk это open-source (AGPL-3.0-or-later) ассистент для созвонов под macOS и Windows: транскрипция речи в реальном времени (микрофон и звук выбранного приложения), локальное хранилище созвонов и документов в Markdown, ответы нейросети по клавише в стелс-оверлее, скрытом от многих способов демонстрации экрана. Без телеметрии; единственный автоматический сетевой запрос это проверка обновлений (отключается).

English version: [README.md](README.md)

## Статус

**Pre-alpha (веха M0, каркас).** Пока ничего нельзя использовать. Код под Windows написан и проверен только компиляцией, на реальном железе не запускался.

Документы: [docs/BRIEF.md](docs/BRIEF.md), [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), [docs/ROADMAP.md](docs/ROADMAP.md).

## Сборка

Нужны: Rust (версия закреплена в `rust-toolchain.toml`), Node 22 + pnpm 10, [`just`](https://github.com/casey/just), Tauri CLI (`cargo install tauri-cli --version "^2" --locked`), `cargo-deny`. На macOS нужны Xcode command line tools; на Windows MSVC build tools и WebView2.

```sh
just dev        # Tauri + Vite с горячей перезагрузкой
just build      # production-сборка
just bindings   # пересоздать ui/src/lib/ipc/bindings.ts из Rust-типов
just ci         # fmt, clippy, правила зависимостей, лицензии, тесты, проверки UI
```

Структура: `crates/` (13 Rust-крейтов, внизу `lowk-types`), `ui/` (Svelte 5 + Vite), `tools/xtask`, `docs/`. См. раздел 1 архитектуры.

## Лицензия

AGPL-3.0-or-later. См. [LICENSE](LICENSE) и [NOTICE](NOTICE). Вклад принимается с DCO sign-off.
