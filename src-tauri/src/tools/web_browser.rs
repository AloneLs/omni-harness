use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tungstenite::{connect, Message};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForumAnswer {
    pub author: String,
    pub score: i32,
    pub is_accepted: bool,
    pub body_markdown: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForumThreadResult {
    pub title: String,
    pub question_body: String,
    pub author: String,
    pub score: i32,
    pub answers: Vec<ForumAnswer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentationResult {
    pub title: String,
    pub breadcrumbs: Vec<String>,
    pub main_markdown: String,
    pub code_blocks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdpTarget {
    pub id: String,
    pub title: String,
    pub url: String,
    pub web_socket_debugger_url: Option<String>,
}

pub struct CdpController {
    debug_port: u16,
    req_counter: AtomicU64,
}

impl Clone for CdpController {
    fn clone(&self) -> Self {
        Self {
            debug_port: self.debug_port,
            req_counter: AtomicU64::new(self.req_counter.load(Ordering::Relaxed)),
        }
    }
}

fn decode_html_entities(input: &str) -> String {
    let mut s = input
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&apos;", "'")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&hellip;", "...");
    if s.contains("&#") {
        let num_re = Regex::new(r"&#(?:x([0-9a-fA-F]+)|([0-9]+));").unwrap();
        s = num_re
            .replace_all(&s, |caps: &regex::Captures| {
                let code = if let Some(hex) = caps.get(1) {
                    u32::from_str_radix(hex.as_str(), 16).ok()
                } else if let Some(dec) = caps.get(2) {
                    dec.as_str().parse::<u32>().ok()
                } else {
                    None
                };
                code.and_then(char::from_u32)
                    .map(|c| c.to_string())
                    .unwrap_or_default()
            })
            .into_owned();
    }
    s
}

pub fn focus_extract_markdown(md: &str, focus_prompt: &str, max_chars: usize) -> String {
    let trimmed = md.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let keywords: Vec<String> = focus_prompt
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .map(|w| w.trim().to_lowercase())
        .filter(|w| w.chars().count() >= 3)
        .collect();
    if keywords.is_empty() || trimmed.chars().count() <= max_chars {
        return crate::engine::context_engine::safe_slice_head(trimmed, max_chars).to_string();
    }
    let blocks: Vec<&str> = trimmed.split("\n\n").collect();
    let mut scored: Vec<(usize, usize, &str)> = blocks
        .iter()
        .enumerate()
        .map(|(idx, blk)| {
            let low = blk.to_lowercase();
            let hits = keywords.iter().filter(|kw| low.contains(kw.as_str())).count();
            (hits, idx, *blk)
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let mut selected_indices: Vec<usize> = Vec::new();
    let mut used = 0usize;
    for (hits, idx, blk) in &scored {
        if *hits == 0 && !selected_indices.is_empty() {
            break;
        }
        let blen = blk.chars().count() + 2;
        if used + blen > max_chars && !selected_indices.is_empty() {
            continue;
        }
        selected_indices.push(*idx);
        used += blen;
        if used >= max_chars {
            break;
        }
    }
    if selected_indices.is_empty() {
        return crate::engine::context_engine::safe_slice_head(trimmed, max_chars).to_string();
    }
    selected_indices.sort_unstable();
    selected_indices
        .into_iter()
        .map(|i| blocks[i])
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn duckduckgo_search(query: &str, max_results: usize) -> Result<Vec<SearchResult>, String> {
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36");
    if let Some(sys_proxy) = crate::resolve_windows_system_proxy() {
        if let Ok(proxy) = reqwest::Proxy::all(&sys_proxy) {
            builder = builder.proxy(proxy.no_proxy(reqwest::NoProxy::from_string("localhost,127.0.0.1,::1,*.local,*.internal")));
        }
    }
    let client = builder.build().map_err(|e| e.to_string())?;

    let tag_cleaner = Regex::new(r"<[^>]*>").unwrap();
    let clean_tags = |s: &str| -> String {
        decode_html_entities(&tag_cleaner.replace_all(s, "")).trim().to_string()
    };

    let url = format!(
        "https://html.duckduckgo.com/html/?q={}",
        urlencoding::encode(query)
    );

    let mut results = Vec::new();
    if let Ok(resp) = client
        .get(&url)
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
    {
        if let Ok(html) = resp.text() {
            let block_title_re = Regex::new(r#"(?is)<a[^>]*class="[^"]*result__a[^"]*"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#).unwrap();
            let snippet_re = Regex::new(r#"(?is)<(?:a|div)[^>]*class="[^"]*result__snippet[^"]*"[^>]*>(.*?)</(?:a|div)>"#).unwrap();

            let result_blocks: Vec<&str> = html.split("result__body").skip(1).collect();
            for block in result_blocks {
                let mut final_url = String::new();
                let mut final_title = String::new();
                let mut final_snippet = String::new();

                if let Some(caps) = block_title_re.captures(block) {
                    let raw_url = decode_html_entities(&caps[1]);
                    final_title = clean_tags(&caps[2]);
                    if let Some(pos) = raw_url.find("uddg=") {
                        let sub = &raw_url[pos + 5..];
                        let enc_url = sub.split('&').next().unwrap_or(sub);
                        final_url = urlencoding::decode(enc_url).unwrap_or(enc_url.into()).to_string();
                    } else {
                        final_url = raw_url;
                    }
                }
                if let Some(caps) = snippet_re.captures(block) {
                    final_snippet = clean_tags(&caps[1]);
                }

                if !final_url.is_empty() && !final_title.is_empty() && final_url.starts_with("http") {
                    results.push(SearchResult {
                        title: final_title,
                        url: final_url,
                        snippet: final_snippet,
                    });
                    if results.len() >= max_results {
                        return Ok(results);
                    }
                }
            }
        }
    }

    // Fallback: DuckDuckGo Lite HTML endpoint
    if results.is_empty() {
        let lite_url = format!(
            "https://lite.duckduckgo.com/lite/?q={}",
            urlencoding::encode(query)
        );
        if let Ok(resp) = client.get(&lite_url).send() {
            if let Ok(html) = resp.text() {
                let lite_link_re = Regex::new(r#"(?is)<a[^>]*class="[^"]*result-link[^"]*"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#).unwrap();
                let lite_snip_re = Regex::new(r#"(?is)<td[^>]*class="[^"]*result-snippet[^"]*"[^>]*>(.*?)</td>"#).unwrap();
                let snippets: Vec<String> = lite_snip_re
                    .captures_iter(&html)
                    .map(|c| clean_tags(&c[1]))
                    .collect();
                for (idx, caps) in lite_link_re.captures_iter(&html).enumerate() {
                    let raw_url = decode_html_entities(&caps[1]);
                    let final_url = if let Some(pos) = raw_url.find("uddg=") {
                        let sub = &raw_url[pos + 5..];
                        let enc_url = sub.split('&').next().unwrap_or(sub);
                        urlencoding::decode(enc_url).unwrap_or(enc_url.into()).to_string()
                    } else {
                        raw_url
                    };
                    let title = clean_tags(&caps[2]);
                    if !final_url.is_empty() && !title.is_empty() && final_url.starts_with("http") {
                        results.push(SearchResult {
                            title,
                            url: final_url,
                            snippet: snippets.get(idx).cloned().unwrap_or_default(),
                        });
                        if results.len() >= max_results {
                            break;
                        }
                    }
                }
            }
        }
    }

    Ok(results)
}

pub fn searxng_search(
    instance_url: &str,
    query: &str,
    max_results: usize,
) -> Result<Vec<SearchResult>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| e.to_string())?;

    let trimmed_base = instance_url.trim_end_matches('/');
    let target_url = format!(
        "{}/search?q={}&format=json",
        trimmed_base,
        urlencoding::encode(query)
    );

    let resp = client
        .get(&target_url)
        .send()
        .map_err(|e| format!("SearXNG request failed: {}", e))?;

    let val: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    let mut results = Vec::new();

    if let Some(items) = val.get("results").and_then(|r| r.as_array()) {
        for item in items {
            let title = item.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string();
            let url = item.get("url").and_then(|u| u.as_str()).unwrap_or("").to_string();
            let snippet = item.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string();

            if !url.is_empty() {
                results.push(SearchResult {
                    title,
                    url,
                    snippet,
                });
                if results.len() >= max_results {
                    break;
                }
            }
        }
    }

    Ok(results)
}

pub fn html_to_markdown(raw_html: &str) -> String {
    let mut clean = raw_html.to_string();
    for tag in ["script", "style", "svg", "noscript", "header", "footer", "nav"] {
        let pat = format!(r"(?is)<{}[^>]*>.*?</{}>", tag, tag);
        if let Ok(re) = Regex::new(&pat) {
            clean = re.replace_all(&clean, "").to_string();
        }
    }

    let h1_re = Regex::new(r"(?is)<h1[^>]*>(.*?)</h1>").unwrap();
    let text = h1_re.replace_all(&clean, "\n# $1\n");

    let h2_re = Regex::new(r"(?is)<h2[^>]*>(.*?)</h2>").unwrap();
    let text = h2_re.replace_all(&text, "\n## $1\n");

    let h3_re = Regex::new(r"(?is)<h3[^>]*>(.*?)</h3>").unwrap();
    let text = h3_re.replace_all(&text, "\n### $1\n");

    let h4_re = Regex::new(r"(?is)<h[456][^>]*>(.*?)<\/h[456]>").unwrap();
    let text = h4_re.replace_all(&text, "\n#### $1\n");

    let p_re = Regex::new(r"(?is)<p[^>]*>(.*?)</p>").unwrap();
    let text = p_re.replace_all(&text, "\n$1\n");

    let pre_code_re = Regex::new(r"(?is)<pre[^>]*><code(?: class=[^>]*)?>(.*?)</code></pre>").unwrap();
    let text = pre_code_re.replace_all(&text, "\n```\n$1\n```\n");

    let code_re = Regex::new(r"(?is)<code[^>]*>(.*?)</code>").unwrap();
    let text = code_re.replace_all(&text, "`$1`");

    let strong_re = Regex::new(r"(?is)<(strong|b)[^>]*>(.*?)</(strong|b)>").unwrap();
    let text = strong_re.replace_all(&text, "**$2**");

    let em_re = Regex::new(r"(?is)<(em|i)[^>]*>(.*?)</(em|i)>").unwrap();
    let text = em_re.replace_all(&text, "*$2*");

    let li_re = Regex::new(r"(?is)<li[^>]*>(.*?)</li>").unwrap();
    let text = li_re.replace_all(&text, "\n- $1");

    let tr_re = Regex::new(r"(?is)<tr[^>]*>(.*?)</tr>").unwrap();
    let cell_re = Regex::new(r"(?is)<t[hd][^>]*>(.*?)</t[hd]>").unwrap();
    let tag_in_cell_re = Regex::new(r"(?is)<[^>]+>").unwrap();
    let text = tr_re.replace_all(&text, |caps: &regex::Captures| {
        let row_html = &caps[1];
        let cols: Vec<String> = cell_re
            .captures_iter(row_html)
            .map(|c| tag_in_cell_re.replace_all(&c[1], "").trim().replace('|', "\\|"))
            .collect();
        if cols.is_empty() {
            String::new()
        } else {
            format!("\n| {} |", cols.join(" | "))
        }
    });

    let a_re = Regex::new(r#"(?is)<a[^>]*href="([^"]*)"[^>]*>(.*?)</a>"#).unwrap();
    let text = a_re.replace_all(&text, "[$2]($1)");

    let br_re = Regex::new(r"(?is)<br[\s/]*>").unwrap();
    let text = br_re.replace_all(&text, "\n");

    let any_tag_re = Regex::new(r"(?is)<[^>]+>").unwrap();
    let text = any_tag_re.replace_all(&text, "");

    let decode_entities = decode_html_entities(&text);

    let multiple_newlines = Regex::new(r"\n{3,}").unwrap();
    let cleaned_lines = multiple_newlines.replace_all(&decode_entities, "\n\n");

    cleaned_lines.trim().to_string()
}

pub fn parse_forum_thread(_url: &str, raw_html: &str) -> ForumThreadResult {
    let title_re = Regex::new(r#"(?is)<title>(.*?)</title>"#).unwrap();
    let title = title_re
        .captures(raw_html)
        .map(|c| c[1].trim().to_string())
        .unwrap_or_else(|| "Forum Thread".to_string());

    let question_re = Regex::new(r#"(?is)<div[^>]*class="[^"]*(?:post-text|entry-content|question-body)[^"]*"[^>]*>(.*?)</div>"#).unwrap();
    let question_body = question_re
        .captures(raw_html)
        .map(|c| html_to_markdown(&c[1]))
        .unwrap_or_else(|| html_to_markdown(raw_html));

    let answer_re = Regex::new(r#"(?is)<div[^>]*class="[^"]*(?:answer|comment-body)[^"]*"[^>]*>(.*?)</div>"#).unwrap();
    let mut answers = Vec::new();

    for caps in answer_re.captures_iter(raw_html) {
        let block = &caps[1];
        let md = html_to_markdown(block);
        if !md.is_empty() {
            let is_accepted = block.contains("accepted-answer") || block.contains("is-accepted");
            answers.push(ForumAnswer {
                author: "Participant".to_string(),
                score: 1,
                is_accepted,
                body_markdown: md,
            });
        }
    }

    ForumThreadResult {
        title,
        question_body,
        author: "OriginalPoster".to_string(),
        score: 1,
        answers,
    }
}

pub fn parse_documentation(_url: &str, raw_html: &str) -> DocumentationResult {
    let title_re = Regex::new(r#"(?is)<title>(.*?)</title>"#).unwrap();
    let title = title_re
        .captures(raw_html)
        .map(|c| c[1].trim().to_string())
        .unwrap_or_else(|| "Documentation".to_string());

    let breadcrumb_re = Regex::new(r#"(?is)<(?:nav|ul|ol)[^>]*class="[^"]*breadcrumb[^"]*"[^>]*>(.*?)</(?:nav|ul|ol)>"#).unwrap();
    let mut breadcrumbs = Vec::new();
    if let Some(caps) = breadcrumb_re.captures(raw_html) {
        let item_re = Regex::new(r#"(?is)<li[^>]*>(.*?)</li>"#).unwrap();
        for item in item_re.captures_iter(&caps[1]) {
            let tag_cleaner = Regex::new(r"<[^>]*>").unwrap();
            let b = tag_cleaner.replace_all(&item[1], "").trim().to_string();
            if !b.is_empty() {
                breadcrumbs.push(b);
            }
        }
    }

    let code_re = Regex::new(r#"(?is)<pre[^>]*><code[^>]*>(.*?)</code></pre>"#).unwrap();
    let mut code_blocks = Vec::new();
    for caps in code_re.captures_iter(raw_html) {
        let block_clean = html_to_markdown(&caps[1]);
        if !block_clean.is_empty() {
            code_blocks.push(block_clean);
        }
    }

    let main_re = Regex::new(r#"(?is)<(?:main|article)[^>]*>(.*?)</(?:main|article)>"#).unwrap();
    let main_markdown = if let Some(caps) = main_re.captures(raw_html) {
        html_to_markdown(&caps[1])
    } else {
        html_to_markdown(raw_html)
    };

    DocumentationResult {
        title,
        breadcrumbs,
        main_markdown,
        code_blocks,
    }
}

impl CdpController {
    pub fn new(debug_port: u16) -> Self {
        Self {
            debug_port,
            req_counter: AtomicU64::new(1),
        }
    }

    pub fn list_targets(&self) -> Result<Vec<CdpTarget>, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(4))
            .build()
            .map_err(|e| e.to_string())?;

        let url = format!("http://127.0.0.1:{}/json", self.debug_port);
        let resp = client
            .get(&url)
            .send()
            .map_err(|e| format!("CDP connection failed: {}", e))?;

        let list: Vec<serde_json::Value> = resp.json().map_err(|e| e.to_string())?;
        let mut targets = Vec::new();

        for item in list {
            let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let url_val = item.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let ws_url = item.get("webSocketDebuggerUrl").and_then(|v| v.as_str()).map(|s| s.to_string());

            if !id.is_empty() {
                targets.push(CdpTarget {
                    id,
                    title,
                    url: url_val,
                    web_socket_debugger_url: ws_url,
                });
            }
        }

        Ok(targets)
    }

    fn send_command(
        &self,
        ws_url: &str,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let (mut socket, _) = connect(ws_url)
            .map_err(|e| format!("Failed to connect to CDP WebSocket: {}", e))?;

        let id = self.req_counter.fetch_add(1, Ordering::SeqCst);
        let payload = json!({
            "id": id,
            "method": method,
            "params": params
        });

        socket
            .send(Message::Text(payload.to_string()))
            .map_err(|e| format!("Failed to send CDP message: {}", e))?;

        loop {
            let msg = socket
                .read()
                .map_err(|e| format!("Failed to read CDP response: {}", e))?;

            if let Message::Text(text) = msg {
                let resp_val: serde_json::Value =
                    serde_json::from_str(&text).map_err(|e| e.to_string())?;

                if resp_val.get("id").and_then(|v| v.as_u64()) == Some(id) {
                    if let Some(err) = resp_val.get("error") {
                        return Err(err.to_string());
                    }
                    return Ok(resp_val.get("result").cloned().unwrap_or(json!({})));
                }
            }
        }
    }

    pub fn navigate(&self, ws_url: &str, target_url: &str) -> Result<(), String> {
        self.send_command(ws_url, "Page.navigate", json!({ "url": target_url }))?;
        Ok(())
    }

    pub fn capture_screenshot(&self, ws_url: &str, format_opt: Option<&str>) -> Result<String, String> {
        let fmt = format_opt.unwrap_or("png");
        let result = self.send_command(
            ws_url,
            "Page.captureScreenshot",
            json!({ "format": fmt, "quality": 90 }),
        )?;

        let b64 = result
            .get("data")
            .and_then(|d| d.as_str())
            .ok_or_else(|| "Missing screenshot data in CDP response".to_string())?;

        Ok(b64.to_string())
    }

    pub fn evaluate_script(&self, ws_url: &str, script: &str) -> Result<serde_json::Value, String> {
        let result = self.send_command(
            ws_url,
            "Runtime.evaluate",
            json!({
                "expression": script,
                "returnByValue": true,
                "awaitPromise": true
            }),
        )?;

        Ok(result.get("result").and_then(|r| r.get("value")).cloned().unwrap_or(result))
    }

    pub fn click_element(&self, ws_url: &str, selector: &str) -> Result<(), String> {
        let script = format!(
            r#"(() => {{
                const el = document.querySelector({});
                if (!el) throw new Error("Element not found: " + {});
                el.scrollIntoView({{ behavior: 'instant', block: 'center' }});
                el.click();
                return true;
            }})()"#,
            serde_json::to_string(selector).unwrap(),
            serde_json::to_string(selector).unwrap()
        );

        self.evaluate_script(ws_url, &script)?;
        Ok(())
    }

    pub fn type_element(&self, ws_url: &str, selector: &str, text: &str) -> Result<(), String> {
        let script = format!(
            r#"(() => {{
                const el = document.querySelector({});
                if (!el) throw new Error("Element not found: " + {});
                el.focus();
                el.value = {};
                el.dispatchEvent(new Event('input', {{ bubbles: true }}));
                el.dispatchEvent(new Event('change', {{ bubbles: true }}));
                return true;
            }})()"#,
            serde_json::to_string(selector).unwrap(),
            serde_json::to_string(selector).unwrap(),
            serde_json::to_string(text).unwrap()
        );

        self.evaluate_script(ws_url, &script)?;
        Ok(())
    }

    pub fn get_content(&self, ws_url: &str) -> Result<String, String> {
        let result = self.evaluate_script(ws_url, "document.documentElement.outerHTML")?;
        let html_str = result.as_str().unwrap_or("").to_string();
        Ok(html_to_markdown(&html_str))
    }

    pub fn inspect_interactive_dom(&self, ws_url: &str, selector_opt: Option<&str>) -> Result<serde_json::Value, String> {
        let root_js = selector_opt
            .map(|s| format!("document.querySelector({}) || document.body", serde_json::to_string(s).unwrap()))
            .unwrap_or_else(|| "document.body".to_string());
        let script = format!(
            r#"(() => {{
                const root = {};
                if (!root) return {{ url: location.href, title: document.title, elements: [] }};
                const els = Array.from(root.querySelectorAll('a,button,input,textarea,select,[role="button"],[role="link"],[role="textbox"],[contenteditable="true"]'));
                const out = [];
                for (const el of els) {{
                    const r = el.getBoundingClientRect();
                    if (r.width < 2 || r.height < 2) continue;
                    const st = getComputedStyle(el);
                    if (st.visibility === 'hidden' || st.display === 'none') continue;
                    let sel = '';
                    if (el.id) sel = '#' + CSS.escape(el.id);
                    else if (el.getAttribute('data-testid')) sel = `[data-testid="${{el.getAttribute('data-testid')}}"]`;
                    else if (el.name) sel = `${{el.tagName.toLowerCase()}}[name="${{el.name}}"]`;
                    else {{
                        const cls = (typeof el.className === 'string' ? el.className : '').trim().split(/\s+/).filter(Boolean).slice(0, 2);
                        sel = el.tagName.toLowerCase() + (cls.length ? '.' + cls.map(c => CSS.escape(c)).join('.') : '');
                    }}
                    out.push({{
                        tag: el.tagName.toLowerCase(),
                        type: el.getAttribute('type') || undefined,
                        text: (el.innerText || el.value || el.getAttribute('aria-label') || el.getAttribute('placeholder') || '').trim().slice(0, 80),
                        placeholder: el.getAttribute('placeholder') || undefined,
                        href: el.getAttribute('href') || undefined,
                        selector: sel,
                        rect: [Math.round(r.x), Math.round(r.y), Math.round(r.width), Math.round(r.height)]
                    }});
                    if (out.length >= 120) break;
                }}
                return {{ url: location.href, title: document.title, scrollY: Math.round(window.scrollY), elements: out }};
            }})()"#,
            root_js
        );
        self.evaluate_script(ws_url, &script)
    }

    pub fn click_smart(&self, ws_url: &str, selector_or_text: &str) -> Result<(), String> {
        let q = serde_json::to_string(selector_or_text).unwrap();
        let script = format!(
            r#"(() => {{
                const q = {};
                let el = null;
                try {{ el = document.querySelector(q); }} catch {{}}
                if (!el) {{
                    const low = q.toLowerCase();
                    const cands = Array.from(document.querySelectorAll('a,button,[role="button"],input[type="submit"],input[type="button"],[role="link"]'));
                    el = cands.find(e => ((e.innerText || e.value || e.getAttribute('aria-label') || '').trim().toLowerCase()).includes(low));
                }}
                if (!el) throw new Error("Element not found: " + q);
                el.scrollIntoView({{ behavior: 'instant', block: 'center' }});
                el.dispatchEvent(new PointerEvent('pointerdown', {{ bubbles: true }}));
                el.dispatchEvent(new MouseEvent('mousedown', {{ bubbles: true }}));
                el.dispatchEvent(new PointerEvent('pointerup', {{ bubbles: true }}));
                el.dispatchEvent(new MouseEvent('mouseup', {{ bubbles: true }}));
                el.click();
                return true;
            }})()"#,
            q
        );
        self.evaluate_script(ws_url, &script)?;
        Ok(())
    }

    pub fn type_smart(&self, ws_url: &str, selector: &str, text: &str, submit: bool) -> Result<(), String> {
        let script = format!(
            r#"(() => {{
                const q = {};
                let el = null;
                try {{ el = document.querySelector(q); }} catch {{}}
                if (!el) {{
                    const low = q.toLowerCase();
                    el = Array.from(document.querySelectorAll('input,textarea,[contenteditable="true"]')).find(e =>
                        (e.getAttribute('placeholder') || e.getAttribute('aria-label') || e.name || '').toLowerCase().includes(low)
                    );
                }}
                if (!el) throw new Error("Input not found: " + q);
                el.scrollIntoView({{ behavior: 'instant', block: 'center' }});
                el.focus();
                const val = {};
                if (el.isContentEditable) {{
                    el.innerText = val;
                }} else {{
                    const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
                    const setter = Object.getOwnPropertyDescriptor(proto, 'value')?.set;
                    if (setter) setter.call(el, val); else el.value = val;
                }}
                el.dispatchEvent(new Event('input', {{ bubbles: true }}));
                el.dispatchEvent(new Event('change', {{ bubbles: true }}));
                if ({}) {{
                    el.dispatchEvent(new KeyboardEvent('keydown', {{ key: 'Enter', code: 'Enter', keyCode: 13, bubbles: true }}));
                    el.dispatchEvent(new KeyboardEvent('keyup', {{ key: 'Enter', code: 'Enter', keyCode: 13, bubbles: true }}));
                    if (el.form && typeof el.form.requestSubmit === 'function') el.form.requestSubmit();
                }}
                return true;
            }})()"#,
            serde_json::to_string(selector).unwrap(),
            serde_json::to_string(text).unwrap(),
            if submit { "true" } else { "false" }
        );
        self.evaluate_script(ws_url, &script)?;
        Ok(())
    }

    pub fn scroll_page(&self, ws_url: &str, dx: i32, dy: i32, selector_opt: Option<&str>) -> Result<(), String> {
        let script = if let Some(sel) = selector_opt {
            format!(
                "(() => {{ const el = document.querySelector({}); if (el) el.scrollBy({}, {}); else window.scrollBy({}, {}); return true; }})()",
                serde_json::to_string(sel).unwrap(), dx, dy, dx, dy
            )
        } else {
            format!("(() => {{ window.scrollBy({}, {}); return true; }})()", dx, dy)
        };
        self.evaluate_script(ws_url, &script)?;
        Ok(())
    }

    pub fn wait_for_selector(&self, ws_url: &str, selector: &str, timeout_ms: u64) -> Result<bool, String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms.clamp(200, 15_000));
        let check_js = format!("Boolean(document.querySelector({}))", serde_json::to_string(selector).unwrap());
        loop {
            if let Ok(v) = self.evaluate_script(ws_url, &check_js) {
                if v.as_bool() == Some(true) {
                    return Ok(true);
                }
            }
            if std::time::Instant::now() >= deadline {
                return Ok(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }

    pub fn collect_preview_diagnostics(&self, ws_url: &str, do_screenshot: bool) -> Result<serde_json::Value, String> {
        let diag_js = r#"(() => {
            const errs = [];
            if (Array.isArray(window.__omniConsoleErrors)) {
                for (const e of window.__omniConsoleErrors.slice(-20)) errs.push(String(e));
            }
            const perf = performance.getEntriesByType ? performance.getEntriesByType('resource').filter(r => r.responseStatus >= 400).slice(0, 15).map(r => `${r.responseStatus} ${r.name}`) : [];
            return {
                url: location.href,
                title: document.title,
                readyState: document.readyState,
                textLength: (document.body?.innerText || '').trim().length,
                consoleErrors: errs,
                failedResources: perf,
            };
        })()"#;
        let dom_info = self.evaluate_script(ws_url, diag_js)?;
        if do_screenshot {
            let b64 = self.capture_screenshot(ws_url, Some("png"))?;
            Ok(json!({
                "dom": dom_info,
                "dataUrl": format!("data:image/png;base64,{}", b64),
            }))
        } else {
            Ok(dom_info)
        }
    }
}
