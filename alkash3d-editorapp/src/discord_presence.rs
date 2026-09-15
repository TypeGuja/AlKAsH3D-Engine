// src/discord_presence.rs
//
// Discord Rich Presence — статус вида "Editing <scene> — N objects" в
// профиле пользователя, пока запущен эдитор и локальный десктоп-клиент
// Discord (см. https://discord.com/developers/docs/rich-presence/how-to).
//
// ВАЖНО: это НЕ настоящий игровой оверлей Discord (чат/войс-панель поверх
// окна) — тот хукается самим клиентом Discord через "Активность в игре" в
// его настройках и кода в принципе не требует. Rich Presence — единственная
// часть статуса "я сейчас в этой программе", которую можно включить кодом.
//
// Application ID берётся из https://discord.com/developers/applications
// (см. `DISCORD_CLIENT_ID` ниже) — у Rich Presence нет "общего" ID, каждое
// приложение регистрирует своё. Если константу когда-нибудь обнулят
// (плейсхолдер "0000000000000000"), `is_configured()` тихо выключает весь
// презенс, а не пытается подключаться с заведомо неверным ID.
//
// Осторожно с локальным десктоп-клиентом: он не всегда запущен, и IPC-
// подключение к нему может пропасть в любой момент (клиент закрылся,
// перезапустился) — всё здесь спроектировано так, чтобы это было тихим
// no-op (лог предупреждением при первом же провале), а не паникой/крашем
// эдитора, тем же принципом "нет данных/связи — не крах", что уже
// применяется во всех converters/al*.rs этого эдитора.

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};

/// Application ID приложения "AlKAsH3D Editor" из Discord Developer Portal.
const DISCORD_CLIENT_ID: &str = "1549297119360327772";

pub struct DiscordPresence {
    client: Option<DiscordIpcClient>,
    start_time: i64,
    last_details: String,
    last_state: String,
    last_connect_attempt: std::time::Instant,
    warned_once: bool,
}

impl DiscordPresence {
    pub fn new() -> Self {
        let start_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let mut presence = Self {
            client: None,
            start_time,
            last_details: String::new(),
            last_state: String::new(),
            // В прошлом — чтобы первая же попытка подключения в try_connect()
            // не была отложена десятисекундным кулдауном ниже.
            last_connect_attempt: std::time::Instant::now() - std::time::Duration::from_secs(60),
            warned_once: false,
        };
        presence.try_connect();
        presence
    }

    fn is_configured() -> bool {
        DISCORD_CLIENT_ID != "0000000000000000"
    }

    /// Пытается поднять IPC-соединение с локальным клиентом Discord.
    /// Кулдаун в 10 секунд между попытками — если Discord не запущен,
    /// незачем долбить именованный пайп каждый кадр.
    fn try_connect(&mut self) {
        if self.client.is_some() || !Self::is_configured() {
            return;
        }
        if self.last_connect_attempt.elapsed() < std::time::Duration::from_secs(10) {
            return;
        }
        self.last_connect_attempt = std::time::Instant::now();

        match DiscordIpcClient::new(DISCORD_CLIENT_ID) {
            Ok(mut client) => {
                if client.connect().is_ok() {
                    self.client = Some(client);
                }
            }
            Err(_) => {}
        }
    }

    /// Обновляет статус, если `details`/`state` реально изменились с
    /// прошлого вызова — вызывающий код (см. `EditorApp::update`) дёшево
    /// зовёт это каждый кадр, а не только на значимых событиях, так что
    /// сравнение здесь обязательно (иначе IPC-сообщение улетало бы 60 раз
    /// в секунду просто так).
    pub fn update(&mut self, details: &str, state: &str) {
        if !Self::is_configured() {
            return;
        }
        self.try_connect();
        let Some(client) = self.client.as_mut() else { return; };
        if details == self.last_details && state == self.last_state {
            return;
        }

        let activity = activity::Activity::new()
            .details(details)
            .state(state)
            .timestamps(activity::Timestamps::new().start(self.start_time));

        if client.set_activity(activity).is_err() {
            // Соединение разорвано (клиент Discord закрылся/перезапустился)
            // — сбрасываем, чтобы try_connect() на следующем вызове
            // попробовал переподключиться с нуля.
            self.client = None;
            if !self.warned_once {
                self.warned_once = true;
                eprintln!("[Discord] Соединение с клиентом Discord потеряно — статус временно не обновляется");
            }
            return;
        }
        self.last_details = details.to_string();
        self.last_state = state.to_string();
    }
}

impl Drop for DiscordPresence {
    fn drop(&mut self) {
        if let Some(client) = self.client.as_mut() {
            let _ = client.close();
        }
    }
}
