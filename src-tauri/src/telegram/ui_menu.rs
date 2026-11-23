use crate::telegram::formatting::compact_text;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineKeyboardButton {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callback_data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl InlineKeyboardButton {
    pub fn callback(text: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            callback_data: Some(data.into()),
            url: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineKeyboardMarkup {
    pub inline_keyboard: Vec<Vec<InlineKeyboardButton>>,
}

pub struct TelegramMenuBuilder;

impl TelegramMenuBuilder {
    pub fn main_menu() -> InlineKeyboardMarkup {
        InlineKeyboardMarkup {
            inline_keyboard: vec![
                vec![
                    InlineKeyboardButton::callback("⚡ Новая сессия", "tg:cmd:new"),
                    InlineKeyboardButton::callback("💬 Сессии", "tg:cmd:chats"),
                ],
                vec![
                    InlineKeyboardButton::callback("📁 Проекты", "tg:cmd:projects"),
                    InlineKeyboardButton::callback("📊 Статус", "tg:cmd:status"),
                ],
                vec![
                    InlineKeyboardButton::callback("🔌 Провайдеры", "tg:cmd:providers"),
                    InlineKeyboardButton::callback("🤖 Модели", "tg:cmd:models"),
                ],
                vec![
                    InlineKeyboardButton::callback("⚙️ Настройки", "tg:cmd:settings"),
                    InlineKeyboardButton::callback("🛑 Стоп", "tg:cmd:stop"),
                ],
            ],
        }
    }

    pub fn session_menu(is_running: bool, has_chat: bool) -> InlineKeyboardMarkup {
        let mut rows = Vec::new();
        if is_running {
            rows.push(vec![
                InlineKeyboardButton::callback("🛑 Остановить", "tg:cmd:stop"),
                InlineKeyboardButton::callback("🔄 Обновить", "tg:cmd:status"),
            ]);
            rows.push(vec![
                InlineKeyboardButton::callback("⚡ Новая сессия", "tg:cmd:new"),
                InlineKeyboardButton::callback("💬 Все сессии", "tg:cmd:chats"),
            ]);
        } else {
            rows.push(vec![
                InlineKeyboardButton::callback("⚡ Новая сессия", "tg:cmd:new"),
                InlineKeyboardButton::callback("💬 Все сессии", "tg:cmd:chats"),
            ]);
        }
        if has_chat && !is_running {
            rows.push(vec![
                InlineKeyboardButton::callback("📜 История", "tg:cmd:history"),
                InlineKeyboardButton::callback("✏️ Имя", "tg:cmd:rename"),
                InlineKeyboardButton::callback("🗑 Удалить", "tg:cmd:delete"),
            ]);
        }
        rows.push(vec![
            InlineKeyboardButton::callback("📁 Проект", "tg:cmd:projects"),
            InlineKeyboardButton::callback("🤖 Модель", "tg:cmd:models"),
            InlineKeyboardButton::callback("⬅️ Меню", "tg:cmd:menu"),
        ]);
        InlineKeyboardMarkup {
            inline_keyboard: rows,
        }
    }

    pub fn projects_menu(
        items: &[(usize, String, bool)],
        choice_token: &str,
    ) -> InlineKeyboardMarkup {
        let mut rows = Vec::new();
        for (idx, name, is_selected) in items {
            let short = compact_text(name, 34);
            let label = if *is_selected {
                format!("✓ {}", short)
            } else {
                short
            };
            rows.push(vec![InlineKeyboardButton::callback(
                label,
                format!("tg:proj:{}:{}", choice_token, idx),
            )]);
        }
        rows.push(vec![
            InlineKeyboardButton::callback("Без проекта", "tg:proj-clear"),
            InlineKeyboardButton::callback("⬅️ Назад", "tg:cmd:menu"),
        ]);
        InlineKeyboardMarkup {
            inline_keyboard: rows,
        }
    }

    pub fn chats_menu(
        items: &[(usize, String, bool)],
        choice_token: &str,
        page: usize,
        page_count: usize,
    ) -> InlineKeyboardMarkup {
        let mut rows = Vec::new();
        for (idx, title, is_selected) in items {
            let short = compact_text(title, 34);
            let label = if *is_selected {
                format!("✓ {}", short)
            } else {
                short
            };
            rows.push(vec![InlineKeyboardButton::callback(
                label,
                format!("tg:chat:{}:{}", choice_token, idx),
            )]);
        }
        let mut nav = Vec::new();
        if page > 0 {
            nav.push(InlineKeyboardButton::callback(
                "←",
                format!("tg:chats-page:{}", page - 1),
            ));
        }
        if page + 1 < page_count {
            nav.push(InlineKeyboardButton::callback(
                "→",
                format!("tg:chats-page:{}", page + 1),
            ));
        }
        if !nav.is_empty() {
            rows.push(nav);
        }
        rows.push(vec![
            InlineKeyboardButton::callback("⚡ Новая сессия", "tg:cmd:new"),
            InlineKeyboardButton::callback("⬅️ Назад", "tg:cmd:menu"),
        ]);
        InlineKeyboardMarkup {
            inline_keyboard: rows,
        }
    }

    pub fn providers_menu(
        items: &[(usize, String, bool)],
        choice_token: &str,
        page: usize,
        page_count: usize,
    ) -> InlineKeyboardMarkup {
        let mut rows = Vec::new();
        for (idx, name, is_selected) in items {
            let short = compact_text(name, 34);
            let label = if *is_selected {
                format!("✓ {}", short)
            } else {
                short
            };
            rows.push(vec![InlineKeyboardButton::callback(
                label,
                format!("tg:prov:{}:{}", choice_token, idx),
            )]);
        }
        let mut nav = Vec::new();
        if page > 0 {
            nav.push(InlineKeyboardButton::callback(
                "←",
                format!("tg:providers-page:{}", page - 1),
            ));
        }
        if page + 1 < page_count {
            nav.push(InlineKeyboardButton::callback(
                "→",
                format!("tg:providers-page:{}", page + 1),
            ));
        }
        if !nav.is_empty() {
            rows.push(nav);
        }
        rows.push(vec![
            InlineKeyboardButton::callback("➕ Провайдер", "tg:prov-add"),
            InlineKeyboardButton::callback("✏️ Изменить", "tg:prov-edit"),
            InlineKeyboardButton::callback("➕ Модель", "tg:model-add"),
        ]);
        rows.push(vec![InlineKeyboardButton::callback("⬅️ Назад", "tg:cmd:settings")]);
        InlineKeyboardMarkup {
            inline_keyboard: rows,
        }
    }

    pub fn models_menu(
        items: &[(usize, String, bool)],
        choice_token: &str,
        page: usize,
        page_count: usize,
    ) -> InlineKeyboardMarkup {
        let mut rows = Vec::new();
        for (idx, name, is_selected) in items {
            let short = compact_text(name, 34);
            let label = if *is_selected {
                format!("✓ {}", short)
            } else {
                short
            };
            rows.push(vec![InlineKeyboardButton::callback(
                label,
                format!("tg:model:{}:{}", choice_token, idx),
            )]);
        }
        let mut nav = Vec::new();
        if page > 0 {
            nav.push(InlineKeyboardButton::callback(
                "←",
                format!("tg:models-page:{}", page - 1),
            ));
        }
        if page + 1 < page_count {
            nav.push(InlineKeyboardButton::callback(
                "→",
                format!("tg:models-page:{}", page + 1),
            ));
        }
        if !nav.is_empty() {
            rows.push(nav);
        }
        rows.push(vec![
            InlineKeyboardButton::callback("🔌 Провайдер", "tg:cmd:providers"),
            InlineKeyboardButton::callback("⬅️ Назад", "tg:cmd:settings"),
        ]);
        InlineKeyboardMarkup {
            inline_keyboard: rows,
        }
    }

    pub fn settings_menu(reasoning: &str) -> InlineKeyboardMarkup {
        InlineKeyboardMarkup {
            inline_keyboard: vec![
                vec![
                    InlineKeyboardButton::callback("🔌 Провайдер", "tg:cmd:providers"),
                    InlineKeyboardButton::callback("🤖 Модель", "tg:cmd:models"),
                ],
                vec![InlineKeyboardButton::callback(
                    format!("🧠 Reasoning: {}", reasoning),
                    "tg:cmd:reasoning",
                )],
                vec![
                    InlineKeyboardButton::callback("🔄 Сбросить pairing", "tg:action:reset_pairing"),
                    InlineKeyboardButton::callback("🛑 Стоп все", "tg:action:abort_all"),
                ],
                vec![InlineKeyboardButton::callback("⬅️ Меню", "tg:cmd:menu")],
            ],
        }
    }

    pub fn reasoning_menu(current: &str) -> InlineKeyboardMarkup {
        let levels = ["low", "medium", "high"];
        let row = levels
            .into_iter()
            .map(|lvl| {
                let label = if lvl == current {
                    format!("✓ {}", lvl)
                } else {
                    lvl.to_string()
                };
                InlineKeyboardButton::callback(label, format!("tg:reasoning:{}", lvl))
            })
            .collect();
        InlineKeyboardMarkup {
            inline_keyboard: vec![
                row,
                vec![InlineKeyboardButton::callback("⬅️ Назад", "tg:cmd:settings")],
            ],
        }
    }

    pub fn confirm_delete_menu(choice_token: &str) -> InlineKeyboardMarkup {
        InlineKeyboardMarkup {
            inline_keyboard: vec![vec![
                InlineKeyboardButton::callback(
                    "🗑 Удалить",
                    format!("tg:delete-chat:{}:0", choice_token),
                ),
                InlineKeyboardButton::callback("Отмена", "tg:cmd:status"),
            ]],
        }
    }

    pub fn history_menu(page: usize, page_count: usize) -> InlineKeyboardMarkup {
        let mut rows = Vec::new();
        if page_count > 1 {
            let mut nav = Vec::new();
            if page > 0 {
                nav.push(InlineKeyboardButton::callback("◀", format!("tg:hist:page:{}", page - 1)));
            } else {
                nav.push(InlineKeyboardButton::callback("·", "tg:hist:noop"));
            }
            nav.push(InlineKeyboardButton::callback(format!("{}/{}", page + 1, page_count), "tg:hist:noop"));
            if page + 1 < page_count {
                nav.push(InlineKeyboardButton::callback("▶", format!("tg:hist:page:{}", page + 1)));
            } else {
                nav.push(InlineKeyboardButton::callback("·", "tg:hist:noop"));
            }
            rows.push(nav);
        }
        rows.push(vec![
            InlineKeyboardButton::callback("📋 Сессия", "tg:cmd:status"),
            InlineKeyboardButton::callback("💬 Все чаты", "tg:cmd:chats"),
            InlineKeyboardButton::callback("⬅️ Меню", "tg:cmd:menu"),
        ]);
        InlineKeyboardMarkup { inline_keyboard: rows }
    }

    pub fn back_menu() -> InlineKeyboardMarkup {
        InlineKeyboardMarkup {
            inline_keyboard: vec![vec![InlineKeyboardButton::callback("⬅️ Меню", "tg:cmd:menu")]],
        }
    }
}
