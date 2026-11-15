use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

pub const TELEGRAM_SAFE_LIMIT: usize = 3800;

pub fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn escape_html_attr(value: &str) -> String {
    escape_html(value).replace('"', "&quot;")
}

pub fn compact_text(value: &str, limit: usize) -> String {
    let normalized: String = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let count = normalized.chars().count();
    if count <= limit {
        return normalized;
    }
    let take = limit.saturating_sub(1).max(1);
    let prefix: String = normalized.chars().take(take).collect();
    format!("{}…", prefix.trim_end())
}

pub fn format_user_error(error: &str) -> String {
    static SECRET_RE: OnceLock<Regex> = OnceLock::new();
    let secret_re = SECRET_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(api[_-]?key|authorization|bearer|token|password|secret)\b\s*[:=]?\s*[^\s,;]+")
            .unwrap()
    });
    let mut clean = secret_re.replace_all(error, "$1: [hidden]").to_string();
    clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.is_empty() {
        clean = "Request failed".to_string();
    }
    if clean.chars().count() > 240 {
        let prefix: String = clean.chars().take(237).collect();
        clean = format!("{}...", prefix.trim_end());
    }
    format!("Ошибка: {}", clean)
}

pub fn telegram_html_to_plain_text(value: &str) -> String {
    static TAG_RE: OnceLock<Regex> = OnceLock::new();
    let tag_re = TAG_RE.get_or_init(|| Regex::new(r"<[^>]*>").unwrap());
    let plain = tag_re
        .replace_all(value, "")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&");
    let trimmed = plain.trim();
    if trimmed.is_empty() {
        "—".to_string()
    } else {
        trimmed.to_string()
    }
}

fn is_safe_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("tg://")
}

pub fn markdown_to_telegram_html(text: &str) -> String {
    static CODE_BLOCK_RE: OnceLock<Regex> = OnceLock::new();
    static INLINE_CODE_RE: OnceLock<Regex> = OnceLock::new();
    static LINK_RE: OnceLock<Regex> = OnceLock::new();
    static HEADER_RE: OnceLock<Regex> = OnceLock::new();
    static HR_RE: OnceLock<Regex> = OnceLock::new();
    static CHECK_X_RE: OnceLock<Regex> = OnceLock::new();
    static CHECK_EMPTY_RE: OnceLock<Regex> = OnceLock::new();
    static BULLET_RE: OnceLock<Regex> = OnceLock::new();
    static SPOILER_RE: OnceLock<Regex> = OnceLock::new();
    static BOLD_AST_RE: OnceLock<Regex> = OnceLock::new();
    static BOLD_UND_RE: OnceLock<Regex> = OnceLock::new();
    static STRIKE_RE: OnceLock<Regex> = OnceLock::new();
    static ITALIC_AST_RE: OnceLock<Regex> = OnceLock::new();
    static MULTI_NL_RE: OnceLock<Regex> = OnceLock::new();

    let code_block_re =
        CODE_BLOCK_RE.get_or_init(|| Regex::new(r"```([^\n`]*)\n([\s\S]*?)```").unwrap());
    let inline_code_re = INLINE_CODE_RE.get_or_init(|| Regex::new(r"`([^`\n]+)`").unwrap());
    let link_re = LINK_RE.get_or_init(|| Regex::new(r"\[([^\]\n]+)\]\(([^)\s]+)\)").unwrap());
    let header_re = HEADER_RE.get_or_init(|| Regex::new(r"(?m)^\s{0,3}#{1,6}\s+(.+?)\s*$").unwrap());
    let hr_re = HR_RE.get_or_init(|| Regex::new(r"(?m)^\s{0,3}(?:[-*_]\s*){3,}$").unwrap());
    let check_x_re = CHECK_X_RE.get_or_init(|| Regex::new(r"(?mi)^\s{0,3}[-+*]\s+\[x\]\s+").unwrap());
    let check_empty_re =
        CHECK_EMPTY_RE.get_or_init(|| Regex::new(r"(?m)^\s{0,3}[-+*]\s+\[ \]\s+").unwrap());
    let bullet_re = BULLET_RE.get_or_init(|| Regex::new(r"(?m)^\s{0,3}[-+*]\s+").unwrap());
    let spoiler_re = SPOILER_RE.get_or_init(|| Regex::new(r"\|\|([^|\n]+)\|\|").unwrap());
    let bold_ast_re = BOLD_AST_RE.get_or_init(|| Regex::new(r"\*\*([^\n]+?)\*\*").unwrap());
    let bold_und_re = BOLD_UND_RE.get_or_init(|| Regex::new(r"__([^\n]+?)__").unwrap());
    let strike_re = STRIKE_RE.get_or_init(|| Regex::new(r"~~([^\n]+?)~~").unwrap());
    let italic_ast_re = ITALIC_AST_RE.get_or_init(|| Regex::new(r"\*([^*\n]+)\*").unwrap());
    let multi_nl_re = MULTI_NL_RE.get_or_init(|| Regex::new(r"\n{3,}").unwrap());

    let cleaned: String = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|&c| c == '\n' || c == '\t' || (c >= ' ' && c != '\x7f'))
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return "—".to_string();
    }

    let mut placeholders: HashMap<String, String> = HashMap::new();
    let mut counter = 0usize;

    let with_blocks = code_block_re.replace_all(trimmed, |caps: &regex::Captures| {
        let lang_raw = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
        let lang: String = lang_raw
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '.' | '-'))
            .take(32)
            .collect();
        let code = escape_html(caps.get(2).map(|m| m.as_str()).unwrap_or("").trim_matches('\n'));
        let rendered = if lang.is_empty() {
            format!("<pre><code>{}</code></pre>", code)
        } else {
            format!("<pre><code class=\"language-{}\">{}</code></pre>", lang, code)
        };
        let key = format!("\x00TG{}\x00", counter);
        counter += 1;
        placeholders.insert(key.clone(), rendered);
        key
    });

    let with_inline = inline_code_re.replace_all(&with_blocks, |caps: &regex::Captures| {
        let code = escape_html(caps.get(1).map(|m| m.as_str()).unwrap_or(""));
        let rendered = format!("<code>{}</code>", code);
        let key = format!("\x00TG{}\x00", counter);
        counter += 1;
        placeholders.insert(key.clone(), rendered);
        key
    });

    let mut escaped = escape_html(&with_inline);

    escaped = link_re
        .replace_all(&escaped, |caps: &regex::Captures| {
            let label = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let raw_url = caps
                .get(2)
                .map(|m| m.as_str())
                .unwrap_or("")
                .replace("&amp;", "&");
            if is_safe_url(&raw_url) {
                let rendered = format!("<a href=\"{}\">{}</a>", escape_html_attr(&raw_url), label);
                let key = format!("\x00TG{}\x00", counter);
                counter += 1;
                placeholders.insert(key.clone(), rendered);
                key
            } else {
                label.to_string()
            }
        })
        .to_string();

    escaped = header_re.replace_all(&escaped, "<b>$1</b>").to_string();
    escaped = hr_re.replace_all(&escaped, "────────").to_string();
    escaped = check_x_re.replace_all(&escaped, "☑ ").to_string();
    escaped = check_empty_re.replace_all(&escaped, "☐ ").to_string();
    escaped = bullet_re.replace_all(&escaped, "• ").to_string();
    escaped = spoiler_re
        .replace_all(&escaped, "<tg-spoiler>$1</tg-spoiler>")
        .to_string();
    escaped = bold_ast_re.replace_all(&escaped, "<b>$1</b>").to_string();
    escaped = bold_und_re.replace_all(&escaped, "<b>$1</b>").to_string();
    escaped = strike_re.replace_all(&escaped, "<s>$1</s>").to_string();
    escaped = italic_ast_re.replace_all(&escaped, "<i>$1</i>").to_string();

    for (key, rendered) in &placeholders {
        escaped = escaped.replace(key, rendered);
    }

    escaped = multi_nl_re.replace_all(&escaped, "\n\n").trim().to_string();
    if escaped.is_empty() {
        "—".to_string()
    } else {
        escaped
    }
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

fn char_index_for_utf16_limit(s: &str, limit: usize) -> usize {
    let mut used = 0usize;
    for (idx, ch) in s.char_indices() {
        used += ch.len_utf16();
        if used > limit {
            return idx;
        }
    }
    s.len()
}

fn split_plain_segment(value: &str, limit: usize) -> Vec<String> {
    if utf16_len(value) <= limit {
        return vec![value.to_string()];
    }
    let mut chunks = Vec::new();
    let mut rem = value;
    while utf16_len(rem) > limit {
        let hard_cut = char_index_for_utf16_limit(rem, limit).max(1);
        let mut cut = rem[..hard_cut]
            .rfind('\n')
            .filter(|&p| p >= hard_cut / 2)
            .or_else(|| rem[..hard_cut].rfind(' ').filter(|&p| p >= hard_cut / 2))
            .unwrap_or(hard_cut);
        while !rem.is_char_boundary(cut) {
            cut -= 1;
        }
        if cut == 0 {
            cut = rem.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        }
        chunks.push(rem[..cut].to_string());
        rem = rem[cut..].trim_start_matches(['\n', ' ']);
    }
    if !rem.is_empty() {
        chunks.push(rem.to_string());
    }
    chunks
}

fn parse_supported_tag(token: &str) -> Option<(String, bool)> {
    const SUPPORTED: &[&str] = &[
        "b",
        "strong",
        "i",
        "em",
        "u",
        "ins",
        "s",
        "strike",
        "del",
        "code",
        "pre",
        "a",
        "blockquote",
        "tg-spoiler",
    ];
    if let Some(inner) = token.strip_prefix("</").and_then(|s| s.strip_suffix('>')) {
        let name = inner.trim().to_ascii_lowercase();
        if SUPPORTED.contains(&name.as_str()) {
            return Some((name, true));
        }
        return None;
    }
    if let Some(inner) = token.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
        let name = inner
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if SUPPORTED.contains(&name.as_str()) {
            return Some((name, false));
        }
    }
    None
}

pub fn split_telegram_html(text: &str, limit: usize) -> Vec<String> {
    static TOKEN_RE: OnceLock<Regex> = OnceLock::new();
    let token_re = TOKEN_RE.get_or_init(|| {
        Regex::new(r"(<[^>]+>|&(?:#[0-9]+|#x[0-9a-fA-F]+|[a-zA-Z][a-zA-Z0-9]+);)").unwrap()
    });

    let trimmed = text.trim();
    let value = if trimmed.is_empty() { "—" } else { trimmed };
    if utf16_len(value) <= limit {
        return vec![value.to_string()];
    }

    let mut tokens: Vec<&str> = Vec::new();
    let mut last = 0usize;
    for m in token_re.find_iter(value) {
        if m.start() > last {
            tokens.push(&value[last..m.start()]);
        }
        tokens.push(m.as_str());
        last = m.end();
    }
    if last < value.len() {
        tokens.push(&value[last..]);
    }

    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut open_tags: Vec<(String, String)> = Vec::new();

    let closing_suffix = |tags: &[(String, String)]| -> String {
        tags.iter()
            .rev()
            .map(|(n, _)| format!("</{}>", n))
            .collect()
    };
    let opening_prefix = |tags: &[(String, String)]| -> String {
        tags.iter().map(|(_, raw)| raw.as_str()).collect()
    };

    for token in tokens {
        if let Some((name, is_close)) = parse_supported_tag(token) {
            let suffix = closing_suffix(&open_tags);
            let projected = utf16_len(&current) + utf16_len(token) + utf16_len(&suffix);
            let prefix_len = utf16_len(&opening_prefix(&open_tags));
            if projected > limit && !current.trim().is_empty() && utf16_len(&current) > prefix_len {
                let body = format!("{}{}", current, suffix).trim().to_string();
                if !body.is_empty() {
                    chunks.push(body);
                }
                current = opening_prefix(&open_tags);
            }
            current.push_str(token);
            if is_close {
                if let Some(pos) = open_tags.iter().rposition(|(n, _)| n == &name) {
                    open_tags.remove(pos);
                }
            } else {
                open_tags.push((name, token.to_string()));
            }
            continue;
        }

        let suffix = closing_suffix(&open_tags);
        let available = limit
            .saturating_sub(utf16_len(&current))
            .saturating_sub(utf16_len(&suffix))
            .max(64);
        let pieces = split_plain_segment(token, available);
        for (idx, piece) in pieces.iter().enumerate() {
            let cur_suffix = closing_suffix(&open_tags);
            let projected = utf16_len(&current) + utf16_len(piece) + utf16_len(&cur_suffix);
            if (idx > 0 || projected > limit) && !current.trim().is_empty() {
                let body = format!("{}{}", current, cur_suffix).trim().to_string();
                if !body.is_empty() {
                    chunks.push(body);
                }
                current = opening_prefix(&open_tags);
            }
            current.push_str(piece);
        }
    }

    let tail = format!("{}{}", current, closing_suffix(&open_tags))
        .trim()
        .to_string();
    if !tail.is_empty() {
        chunks.push(tail);
    }
    if chunks.is_empty() {
        vec!["—".to_string()]
    } else {
        chunks
    }
}
