//! Provider shapes.
//!
//! Most providers speak the OpenAI chat-completions dialect, so they share one
//! request builder and differ only in base URL. Anthropic and Gemini have their
//! own shapes and get their own arms — guessing that "close enough" would work
//! is how silent 400s happen.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Anthropic,
    Openai,
    Gemini,
    Deepseek,
    Qwen,
    Moonshot,
    Zai,
    /// Any OpenAI-compatible endpoint the user points at.
    OpenaiCompatible,
    /// Local, and the only configuration fully consistent with this app's
    /// privacy claims: nothing leaves the machine.
    Ollama,
}

impl Provider {
    pub fn parse(s: &str) -> Option<Provider> {
        Some(match s {
            "anthropic" => Provider::Anthropic,
            "openai" => Provider::Openai,
            "gemini" => Provider::Gemini,
            "deepseek" => Provider::Deepseek,
            "qwen" => Provider::Qwen,
            "moonshot" => Provider::Moonshot,
            "zai" => Provider::Zai,
            "openai_compatible" => Provider::OpenaiCompatible,
            "ollama" => Provider::Ollama,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::Openai => "openai",
            Provider::Gemini => "gemini",
            Provider::Deepseek => "deepseek",
            Provider::Qwen => "qwen",
            Provider::Moonshot => "moonshot",
            Provider::Zai => "zai",
            Provider::OpenaiCompatible => "openai_compatible",
            Provider::Ollama => "ollama",
        }
    }

    pub fn default_base_url(self) -> &'static str {
        match self {
            Provider::Anthropic => "https://api.anthropic.com",
            Provider::Openai => "https://api.openai.com",
            Provider::Gemini => "https://generativelanguage.googleapis.com",
            Provider::Deepseek => "https://api.deepseek.com",
            Provider::Qwen => "https://dashscope-intl.aliyuncs.com/compatible-mode",
            Provider::Moonshot => "https://api.moonshot.cn",
            Provider::Zai => "https://api.z.ai/api/paas/v4",
            Provider::OpenaiCompatible => "",
            Provider::Ollama => "http://localhost:11434",
        }
    }

    pub fn requires_key(self) -> bool {
        !matches!(self, Provider::Ollama)
    }

    /// True when the endpoint is on this machine, so "sending" it never leaves.
    pub fn is_local(self) -> bool {
        matches!(self, Provider::Ollama)
    }

    /// Whether *this address* is on this machine. Ollama pointed at another
    /// computer is not "local", whatever the provider is called.
    pub fn is_local_at(self, base_url: Option<&str>) -> bool {
        if !self.is_local() {
            return false;
        }
        let base = base_url
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| self.default_base_url());
        host_class(base) == HostClass::Loopback
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum HostClass {
    Loopback,
    /// A private-network or `.local` address: plain http is tolerable there.
    PrivateNetwork,
    Remote,
}

/// Where an address points, judged from its text alone (no lookups).
pub fn host_class(base_url: &str) -> HostClass {
    let Ok(url) = reqwest::Url::parse(base_url.trim()) else {
        return HostClass::Remote;
    };
    let Some(host) = url.host_str() else {
        return HostClass::Remote;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => {
            if ip.is_loopback() {
                HostClass::Loopback
            } else if ip.is_private() || ip.is_link_local() {
                HostClass::PrivateNetwork
            } else {
                HostClass::Remote
            }
        }
        Ok(std::net::IpAddr::V6(ip)) => {
            let first = ip.segments()[0];
            if ip.is_loopback() {
                HostClass::Loopback
            } else if (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80 {
                HostClass::PrivateNetwork
            } else {
                HostClass::Remote
            }
        }
        Err(_) => {
            let d = host.to_ascii_lowercase();
            if d == "localhost" || d.ends_with(".localhost") {
                HostClass::Loopback
            } else if d.ends_with(".local") || d.ends_with(".lan") || d.ends_with(".internal") {
                HostClass::PrivateNetwork
            } else {
                HostClass::Remote
            }
        }
    }
}

/// An address a request may be sent to: http(s) only, and plain http only to
/// this machine or a private network — a key and finance data in clear text
/// across the internet is refused.
pub fn check_endpoint(base_url: &str) -> Result<(), String> {
    let url = reqwest::Url::parse(base_url.trim())
        .map_err(|_| "that address is not a valid URL".to_string())?;
    match url.scheme() {
        "https" => Ok(()),
        "http" if host_class(base_url) != HostClass::Remote => Ok(()),
        "http" => Err("a remote address must use https".into()),
        _ => Err("the address must start with http:// or https://".into()),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub provider: String,
    pub display_name: Option<String>,
    pub model: String,
    pub base_url: Option<String>,
    pub is_enabled: bool,
    pub max_output_tokens: Option<i64>,
    pub temperature: Option<f64>,
    pub system_prompt: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Request {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: serde_json::Value,
}

const DEFAULT_MAX_TOKENS: i64 = 1024;

/// The context window asked of a local model: room for the largest request
/// Coin Heaven builds, several times over, and small enough to load on a laptop.
const OLLAMA_CONTEXT: i64 = 16_384;

/// End-of-turn markers that leak into the text of a mis-templated model.
const END_MARKERS: [&str; 4] = ["<|endoftext|>", "<|im_end|>", "<|eot_id|>", "<|end|>"];

/// Drop the `think` switch from an Ollama request, for one retry after a
/// model that has no such switch refuses it. False when there was none.
pub fn without_think(req: &mut Request) -> bool {
    req.body
        .as_object_mut()
        .is_some_and(|o| o.remove("think").is_some())
}

/// Ask the provider itself to answer in JSON and nothing else.
///
/// A prompt can only request that; these switches make the endpoint refuse to
/// produce anything that is not a JSON value, which is the difference between
/// "usually" and "always" for a reply a program has to read. They are
/// requests a provider may not know — a self-hosted endpoint, an older
/// model — so `without_json_mode` takes them back out for a retry.
/// Returns whether this provider has such a switch (Anthropic's does not).
pub fn json_mode(p: Provider, req: &mut Request) -> bool {
    match p {
        Provider::Anthropic => false,
        Provider::Gemini => {
            req.body["generationConfig"]["responseMimeType"] =
                serde_json::json!("application/json");
            true
        }
        Provider::Ollama => {
            req.body["format"] = serde_json::json!("json");
            true
        }
        // DeepSeek, OpenAI, Qwen, Moonshot, Z.ai and compatible endpoints.
        // The mode wants an object at the top level, not a bare array.
        _ => {
            req.body["response_format"] = serde_json::json!({ "type": "json_object" });
            true
        }
    }
}

/// Take `json_mode` back out. True if there was anything to remove.
pub fn without_json_mode(req: &mut Request) -> bool {
    let Some(o) = req.body.as_object_mut() else {
        return false;
    };
    let mut removed = o.remove("response_format").is_some() | o.remove("format").is_some();
    if let Some(g) = o
        .get_mut("generationConfig")
        .and_then(|g| g.as_object_mut())
    {
        removed |= g.remove("responseMimeType").is_some();
    }
    removed
}

/// Assemble the HTTP request. Pure: no network, so it can be tested exactly.
pub fn build(
    p: Provider,
    cfg: &ProviderConfig,
    api_key: &str,
    system: &str,
    user: &str,
) -> Request {
    let base = cfg
        .base_url
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| p.default_base_url())
        .trim_end_matches('/')
        .to_string();
    let max_tokens = cfg.max_output_tokens.unwrap_or(DEFAULT_MAX_TOKENS);
    let temperature = cfg.temperature.unwrap_or(0.0);

    match p {
        Provider::Anthropic => Request {
            url: format!("{base}/v1/messages"),
            headers: vec![
                ("x-api-key".into(), api_key.into()),
                ("anthropic-version".into(), "2023-06-01".into()),
                ("content-type".into(), "application/json".into()),
            ],
            // No `temperature`: current Claude models reject sampling
            // parameters outright with a 400, and on older ones the default is
            // fine for bookkeeping. Sending it would break the assistant on
            // exactly the models people are most likely to configure.
            body: serde_json::json!({
                "model": cfg.model,
                "max_tokens": max_tokens,
                "system": system,
                "messages": [{ "role": "user", "content": user }],
            }),
        },
        Provider::Gemini => Request {
            // The key travels as a header, never in the URL: query strings end
            // up in logs and proxies.
            url: format!("{base}/v1beta/models/{}:generateContent", cfg.model),
            headers: vec![
                ("x-goog-api-key".into(), api_key.into()),
                ("content-type".into(), "application/json".into()),
            ],
            body: serde_json::json!({
                "systemInstruction": { "parts": [{ "text": system }] },
                "contents": [{ "role": "user", "parts": [{ "text": user }] }],
                "generationConfig": {
                    "temperature": temperature,
                    "maxOutputTokens": max_tokens,
                },
            }),
        },
        // Ollama is spoken to in its own dialect rather than through its
        // OpenAI-compatible route, for one reason: only the native API lets
        // the request set the context window. Left to the model's own
        // default — a quarter of a million tokens on some — a mid-sized
        // model asks for more memory than the machine has, and the whole
        // computer stalls on a question that needed a few thousand.
        Provider::Ollama => Request {
            url: format!("{base}/api/chat"),
            headers: vec![("content-type".into(), "application/json".into())],
            body: serde_json::json!({
                "model": cfg.model,
                "stream": false,
                // Answer, do not deliberate at length: these are bookkeeping
                // questions, and on a laptop the reasoning is most of the
                // wait. A model with no such switch refuses the field; the
                // caller retries without it (`without_think`).
                "think": false,
                "messages": [
                    { "role": "system", "content": system },
                    { "role": "user", "content": user },
                ],
                "options": {
                    "temperature": temperature,
                    "num_predict": max_tokens,
                    "num_ctx": OLLAMA_CONTEXT,
                    // A model imported with the wrong template never emits
                    // a real end-of-turn: it prints the marker as text and
                    // carries on until the length limit, answering questions
                    // nobody asked. Stopping on the markers themselves ends
                    // the turn where the model meant to.
                    "stop": END_MARKERS,
                },
            }),
        },
        // Everything else speaks the OpenAI dialect.
        _ => {
            let path = format!("{base}/v1/chat/completions");
            let mut headers = vec![("content-type".into(), "application/json".into())];
            if p.requires_key() {
                headers.push(("authorization".into(), format!("Bearer {api_key}")));
            }
            Request {
                url: path,
                headers,
                body: serde_json::json!({
                    "model": cfg.model,
                    "max_tokens": max_tokens,
                    "temperature": temperature,
                    "messages": [
                        { "role": "system", "content": system },
                        { "role": "user", "content": user },
                    ],
                }),
            }
        }
    }
}

/// Pull the assistant text out of whichever envelope came back.
pub fn extract_text(p: Provider, body: &serde_json::Value) -> Option<String> {
    match p {
        Provider::Anthropic => body
            .get("content")?
            .as_array()?
            .iter()
            .find_map(|c| c.get("text")?.as_str())
            .map(str::to_string),
        Provider::Gemini => body
            .get("candidates")?
            .as_array()?
            .first()?
            .get("content")?
            .get("parts")?
            .as_array()?
            .first()?
            .get("text")?
            .as_str()
            .map(str::to_string),
        Provider::Ollama => body
            .get("message")?
            .get("content")?
            .as_str()
            .map(strip_reasoning),
        _ => body
            .get("choices")?
            .as_array()?
            .first()?
            .get("message")?
            .get("content")?
            .as_str()
            .map(strip_reasoning),
    }
}

/// Remove a `<think>…</think>` preamble some open models put in the answer
/// itself. What the person asked for is what comes after it.
fn strip_reasoning(text: &str) -> String {
    // Nothing after a leaked end-of-turn marker is part of the answer.
    let text = END_MARKERS
        .iter()
        .filter_map(|m| text.find(m))
        .min()
        .map_or(text, |cut| &text[..cut]);
    match (text.find("<think>"), text.rfind("</think>")) {
        (Some(a), Some(b)) if a < b => format!("{}{}", &text[..a], &text[b + "</think>".len()..])
            .trim()
            .to_string(),
        _ => text.trim().to_string(),
    }
}

/* ----------------------------------------------------------- web search -- */

const SEARCH_TOOL: &str = "web_search_20260209";
const SEARCH_TOOL_BASIC: &str = "web_search_20250305";

/// The Claude web search tool version this model takes.
///
/// The dynamic-filtering variant is the default; only the model families
/// known to predate it get the basic tool. The list is of *old* names on
/// purpose: a model released after this was written is not on it, so it gets
/// the current tool rather than being mistaken for a legacy one. Should that
/// guess be wrong, [`other_search_tool`] lets the caller try the other.
fn claude_search_tool(model: &str) -> &'static str {
    let m = model.to_ascii_lowercase();
    let legacy = [
        "claude-3",
        "claude-2",
        "haiku",
        "opus-4-0",
        "opus-4-1",
        "opus-4-5",
        "opus-4-2",
        "sonnet-4-0",
        "sonnet-4-5",
        "sonnet-4-2",
    ];
    if legacy.iter().any(|n| m.contains(n)) {
        SEARCH_TOOL_BASIC
    } else {
        SEARCH_TOOL
    }
}

/// Swap a search request to the other tool version, for one retry after the
/// provider rejects the first. Returns false when there is nothing to swap.
pub fn other_search_tool(req: &mut Request) -> bool {
    let Some(t) = req.body.pointer_mut("/tools/0/type") else {
        return false;
    };
    let next = match t.as_str() {
        Some(SEARCH_TOOL) => SEARCH_TOOL_BASIC,
        Some(SEARCH_TOOL_BASIC) => SEARCH_TOOL,
        _ => return false,
    };
    *t = serde_json::json!(next);
    true
}

/// A request that lets the model search the web before answering, or `None`
/// for providers with no search built in — for those a price lookup would be
/// the model's memory dressed up as a fact.
pub fn build_search(
    p: Provider,
    cfg: &ProviderConfig,
    api_key: &str,
    system: &str,
    user: &str,
) -> Option<Request> {
    let mut req = build(p, cfg, api_key, system, user);
    match p {
        Provider::Anthropic => {
            req.body["tools"] = serde_json::json!([{
                "type": claude_search_tool(&cfg.model),
                "name": "web_search",
                "max_uses": 8,
            }]);
            // Search results and the reasoning over them need room; the
            // assistant's usual budget is sized for a sentence.
            req.body["max_tokens"] = serde_json::json!(8_000);
            Some(req)
        }
        Provider::Gemini => {
            req.body["tools"] = serde_json::json!([{ "google_search": {} }]);
            Some(req)
        }
        // Chat completions search only on OpenAI's search models, which
        // reject `temperature`; anything else answers the error itself.
        Provider::Openai => {
            req.body["web_search_options"] = serde_json::json!({});
            if let Some(o) = req.body.as_object_mut() {
                o.remove("temperature");
            }
            Some(req)
        }
        _ => None,
    }
}

/// All the text in a reply, joined. A search reply interleaves text blocks
/// with tool calls, so the first block is rarely the answer.
pub fn extract_all_text(p: Provider, body: &serde_json::Value) -> Option<String> {
    let joined = match p {
        Provider::Anthropic => body
            .get("content")?
            .as_array()?
            .iter()
            .filter(|c| c.get("type").and_then(|t| t.as_str()) == Some("text"))
            .filter_map(|c| c.get("text")?.as_str())
            .collect::<Vec<_>>()
            .join(""),
        Provider::Gemini => body
            .get("candidates")?
            .as_array()?
            .first()?
            .get("content")?
            .get("parts")?
            .as_array()?
            .iter()
            .filter_map(|p| p.get("text")?.as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => return extract_text(p, body),
    };
    (!joined.is_empty()).then_some(joined)
}

/// Every page the search actually returned or cited.
///
/// Used to check a price's claimed source against what was really read: a
/// source URL the search never touched is a source the model made up.
pub fn source_urls(p: Provider, body: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |u: Option<&str>| {
        if let Some(u) = u {
            out.push(u.to_string());
        }
    };
    match p {
        Provider::Anthropic => {
            for block in body
                .get("content")
                .and_then(|c| c.as_array())
                .into_iter()
                .flatten()
            {
                // Results: a list on success, an error object otherwise.
                if let Some(results) = block.get("content").and_then(|c| c.as_array()) {
                    for r in results {
                        push(r.get("url").and_then(|u| u.as_str()));
                    }
                }
                for cit in block
                    .get("citations")
                    .and_then(|c| c.as_array())
                    .into_iter()
                    .flatten()
                {
                    push(cit.get("url").and_then(|u| u.as_str()));
                }
            }
        }
        Provider::Gemini => {
            let chunks = body
                .pointer("/candidates/0/groundingMetadata/groundingChunks")
                .and_then(|c| c.as_array());
            for ch in chunks.into_iter().flatten() {
                push(ch.pointer("/web/uri").and_then(|u| u.as_str()));
            }
        }
        _ => {
            let anns = body
                .pointer("/choices/0/message/annotations")
                .and_then(|a| a.as_array());
            for a in anns.into_iter().flatten() {
                push(a.pointer("/url_citation/url").and_then(|u| u.as_str()));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// True when the server paused a long search turn and wants it resumed.
pub fn paused(p: Provider, body: &serde_json::Value) -> bool {
    p == Provider::Anthropic
        && body.get("stop_reason").and_then(|s| s.as_str()) == Some("pause_turn")
}
