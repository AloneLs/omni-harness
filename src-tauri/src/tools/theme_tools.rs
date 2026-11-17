use crate::llm::request_compat::{FunctionSpec, ToolDefinition};
use serde_json::{json, Value};
use tauri::Emitter;

pub fn theme_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionSpec {
                name: "theme_get_dom_spec".to_string(),
                description: "Get the complete DOM selector map (data-omni-*), CSS custom properties, icon names, and text override keys for OmniHarness.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionSpec {
                name: "theme_read_current".to_string(),
                description: "Read the current theme CSS, iconOverrides, textOverrides, and asset names.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionSpec {
                name: "theme_apply_css".to_string(),
                description: "Replace or patch the theme CSS and immediately live-update the preview.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "css": { "type": "string", "description": "Full CSS content to set (if replacing whole CSS)." },
                        "append": { "type": "string", "description": "CSS snippet to append to existing CSS." },
                        "old_str": { "type": "string", "description": "Exact substring in current CSS to replace." },
                        "new_str": { "type": "string", "description": "Replacement CSS substring." }
                    },
                    "required": []
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionSpec {
                name: "theme_set_icon".to_string(),
                description: "Override or reset an SVG icon by name.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Icon name (e.g. send, plus, settings, attach, sidebar, spark, check, copy)." },
                        "svg": { "type": "string", "description": "Full <svg>...</svg> string or SVG path d attribute. Pass empty string to reset." }
                    },
                    "required": ["name", "svg"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionSpec {
                name: "theme_set_texts".to_string(),
                description: "Override UI labels or language strings (e.g. new_task, ask_placeholder, worked_for, working, settings, search_chats).".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "overrides": {
                            "type": "object",
                            "description": "Map of text key to replacement string."
                        }
                    },
                    "required": ["overrides"]
                }),
            },
        },
    ]
}

pub async fn execute_theme_tool(
    app_handle: &tauri::AppHandle,
    theme_id: &str,
    name: &str,
    args: &Value,
) -> Result<String, String> {
    match name {
        "theme_get_dom_spec" => Ok(json!({
            "cssVariables": [
                "--font-sans", "--font-mono",
                "--color-bg-base", "--color-bg-sidebar", "--color-bg-surface",
                "--color-bg-card", "--color-bg-elevated", "--color-bg-hover",
                "--color-border-subtle", "--color-border-default", "--color-border-active",
                "--color-text-primary", "--color-text-secondary", "--color-text-muted",
                "--color-accent-primary"
            ],
            "selectors": [
                "[data-omni-app]", "[data-omni-titlebar]", "[data-omni-main]",
                "[data-omni-sidebar]", "[data-omni-sidebar-header]", "[data-omni-chat-list]", "[data-omni-chat-item]",
                "[data-omni-chat-view]", "[data-omni-messages]", "[data-omni-message='user']", "[data-omni-message='assistant']",
                "[data-omni-message-bubble]", "[data-omni-timeline]", "[data-omni-timeline-header]", "[data-omni-tool-row]",
                "[data-omni-composer]", "[data-omni-composer-box]", "[data-omni-composer-input]", "[data-omni-button='send']",
                "[data-omni-button='new-chat']", "[data-omni-button='settings']", "[data-omni-icon='<name>']"
            ],
            "icons": [
                "send", "stop", "plus", "close", "settings", "sidebar", "attach", "mic",
                "edit", "copy", "check", "refresh", "chevron-down", "chevron-right", "trash",
                "folder", "spark", "terminal", "file", "search"
            ],
            "textKeys": [
                "new_task", "task_list", "search_chats", "ask_placeholder",
                "working", "worked_for", "thought_for", "settings", "send", "stop"
            ],
            "notes": "Use omni-asset://<asset_name> inside url(...) to reference uploaded fonts or images. Keyframes and @font-face are supported."
        }).to_string()),
        "theme_read_current" => {
            let t = crate::themes::load_theme(theme_id)?;
            let asset_names: Vec<String> = t.assets.keys().cloned().collect();
            Ok(json!({
                "id": t.id,
                "name": t.name,
                "css": t.css,
                "iconOverrides": t.icon_overrides,
                "textOverrides": t.text_overrides,
                "assets": asset_names
            }).to_string())
        }
        "theme_apply_css" => {
            let mut t = crate::themes::load_theme(theme_id)?;
            if let Some(css) = args.get("css").and_then(|v| v.as_str()) {
                t.css = css.to_string();
            } else if let (Some(old_s), Some(new_s)) = (
                args.get("old_str").and_then(|v| v.as_str()),
                args.get("new_str").and_then(|v| v.as_str()),
            ) {
                if !t.css.contains(old_s) {
                    return Err("old_str not found in current CSS".to_string());
                }
                t.css = t.css.replacen(old_s, new_s, 1);
            } else if let Some(app_s) = args.get("append").and_then(|v| v.as_str()) {
                if !t.css.is_empty() && !t.css.ends_with('\n') {
                    t.css.push('\n');
                }
                t.css.push_str(app_s);
            } else {
                return Err("Provide 'css', 'append', or 'old_str'+'new_str'".to_string());
            }
            let saved = crate::themes::save_theme(t)?;
            let _ = app_handle.emit("theme-studio-patch", &saved);
            Ok(format!("CSS updated ({} bytes) and applied to live preview.", saved.css.len()))
        }
        "theme_set_icon" => {
            let icon_name = args.get("name").and_then(|v| v.as_str()).ok_or("Missing icon name")?.trim().to_string();
            let svg = args.get("svg").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
            let mut t = crate::themes::load_theme(theme_id)?;
            if svg.is_empty() {
                t.icon_overrides.remove(&icon_name);
            } else {
                t.icon_overrides.insert(icon_name.clone(), svg);
            }
            let saved = crate::themes::save_theme(t)?;
            let _ = app_handle.emit("theme-studio-patch", &saved);
            Ok(format!("Icon '{}' updated.", icon_name))
        }
        "theme_set_texts" => {
            let overrides = args.get("overrides").and_then(|v| v.as_object()).ok_or("Missing overrides object")?;
            let mut t = crate::themes::load_theme(theme_id)?;
            for (k, v) in overrides {
                if let Some(s) = v.as_str() {
                    if s.is_empty() {
                        t.text_overrides.remove(k);
                    } else {
                        t.text_overrides.insert(k.clone(), s.to_string());
                    }
                }
            }
            let saved = crate::themes::save_theme(t)?;
            let _ = app_handle.emit("theme-studio-patch", &saved);
            Ok("Text overrides updated.".to_string())
        }
        other => Err(format!("Unknown theme tool: {}", other)),
    }
}
