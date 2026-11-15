use base64::Engine;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubRepo {
    pub id: u64,
    pub name: String,
    pub full_name: String,
    pub html_url: String,
    pub description: Option<String>,
    pub default_branch: String,
    pub stargazers_count: u64,
    pub forks_count: u64,
    pub open_issues_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubCommit {
    pub sha: String,
    pub message: String,
    pub author_name: String,
    pub author_date: String,
    pub html_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubIssue {
    pub id: u64,
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub html_url: String,
    pub user_login: String,
    pub comments_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubPullRequest {
    pub id: u64,
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub html_url: String,
    pub diff_url: String,
    pub head_ref: String,
    pub base_ref: String,
    pub merged: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubTreeEntry {
    pub path: String,
    pub mode: String,
    pub item_type: String,
    pub sha: String,
    pub size: Option<u64>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubFileContent {
    pub name: String,
    pub path: String,
    pub sha: String,
    pub size: u64,
    pub encoding: String,
    pub content: String,
    pub decoded_text: Option<String>,
}

#[derive(Clone)]
pub struct GithubClient {
    client: reqwest::blocking::Client,
    token: Option<String>,
    base_url: String,
}

impl GithubClient {
    pub fn new(token: Option<String>) -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| e.to_string())?;

        Ok(Self {
            client,
            token,
            base_url: "https://api.github.com".to_string(),
        })
    }

    fn headers(&self, accept_header: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("omni-harness/1.0"));
        let accept_val = accept_header.unwrap_or("application/vnd.github.v3+json");
        headers.insert(
            ACCEPT,
            HeaderValue::from_str(accept_val).unwrap_or(HeaderValue::from_static("application/vnd.github.v3+json")),
        );
        if let Some(ref t) = self.token {
            if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", t)) {
                headers.insert(AUTHORIZATION, val);
            }
        }
        headers
    }

    pub fn get_repo(&self, owner: &str, repo: &str) -> Result<GithubRepo, String> {
        let url = format!("{}/repos/{}/{}", self.base_url, owner, repo);
        let resp = self
            .client
            .get(&url)
            .headers(self.headers(None))
            .send()
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("GitHub API error: status {}", resp.status()));
        }

        let val: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
        Ok(GithubRepo {
            id: val.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
            name: val.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            full_name: val.get("full_name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            html_url: val.get("html_url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            description: val.get("description").and_then(|v| v.as_str()).map(|s| s.to_string()),
            default_branch: val.get("default_branch").and_then(|v| v.as_str()).unwrap_or("main").to_string(),
            stargazers_count: val.get("stargazers_count").and_then(|v| v.as_u64()).unwrap_or(0),
            forks_count: val.get("forks_count").and_then(|v| v.as_u64()).unwrap_or(0),
            open_issues_count: val.get("open_issues_count").and_then(|v| v.as_u64()).unwrap_or(0),
        })
    }

    pub fn search_repositories(&self, query: &str, per_page: usize) -> Result<Vec<GithubRepo>, String> {
        let url = format!(
            "{}/search/repositories?q={}&per_page={}",
            self.base_url,
            urlencoding::encode(query),
            per_page
        );

        let resp = self
            .client
            .get(&url)
            .headers(self.headers(None))
            .send()
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("GitHub API error: status {}", resp.status()));
        }

        let val: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
        let mut repos = Vec::new();

        if let Some(items) = val.get("items").and_then(|i| i.as_array()) {
            for item in items {
                repos.push(GithubRepo {
                    id: item.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
                    name: item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    full_name: item.get("full_name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    html_url: item.get("html_url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    description: item.get("description").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    default_branch: item.get("default_branch").and_then(|v| v.as_str()).unwrap_or("main").to_string(),
                    stargazers_count: item.get("stargazers_count").and_then(|v| v.as_u64()).unwrap_or(0),
                    forks_count: item.get("forks_count").and_then(|v| v.as_u64()).unwrap_or(0),
                    open_issues_count: item.get("open_issues_count").and_then(|v| v.as_u64()).unwrap_or(0),
                });
            }
        }

        Ok(repos)
    }

    pub fn list_commits(
        &self,
        owner: &str,
        repo: &str,
        branch: Option<&str>,
        per_page: usize,
    ) -> Result<Vec<GithubCommit>, String> {
        let mut url = format!(
            "{}/repos/{}/{}/commits?per_page={}",
            self.base_url, owner, repo, per_page
        );
        if let Some(b) = branch {
            url.push_str(&format!("&sha={}", urlencoding::encode(b)));
        }

        let resp = self
            .client
            .get(&url)
            .headers(self.headers(None))
            .send()
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("GitHub API error: status {}", resp.status()));
        }

        let val: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
        let mut commits = Vec::new();

        if let Some(items) = val.as_array() {
            for item in items {
                let sha = item.get("sha").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let html_url = item.get("html_url").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let commit_obj = item.get("commit");
                let message = commit_obj
                    .and_then(|c| c.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("")
                    .to_string();

                let author_obj = commit_obj.and_then(|c| c.get("author"));
                let author_name = author_obj
                    .and_then(|a| a.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                let author_date = author_obj
                    .and_then(|a| a.get("date"))
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();

                commits.push(GithubCommit {
                    sha,
                    message,
                    author_name,
                    author_date,
                    html_url,
                });
            }
        }

        Ok(commits)
    }

    pub fn compare_commits(
        &self,
        owner: &str,
        repo: &str,
        base: &str,
        head: &str,
    ) -> Result<serde_json::Value, String> {
        let url = format!(
            "{}/repos/{}/{}/compare/{}...{}",
            self.base_url, owner, repo, base, head
        );
        let resp = self
            .client
            .get(&url)
            .headers(self.headers(None))
            .send()
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("GitHub API error: status {}", resp.status()));
        }

        resp.json().map_err(|e| e.to_string())
    }

    pub fn list_issues(
        &self,
        owner: &str,
        repo: &str,
        state: Option<&str>,
        per_page: usize,
    ) -> Result<Vec<GithubIssue>, String> {
        let state_val = state.unwrap_or("open");
        let url = format!(
            "{}/repos/{}/{}/issues?state={}&per_page={}",
            self.base_url, owner, repo, state_val, per_page
        );

        let resp = self
            .client
            .get(&url)
            .headers(self.headers(None))
            .send()
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("GitHub API error: status {}", resp.status()));
        }

        let val: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
        let mut issues = Vec::new();

        if let Some(items) = val.as_array() {
            for item in items {
                if item.get("pull_request").is_some() {
                    continue;
                }
                issues.push(GithubIssue {
                    id: item.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
                    number: item.get("number").and_then(|v| v.as_u64()).unwrap_or(0),
                    title: item.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    body: item.get("body").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    state: item.get("state").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    html_url: item.get("html_url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    user_login: item.get("user").and_then(|u| u.get("login")).and_then(|l| l.as_str()).unwrap_or("").to_string(),
                    comments_count: item.get("comments").and_then(|v| v.as_u64()).unwrap_or(0),
                });
            }
        }

        Ok(issues)
    }

    pub fn create_issue(
        &self,
        owner: &str,
        repo: &str,
        title: &str,
        body: Option<&str>,
        labels: Option<Vec<String>>,
    ) -> Result<GithubIssue, String> {
        let url = format!("{}/repos/{}/{}/issues", self.base_url, owner, repo);
        let payload = json!({
            "title": title,
            "body": body.unwrap_or(""),
            "labels": labels.unwrap_or_default(),
        });

        let resp = self
            .client
            .post(&url)
            .headers(self.headers(None))
            .json(&payload)
            .send()
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("GitHub API error: status {}", resp.status()));
        }

        let item: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
        Ok(GithubIssue {
            id: item.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
            number: item.get("number").and_then(|v| v.as_u64()).unwrap_or(0),
            title: item.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            body: item.get("body").and_then(|v| v.as_str()).map(|s| s.to_string()),
            state: item.get("state").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            html_url: item.get("html_url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            user_login: item.get("user").and_then(|u| u.get("login")).and_then(|l| l.as_str()).unwrap_or("").to_string(),
            comments_count: item.get("comments").and_then(|v| v.as_u64()).unwrap_or(0),
        })
    }

    pub fn add_issue_comment(
        &self,
        owner: &str,
        repo: &str,
        issue_number: u64,
        comment: &str,
    ) -> Result<String, String> {
        let url = format!(
            "{}/repos/{}/{}/issues/{}/comments",
            self.base_url, owner, repo, issue_number
        );
        let payload = json!({ "body": comment });

        let resp = self
            .client
            .post(&url)
            .headers(self.headers(None))
            .json(&payload)
            .send()
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("GitHub API error: status {}", resp.status()));
        }

        let val: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
        Ok(val.get("html_url").and_then(|v| v.as_str()).unwrap_or("").to_string())
    }

    pub fn list_pull_requests(
        &self,
        owner: &str,
        repo: &str,
        state: Option<&str>,
        per_page: usize,
    ) -> Result<Vec<GithubPullRequest>, String> {
        let state_val = state.unwrap_or("open");
        let url = format!(
            "{}/repos/{}/{}/pulls?state={}&per_page={}",
            self.base_url, owner, repo, state_val, per_page
        );

        let resp = self
            .client
            .get(&url)
            .headers(self.headers(None))
            .send()
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("GitHub API error: status {}", resp.status()));
        }

        let val: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
        let mut prs = Vec::new();

        if let Some(items) = val.as_array() {
            for item in items {
                prs.push(GithubPullRequest {
                    id: item.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
                    number: item.get("number").and_then(|v| v.as_u64()).unwrap_or(0),
                    title: item.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    body: item.get("body").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    state: item.get("state").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    html_url: item.get("html_url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    diff_url: item.get("diff_url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    head_ref: item.get("head").and_then(|h| h.get("ref")).and_then(|r| r.as_str()).unwrap_or("").to_string(),
                    base_ref: item.get("base").and_then(|b| b.get("ref")).and_then(|r| r.as_str()).unwrap_or("").to_string(),
                    merged: item.get("merged").and_then(|m| m.as_bool()),
                });
            }
        }

        Ok(prs)
    }

    pub fn get_pr_diff(&self, owner: &str, repo: &str, pr_number: u64) -> Result<String, String> {
        let url = format!(
            "{}/repos/{}/{}/pulls/{}",
            self.base_url, owner, repo, pr_number
        );
        let resp = self
            .client
            .get(&url)
            .headers(self.headers(Some("application/vnd.github.v3.diff")))
            .send()
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("GitHub API error: status {}", resp.status()));
        }

        resp.text().map_err(|e| e.to_string())
    }

    pub fn create_pull_request(
        &self,
        owner: &str,
        repo: &str,
        title: &str,
        head: &str,
        base: &str,
        body: Option<&str>,
        draft: bool,
    ) -> Result<GithubPullRequest, String> {
        let url = format!("{}/repos/{}/{}/pulls", self.base_url, owner, repo);
        let payload = json!({
            "title": title,
            "head": head,
            "base": base,
            "body": body.unwrap_or(""),
            "draft": draft
        });

        let resp = self
            .client
            .post(&url)
            .headers(self.headers(None))
            .json(&payload)
            .send()
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("GitHub API error: status {}", resp.status()));
        }

        let item: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
        Ok(GithubPullRequest {
            id: item.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
            number: item.get("number").and_then(|v| v.as_u64()).unwrap_or(0),
            title: item.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            body: item.get("body").and_then(|v| v.as_str()).map(|s| s.to_string()),
            state: item.get("state").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            html_url: item.get("html_url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            diff_url: item.get("diff_url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            head_ref: item.get("head").and_then(|h| h.get("ref")).and_then(|r| r.as_str()).unwrap_or("").to_string(),
            base_ref: item.get("base").and_then(|b| b.get("ref")).and_then(|r| r.as_str()).unwrap_or("").to_string(),
            merged: item.get("merged").and_then(|m| m.as_bool()),
        })
    }

    pub fn get_tree(
        &self,
        owner: &str,
        repo: &str,
        tree_sha: &str,
        recursive: bool,
    ) -> Result<Vec<GithubTreeEntry>, String> {
        let mut url = format!(
            "{}/repos/{}/{}/git/trees/{}",
            self.base_url, owner, repo, tree_sha
        );
        if recursive {
            url.push_str("?recursive=1");
        }

        let resp = self
            .client
            .get(&url)
            .headers(self.headers(None))
            .send()
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("GitHub API error: status {}", resp.status()));
        }

        let val: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
        let mut entries = Vec::new();

        if let Some(items) = val.get("tree").and_then(|t| t.as_array()) {
            for item in items {
                entries.push(GithubTreeEntry {
                    path: item.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    mode: item.get("mode").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    item_type: item.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    sha: item.get("sha").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    size: item.get("size").and_then(|v| v.as_u64()),
                    url: item.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                });
            }
        }

        Ok(entries)
    }

    pub fn get_file_content(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
        reference: Option<&str>,
    ) -> Result<GithubFileContent, String> {
        let mut url = format!(
            "{}/repos/{}/{}/contents/{}",
            self.base_url, owner, repo, path
        );
        if let Some(r) = reference {
            url.push_str(&format!("?ref={}", urlencoding::encode(r)));
        }

        let resp = self
            .client
            .get(&url)
            .headers(self.headers(None))
            .send()
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("GitHub API error: status {}", resp.status()));
        }

        let val: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
        let encoding = val.get("encoding").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let content_raw = val.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let decoded_text = if encoding == "base64" {
            let cleaned = content_raw.replace('\n', "").replace('\r', "");
            base64::engine::general_purpose::STANDARD
                .decode(cleaned)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
        } else {
            None
        };

        Ok(GithubFileContent {
            name: val.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            path: val.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            sha: val.get("sha").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            size: val.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
            encoding,
            content: content_raw,
            decoded_text,
        })
    }

    pub fn create_or_update_file(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
        message: &str,
        content_plain: &str,
        sha: Option<&str>,
        branch: Option<&str>,
    ) -> Result<String, String> {
        let url = format!(
            "{}/repos/{}/{}/contents/{}",
            self.base_url, owner, repo, path
        );
        let b64_content = base64::engine::general_purpose::STANDARD.encode(content_plain.as_bytes());

        let mut payload = json!({
            "message": message,
            "content": b64_content
        });

        if let Some(s) = sha {
            payload["sha"] = json!(s);
        }
        if let Some(b) = branch {
            payload["branch"] = json!(b);
        }

        let resp = self
            .client
            .put(&url)
            .headers(self.headers(None))
            .json(&payload)
            .send()
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("GitHub API error: status {}", resp.status()));
        }

        let val: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
        let commit_sha = val
            .get("commit")
            .and_then(|c| c.get("sha"))
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();

        Ok(commit_sha)
    }
}
