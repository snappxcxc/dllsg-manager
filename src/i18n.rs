//! Minimal runtime localization (Russian / English).
//!
//! The program is written in Russian; English is produced by translating the
//! *rendered* string. `tr()` first looks for an exact match in [`TABLE`], then
//! tries the wildcard templates (entries with `{}` placeholders) that cover all
//! messages built with `format!`. Anything unknown falls through unchanged, so a
//! missing translation degrades to Russian instead of breaking the UI.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Ru,
    En,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::Ru => "ru",
            Lang::En => "en",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Lang::Ru => "RU",
            Lang::En => "EN",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        match code.trim().to_ascii_lowercase().as_str() {
            "ru" | "rus" | "russian" | "рус" | "русский" => Some(Lang::Ru),
            "en" | "eng" | "english" | "англ" | "английский" => Some(Lang::En),
            _ => None,
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn lang() -> Lang {
    if CURRENT.load(Ordering::Relaxed) == 1 {
        Lang::En
    } else {
        Lang::Ru
    }
}

pub fn set_lang(lang: Lang) {
    CURRENT.store(
        if lang == Lang::En { 1 } else { 0 },
        Ordering::Relaxed,
    );
}

pub fn is_en() -> bool {
    lang() == Lang::En
}

/// UI language of Windows itself: Russian systems start in Russian, the rest in
/// English.
pub fn system_lang() -> Lang {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
        use winreg::RegKey;
        if let Ok(key) = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(r"Control Panel\International", KEY_READ)
        {
            let locale: String = key.get_value("LocaleName").unwrap_or_default();
            if !locale.trim().is_empty() {
                return if locale.to_ascii_lowercase().starts_with("ru") {
                    Lang::Ru
                } else {
                    Lang::En
                };
            }
        }
    }
    Lang::Ru
}

struct Tables {
    exact: HashMap<&'static str, &'static str>,
    patterns: Vec<(&'static str, &'static str)>,
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut exact = HashMap::with_capacity(TABLE.len());
        let mut patterns = Vec::new();
        for (ru, en) in TABLE {
            if ru.contains("{}") || en.contains("{}") {
                patterns.push((*ru, *en));
            } else {
                exact.insert(*ru, *en);
            }
        }
        // Longest templates first: more specific messages win.
        patterns.sort_by_key(|(ru, _)| std::cmp::Reverse(ru.len()));
        Tables { exact, patterns }
    })
}

/// Translate an already formatted Russian string.
pub fn tr(text: &str) -> Cow<'_, str> {
    if lang() == Lang::Ru || text.is_empty() {
        return Cow::Borrowed(text);
    }
    if let Some(en) = tables().exact.get(text) {
        return Cow::Borrowed(en);
    }
    for (ru, en) in &tables().patterns {
        if let Some(parts) = split_match(ru, text) {
            return Cow::Owned(join(en, &parts));
        }
    }
    Cow::Borrowed(text)
}

/// Translate a static string (window titles, labels with `'static` lifetime).
pub fn t(text: &'static str) -> &'static str {
    if lang() == Lang::Ru {
        return text;
    }
    if let Some(en) = tables().exact.get(text) {
        return en;
    }
    for (ru, en) in &tables().patterns {
        if *ru == text {
            return en;
        }
    }
    text
}

fn join(template: &str, parts: &[&str]) -> String {
    let slots: Vec<&str> = template.split("{}").collect();
    let mut out = String::with_capacity(template.len() + 16);
    for (i, slot) in slots.iter().enumerate() {
        out.push_str(slot);
        if i < parts.len() {
            out.push_str(parts[i]);
        }
    }
    out
}

/// Matches `input` against a template with `{}` placeholders.
fn split_match<'a>(template: &str, input: &'a str) -> Option<Vec<&'a str>> {
    let parts: Vec<&str> = template.split("{}").collect();
    if parts.len() == 1 {
        return if template == input { Some(Vec::new()) } else { None };
    }
    let mut rest = input.strip_prefix(parts[0])?;
    let mut captures = Vec::with_capacity(parts.len() - 1);
    for (i, part) in parts.iter().enumerate().skip(1) {
        if i + 1 == parts.len() {
            captures.push(rest.strip_suffix(part)?);
        } else {
            let pos = rest.find(part)?;
            captures.push(&rest[..pos]);
            rest = &rest[pos + part.len()..];
        }
    }
    Some(captures)
}

/// `RU -> EN` table. Entries with `{}` are templates for `format!`-built text.
#[rustfmt::skip]
const TABLE: &[(&str, &str)] = &[
    // ---- header / shell -------------------------------------------------
    ("DLSSG Manager — Frame Generation для RTX 20/30", "DLSSG Manager — Frame Generation for RTX 20/30"),
    ("Frame Generation для RTX 20/30 · прокси-мод dlssg_sm86", "Frame Generation for RTX 20/30 · dlssg_sm86 proxy mod"),
    ("Установка", "Install"),
    ("Обновления", "Updates"),
    ("О программе", "About"),
    ("Готов", "Ready"),
    ("Анализ", "Scan"),
    ("Отменить", "Cancel"),
    ("Отмена", "Cancel"),
    ("Идёт операция…", "Work in progress…"),

    // ---- sidebar --------------------------------------------------------
    ("Конфигурация ПК", "Hardware"),
    ("драйвер {}", "driver {}"),
    ("права администратора", "administrator rights"),
    ("ОС:", "OS:"),
    ("Нужен драйвер NVIDIA R{} или новее.", "NVIDIA driver R{} or newer is required."),
    ("Пакет мода", "Mod package"),
    (" (последняя)", " (latest)"),
    ("Пакетов в кэше нет — скачайте на вкладке «Обновления».", "No cached packages yet — download one on the «Updates» tab."),
    ("Источник:", "Source:"),
    ("включает Super Resolution", "includes Super Resolution"),
    ("Обновить с GitHub", "Update from GitHub"),
    ("Эта версия уже в кэше", "This version is already cached"),
    ("Импорт папки", "Import folder"),
    ("Мои установки", "My installs"),
    ("Пока ничего не установлено.", "Nothing installed yet."),
    ("Журнал", "Log"),
    ("Открыть папку", "Open folder"),
    ("Папка программы", "App folder"),

    // ---- step 1: game ---------------------------------------------------
    ("Шаг 1 · Игра", "Step 1 · Game"),
    ("Найти игры", "Find games"),
    ("Автопоиск: Steam, Epic Games, GOG, Xbox, EA, Ubisoft и установленные программы.", "Auto-detect: Steam, Epic Games, GOG, Xbox, EA, Ubisoft and installed programs."),
    ("Обзор…", "Browse…"),
    ("Папка с EXE игры", "Folder with the game exe"),
    ("Проверить", "Check"),
    ("рендер-папка", "renderer folder"),
    ("нет D3D12", "no D3D12"),
    ("Подойдёт папка, где лежит EXE, которым рендерит игра (например …\\Game\\Binaries\\Win64). Можно выбрать игру карточкой выше, указать путь вручную или найти папку через «Обзор…».", "Pick the folder that holds the exe the game renders with (for example …\\Game\\Binaries\\Win64). You can also click a game card above, type the path or use «Browse…»."),
    ("Сканирую: {}", "Scanning: {}"),
    ("Иконка: {}", "Icon: {}"),
    ("Найдено игр: {}", "Games found: {}"),
    ("Найдено игр: {} (с DLSS Frame Generation: {})", "Games found: {} (with DLSS Frame Generation: {})"),
    ("Автопоиск: {} игр, {} с nvngx_dlssg.dll", "Auto-detect: {} games, {} with nvngx_dlssg.dll"),
    ("Выбрана игра: {}", "Game selected: {}"),
    ("Поиск установленных игр…", "Looking for installed games…"),
    ("Автопоиск игр", "Game auto-detect"),
    ("Ищу установленные игры…", "Looking for installed games…"),
    ("Укажите папку игры", "Choose the game folder"),
    ("Папка с файлами пакета (manifest.json + version.dll)", "Folder with the package files (manifest.json + version.dll)"),
    ("Установленные программы", "Installed programs"),
    ("DLSS без FG", "DLSS without FG"),
    ("DLSS не найден", "DLSS not found"),
    ("мод {}", "mod {}"),

    // ---- step 2: compatibility -----------------------------------------
    ("Шаг 2 · Проверка совместимости", "Step 2 · Compatibility check"),
    ("Укажите папку игры и нажмите «Проверить».", "Choose the game folder and press «Check»."),
    ("{} критичных замечаний, {} предупреждений", "{} critical issues, {} warnings"),
    ("{} предупреждений, критичных нет", "{} warnings, no critical issues"),
    ("Все проверки пройдены", "All checks passed"),
    ("Можно устанавливать", "Ready to install"),
    ("Можно, но есть замечания", "Possible, but there are warnings"),
    ("Установка не рекомендуется", "Installing is not recommended"),
    ("Папка не найдена", "Folder not found"),
    ("{} не существует или недоступна", "{} does not exist or is not accessible"),
    ("Видеокарта не NVIDIA", "GPU is not NVIDIA"),
    ("Найдено: {}. Мод работает только на NVIDIA RTX 20/30.", "Found: {}. The mod only works on NVIDIA RTX 20/30."),
    ("Frame Generation уже есть нативно", "Frame Generation is already native"),
    ("{} — на RTX 40/50 DLSS Frame Generation работает из коробки, мод не нужен и ничего не добавит.", "{} — on RTX 40/50 DLSS Frame Generation works out of the box; the mod is unnecessary and adds nothing."),
    ("Архитектура GPU не распознана", "GPU architecture not recognised"),
    ("{} — мод рассчитан на RTX 20 (SM75) и RTX 30 (SM86).", "{} — the mod targets RTX 20 (SM75) and RTX 30 (SM86)."),
    ("{} — это ровно та архитектура, под которую собран пакет.", "{} — exactly the architecture the package was built for."),
    ("{} — поддерживается, но пакет оптимизирован под SM86.", "{} — supported, though the package is tuned for SM86."),
    ("GPU совместим: {}", "GPU compatible: {}"),
    ("Драйвер NVIDIA {}", "NVIDIA driver {}"),
    ("Требование мода — R{} или новее.", "The mod requires R{} or newer."),
    ("Драйвер старше R580", "Driver older than R580"),
    ("Установлен {}. Мод не включит генерацию кадров (в логе будет driver_below_floor) — обновите драйвер NVIDIA.", "Installed: {}. The mod will not enable frame generation (the log will say driver_below_floor) — update the NVIDIA driver."),
    ("неизвестный драйвер", "unknown driver"),
    ("Игра рендерит через D3D12", "The game renders through D3D12"),
    ("{} импортирует {}. Прокси-мод работает с D3D12.", "{} imports {}. The proxy mod works with D3D12."),
    ("D3D12 в импортах не найден", "D3D12 not found in the imports"),
    ("{} не ссылается на d3d12.dll/dxgi.dll (возможна отложенная загрузка). Если игра на Vulkan/D3D11 — мод не заработает.", "{} does not reference d3d12.dll/dxgi.dll (it may be delay-loaded). If the game runs on Vulkan/D3D11 the mod will not work."),
    ("EXE игры не найден", "Game exe not found"),
    ("В папке нет ни одного .exe — выберите каталог с исполняемым файлом игры.", "There is no .exe in the folder — choose the directory with the game executable."),
    ("DLSS Frame Generation есть в игре", "The game has DLSS Frame Generation"),
    ("Найден nvngx_dlssg.dll ({}) — игре есть что подменять, генерация кадров должна появиться в настройках.", "nvngx_dlssg.dll found ({}): there is something to replace, frame generation should appear in the game settings."),
    ("nvngx_dlssg.dll в папке игры не найден", "nvngx_dlssg.dll not found in the game folder"),
    ("Мод подменяет загрузку DLSS-G. Без этого плагина игра, скорее всего, не умеет Frame Generation — установка ничего не изменит. Проверьте, что выбрана папка с EXE рендера (например, ...\\Binaries\\Win64).", "The mod replaces the DLSS-G plugin load. Without that plugin the game most likely has no Frame Generation, so installing changes nothing. Make sure you picked the folder with the renderer exe (e.g. ...\\Binaries\\Win64)."),
    ("Обнаружен античит", "Anti-cheat detected"),
    ("{} — прокси-DLL рядом с античитом может привести к бану. Устанавливайте только если уверены, что античит не проверяет целостность файлов.", "{} — a proxy DLL next to an anti-cheat may lead to a ban. Install only if you are sure the anti-cheat does not verify file integrity."),
    ("Античит не обнаружен", "No anti-cheat detected"),
    ("Файлы EasyAntiCheat / BattlEye / Vanguard рядом с игрой не найдены — подмена DLL не блокируется.", "No EasyAntiCheat / BattlEye / Vanguard files near the game — DLL replacement is not blocked."),
    ("Игра не запущена", "The game is not running"),
    ("Файлы не заблокированы, установка возможна.", "Files are not locked, installation is possible."),
    ("Игра запущена", "The game is running"),
    ("Закройте процесс: {}", "Close the process: {}"),
    ("Папка доступна для записи", "Folder is writable"),
    ("Права администратора не нужны.", "No administrator rights needed."),
    ("Нет прав на запись", "Folder is not writable"),
    ("Папка защищена (Program Files?). Запустите программу от имени администратора.", "The folder is protected (Program Files?). Run the program as administrator."),
    ("Установлена версия {}", "Version {} is installed"),
    ("Доступна более новая версия пакета — обновите через вкладку «Обновления».", "A newer package is available — update it on the «Updates» tab."),
    ("Уже установлено: {}", "Already installed: {}"),
    ("Прокси: {}. Можно переустановить или удалить.", "Proxy: {}. You can reinstall or remove it."),
    ("Мод не установлен", "Mod is not installed"),
    ("Файлов dlssg_sm86 в папке нет.", "There are no dlssg_sm86 files in the folder."),
    ("В папке есть чужие DLL с теми же именами", "The folder has foreign DLLs with the same names"),
    ("{}. При установке они будут сохранены в .dlssg-manager\\backups и восстановлены при удалении.", "{}. On install they are saved to .dlssg-manager\\backups and restored on removal."),
    ("Несколько прокси мода в одной папке", "Several mod proxies in one folder"),
    ("{} — работает только первый загруженный, остальные лучше убрать (это делает установщик).", "{} — only the first one loaded works; the rest should be removed (the installer does that)."),
    ("нейросетевой апскейлер", "neural upscaler"),

    // ---- step 3: proxy --------------------------------------------------
    ("Шаг 3 · Способ внедрения (прокси DLL)", "Step 3 · Injection method (proxy DLL)"),
    ("Мод попадает в игру под именем системной DLL. Начните с version.dll; если игра его не грузит — поставьте следующий вариант по порядку.", "The mod enters the game under the name of a system DLL. Start with version.dll; if the game does not load it, try the next one in order."),
    ("Убрать лишние прокси прошлых установок (рекомендуется)", "Remove leftover proxies from previous installs (recommended)"),
    ("Лучший выбор: его загружает почти любая игра.", "Best choice: almost every game loads it."),
    ("Второй по надёжности (мультимедиа-API).", "Second most reliable (multimedia API)."),
    ("Третий: его часто грузят игры и оверлеи.", "Third: games and overlays load it often."),
    ("Четвёртый: старый DirectInput8.", "Fourth: the old DirectInput8."),
    ("Рискованно: стоит в рендер-пути, но встречается часто.", "Risky: it sits in the render path, but is common."),
    ("Последний вариант: только если dxgi.dll не сработал.", "Last resort: only if dxgi.dll did not work."),

    // ---- step 4: install ------------------------------------------------
    ("Шаг 4 · Установка", "Step 4 · Installation"),
    ("Обновить {} → {}", "Update {} → {}"),
    ("Переустановить {}", "Reinstall {}"),
    ("Установить {}", "Install {}"),
    ("Установить", "Install"),
    ("Точно удалить?", "Really remove?"),
    ("Удалить", "Remove"),
    ("Установлено", "Installed"),
    ("Игра может не поддерживать D3D12 — проверьте настройки графики после установки.", "The game may not support D3D12 — check the graphics settings after installing."),
    ("Сначала проверьте папку игры", "Check the game folder first"),
    ("Закройте игру", "Close the game"),
    ("Нужны права администратора", "Administrator rights required"),
    ("Нужна видеокарта NVIDIA RTX", "An NVIDIA RTX GPU is required"),
    ("Драйвер младше R{}", "Driver older than R{}"),
    ("Есть критичные замечания", "There are critical issues"),
    ("Нет пакета в кэше", "No package in the cache"),
    ("Нет пакета: обновите или импортируйте вручную", "No package: update or import one manually"),

    // ---- step 5: verifying ----------------------------------------------
    ("Шаг 5 · Проверка, что мод работает", "Step 5 · Verifying that the mod works"),
    ("Включить Level=2", "Enable Level=2"),
    ("Проверить лог", "Check the log"),
    ("Открыть лог", "Open the log"),
    ("Открыть запись в INI", "Open the INI entry"),
    ("dlssg_sm86\\logs\\dlssg_sm86.log ещё нет", "dlssg_sm86\\logs\\dlssg_sm86.log does not exist yet"),
    ("В логе нет строк с именем игры — похоже, выбранный прокси не загружается. Попробуйте следующий вариант из шага 3.", "There are no lines with the game name in the log — the selected proxy does not seem to load. Try the next option from step 3."),
    ("Найдено {} строк про {} — прокси работает.", "Found {} lines about {} — the proxy works."),
    ("Лог не найден. Включите Level=2, зайдите в игру, включите генерацию кадров и выйдите.", "Log not found. Enable Level=2, launch the game, turn on frame generation, then quit."),
    ("Прокси работает: записи об игре есть в логе", "The proxy works: the log has entries about the game"),
    ("Записей об игре нет — попробуйте другой прокси DLL", "No entries about the game — try another proxy DLL"),
    ("Уровень логирования = 2. Запустите игру с включённой генерацией кадров, затем нажмите «Проверить лог».", "Logging level = 2. Launch the game with frame generation enabled, then press «Check the log»."),

    // ---- updates tab ----------------------------------------------------
    ("Источник обновлений", "Update source"),
    ("Проверить сейчас", "Check now"),
    ("Открыть на GitHub", "Open on GitHub"),
    ("Репозиторий зашит в программу: скачиваются только официальные релизы мода.", "The repository is built into the app: only official mod releases are downloaded."),
    ("Проверять обновления при запуске", "Check for updates on startup"),
    ("Автоматически скачивать новый пакет в кэш", "Download a new package to the cache automatically"),
    ("Сразу устанавливать обновление во все папки игры после скачивания", "Install the update into all game folders right after downloading"),
    ("Искать установленные игры при запуске", "Look for installed games on startup"),
    ("Сохранить настройки", "Save settings"),
    ("Настройки сохранены", "Settings saved"),
    ("Язык", "Language"),
    ("Язык: {}", "Language: {}"),
    ("Последний релиз", "Latest release"),
    ("Запрос к GitHub…", "Querying GitHub…"),
    ("Нажмите «Проверить сейчас»", "Press «Check now»"),
    ("Версия {}", "Version {}"),
    ("доступно обновление", "update available"),
    ("уже в кэше", "already cached"),
    ("актуально", "up to date"),
    ("Файл:", "File:"),
    ("Размер:", "Size:"),
    ("Опубликован:", "Published:"),
    ("Установленная версия:", "Installed version:"),
    ("нет", "none"),
    ("Пакет уже скачан", "Package already downloaded"),
    ("Скачать и подготовить", "Download and prepare"),
    ("Показать заметки", "Show notes"),
    ("Страница релиза", "Release page"),
    ("Автоматическое обновление всех установок", "Update all installations"),
    ("Нет папок, куда установлен мод.", "No folders have the mod installed."),
    ("Обновить", "Update"),
    ("Нажмите «Установить» после проверки — файлы будут заменены с бэкапом", "Press «Install» after the check — files are replaced with a backup"),
    ("Обновить все папки сразу", "Update all folders at once"),
    ("Проверить обновления", "Check for updates"),
    ("Цель: {}", "Target: {}"),
    ("нет пакета", "no package"),
    ("Обновление {} папок до {}", "Updating {} folders to {}"),
    ("Обновление всех установок", "Updating all installations"),
    ("Обновлено папок: {}", "Folders updated: {}"),
    ("{}. Ошибки: {}", "{}. Errors: {}"),
    ("Обновление завершено: {}", "Update finished: {}"),

    // ---- about ----------------------------------------------------------
    ("Что делает программа", "What the program does"),
    ("Автопоиск", "Auto-detect"),
    ("Находит игры в библиотеках Steam, Epic Games, GOG, Xbox/Game Pass, EA, Ubisoft и Battle.net, а также среди установленных программ — но там только те папки, где реально лежит nvngx_dlss*.dll. Для каждой игры показывается иконка её EXE и сразу выполняется проверка совместимости.", "Finds games in Steam, Epic Games, GOG, Xbox/Game Pass, EA, Ubisoft and Battle.net libraries, and among installed programs — but there only folders that actually contain nvngx_dlss*.dll. Every game shows its exe icon and is checked for compatibility right away."),
    ("Проверка совместимости", "Compatibility check"),
    ("Читает GPU, драйвер, импорты EXE (d3d12/ngx), наличие nvngx_dlssg.dll, античитов и права на папку. Если античит найден — установка запрещена.", "Reads the GPU, driver, exe imports (d3d12/ngx), presence of nvngx_dlssg.dll, anti-cheats and folder permissions. If an anti-cheat is found, installation is blocked."),
    ("Кладёт version.dll (или альтернативный прокси), dlssg_sm86.ini и папку dlssg_sm86 рядом с EXE. Старые файлы сохраняются в .dlssg-manager\\backups и восстанавливаются при удалении.", "Puts version.dll (or an alternative proxy), dlssg_sm86.ini and the dlssg_sm86 folder next to the exe. Replaced files go to .dlssg-manager\\backups and are restored on removal."),
    ("Раз в запуск (и по кнопке) проверяет релизы GitHub, скачивает zip с прогрессом, проверяет размер и SHA-256 основных файлов из manifest.json, кэширует и умеет обновлять все папки одной кнопкой.", "Once per run (and on demand) it checks GitHub releases, downloads the zip with progress, verifies size and SHA-256 of the main files from manifest.json, caches packages and updates all folders with one click."),
    ("Проверка работы", "Verifying it works"),
    ("Кнопка «Установить Level=2» в шаге 5 включает подробный лог, после запуска игры «Проверить лог» ищет строки с именем EXE — так видно, загрузился ли прокси.", "The «Enable Level=2» button in step 5 turns on the verbose log; after the game ran, «Check the log» looks for lines with the exe name — that shows whether the proxy loaded."),
    ("Как это работает", "How it works"),
    ("Прокси DLL кладётся рядом с EXE и перехватывает загрузку nvngx_dlssg.dll, подсовывая рантайм из dlssg_sm86\\runtime, который ставит генерацию кадров на RTX 20/30 (SM75/SM86). На RTX 40/50 генерация кадров есть нативно — мод им не нужен. Требования: Windows 10/11 x64, D3D12, NVIDIA R580+.", "The proxy DLL sits next to the exe and intercepts the nvngx_dlssg.dll load, injecting the runtime from dlssg_sm86\\runtime that enables frame generation on RTX 20/30 (SM75/SM86). RTX 40/50 have native frame generation and do not need the mod. Requirements: Windows 10/11 x64, D3D12, NVIDIA R580+."),
    ("Альтернативы прокси (по порядку):", "Proxy alternatives (in order):"),
    ("Проект, авторы и лицензии", "Project, credits and licenses"),
    ("Проект и лицензии", "Project and licenses"),
    ("Мод и рантайм", "Mod and runtime"),
    ("sdli1995/dlssg_for_sm86 (GitHub). Все файлы мода и рантайма принадлежат их автору; программа не распространяет их, а скачивает официальный релиз с GitHub и раскладывает по папке игры.", "sdli1995/dlssg_for_sm86 (GitHub). All mod and runtime files belong to their author; this program does not redistribute them — it downloads the official release from GitHub and unpacks it into the game folder."),
    ("Эта программа", "This program"),
    ("Автор: ", "Author: "),
    (". Лицензия MIT — используйте на свой риск, без каких-либо гарантий.", ". MIT license — use at your own risk, without any warranty."),
    ("Товарные знаки", "Trademarks"),
    ("DLSS, NVIDIA, GeForce и RTX — товарные знаки NVIDIA Corporation. Программа не связана с NVIDIA и не поддерживается ею; файлы nvngx_* распространяются по лицензии NVIDIA.", "DLSS, NVIDIA, GeForce and RTX are trademarks of NVIDIA Corporation. This program is not affiliated with or endorsed by NVIDIA; nvngx_* files are distributed under the NVIDIA license."),
    ("Репозиторий мода", "Mod repository"),
    ("Репозиторий программы", "App repository"),

    // ---- jobs / toasts / messages ---------------------------------------
    ("Проверка обновлений на GitHub", "Checking GitHub for updates"),
    ("GitHub: доступна версия {} ({})", "GitHub: version {} is available ({})"),
    ("GitHub: {}", "GitHub: {}"),
    ("Новая версия {}", "New version {}"),
    ("На GitHub новая версия {}", "A new version {} is on GitHub"),
    ("Автозагрузка {} с GitHub", "Auto-downloading {} from GitHub"),
    ("Проверка обновлений: {}", "Update check: {}"),
    ("Не удалось проверить обновления", "Could not check for updates"),
    ("Проверка обновлений", "Update check"),
    ("Анализ папки игры", "Scanning the game folder"),
    ("Анализ завершён", "Scan finished"),
    ("Анализ: {} — {} (EXE: {})", "Scan: {} — {} (EXE: {})"),
    ("Проверка после установки", "Post-install check"),
    ("Скачивание версии {}", "Downloading version {}"),
    ("Загрузка {} с GitHub", "Downloading {} from GitHub"),
    ("Архив скачан: {} ({})", "Archive downloaded: {} ({})"),
    ("Пакет {} готов: {}", "Package {} is ready: {}"),
    ("Пакет {} готов к установке", "Package {} is ready to install"),
    ("Ошибка пакета: {}", "Package error: {}"),
    ("Установка мода", "Installing the mod"),
    ("Установлено {} в {}", "Installed {} into {}"),
    ("Мод {} установлен", "Mod {} installed"),
    ("Автоприменение: запустить повторный анализ", "Auto-apply: running the scan again"),
    ("Ошибка установки: {}", "Install error: {}"),
    ("Удаление мода", "Removing the mod"),
    ("Мод удалён, бэкапы восстановлены", "Mod removed, backups restored"),
    ("Мод удалён", "Mod removed"),
    ("Ошибка удаления: {}", "Removal error: {}"),
    ("Чтение лога мода", "Reading the mod log"),
    ("Импорт локального пакета", "Importing a local package"),
    ("Включение подробного лога", "Enabling the verbose log"),
    ("Не удалось изменить INI: {}", "Could not modify the INI: {}"),
    ("Дождитесь завершения текущей операции", "Wait for the current operation to finish"),
    ("Внимание: NVIDIA GPU не обнаружен", "Warning: no NVIDIA GPU detected"),
    ("Внимание: драйвер старше R{}, генерация кадров не включится", "Warning: the driver is older than R{}, frame generation will not work"),
    ("DLSSG Manager запущен. GPU: {} / драйвер {} / {}", "DLSSG Manager started. GPU: {} / driver {} / {}"),
    ("{} · драйвер {} · {}", "{} · driver {} · {}"),
    ("Не найден", "Not found"),
    ("не найден", "not found"),

    // ---- installer / packages / github errors ---------------------------
    ("Папка игры не найдена", "Game folder not found"),
    ("В пакете нет прокси {} (alternatives)", "The package has no proxy {} (alternatives)"),
    ("В пакете нет {}", "The package has no {}"),
    ("В пакете нет папки dlssg_sm86", "The package has no dlssg_sm86 folder"),
    ("Игра запущена ({}). Полностью закройте её и повторите установку.", "The game is running ({}). Close it completely and retry the installation."),
    ("Нет прав на запись в папку игры. Запустите программу от имени администратора.", "No write permission in the game folder. Run the program as administrator."),
    ("Убран лишний прокси прошлой установки: {}", "Removed a leftover proxy from a previous install: {}"),
    ("Старый {} сохранён в бэкап", "Old {} saved to a backup"),
    ("В папке уже был посторонний {} — он не тронут (это может быть другой мод)", "The folder already had a foreign {} — it was left alone (it may be another mod)"),
    ("Старая папка dlssg_sm86 перемещена в бэкап", "The old dlssg_sm86 folder was moved to a backup"),
    ("Копирование {} в {}", "Copying {} to {}"),
    ("Не записать маркер: {}", "Could not write the marker: {}"),
    ("Готово: {} / прокси {} / {:.1} МБ", "Done: {} / proxy {} / {} MB"),
    ("Готово: {} / прокси {} / {} МБ", "Done: {} / proxy {} / {} MB"),
    ("Не скопировать {}: {}", "Could not copy {}: {}"),
    ("Копирование: {}", "Copying: {}"),
    ("Игра запущена ({}). Закройте её перед удалением.", "The game is running ({}). Close it before removal."),
    ("Удаление: {}", "Removing: {}"),
    ("{} не принадлежит моду — оставлен на месте", "{} does not belong to the mod — left in place"),
    ("Файлы мода перемещены в .dlssg-manager\\backups (удаление обратимо)", "Mod files moved to .dlssg-manager\\backups (removal is reversible)"),
    ("Восстановлен {} из бэкапа", "Restored {} from a backup"),
    ("очистить бэкапы", "clean backups"),
    ("Бэкап {}: {}", "Backup {}: {}"),
    ("Удалено", "Removed"),
    ("В папке {} нет manifest.json + version.dll — это не пакет проекта.", "The folder {} has no manifest.json + version.dll — it is not a package of this project."),
    ("Не удалось прочитать manifest.json", "Could not read manifest.json"),
    ("Пакет уже в кэше", "Package is already cached"),
    ("Готово", "Done"),
    ("В пакете не хватает: {}. Скопируйте файлы из архива целиком.", "The package is missing: {}. Copy all files from the archive."),
    ("Не открыть архив: {}", "Could not open the archive: {}"),
    ("Повреждённый архив: {}", "Corrupted archive: {}"),
    ("Распаковка: {}", "Extracting: {}"),
    ("Распаковка архива", "Extracting the archive"),
    ("нет файла {}", "missing file {}"),
    ("{}: SHA-256 не совпадает (ожидался {}, получен {})", "{}: SHA-256 mismatch (expected {}, got {})"),
    ("{}: не прочитать ({})", "{}: cannot read ({})"),
    ("нет dlssg_sm86\\runtime\\nvngx_dlssg.dll", "missing dlssg_sm86\\runtime\\nvngx_dlssg.dll"),
    ("нет dlssg_sm86\\kernels\\fg_kernels.dll", "missing dlssg_sm86\\kernels\\fg_kernels.dll"),
    ("Ответ GitHub не прочитан: {}", "Could not read the GitHub response: {}"),
    ("У {} нет опубликованных релизов", "{} has no published releases"),
    ("GitHub API ответил {}. {}", "GitHub API replied {}. {}"),
    ("Нет связи с GitHub: {}", "Cannot reach GitHub: {}"),
    ("В ответе GitHub нет tag_name", "The GitHub response has no tag_name"),
    ("В релизе {} нет zip-архива с пакетом (проверьте {}/releases)", "Release {} has no package zip (check {}/releases)"),
    ("Не удалось определить тег последнего релиза", "Could not determine the latest release tag"),
    ("Проверено через страницу релизов GitHub (API недоступен).", "Checked via the GitHub releases page (API unavailable)."),
    ("Не удалось найти zip-архив в последнем релизе", "Could not find a zip archive in the latest release"),
    ("Загрузка не началась: {}", "Download did not start: {}"),
    ("Не создать файл: {}", "Could not create the file: {}"),
    ("Загрузка отменена", "Download cancelled"),
    ("Обрыв загрузки: {}", "Download interrupted: {}"),
    ("Скачано {} из {}", "Downloaded {} of {}"),
    ("Не переименовать файл: {}", "Could not rename the file: {}"),
    ("Размер не совпал: ожидалось {}, получено {}", "Size mismatch: expected {}, got {}"),
    ("Не записать конфиг: {}", "Could not write the config: {}"),
    ("неизвестно", "unknown"),
    ("неизвестна", "unknown"),
    ("не определено", "not detected"),
    ("не Windows", "not Windows"),

    // ---- value fragments & levels --------------------------------------
    ("Б", "B"),
    ("КБ", "KB"),
    ("МБ", "MB"),
    ("ГБ", "GB"),
    ("ТБ", "TB"),
    ("Установка {version}", "Installing {version}"),
    ("ВНИМАНИЕ", "WARNING"),
    ("ПРОБЛЕМА", "PROBLEM"),
    ("ИНФО", "INFO"),
    ("OK", "OK"),

    // ---- CLI ------------------------------------------------------------
    ("Вердикт: {}", "Verdict: {}"),
    ("\nРекомендуемый EXE: {} ({})", "\nRecommended EXE: {} ({})"),
    ("GPU: {} | драйвер {} | {}", "GPU: {} | driver {} | {}"),
    ("ОС: {}", "OS: {}"),
    ("Последний релиз: {}", "Latest release: {}"),
    ("Последний релиз: {} ({}) {}", "Latest release: {} ({}) {}"),
    ("Ошибка: {}", "Error: {}"),
    ("Поиск игр…", "Looking for games…"),
    ("Игры не найдены.", "No games found."),
    ("Установлено: {}", "Installed: {}"),
    ("Доступно обновление", "Update available"),
    ("Актуально", "Up to date"),
    ("Нет пакета в кэше. Скачайте: dlssgm-cli download", "No package in the cache. Download it: dlssgm-cli download"),
    ("  dlssgm-cli scan <папка игры>", "  dlssgm-cli scan <game folder>"),
    ("  dlssgm-cli import <папка пакета>", "  dlssgm-cli import <package folder>"),
    ("  dlssgm-cli install <папка игры> [прокси]", "  dlssgm-cli install <game folder> [proxy]"),
    ("  dlssgm-cli uninstall <папка игры>", "  dlssgm-cli uninstall <game folder>"),
    ("Импортирован пакет {}: {}", "Imported package {}: {}"),
];
