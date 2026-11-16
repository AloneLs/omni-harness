# OmniHarness — постоянная память проекта

## Что это
Windows 11 AI-agent harness (десктоп-приложение). Frontend: Svelte 5 + Vite 6 + Tailwind 4 (TypeScript).
Backend: Rust + Tauri 2.2 (webview runtime — WebView2). SQLite через `rusqlite` (bundled).
Корневой путь: `C:\SyntaxSolutions\omni-harness`
Идентификатор бандла: `com.omniharness.desktop`, productName `OmniHarness`, version `1.0.0`.

## Структура
- `src/` — Svelte-фронтенд (`components/`, `lib/`, `stores/`).
- `src-tauri/src/` — Rust: `lib.rs` (Tauri commands), `engine/` (context_engine, агентский цикл), `db/` (`mod.rs`, `memory.rs`).
- `src-tauri/tauri.conf.json` — конфиг окна/CSP/assetProtocol. `bundle.active=false`, `targets=[]` → installer не собирается, только raw exe.
- `src-tauri/icons/icon.png` — логотип приложения (PNG, 174 KB).
- `dist/` — собранный фронтенд (вшивается в exe через `frontendDist`).

## Команда сборки (Windows PowerShell)
```powershell
cd C:\SyntaxSolutions\omni-harness
$env:PATH = [Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [Environment]::GetEnvironmentVariable("Path","User") + ";C:\SyntaxSolutions\omni-harness\node_modules\.bin;C:\Users\wasre\.cargo\bin;C:\Users\wasre\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin"
npm run check      # svelte-check (типы; допускаются warnings, ошибок быть не должно)
npx tauri build    # vite build + cargo release build
```
Отдельные шаги: `npm run build` (только фронт), `npm run dev` (vite на :1420).

## Артефакты релизной сборки
- `src-tauri\target\release\omni-harness.exe` (~23.2 МБ) — приложение.
- `src-tauri\target\release\WebView2Loader.dll` (~161 КБ) — ОБЯЗАТЕЛЕН рядом с exe.
  Без него exe не стартует (delay-load WebView2Loader). Раздавать только парой exe + dll.
- Профиль release: `lto="thin"`, `codegen-units=1`, `strip="symbols"`, `opt-level=3` → линковка долгая (5–8 мин), не считать зависанием.

## Известные особенности
- `svelte-check` даёт 3 предупреждения в `src/components/TimelineBlock.svelte:41` (`state_referenced_locally` для `isStreaming/error/paused`) — не ошибки.
- Vite предупреждает о чанке >500 kB (`index-*.js`) и о dynamic+static импорте `@tauri-apps/plugin-dialog` — не блокеры.
- Память проекта читается харнессом из `AGENTS.md` / `CLAUDE.md` / `.omniharness/rules.md` в корне workspace (`engine/context_engine.rs::read_project_memory_prefix`). Долговременные записи — таблица `memory_entries` в `C:\Users\wasre\AppData\Local\OmniHarness\omni_harness.db`.
- Данные приложения: `C:\Users\wasre\AppData\Local\OmniHarness\` (`omni_harness.db`, `providers_snapshot.json`, `workspaces_snapshot.json`, `telegram_config.json`, `models/`).

## Распространение сборки
Telegram-бот `@impact_hermes_bot` (id 8689899535), целевой получатель chat_id `1417914108` (`@was_record`).
Отправка: `curl.exe -F chat_id=1417914108 -F document=@<path> https://api.telegram.org/bot<TOKEN>/sendDocument`.

### КРИТИЧНО: канал до api.telegram.org медленный и рвётся
Замеры с этой машины: `speed_upload ≈ 20–27 КБ/с`, одиночная заливка >2–3 МБ обрывается (`curl exit 28/56`, HTTP 100 → reset).
Рабочая схема — нарезка на части по 1 МБ и поштучная отправка с ретраями:
`C:\SyntaxSolutions\omni-harness\ship-telegram.ps1` (части в `ship-parts\omni_01..omni_NN.bin`).
Сборка пакуется в `OmniHarness-1.0.0-win64.zip` (~10.6 МБ) через `Compress-Archive` (exe+dll+icon.png).
Склейка у получателя: `copy /b omni_01.bin+omni_02.bin+... OmniHarness-1.0.0-win64.zip`.

### Эталонные хеши сборки 1.0.0 (16.11.2026)
- zip SHA256 `95374EE4B6EB3948A14A5EAAB3B4A745025CC005B62D30DF78F91DF0C22FDAE2`
- exe SHA256 `79054D2A27C986FC549EB8F33A310DAFEE5A110F5F1CCBA0C3086930CBD25B7D` (23 250 432 байт)
- dll SHA256 `86545B66CDB0603BC26B626FB9AD610CB6E71F28D468F5EA66DF23B03DDA96D5` (160 880 байт)