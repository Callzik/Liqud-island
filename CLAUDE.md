# Island — контекст проекта для Claude

Жидкий «Dynamic Island» для Windows 10/11. Репозиторий **Callzik/Liqud-island** (ветка `main`).
Стек: Tauri 2 (Rust, WebView2) + React 19 + TypeScript + Vite 6. Идентификатор `dev.callzik.island`,
данные пользователя — `%APPDATA%\dev.callzik.island\`. Текущая версия — в `package.json`.

## Как работаем

- Ответы и тексты релизов — по-русски, коротко, без воды.
- Каждое изменение: коммит + пуш в `main`, CI «Build Windows installer» должен быть зелёным.
- **Не добавлять в коммиты строки `Co-Authored-By` и `Claude-Session`** — они попадают в описания релизов.
- Новая версия: поднять номер в трёх местах — `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`
  (и `src-tauri/Cargo.lock`, пакет `island`). Сообщение коммита — по-русски, это название фичи.
- Тег и релиз `vX.Y.Z` пользователь создаёт сам через веб-интерфейс GitHub (из облачной сессии пуш тегов
  блокируется прокси). Workflow на тег сам прикрепляет `Island_X.Y.Z_x64-setup.exe`.
  По просьбе — дать короткий заголовок и список изменений для релиза.
- Не ломать существующие функции. Новые крейты — с фиксированной мажорной версией.
- Новые режимы острова рисуются через `liquid.ts` и обновляют зону кликов (`set_hit`).
- Пользователь иногда заливает файлы через веб GitHub («Add files via upload») — они падают в корень,
  workflow `fix-layout.yml` раскладывает их по одноимённым файлам в `src/` и `src-tauri/`.
  Перед работой — `git pull`.

## Проверка без Windows

- Rust: `rustup target add x86_64-pc-windows-msvc` и `cargo check --target x86_64-pc-windows-msvc` в `src-tauri/`.
- Фронт: `npm run build` (tsc + vite).
- Интерфейс: `npm run dev` → http://localhost:1420, демо-режим с данными из `src/mock.ts`;
  скриншоты через Playwright (Chromium в `/opt/pw-browsers/chromium`).
- Дев-сервер запускать через `setsid nohup … &` с PID-файлом; **не использовать `pkill -f`** — убивает свою же оболочку.
- `gh` через прокси: только пути `repos/Callzik/liqud-island/...`.
- Всё, что завязано на Windows API, проверяется только сборкой — честно говорить пользователю, что не проверено вживую.

## Устройство

```
src/
  App.tsx              состояния острова (music/voice/download/rec…), события из Rust, тема, ширина острова
  liquid.ts            жидкая форма на canvas: пружины, волна, капли; setGlass/setBackdrop/paintGlass
  mock.ts              демо-режим для браузера (mockListen — новая обёртка на каждую подписку, иначе StrictMode теряет события)
  components/          Home (плитки 2×4, плеер, ♥/👎 ЯМ, «E · без цензуры»), SettingsTab, Lyrics, Compact,
                       Overlay (захват), Chat, Weather, VoicePill, DownloadPill, RecPill, Residents, Toast, Panel,
                       Launcher, ClipboardTab, ShelfTab, TextTab, NotesTab, DropZones
  lib/lyrics.ts        поиск текста: Яндекс Музыка → LRCLIB (/get + поиск по вариантам названия), parseLrc (enhanced-LRC, слоги)
  lib/lyricSync.ts     синхронизация подсветки слов (сглаживание, LEAD=90 мс)
  lib/lyricOffset.ts   ручной сдвиг текста для трека (localStorage)
  lib/residents.tsx    25 жильцов, кадры idle/walk/react
src-tauri/src/
  main.rs              окна, трей, хоткеи, фоновые потоки, все команды (invoke_handler в конце файла)
  store.rs             Settings (serde camelCase), полка, ярлыки
  ym.rs                Яндекс Музыка: токен, ♥/👎, текст песен
  yt.rs                YouTube Data API: поиск того же трека
  record.rs            запись экрана в MP4 (windows-capture)
  ai.rs weather.rs voice.rs downloads.rs clips.rs
  log.rs               журнал ошибок logs\island.log (паники, toast_error, ошибки JS через log_js), трей «Открыть/Скопировать журнал»
  spectrum.rs          WASAPI loopback (cpal) + FFT → 4 полосы для эквалайзера (событие spectrum)
  update.rs            автообновление: releases/latest → *-setup.exe в %TEMP% → запуск с /P /UPDATE /R
  native/              media (GSMTC), audio (Core Audio, IPolicyConfig), capture (GDI, OCR, QR), speech, clip, input, shell, img
```

Фоновые потоки в `main.rs`: media (опрос 300 мс при игре, иначе 700), backdrop (для стекла), weather, voice, downloads,
update (через 30 с после старта и каждые 6 ч; тост «Обновить / Пропустить», `settings.updateCheck`, `updateSkip`).

## Функции (что уже есть)

Музыка из любого плеера (GSMTC), громкость и устройство вывода, полка, лаунчер `Alt+Space` (программы,
калькулятор, валюты, величины), буфер обмена, захват экрана (область / экран / OCR / QR / пипетка,
`Ctrl+Shift+S` / `Ctrl+Shift+T`), запись экрана в `Videos\Island`, ИИ-чат (OpenAI-совместимый, по умолчанию
LM Studio), погода (Open-Meteo + ipwho.is), голос → текст (`Ctrl+Alt+Space`, Windows или Whisper), загрузки,
вкладки «Текст» и «Заметки», жильцы, текст песни с подсветкой слов, ♥/«не рекомендовать» Яндекс Музыки,
тема Liquid Glass / чёрная, режим «Без цензуры», прятанье в полноэкранных приложениях, автозапуск.

## Интеграции — детали

- **Яндекс Музыка** (`ym.rs`): API `api.music.yandex.net`, заголовки `Authorization: OAuth <token>` и
  `X-Yandex-Music-Client: YandexMusicAndroid/24023621`. Вход — окно OAuth (`ym::LOGIN_URL`), токен из
  `#access_token=` в редиректе; команда `ym_login` асинхронная (иначе белое окно), есть ручная вставка
  токена (`ym_set_token`). Токен хранится в `settings.ymToken`.
  Текст: `/tracks/{id}/lyrics?format=LRC|TEXT&timeStamp&sign`, подпись HMAC-SHA256 от `{id}{ts}`
  ключом `p93jhgh689SBReK6ghtw62`, base64; затем скачать `downloadUrl`.
- **LRCLIB**: `lrclib.net/api` (`/get`, `/search`), запросы прямо из JS (CORS открыт).
- **«Без цензуры»** (`yt.rs`, `main.rs`: `uncensored`, `yt_open`, `yt_switch`, `auto_uncensored`, `route_media`):
  Яндекс выбирает трек → Яндекс на паузу → тот же трек ищется через YouTube Data API v3 (`settings.ytKey`,
  ближайшая длительность) → играет в скрытом окне `yt` (свой `data_directory` `youtube-webview`,
  autoplay без жеста). Конец трека: скрипт переходит на `https://island.invalid/ended`, это ловит
  `on_navigation` и шлёт Яндексу «вперёд». Кнопки острова маршрутизируются в YouTube (eval) или Яндекс.
  Не нашли трек → играет Яндекс как есть, тост «Играет обычная версия». Без ключа кнопка открывает
  поиск YouTube Music. Имя медиасессии своего WebView2 (`OWN_PLAYER = "island|webview"`) — догадка, не проверено.
- **Стекло**: поток backdrop снимает экран под островом (GDI), уменьшает и шлёт PNG событием `backdrop`;
  canvas рисует его размытым. Окно острова исключено из захвата (`SetWindowDisplayAffinity`), пока включено стекло или идёт запись.

## Не проверено на настоящей Windows

Захват и запись экрана, OCR и распознавание речи, переключение устройства вывода, слежение за загрузками,
скрытое окно YouTube и его медиасессия, стекло (частота и нагрузка снимков фона), спектр звука (loopback).

## Идеи на будущее (пользователь ещё не выбрал)

2. Уведомления Windows в острове (UserNotificationListener).
3. Громкость/яркость — полоска в острове вместо системного окна. 6. Анимация наушников и зарядки. 7. Таймер / секундомер / помодоро.
8. Яндекс Музыка глубже: «Моя волна», очередь, плейлисты. 9. Перевод текста песни.

## История версий (кратко)

- 0.1.0–0.1.1 — первый релиз, плавное закрытие.
- 0.2.0 — текст песни с подсветкой слов; 0.2.1 — быстрый поиск и строка в свёрнутом острове.
- 0.3.0 — этапы ТЗ 0–9 (захват, ИИ, погода, голос, загрузки, устройство вывода, текст/заметки, жильцы).
- 0.3.1–0.3.5 — запись экрана, правки другого ИИ через веб-загрузку, раскладка файлов по папкам.
- 0.3.6 — ♥ и «не рекомендовать» Яндекс Музыки; 0.3.7 — точнее подсветка слов, ручной сдвиг;
  0.3.8 — вход в ЯМ без белого экрана; 0.3.9 — тексты из ЯМ и настойчивый поиск в LRCLIB.
- 0.4.0 — Liquid Glass / чёрный; 0.4.1 — «Без цензуры» (окно YouTube); 0.4.2 — «Без цензуры» автоматически;
  0.4.3 — текст песни в свёрнутом острове можно выключить.
- 0.4.4–0.4.6 — медиасессии, Dotify; 0.4.7 — автообновление из релизов GitHub (тост, трей, настройки).
- 0.4.8 — журнал ошибок; стекло без PNG (BMP через протокол backdrop://, StretchBlt), курсор вдали опрашивается реже;
  эквалайзер по настоящему спектру, цвет острова под обложку (`settings.coverTint`).
