//! What every outbound client shares: where a redirect may lead and how much
//! of a reply is read.
//!
//! Without these, a server (or anything squatting on a local port) could send
//! the request, API-key header included, on to another host, or stream a reply
//! large enough to exhaust memory before the timeout fires.

use crate::error::{AppError, AppResult};

/// The most any reply is read: a model answer or a results page is far smaller.
pub const MAX_BODY: usize = 8 * 1024 * 1024;

/// Follow a redirect only within the same host and never from https to http,
/// at most three times. A hop to another host would carry the request's
/// headers (an API key) and, for 307/308, the whole body with it.
pub fn redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        let same_host = attempt
            .previous()
            .last()
            .is_some_and(|prev| prev.host_str() == attempt.url().host_str());
        let downgrade = attempt
            .previous()
            .last()
            .is_some_and(|prev| prev.scheme() == "https" && attempt.url().scheme() != "https");
        if attempt.previous().len() >= 3 || !same_host || downgrade {
            attempt.stop()
        } else {
            attempt.follow()
        }
    })
}

/// Read a reply as text, giving up once it passes `max` bytes.
pub async fn text_capped(mut resp: reqwest::Response, max: usize) -> AppResult<String> {
    if resp.content_length().is_some_and(|n| n as usize > max) {
        return Err(AppError::Network("the reply was too large to read".into()));
    }
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| AppError::Network(e.to_string()))?
    {
        if buf.len() + chunk.len() > max {
            return Err(AppError::Network("the reply was too large to read".into()));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}
