# Lowk roadmap

## Available now (pre-alpha)
1. Live transcription (Me / the app you pick)
2. Context: folders, calls as `.md`, documents, search, summary
3. AI answers on a hotkey, plus screenshots
4. Stealth overlay with adaptive colour
5. Onboarding, RU/EN, a notice about recording laws
6. Microphone on/off: manual toggle (button, `toggle_mic` bind, per-folder preset) and an "auto" mode that records the mic only while the call app holds it open (needs a live check on macOS and Windows). Muting inside Zoom or another call app is invisible to the OS, so following it is only a silence heuristic.

## Later
- Detect the start of a call and offer to record
- Speaker diarization instead of only Me / System
- Slide and screen-share capture
- Looped virtual camera
- Chat with the model about a finished call (today: summary only)
- Call rating at the end of a recording
- Vault encryption with a password
- Sync between devices
- Per-application audio capture on Windows 10
- Process disguise (name and icon)
- Subscription or tokens on top of bring-your-own-key
- Re-transcription of a finished call (more accurate Whisper pass)
- Privacy layer for AI requests: mask personal data and secrets before they reach a model and restore them in the answer

---

# Роадмап

## Сейчас (пре-альфа)
1. Транскрипция в реальном времени (я / система-приложение)
2. Контекст: папки, созвоны в `.md`, документы, поиск, саммари
3. Ответ нейронки по биндам и скриншоты
4. Стелс-оверлей с адаптивным цветом
5. Онбординг, RU/EN, предупреждение о законности записи
6. Микрофон вкл/выкл: ручной переключатель (кнопка, бинд `toggle_mic`, пресет на папку) и режим «авто» (микрофон пишется, только пока приложение созвона держит его открытым; нужна живая проверка на macOS и Windows). Кнопку mute внутри Zoom или другого приложения ОС не видит, поэтому синхронизация с ней — только эвристика по тишине.

## Позже
- Автоопределение начала созвона и предложение записать
- Диаризация собеседников, а не только «я/система»
- Слайды и снимки трансляции
- Зацикленная виртуальная камера
- Диалог с нейронкой по итогам созвона (сейчас — только саммари)
- Оценка созвона в конце записи
- Шифрование хранилища паролем
- Синхронизация между устройствами
- Захват отдельного приложения на Windows 10
- Маскировка процесса (имя и иконка)
- Подписка или токены поверх BYOK
- Перетранскрибирование созвона после окончания (Whisper, точнее)
- Приватность данных для ИИ: маскирование персональных данных и секретов перед отправкой в нейронку и возврат их в ответе
