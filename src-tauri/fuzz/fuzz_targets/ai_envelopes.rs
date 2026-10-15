//! Everything else an AI provider or a search engine sends back, and
//! everything sent to them that is built from the person's own text.
//!
//! The first byte picks the job:
//!
//! 0. a provider's reply envelope: where the text is, which pages were cited,
//!    whether the turn was paused;
//! 1. search results, as SearXNG or Exa JSON or DuckDuckGo's page;
//! 2. the answer text itself: a mapping, suggestions, merchant answers;
//! 3. the requests built from the person's descriptions, which are held to the
//!    egress rule: no address or long number goes out in them.
#![no_main]
use coin_heaven_fuzz::{Input, Leak, json, leak};
use coin_heaven_lib::ai::provider::{self, Provider};
use coin_heaven_lib::ai::{self, MerchantItem, MonthlyTotal, SearchHit, VendorItem};
use libfuzzer_sys::fuzz_target;
use serde_json::Value;

const PROVIDERS: [Provider; 9] = [
    Provider::Anthropic,
    Provider::Openai,
    Provider::Gemini,
    Provider::Deepseek,
    Provider::Qwen,
    Provider::Moonshot,
    Provider::Zai,
    Provider::OpenaiCompatible,
    Provider::Ollama,
];

/// 0. The envelope of a reply.
fn envelope(inp: &mut Input) {
    let p = inp.pick(&PROVIDERS);
    let Some(body) = json(inp.rest()) else { return };

    if let Some(text) = provider::extract_text(p, &body) {
        // A reply that went through the reasoning strip is trimmed; the two
        // providers that take the first block as it is are not.
        if !matches!(p, Provider::Anthropic | Provider::Gemini) {
            assert_eq!(text, text.trim());
        }
    }
    if let Some(all) = provider::extract_all_text(p, &body) {
        assert!(!all.is_empty(), "an empty answer is no answer");
    }
    let urls = provider::source_urls(p, &body);
    assert!(urls.windows(2).all(|w| w[0] < w[1]), "sorted and unique: {urls:?}");
    if provider::paused(p, &body) {
        assert_eq!(p, Provider::Anthropic);
        assert_eq!(body["stop_reason"], "pause_turn");
    }
}

fn check_hits(hits: &[SearchHit], limit: usize) {
    assert!(hits.len() <= limit);
    for h in hits {
        assert!(h.url.starts_with("http"), "{:?}", h.url);
        assert!(h.title.chars().count() <= 400 && h.snippet.chars().count() <= 400);
        assert!(!h.url.contains("duckduckgo.com/y.js"), "an advertisement");
    }
}

/// 1. Search results.
fn results(inp: &mut Input) {
    let kind = inp.byte() % 3;
    let limit = usize::from(inp.byte() % 8);
    match kind {
        0 | 1 => {
            let Some(body) = json(inp.rest()) else { return };
            let hits = if kind == 0 {
                ai::searx_hits(&body, limit)
            } else {
                let h = ai::exa_hits(&body, limit);
                // Exa text is tidied: one line, single spaces.
                for x in &h {
                    for s in [&x.title, &x.snippet] {
                        assert!(!s.contains(['\n', '\t', '\r']), "{s:?}");
                    }
                }
                h
            };
            check_hits(&hits, limit);
        }
        _ => {
            let html = String::from_utf8_lossy(inp.rest());
            let hits = ai::ddg_hits(&html, limit);
            check_hits(&hits, limit);
            for x in &hits {
                for s in [&x.title, &x.snippet] {
                    assert!(!s.contains(['\n', '\t']), "{s:?}");
                }
            }
        }
    }
}

/// 2. The text of an answer.
fn answers(inp: &mut Input) {
    let reply = String::from_utf8_lossy(inp.rest()).into_owned();

    if let Some(arr) = ai::json_array(&reply) {
        assert!(arr.starts_with('[') && arr.ends_with(']') && arr.len() >= 2);
        assert!(reply.contains(arr));
        assert_eq!(reply.find('['), Some(reply.len() - reply.trim_start_matches(|c| c != '[').len()));
    }

    let hint = ai::parse_mapping_hint(&reply);
    assert_eq!(hint, ai::parse_mapping_hint(&reply), "deterministic");
    // Chatter around an object changes nothing: models say "Sure!".
    let t = reply.trim();
    if hint.is_some() && t.starts_with('{') && t.ends_with('}') {
        let chatty = format!("Sure! Here you go:\n{t}\nHope that helps.");
        assert_eq!(ai::parse_mapping_hint(&chatty), hint);
    }

    // Suggestions survive being written out and read back, bare or fenced.
    let suggestions = ai::parse_suggestions(&reply);
    let written = serde_json::to_string(&suggestions).unwrap();
    assert_eq!(ai::parse_suggestions(&written), suggestions);
    assert_eq!(
        ai::parse_suggestions(&format!("```json\n{written}\n```")),
        suggestions
    );

    for a in ai::parse_merchant_answers(&reply) {
        assert!(a.confidence.is_none_or(f64::is_finite));
    }
}

fn leaks_nothing(what: &str, text: &str) {
    if let Some(l) = leak(text) {
        match l {
            Leak::Email(e) => panic!("{what}: an address went out: {e:?}"),
            Leak::Number(n) => panic!("{what}: a number went out: {n:?}"),
            Leak::Glued(n) => panic!("{what}: a glued number went out: {n:?}"),
        }
    }
}

/// Only letters and the spaces between them: what normalising leaves.
fn plain_words(s: &str) -> bool {
    s.chars().all(|c| c == ' ' || c.is_alphabetic())
}

/// 3. Requests built from the person's text.
fn payloads(inp: &mut Input) {
    let allow_raw = inp.byte() & 1 == 1;
    let n = 1 + usize::from(inp.byte() % 4);

    let merchants: Vec<MerchantItem> = (0..n)
        .map(|i| MerchantItem {
            id: i as i64 * 7 + 1,
            merchant: inp.text(),
            example: inp.text(),
            money_in: inp.byte() & 1 == 1,
            typical: i64::from(i32::from_le_bytes([inp.byte(), inp.byte(), inp.byte(), inp.byte()])),
            count: i64::from(inp.byte()),
        })
        .collect();
    let categories = vec![("Food".to_string(), "expense".to_string()), (inp.text(), "income".into())];
    let habits = vec![(inp.text(), "Food".to_string())];

    let (payload, _) = ai::merchant_payload(&merchants, &categories, &habits, "Spain", "EUR", allow_raw);
    // What went out can be matched back to what was asked, id for id.
    let sent = ai::merchants_in(&payload);
    assert_eq!(sent.len(), merchants.len(), "{payload}");
    for m in &merchants {
        assert_eq!(sent.get(&m.id), Some(&m.merchant), "id {}", m.id);
    }
    // The part of a description that travels has been through the mask.
    let doc: Value = serde_json::from_str(&payload[..payload.rfind('}').unwrap() + 1])
        .expect("the request is a JSON document followed by its cue");
    for m in doc["merchants_to_file"].as_array().unwrap() {
        let example = m["example"].as_str().unwrap();
        leaks_nothing("merchant example", example);
        if !allow_raw {
            assert!(plain_words(example), "raw text went out: {example:?}");
        }
    }

    let vendors: Vec<VendorItem> = merchants
        .iter()
        .map(|m| VendorItem {
            id: m.id,
            description: m.example.clone(),
            amount_minor: m.typical,
        })
        .collect();
    let (payload, count, _) = ai::categorisation_payload(&vendors, &["Food".to_string()], allow_raw);
    assert_eq!(count, vendors.len());
    let doc: Value = serde_json::from_str(&payload).unwrap();
    for v in doc["vendors"].as_array().unwrap() {
        let vendor = v["vendor"].as_str().unwrap();
        leaks_nothing("vendor", vendor);
        if !allow_raw {
            assert!(plain_words(vendor), "raw text went out: {vendor:?}");
        }
    }

    // A question is masked wherever it sits in the request.
    let question = inp.text();
    let totals = [MonthlyTotal {
        month: "2026-01".into(),
        category: "Food".into(),
        net_major: -12.5,
    }];
    let asked = ai::question_payload(&question, &totals, "EUR");
    let doc: Value = serde_json::from_str(&asked).unwrap();
    leaks_nothing("question", doc["question"].as_str().unwrap());

    // Header and sample cells of a layout request: masked, the sample capped.
    let headers: Vec<String> = (0..3).map(|_| inp.text()).collect();
    let samples: Vec<Vec<String>> = (0..6).map(|_| vec![inp.text(), inp.text()]).collect();
    let (asked, _) = ai::mapping_payload(&headers, &samples);
    let doc: Value = serde_json::from_str(&asked).unwrap();
    for cell in doc["header"].as_array().unwrap() {
        leaks_nothing("header cell", cell.as_str().unwrap());
    }
    let rows = doc["sample_rows"].as_array().unwrap();
    assert!(rows.len() <= 4, "a handful of rows, never the file");
    for row in rows {
        for cell in row.as_array().unwrap() {
            leaks_nothing("sample cell", cell.as_str().unwrap());
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let mut inp = Input::new(data);
    match inp.byte() % 4 {
        0 => envelope(&mut inp),
        1 => results(&mut inp),
        2 => answers(&mut inp),
        _ => payloads(&mut inp),
    }
});
