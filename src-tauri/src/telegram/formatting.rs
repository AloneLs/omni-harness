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

pub fn char_display_width(c: char) -> usize {
    let u = c as u32;
    if u < 0x20 || (0x7f..0xa0).contains(&u) {
        0
    } else if (0x1100..=0x115f).contains(&u)
        || (0x2329..=0x232a).contains(&u)
        || (0x2e80..=0xa4cf).contains(&u)
        || (0xac00..=0xd7a3).contains(&u)
        || (0xf900..=0xfaff).contains(&u)
        || (0xfe10..=0xfe19).contains(&u)
        || (0xfe30..=0xfe6f).contains(&u)
        || (0xff00..=0xff60).contains(&u)
        || (0xffe0..=0xffe6).contains(&u)
        || (0x1f300..=0x1faff).contains(&u)
        || (0x2600..=0x27bf).contains(&u)
    {
        2
    } else {
        1
    }
}

pub fn str_display_width(s: &str) -> usize {
    s.chars().map(char_display_width).sum()
}

fn pad_left(s: &str, target_width: usize) -> String {
    let cur = str_display_width(s);
    if cur >= target_width {
        return s.to_string();
    }
    let spaces = " ".repeat(target_width - cur);
    format!("{}{}", spaces, s)
}

fn pad_right(s: &str, target_width: usize) -> String {
    let cur = str_display_width(s);
    if cur >= target_width {
        return s.to_string();
    }
    let spaces = " ".repeat(target_width - cur);
    format!("{}{}", s, spaces)
}

fn pad_center(s: &str, target_width: usize) -> String {
    let cur = str_display_width(s);
    if cur >= target_width {
        return s.to_string();
    }
    let total = target_width - cur;
    let left = total / 2;
    let right = total - left;
    format!("{}{}{}", " ".repeat(left), s, " ".repeat(right))
}

fn clean_cell_text(raw: &str) -> String {
    let s = raw.trim();
    s.replace("**", "")
        .replace("__", "")
        .replace("~~", "")
        .replace('`', "")
}

fn split_table_row(line: &str) -> Vec<String> {
    let mut s = line.trim();
    if s.starts_with('|') {
        s = &s[1..];
    }
    if s.ends_with('|') && !s.ends_with("\\|") {
        s = &s[..s.len() - 1];
    }
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for ch in s.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '|' {
            cells.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(ch);
        }
    }
    cells.push(current.trim().to_string());
    cells
}

fn is_table_delimiter_row(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.contains('-') || !trimmed.contains('|') {
        return false;
    }
    let cells = split_table_row(trimmed);
    if cells.is_empty() {
        return false;
    }
    cells.iter().all(|col| {
        let clean = col.trim_matches(':');
        !clean.is_empty() && clean.chars().all(|c| c == '-')
    })
}

fn parse_table_alignment(cell: &str) -> &'static str {
    let s = cell.trim();
    if s.starts_with(':') && s.ends_with(':') {
        "center"
    } else if s.ends_with(':') {
        "right"
    } else {
        "left"
    }
}

fn render_box_table(header: &[String], aligns: &[&str], rows: &[Vec<String>]) -> String {
    let num_cols = header.len();
    if num_cols == 0 {
        return String::new();
    }
    let mut widths: Vec<usize> = header.iter().map(|h| str_display_width(&clean_cell_text(h))).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate().take(num_cols) {
            widths[i] = widths[i].max(str_display_width(&clean_cell_text(cell)));
        }
    }
    for w in &mut widths {
        *w = (*w).max(2);
    }

    let top = format!("┌{}┐", widths.iter().map(|w| "─".repeat(w + 2)).collect::<Vec<_>>().join("┬"));
    let mut header_cells = Vec::new();
    for (i, cell) in header.iter().enumerate() {
        let cleaned = clean_cell_text(cell);
        let padded = pad_center(&cleaned, widths[i]);
        header_cells.push(format!(" {} ", escape_html(&padded)));
    }
    let header_line = format!("│{}│", header_cells.join("│"));
    let mid = format!("├{}┤", widths.iter().map(|w| "─".repeat(w + 2)).collect::<Vec<_>>().join("┼"));

    let mut row_lines = Vec::new();
    for row in rows {
        let mut row_cells = Vec::new();
        for i in 0..num_cols {
            let val = row.get(i).map(|s| s.as_str()).unwrap_or("");
            let cleaned = clean_cell_text(val);
            let align = aligns.get(i).copied().unwrap_or("left");
            let padded = match align {
                "right" => pad_left(&cleaned, widths[i]),
                "center" => pad_center(&cleaned, widths[i]),
                _ => pad_right(&cleaned, widths[i]),
            };
            row_cells.push(format!(" {} ", escape_html(&padded)));
        }
        row_lines.push(format!("│{}│", row_cells.join("│")));
    }

    let bot = format!("└{}┘", widths.iter().map(|w| "─".repeat(w + 2)).collect::<Vec<_>>().join("┴"));

    let mut lines = Vec::new();
    lines.push(top);
    lines.push(header_line);
    lines.push(mid);
    lines.extend(row_lines);
    lines.push(bot);
    format!("<pre><code>{}</code></pre>", lines.join("\n"))
}

fn extract_markdown_tables(
    text: &str,
    placeholders: &mut HashMap<String, String>,
    counter: &mut usize,
) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out_lines: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if i + 1 < lines.len() && line.contains('|') && is_table_delimiter_row(lines[i + 1]) {
            let header = split_table_row(line);
            let delim = split_table_row(lines[i + 1]);
            let aligns: Vec<&str> = delim.iter().map(|d| parse_table_alignment(d)).collect();
            let mut rows = Vec::new();
            i += 2;
            while i < lines.len() && lines[i].contains('|') && !lines[i].trim().is_empty() {
                rows.push(split_table_row(lines[i]));
                i += 1;
            }
            let rendered = render_box_table(&header, &aligns, &rows);
            let key = format!("\x00TG{}\x00", *counter);
            *counter += 1;
            placeholders.insert(key.clone(), rendered);
            out_lines.push(key);
            continue;
        }
        out_lines.push(line.to_string());
        i += 1;
    }
    out_lines.join("\n")
}

fn extract_markdown_blockquotes(
    text: &str,
    placeholders: &mut HashMap<String, String>,
    counter: &mut usize,
) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.trim_start().starts_with('>') {
            let mut quote_lines = Vec::new();
            while i < lines.len() {
                let cur = lines[i].trim_start();
                if cur.starts_with('>') {
                    let stripped = cur.strip_prefix('>').unwrap_or("");
                    let content = stripped.strip_prefix(' ').unwrap_or(stripped);
                    quote_lines.push(content);
                    i += 1;
                } else if !quote_lines.is_empty() && !lines[i].trim().is_empty() && !lines[i].trim().starts_with('#') {
                    quote_lines.push(lines[i].trim());
                    i += 1;
                } else {
                    break;
                }
            }

            let raw_body = quote_lines.join("\n").trim().to_string();
            let mut tag = if quote_lines.len() >= 4 {
                "blockquote expandable"
            } else {
                "blockquote"
            };

            let mut final_body = escape_html(&raw_body);
            let upper = raw_body.to_uppercase();
            for (token, header) in [
                ("[!NOTE]", "ℹ️ <b>Заметка</b>"),
                ("[!TIP]", "💡 <b>Совет</b>"),
                ("[!IMPORTANT]", "📌 <b>Важно</b>"),
                ("[!WARNING]", "⚠️ <b>Внимание</b>"),
                ("[!CAUTION]", "🚨 <b>Осторожно</b>"),
            ] {
                if upper.starts_with(token) {
                    tag = "blockquote expandable";
                    let rest = escape_html(raw_body[token.len()..].trim_start());
                    final_body = if rest.is_empty() {
                        header.to_string()
                    } else {
                        format!("{}\n{}", header, rest)
                    };
                    break;
                }
            }

            let rendered = format!(
                "<{}>{}</{}>",
                tag,
                final_body,
                tag.split_whitespace().next().unwrap_or("blockquote")
            );
            let key = format!("\x00TG{}\x00", *counter);
            *counter += 1;
            placeholders.insert(key.clone(), rendered);
            out.push(key);
            continue;
        }
        out.push(line.to_string());
        i += 1;
    }
    out.join("\n")
}

fn extract_markdown_details(
    text: &str,
    placeholders: &mut HashMap<String, String>,
    counter: &mut usize,
) -> String {
    static DETAILS_RE: OnceLock<Regex> = OnceLock::new();
    let details_re = DETAILS_RE.get_or_init(|| {
        Regex::new(r"(?is)<details[^>]*>\s*<summary>(.*?)</summary>\s*(.*?)\s*</details>").unwrap()
    });
    details_re
        .replace_all(text, |caps: &regex::Captures| {
            let summary = escape_html(caps.get(1).map(|m| m.as_str().trim()).unwrap_or("Подробнее"));
            let body = escape_html(caps.get(2).map(|m| m.as_str().trim()).unwrap_or(""));
            let rendered = format!("<blockquote expandable><b>{}</b>\n{}</blockquote>", summary, body);
            let key = format!("\x00TG{}\x00", *counter);
            *counter += 1;
            placeholders.insert(key.clone(), rendered);
            key
        })
        .to_string()
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

    let with_details = extract_markdown_details(&with_blocks, &mut placeholders, &mut counter);
    let with_tables = extract_markdown_tables(&with_details, &mut placeholders, &mut counter);
    let with_quotes = extract_markdown_blockquotes(&with_tables, &mut placeholders, &mut counter);

    let with_inline = inline_code_re.replace_all(&with_quotes, |caps: &regex::Captures| {
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
