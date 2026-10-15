//! Telling the person a newer version exists.
//!
//! This is the one request Coin Heaven makes that is about Coin Heaven rather than about
//! the person, so it is held to the same standard as the rest: it is off
//! until the person turns it on or presses the button, it starts in Rust, it
//! carries nothing but the app's name and version (no identifier, no data),
//! and it only reads the project's public release page. It installs nothing.

use crate::error::{AppError, AppResult};
use serde::Serialize;

/// The project's address, from the package manifest, so there is one place to
/// set it. While it is still the placeholder the check says so plainly rather
/// than asking a stranger's repository.
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    /// The release page to open.
    pub url: String,
    pub newer: bool,
}

/// "v1.2.10" → [1, 2, 10]; anything after a `-` (a pre-release) is ignored.
fn parts(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches(['v', 'V'])
        .split('-')
        .next()
        .unwrap_or("")
        .split('.')
        .map(|p| p.parse().unwrap_or(0))
        .collect()
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    let (a, b) = (parts(latest), parts(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

/// `https://github.com/owner/repo` → the releases API address, or `None`.
fn api_url(repository: &str) -> Option<String> {
    let rest = repository
        .trim()
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .strip_prefix("https://github.com/")?;
    let mut it = rest.split('/');
    let (owner, repo) = (it.next()?, it.next()?);
    if owner.is_empty() || repo.is_empty() || owner.eq_ignore_ascii_case("OWNER") {
        return None;
    }
    Some(format!(
        "https://api.github.com/repos/{owner}/{repo}/releases/latest"
    ))
}

/// Read a GitHub "latest release" response.
pub fn parse_release(body: &serde_json::Value, current: &str) -> AppResult<UpdateInfo> {
    let latest = body
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AppError::Network("the release page did not say which version is latest".into())
        })?
        .to_string();
    let url = body
        .get("html_url")
        .and_then(|v| v.as_str())
        .filter(|u| u.starts_with("https://"))
        .unwrap_or("")
        .to_string();
    Ok(UpdateInfo {
        newer: is_newer(&latest, current),
        current: current.to_string(),
        latest,
        url,
    })
}

#[tauri::command]
pub async fn update_check() -> AppResult<UpdateInfo> {
    let url = api_url(REPOSITORY)
        .ok_or_else(|| AppError::Invalid("update checks are not set up for this build".into()))?;
    fetch_latest(&url, VERSION).await
}

/// Ask a releases address which version is latest and compare it with `current`.
pub async fn fetch_latest(url: &str, current: &str) -> AppResult<UpdateInfo> {
    let client = reqwest::Client::builder()
        .redirect(crate::net::redirect_policy())
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(format!("Coin-Heaven/{VERSION}"))
        .build()
        .map_err(|e| AppError::Network(e.to_string()))?;
    let resp = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| AppError::Network(format!("could not reach the release page: {e}")))?;
    let status = resp.status();
    if status.as_u16() == 404 {
        // No release has been published yet.
        return Ok(UpdateInfo {
            current: current.into(),
            latest: current.into(),
            url: String::new(),
            newer: false,
        });
    }
    if !status.is_success() {
        return Err(AppError::Network(format!(
            "the release page answered {status}"
        )));
    }
    let text = crate::net::text_capped(resp, crate::net::MAX_BODY).await?;
    let body: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| AppError::Network("the release page did not return JSON".into()))?;
    parse_release(&body, current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number_not_by_text() {
        assert!(is_newer("v1.2.10", "1.2.9"));
        assert!(is_newer("2.0.0", "1.99.99"));
        assert!(is_newer("1.0.1", "1.0"));
        assert!(!is_newer("v1.0.0", "1.0.0"));
        assert!(!is_newer("0.9.0", "1.0.0"));
        assert!(!is_newer("garbage", "1.0.0"));
        assert!(
            is_newer("1.1.0-beta.1", "1.0.0"),
            "the number before - counts"
        );
    }

    #[test]
    fn only_a_real_repository_is_asked() {
        assert_eq!(
            api_url("https://github.com/someone/coin-heaven").as_deref(),
            Some("https://api.github.com/repos/someone/coin-heaven/releases/latest")
        );
        assert_eq!(
            api_url("https://github.com/someone/coin-heaven.git/").as_deref(),
            Some("https://api.github.com/repos/someone/coin-heaven/releases/latest")
        );
        assert_eq!(
            api_url("https://github.com/OWNER/coin-heaven"),
            None,
            "the placeholder"
        );
        assert_eq!(api_url(""), None);
        assert_eq!(api_url("https://example.com/a/b"), None, "only GitHub");
    }

    #[test]
    fn a_release_response_is_read() {
        let r = serde_json::json!({
            "tag_name": "v1.1.0",
            "html_url": "https://github.com/someone/coin-heaven/releases/tag/v1.1.0",
        });
        let u = parse_release(&r, "1.0.0").unwrap();
        assert!(u.newer);
        assert_eq!(u.latest, "v1.1.0");
        assert!(u.url.ends_with("v1.1.0"));
        assert!(!parse_release(&r, "1.1.0").unwrap().newer);
        // An address that is not https is not offered to open.
        let odd = serde_json::json!({ "tag_name": "v2", "html_url": "javascript:alert(1)" });
        assert_eq!(parse_release(&odd, "1.0.0").unwrap().url, "");
        assert!(parse_release(&serde_json::json!({}), "1.0.0").is_err());
    }

    /// Needs the network: `cargo test live_release -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn live_release_page_answers_and_is_read() {
        let info = fetch_latest(
            "https://api.github.com/repos/cli/cli/releases/latest",
            "0.0.1",
        )
        .await
        .unwrap();
        assert!(info.newer, "{info:?}");
        assert!(info.url.starts_with("https://github.com/cli/cli/releases/"));
        let none = fetch_latest(
            "https://api.github.com/repos/cli/this-repo-does-not-exist-xyz/releases/latest",
            "1.0.0",
        )
        .await
        .unwrap();
        assert!(!none.newer && none.url.is_empty());
    }
}
