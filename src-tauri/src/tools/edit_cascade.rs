pub fn normalize_unicode_ws(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\u{FEFF}' | '\u{200B}' | '\u{200C}' | '\u{200D}' => {}
            '\u{00A0}' | '\u{1680}' | '\u{202F}' | '\u{205F}' | '\u{3000}' => out.push(' '),
            '\u{2000}'..='\u{200A}' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

fn parse_line_prefix(line: &str) -> Option<&str> {
    let trimmed = line.trim_start_matches(' ');
    if let Some(rest) = trimmed.strip_prefix('L').or_else(|| trimmed.strip_prefix('l')) {
        let digits_len = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
        if (1..=7).contains(&digits_len) {
            let after_digits = &rest[digits_len..];
            if let Some(after_colon) = after_digits.strip_prefix(':') {
                return Some(after_colon.strip_prefix(' ').unwrap_or(after_colon));
            }
        }
    }
    let digits_len = trimmed.bytes().take_while(|b| b.is_ascii_digit()).count();
    if (1..=7).contains(&digits_len) {
        let after_digits = trimmed[digits_len..].trim_start_matches(' ');
        if let Some(after_pipe) = after_digits.strip_prefix('|') {
            return Some(after_pipe.strip_prefix(' ').unwrap_or(after_pipe));
        }
    }
    None
}

fn maybe_strip_line_numbers(text: &str) -> (String, bool) {
    let lines: Vec<&str> = text.split('\n').collect();
    let non_empty: Vec<&&str> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    if non_empty.is_empty() {
        return (text.to_string(), false);
    }
    if non_empty.iter().all(|l| parse_line_prefix(l).is_some()) {
        let stripped: Vec<String> = lines
            .into_iter()
            .map(|l| {
                if l.trim().is_empty() {
                    String::new()
                } else {
                    parse_line_prefix(l).unwrap_or(l).to_string()
                }
            })
            .collect();
        return (stripped.join("\n"), true);
    }
    (text.to_string(), false)
}

fn split_preserve_lines(s: &str) -> (Vec<String>, bool) {
    let has_trailing = s.ends_with('\n');
    let body = if has_trailing { &s[..s.len() - 1] } else { s };
    if body.is_empty() {
        return (Vec::new(), has_trailing);
    }
    (body.split('\n').map(|l| l.to_string()).collect(), has_trailing)
}

fn join_preserve_lines(lines: &[String], is_crlf: bool, has_trailing: bool) -> String {
    let sep = if is_crlf { "\r\n" } else { "\n" };
    if lines.is_empty() {
        return String::new();
    }
    let mut out = lines.join(sep);
    if has_trailing {
        out.push_str(sep);
    }
    out
}

fn indent_cols(line: &str) -> usize {
    let mut cols = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => cols += 1,
            '\t' => cols += 4,
            _ => break,
        }
    }
    cols
}

fn strip_indent_cols(line: &str, cols_to_strip: usize) -> &str {
    let mut cols = 0usize;
    let mut byte_idx = 0usize;
    for (idx, ch) in line.char_indices() {
        if cols >= cols_to_strip {
            return &line[idx..];
        }
        match ch {
            ' ' => {
                cols += 1;
                byte_idx = idx + ch.len_utf8();
            }
            '\t' => {
                cols += 4;
                byte_idx = idx + ch.len_utf8();
            }
            _ => return &line[idx..],
        }
    }
    &line[byte_idx..]
}

fn make_indent(cols: usize, use_tabs: bool) -> String {
    if cols == 0 {
        return String::new();
    }
    if use_tabs {
        let tabs = cols / 4;
        let spaces = cols % 4;
        let mut s = String::with_capacity(tabs + spaces);
        for _ in 0..tabs {
            s.push('\t');
        }
        for _ in 0..spaces {
            s.push(' ');
        }
        s
    } else {
        " ".repeat(cols)
    }
}

fn reindent_replacement(src_slice: &[String], old_lines: &[String], new_lines: &[String]) -> Vec<String> {
    if new_lines.is_empty() {
        return Vec::new();
    }
    let first_src_nonempty = src_slice.iter().find(|l| !l.trim().is_empty());
    let use_tabs = first_src_nonempty
        .map(|l| l.starts_with('\t'))
        .unwrap_or(false);
    let src_first_ws: String = src_slice
        .first()
        .map(|l| l.chars().take_while(|c| *c == ' ' || *c == '\t').collect())
        .unwrap_or_default();

    let first_old_is_zero = old_lines
        .first()
        .map(|l| !l.trim().is_empty() && indent_cols(l) == 0)
        .unwrap_or(false);

    if first_old_is_zero && old_lines.len() > 1 && src_slice.len() > 1 {
        let old_rest_min = old_lines[1..]
            .iter()
            .filter(|l| !l.trim().is_empty())
            .map(|l| indent_cols(l))
            .min();
        let src_rest_min = src_slice[1..]
            .iter()
            .filter(|l| !l.trim().is_empty())
            .map(|l| indent_cols(l))
            .min();
        if let (Some(o_min), Some(s_min)) = (old_rest_min, src_rest_min) {
            let delta = s_min as isize - o_min as isize;
            return new_lines
                .iter()
                .enumerate()
                .map(|(idx, line)| {
                    if line.trim().is_empty() {
                        return String::new();
                    }
                    let cur = indent_cols(line);
                    if idx == 0 && cur == 0 {
                        format!("{}{}", src_first_ws, line.trim_start_matches([' ', '\t']))
                    } else {
                        let target_cols = (cur as isize + delta).max(0) as usize;
                        let body = strip_indent_cols(line, cur);
                        format!("{}{}", make_indent(target_cols, use_tabs), body)
                    }
                })
                .collect();
        }
    }

    let old_min = old_lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| indent_cols(l))
        .min()
        .unwrap_or(0);
    let src_min = src_slice
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| indent_cols(l))
        .min()
        .unwrap_or(0);
    let delta = src_min as isize - old_min as isize;

    new_lines
        .iter()
        .map(|line| {
            if line.trim().is_empty() {
                return String::new();
            }
            let cur = indent_cols(line);
            let target_cols = (cur as isize + delta).max(0) as usize;
            let body = strip_indent_cols(line, cur);
            format!("{}{}", make_indent(target_cols, use_tabs), body)
        })
        .collect()
}

#[derive(Clone)]
struct LexToken {
    text: String,
    byte_start: usize,
    byte_end: usize,
}

fn tokenize_with_spans(s: &str) -> Vec<LexToken> {
    let mut tokens = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80 {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] >= 0x80) {
                i += 1;
            }
            tokens.push(LexToken {
                text: s[start..i].to_string(),
                byte_start: start,
                byte_end: i,
            });
        } else {
            let start = i;
            i += 1;
            if i < bytes.len() {
                let pair = &bytes[start..=i];
                if matches!(
                    pair,
                    b"==" | b"!=" | b"<=" | b">=" | b"&&" | b"||" | b"->" | b"=>" | b"::" | b"+=" | b"-="
                ) {
                    i += 1;
                }
            }
            tokens.push(LexToken {
                text: s[start..i].to_string(),
                byte_start: start,
                byte_end: i,
            });
        }
    }
    tokens
}

fn token_sequence_similarity(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a.len() > 600 || b.len() > 600 {
        let mut a_bi = std::collections::HashMap::<(&str, &str), usize>::new();
        for w in a.windows(2) {
            *a_bi.entry((w[0].as_str(), w[1].as_str())).or_insert(0) += 1;
        }
        let mut overlap = 0usize;
        for w in b.windows(2) {
            if let Some(cnt) = a_bi.get_mut(&(w[0].as_str(), w[1].as_str())) {
                if *cnt > 0 {
                    *cnt -= 1;
                    overlap += 1;
                }
            }
        }
        let denom = a.len().saturating_sub(1) + b.len().saturating_sub(1);
        return if denom == 0 { 0.0 } else { (2.0 * overlap as f64) / denom as f64 };
    }
    let mut prev = vec![0usize; b.len() + 1];
    let mut curr = vec![0usize; b.len() + 1];
    for ta in a {
        for (j, tb) in b.iter().enumerate() {
            curr[j + 1] = if ta == tb {
                prev[j] + 1
            } else {
                prev[j + 1].max(curr[j])
            };
        }
        std::mem::swap(&mut prev, &mut curr);
        curr.fill(0);
    }
    let lcs = prev[b.len()];
    (2.0 * lcs as f64) / ((a.len() + b.len()) as f64)
}

pub fn apply_single_edit_cascade(
    source: &str,
    old_text: &str,
    new_text: &str,
    replace_all: bool,
) -> Result<(String, usize), String> {
    if old_text.is_empty() {
        return Err("old_text cannot be empty".to_string());
    }

    if source.contains(old_text) {
        let match_count = source.matches(old_text).count();
        if !replace_all && match_count > 1 {
            return Err(format!(
                "Ambiguous edit: old_text matches {} locations in file. Provide more unique context or set replace_all=true.",
                match_count
            ));
        }
        let out = if replace_all {
            source.replace(old_text, new_text)
        } else {
            source.replacen(old_text, new_text, 1)
        };
        return Ok((out, if replace_all { match_count } else { 1 }));
    }

    let is_crlf = source.contains("\r\n");
    let norm_src = source.replace("\r\n", "\n");
    let raw_norm_old = old_text.replace("\r\n", "\n");
    let raw_norm_new = new_text.replace("\r\n", "\n");

    if norm_src.contains(&raw_norm_old) {
        let match_count = norm_src.matches(&raw_norm_old).count();
        if !replace_all && match_count > 1 {
            return Err(format!(
                "Ambiguous edit: old_text matches {} locations in file. Provide more unique context or set replace_all=true.",
                match_count
            ));
        }
        let out = if replace_all {
            norm_src.replace(&raw_norm_old, &raw_norm_new)
        } else {
            norm_src.replacen(&raw_norm_old, &raw_norm_new, 1)
        };
        let restored = if is_crlf { out.replace('\n', "\r\n") } else { out };
        return Ok((restored, if replace_all { match_count } else { 1 }));
    }

    let (stripped_old, stripped_flag) = maybe_strip_line_numbers(&raw_norm_old);
    let stripped_new = if stripped_flag {
        maybe_strip_line_numbers(&raw_norm_new).0
    } else {
        raw_norm_new.clone()
    };

    let clean_s = normalize_unicode_ws(&norm_src);
    let clean_o = normalize_unicode_ws(&stripped_old);
    let clean_n = stripped_new.clone();

    if !clean_o.is_empty() && clean_s.contains(&clean_o) {
        let match_count = clean_s.matches(&clean_o).count();
        if !replace_all && match_count > 1 {
            return Err(format!(
                "Ambiguous edit: old_text matches {} locations in file. Provide more unique context or set replace_all=true.",
                match_count
            ));
        }
        let out = if replace_all {
            clean_s.replace(&clean_o, &clean_n)
        } else {
            clean_s.replacen(&clean_o, &clean_n, 1)
        };
        let restored = if is_crlf { out.replace('\n', "\r\n") } else { out };
        return Ok((restored, if replace_all { match_count } else { 1 }));
    }

    let esc_o = raw_norm_old.replace('\n', "\\n");
    if esc_o != raw_norm_old && norm_src.contains(&esc_o) {
        let match_count = norm_src.matches(&esc_o).count();
        if !replace_all && match_count > 1 {
            return Err(format!(
                "Ambiguous edit: old_text matches {} locations in file. Provide more unique context or set replace_all=true.",
                match_count
            ));
        }
        let esc_n = raw_norm_new.replace('\n', "\\n");
        let out = if replace_all {
            norm_src.replace(&esc_o, &esc_n)
        } else {
            norm_src.replacen(&esc_o, &esc_n, 1)
        };
        let restored = if is_crlf { out.replace('\n', "\r\n") } else { out };
        return Ok((restored, if replace_all { match_count } else { 1 }));
    }

    let (mut src_lines, has_trailing_nl) = split_preserve_lines(&norm_src);
    let (old_lines, _) = split_preserve_lines(&clean_o);
    let (new_lines, _) = split_preserve_lines(&clean_n);

    if !old_lines.is_empty() && src_lines.len() >= old_lines.len() {
        let src_rtrim: Vec<String> = src_lines
            .iter()
            .map(|l| normalize_unicode_ws(l).trim_end().to_string())
            .collect();
        let old_rtrim: Vec<String> = old_lines.iter().map(|l| l.trim_end().to_string()).collect();

        let mut hits = Vec::new();
        for i in 0..=src_rtrim.len() - old_rtrim.len() {
            if src_rtrim[i..i + old_rtrim.len()] == old_rtrim[..] {
                hits.push(i);
            }
        }
        if hits.len() > 1 && !replace_all {
            return Err(format!(
                "Ambiguous edit: old_text matches {} locations in file. Provide more unique context or set replace_all=true.",
                hits.len()
            ));
        }
        if !hits.is_empty() {
            let targets: &[usize] = if replace_all { &hits } else { &hits[..1] };
            for &idx in targets.iter().rev() {
                src_lines.splice(idx..idx + old_lines.len(), new_lines.clone());
            }
            return Ok((
                join_preserve_lines(&src_lines, is_crlf, has_trailing_nl),
                targets.len(),
            ));
        }
    }

    if !old_lines.is_empty() && old_lines.iter().any(|l| !l.trim().is_empty()) && src_lines.len() >= old_lines.len() {
        let src_trim: Vec<String> = src_lines
            .iter()
            .map(|l| normalize_unicode_ws(l).trim().to_string())
            .collect();
        let old_trim: Vec<String> = old_lines.iter().map(|l| l.trim().to_string()).collect();

        let mut hits = Vec::new();
        for i in 0..=src_trim.len() - old_trim.len() {
            if src_trim[i..i + old_trim.len()] == old_trim[..] {
                hits.push(i);
            }
        }
        if hits.len() > 1 && !replace_all {
            return Err(format!(
                "Ambiguous edit: old_text matches {} locations in file. Provide more unique context or set replace_all=true.",
                hits.len()
            ));
        }
        if !hits.is_empty() {
            let targets: &[usize] = if replace_all { &hits } else { &hits[..1] };
            for &idx in targets.iter().rev() {
                let replacement = reindent_replacement(&src_lines[idx..idx + old_lines.len()], &old_lines, &new_lines);
                src_lines.splice(idx..idx + old_lines.len(), replacement);
            }
            return Ok((
                join_preserve_lines(&src_lines, is_crlf, has_trailing_nl),
                targets.len(),
            ));
        }
    }

    let collapse_ws = |s: &str| -> String {
        normalize_unicode_ws(s)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let old_nonempty: Vec<String> = old_lines
        .iter()
        .map(|l| collapse_ws(l))
        .filter(|l| !l.is_empty())
        .collect();

    if !old_nonempty.is_empty() {
        let src_nonempty: Vec<(usize, String)> = src_lines
            .iter()
            .enumerate()
            .map(|(idx, l)| (idx, collapse_ws(l)))
            .filter(|(_, l)| !l.is_empty())
            .collect();

        if src_nonempty.len() >= old_nonempty.len() {
            let mut span_hits: Vec<(usize, usize)> = Vec::new();
            for i in 0..=src_nonempty.len() - old_nonempty.len() {
                let ok = (0..old_nonempty.len()).all(|k| src_nonempty[i + k].1 == old_nonempty[k]);
                if ok {
                    let start_line = src_nonempty[i].0;
                    let end_line_excl = src_nonempty[i + old_nonempty.len() - 1].0 + 1;
                    span_hits.push((start_line, end_line_excl));
                }
            }
            if span_hits.len() > 1 && !replace_all {
                return Err(format!(
                    "Ambiguous edit: old_text matches {} locations in file. Provide more unique context or set replace_all=true.",
                    span_hits.len()
                ));
            }
            if !span_hits.is_empty() {
                let targets: &[(usize, usize)] = if replace_all { &span_hits } else { &span_hits[..1] };
                for &(start_l, end_l) in targets.iter().rev() {
                    let replacement = reindent_replacement(&src_lines[start_l..end_l], &old_lines, &new_lines);
                    src_lines.splice(start_l..end_l, replacement);
                }
                return Ok((
                    join_preserve_lines(&src_lines, is_crlf, has_trailing_nl),
                    targets.len(),
                ));
            }
        }
    }

    let old_tokens = tokenize_with_spans(&clean_o);
    if old_tokens.len() >= 4 {
        let src_tokens = tokenize_with_spans(&clean_s);
        if src_tokens.len() >= old_tokens.len() {
            let mut tok_hits: Vec<(usize, usize)> = Vec::new();
            let mut idx = 0usize;
            while idx + old_tokens.len() <= src_tokens.len() {
                let matched = (0..old_tokens.len()).all(|k| src_tokens[idx + k].text == old_tokens[k].text);
                if matched {
                    let b_start = src_tokens[idx].byte_start;
                    let b_end = src_tokens[idx + old_tokens.len() - 1].byte_end;
                    tok_hits.push((b_start, b_end));
                    idx += old_tokens.len();
                } else {
                    idx += 1;
                }
            }
            if tok_hits.len() > 1 && !replace_all {
                return Err(format!(
                    "Ambiguous edit: old_text matches {} token locations in file. Provide more unique context or set replace_all=true.",
                    tok_hits.len()
                ));
            }
            if !tok_hits.is_empty() {
                let targets: &[(usize, usize)] = if replace_all { &tok_hits } else { &tok_hits[..1] };
                let mut patched = clean_s.clone();
                let trimmed_new = clean_n.trim_matches(['\r', '\n']);
                for &(b_start, b_end) in targets.iter().rev() {
                    patched.replace_range(b_start..b_end, trimmed_new);
                }
                let restored = if is_crlf {
                    patched.replace('\n', "\r\n")
                } else {
                    patched
                };
                return Ok((restored, targets.len()));
            }
        }
    }

    if !replace_all && old_nonempty.len() >= 2 && old_tokens.len() >= 6 && src_lines.len() >= 2 {
        let old_tok_strs: Vec<String> = old_tokens.iter().map(|t| t.text.clone()).collect();
        let first_anchor_toks: Vec<String> = tokenize_with_spans(&old_nonempty[0])
            .into_iter()
            .map(|t| t.text)
            .collect();
        let last_anchor_toks: Vec<String> = tokenize_with_spans(old_nonempty.last().unwrap())
            .into_iter()
            .map(|t| t.text)
            .collect();

        let line_tokens: Vec<Vec<String>> = src_lines
            .iter()
            .map(|l| {
                tokenize_with_spans(&normalize_unicode_ws(l))
                    .into_iter()
                    .map(|t| t.text)
                    .collect()
            })
            .collect();

        let min_win = old_lines.len().saturating_sub(2).max(2);
        let max_win = (old_lines.len() + 3).min(src_lines.len());

        let mut best_score = 0.0f64;
        let mut second_score = 0.0f64;
        let mut best_span: Option<(usize, usize)> = None;

        for start in 0..src_lines.len() {
            if line_tokens[start].is_empty() {
                continue;
            }
            let anchor_head = token_sequence_similarity(&first_anchor_toks, &line_tokens[start]);
            if anchor_head < 0.65 {
                continue;
            }
            for win_len in min_win..=max_win {
                let end = start + win_len;
                if end > src_lines.len() {
                    break;
                }
                if line_tokens[end - 1].is_empty() {
                    continue;
                }
                let anchor_tail = token_sequence_similarity(&last_anchor_toks, &line_tokens[end - 1]);
                if anchor_tail < 0.65 {
                    continue;
                }
                let candidate_toks: Vec<String> = line_tokens[start..end]
                    .iter()
                    .flat_map(|v| v.iter().cloned())
                    .collect();
                let score = token_sequence_similarity(&old_tok_strs, &candidate_toks);
                if score > best_score {
                    if let Some((prev_s, prev_e)) = best_span {
                        if start >= prev_e || end <= prev_s {
                            second_score = second_score.max(best_score);
                        }
                    }
                    best_score = score;
                    best_span = Some((start, end));
                } else if let Some((bs, be)) = best_span {
                    if start >= be || end <= bs {
                        second_score = second_score.max(score);
                    }
                }
            }
        }

        if let Some((bs, be)) = best_span {
            if best_score >= 0.90 && (best_score - second_score) >= 0.08 {
                let replacement = reindent_replacement(&src_lines[bs..be], &old_lines, &new_lines);
                src_lines.splice(bs..be, replacement);
                return Ok((join_preserve_lines(&src_lines, is_crlf, has_trailing_nl), 1));
            }
        }
    }

    Err(format!(
        "old_text not found in file (even after whitespace/indent/anchor cascade): '{}'. Call read_file around the target lines and pass the exact substring.",
        old_text.lines().next().unwrap_or("")
    ))
}
