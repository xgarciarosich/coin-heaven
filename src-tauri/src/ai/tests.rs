use super::*;
use provider::{Provider, ProviderConfig, build, extract_text};

fn cfg(model: &str) -> ProviderConfig {
    ProviderConfig {
        provider: "openai".into(),
        display_name: None,
        model: model.into(),
        base_url: None,
        is_enabled: true,
        max_output_tokens: Some(512),
        temperature: Some(0.0),
        system_prompt: None,
    }
}

fn items() -> Vec<VendorItem> {
    vec![
        VendorItem {
            id: 1,
            description: "MERCADONA VALENCIA 4471".into(),
            amount_minor: -4520,
        },
        VendorItem {
            id: 2,
            description: "Bizum de Ana Lopez ES9121000418450200051332".into(),
            amount_minor: 3000,
        },
    ]
}

/* ------------------------------------------------------------- payloads -- */

#[test]
fn aggregate_questions_carry_no_vendor_names_or_amounts() {
    let totals = vec![
        MonthlyTotal {
            month: "2026-01".into(),
            category: "Food & groceries".into(),
            net_major: -412.30,
        },
        MonthlyTotal {
            month: "2026-01".into(),
            category: "Housing".into(),
            net_major: -900.0,
        },
    ];
    let p = question_payload("How much did I spend on food in January?", &totals, "EUR");

    assert!(p.contains("Food & groceries"));
    assert!(p.contains("2026-01"));
    // The things that must never be in an analytics request:
    assert!(!p.contains("MERCADONA"));
    assert!(!p.contains("vendor"));
    assert!(!p.contains("transaction"));
}

#[test]
fn categorisation_redacts_structured_identifiers() {
    let (payload, n, red) = categorisation_payload(&items(), &["Food".into()], true);
    assert_eq!(n, 2);
    assert!(
        !payload.contains("ES9121000418450200051332"),
        "an IBAN reached the payload"
    );
    assert!(payload.contains("[IBAN]"));
    assert_eq!(red.ibans, 1);
}

#[test]
fn without_raw_permission_descriptions_are_folded_to_low_entropy_tokens() {
    let (raw, _, _) = categorisation_payload(&items(), &["Food".into()], true);
    let (folded, _, _) = categorisation_payload(&items(), &["Food".into()], false);

    assert!(raw.contains("MERCADONA VALENCIA"));
    // Folded loses case and the branch reference number.
    assert!(!folded.contains("MERCADONA VALENCIA 4471"));
    assert!(folded.contains("mercadona valencia"));
}

#[test]
fn redaction_does_not_and_cannot_remove_a_personal_name() {
    // Asserted so nobody mistakes masking for a complete filter. Keeping names
    // off the wire is the job of the egress policy, not the regex.
    let (payload, _, _) = categorisation_payload(&items(), &["Food".into()], true);
    assert!(payload.contains("Ana Lopez"));
}

#[test]
fn a_payload_hash_is_stable_and_changes_with_content() {
    let a = hash("one");
    assert_eq!(a, hash("one"));
    assert_ne!(a, hash("two"));
    assert_eq!(a.len(), 64);
}

#[test]
fn the_preview_reports_exactly_what_would_be_sent() {
    let (payload, n, red) = categorisation_payload(&items(), &["Food".into()], true);
    let p = preview("openai", "gpt-x", "categorize", payload.clone(), n, red);

    assert_eq!(
        p.payload, payload,
        "the preview must not be an abridged version"
    );
    assert_eq!(p.bytes, payload.len());
    assert_eq!(p.payload_sha256, hash(&payload));
    assert_eq!(p.row_count, 2);
}

/* ------------------------------------------------------------ requests -- */

#[test]
fn anthropic_uses_its_own_header_and_message_shape() {
    let mut c = cfg("claude-x");
    c.provider = "anthropic".into();
    let r = build(Provider::Anthropic, &c, "KEY", "sys", "hello");

    assert_eq!(r.url, "https://api.anthropic.com/v1/messages");
    assert!(
        r.headers
            .iter()
            .any(|(k, v)| k == "x-api-key" && v == "KEY")
    );
    assert!(r.headers.iter().any(|(k, _)| k == "anthropic-version"));
    assert_eq!(r.body["system"], "sys");
    assert_eq!(r.body["messages"][0]["content"], "hello");
    // Anthropic has no "system" role in messages.
    assert!(r.body["messages"].as_array().unwrap().len() == 1);
}

#[test]
fn openai_style_providers_share_one_shape_and_differ_only_in_host() {
    for (p, host) in [
        (Provider::Openai, "https://api.openai.com"),
        (Provider::Deepseek, "https://api.deepseek.com"),
        (Provider::Moonshot, "https://api.moonshot.cn"),
    ] {
        let r = build(p, &cfg("m"), "KEY", "sys", "hi");
        assert_eq!(r.url, format!("{host}/v1/chat/completions"));
        assert_eq!(r.body["messages"][0]["role"], "system");
        assert_eq!(r.body["messages"][1]["content"], "hi");
        assert!(
            r.headers
                .iter()
                .any(|(k, v)| k == "authorization" && v == "Bearer KEY")
        );
    }
}

#[test]
fn gemini_puts_the_key_in_a_header_never_the_url() {
    let r = build(Provider::Gemini, &cfg("gemini-x"), "SECRET", "sys", "hi");
    assert!(
        !r.url.contains("SECRET"),
        "a key in the query string ends up in proxy and server logs"
    );
    assert!(
        r.headers
            .iter()
            .any(|(k, v)| k == "x-goog-api-key" && v == "SECRET")
    );
    assert_eq!(r.body["contents"][0]["parts"][0]["text"], "hi");
}

#[test]
fn a_local_provider_needs_no_key_and_is_recognised_as_local() {
    let r = build(Provider::Ollama, &cfg("llama"), "", "sys", "hi");
    assert!(r.url.starts_with("http://localhost:11434"));
    assert!(!r.headers.iter().any(|(k, _)| k == "authorization"));
    assert!(Provider::Ollama.is_local());
    assert!(!Provider::Ollama.requires_key());
    assert!(Provider::Openai.requires_key());
}

#[test]
fn a_custom_base_url_overrides_the_default() {
    let mut c = cfg("m");
    c.base_url = Some("https://gateway.internal/v1/".into());
    let r = build(Provider::OpenaiCompatible, &c, "K", "s", "u");
    assert_eq!(r.url, "https://gateway.internal/v1/v1/chat/completions");
}

/* ------------------------------------------------------------ replies --- */

#[test]
fn extracts_text_from_each_envelope() {
    let openai = serde_json::json!({ "choices": [{ "message": { "content": "hello" } }] });
    assert_eq!(
        extract_text(Provider::Openai, &openai).as_deref(),
        Some("hello")
    );

    let anthropic = serde_json::json!({ "content": [{ "type": "text", "text": "hi" }] });
    assert_eq!(
        extract_text(Provider::Anthropic, &anthropic).as_deref(),
        Some("hi")
    );

    let gemini = serde_json::json!({
        "candidates": [{ "content": { "parts": [{ "text": "yo" }] } }]
    });
    assert_eq!(
        extract_text(Provider::Gemini, &gemini).as_deref(),
        Some("yo")
    );

    // A malformed envelope yields None rather than panicking.
    assert_eq!(extract_text(Provider::Openai, &serde_json::json!({})), None);
}

#[test]
fn suggestions_survive_the_prose_and_fences_models_add() {
    let plain = r#"[{"id":1,"category":"Food"}]"#;
    assert_eq!(parse_suggestions(plain).len(), 1);

    let fenced = "```json\n[{\"id\":1,\"category\":\"Food\"}]\n```";
    assert_eq!(parse_suggestions(fenced)[0].category, "Food");

    let chatty = "Sure! Here you go:\n[{\"id\":7,\"category\":\"Transport\"}]\nHope that helps.";
    let s = parse_suggestions(chatty);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].id, 7);

    assert!(parse_suggestions("I cannot help with that.").is_empty());
}

#[test]
fn provider_names_round_trip() {
    for name in [
        "anthropic",
        "openai",
        "gemini",
        "deepseek",
        "qwen",
        "moonshot",
        "zai",
        "openai_compatible",
        "ollama",
    ] {
        assert_eq!(Provider::parse(name).unwrap().as_str(), name);
    }
    assert!(Provider::parse("nope").is_none());
}

mod search {
    use crate::ai::provider::*;

    fn cfg(model: &str) -> ProviderConfig {
        ProviderConfig {
            provider: "anthropic".into(),
            display_name: None,
            model: model.into(),
            base_url: None,
            is_enabled: true,
            max_output_tokens: None,
            temperature: Some(0.0),
            system_prompt: None,
        }
    }

    #[test]
    fn claude_gets_the_search_tool_its_model_accepts() {
        let r = build_search(Provider::Anthropic, &cfg("claude-opus-5"), "k", "s", "u").unwrap();
        assert_eq!(r.body["tools"][0]["type"], "web_search_20260209");
        assert_eq!(r.body["tools"][0]["name"], "web_search");
        assert!(
            r.body.get("temperature").is_none(),
            "current models reject sampling params"
        );
        let old =
            build_search(Provider::Anthropic, &cfg("claude-haiku-4-5"), "k", "s", "u").unwrap();
        assert_eq!(old.body["tools"][0]["type"], "web_search_20250305");
    }

    #[test]
    fn a_model_nobody_has_heard_of_yet_gets_the_current_tool_and_can_fall_back() {
        let mut r =
            build_search(Provider::Anthropic, &cfg("claude-opus-9"), "k", "s", "u").unwrap();
        assert_eq!(r.body["tools"][0]["type"], "web_search_20260209");
        // If the provider refuses it, one swap tries the other version.
        assert!(other_search_tool(&mut r));
        assert_eq!(r.body["tools"][0]["type"], "web_search_20250305");
        // A request with no search tool has nothing to swap.
        let mut plain = build(Provider::Anthropic, &cfg("claude-opus-9"), "k", "s", "u");
        assert!(!other_search_tool(&mut plain));
    }

    #[test]
    fn providers_without_search_are_refused_rather_than_asked_to_remember() {
        assert!(build_search(Provider::Deepseek, &cfg("x"), "k", "s", "u").is_none());
        assert!(build_search(Provider::Ollama, &cfg("x"), "k", "s", "u").is_none());
    }

    #[test]
    fn text_and_sources_come_out_of_a_claude_search_reply() {
        let body = serde_json::json!({
            "stop_reason": "end_turn",
            "content": [
                { "type": "text", "text": "Looking it up. " },
                { "type": "server_tool_use", "id": "x", "name": "web_search", "input": {"query": "btc"} },
                { "type": "web_search_tool_result", "tool_use_id": "x", "content": [
                    { "type": "web_search_result", "url": "https://example.com/btc", "title": "BTC" }
                ]},
                { "type": "text", "text": "[{\"id\":1}]", "citations": [
                    { "type": "web_search_result_location", "url": "https://example.com/btc", "cited_text": "…" }
                ]}
            ]
        });
        assert_eq!(
            extract_all_text(Provider::Anthropic, &body).unwrap(),
            "Looking it up. [{\"id\":1}]"
        );
        assert_eq!(
            source_urls(Provider::Anthropic, &body),
            vec!["https://example.com/btc"]
        );
        assert!(!paused(Provider::Anthropic, &body));
    }

    #[test]
    fn a_search_that_errors_is_not_mistaken_for_results() {
        let body = serde_json::json!({ "content": [
            { "type": "web_search_tool_result", "content": { "type": "web_search_tool_result_error", "error_code": "max_uses_exceeded" } }
        ]});
        assert!(source_urls(Provider::Anthropic, &body).is_empty());
    }
}

mod context {
    use super::super::*;

    fn items() -> Vec<MerchantItem> {
        vec![MerchantItem {
            id: 0,
            merchant: "iberdrola".into(),
            example: "RECIBO IBERDROLA ES91 2100 0418 4502 0005 1332 REF 7781".into(),
            money_in: false,
            typical: 7_180,
            count: 9,
        }]
    }

    #[test]
    fn a_categorisation_request_carries_the_context_a_bookkeeper_would_need() {
        let (payload, red) = merchant_payload(
            &items(),
            &[
                ("Housing".into(), "expense".into()),
                ("Salary".into(), "income".into()),
            ],
            &[("mercadona".into(), "Food".into())],
            "Spain",
            "EUR",
            true,
        );
        assert!(
            payload.ends_with(ANSWER_CUE),
            "ends on the start of the answer"
        );
        let json = &payload[..payload.rfind('}').unwrap() + 1];
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(v["country"], "Spain");
        assert_eq!(v["categories"][0]["kind"], "expense");
        assert_eq!(
            v["already_filed_by_this_household"][0]["merchant"],
            "mercadona"
        );
        let m = &v["merchants_to_file"][0];
        assert_eq!(m["direction"], "money out");
        assert_eq!(m["typical_amount"], 7_100, "rounded, not the bill itself");
        assert!(
            !payload.contains("2100 0418"),
            "the IBAN is masked: {payload}"
        );
        assert_eq!(red.ibans, 1);
        // What comes back is matched against exactly what was asked.
        assert_eq!(
            merchants_in(&payload).get(&0).map(String::as_str),
            Some("iberdrola")
        );
    }

    #[test]
    fn answers_survive_fences_nulls_and_one_bad_entry() {
        let reply = "```json\n[{\"id\":0,\"category\":\"Housing\",\"confidence\":0.9},\
                     {\"id\":1,\"category\":null},{\"id\":\"x\"}]\n```";
        let a = parse_merchant_answers(reply);
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].category.as_deref(), Some("Housing"));
        assert_eq!(a[1].category, None);
        assert!(parse_merchant_answers("Sorry, I cannot do that.").is_empty());
    }

    #[test]
    fn the_chat_keeps_to_finance_and_sends_totals_not_identifiers() {
        let system = chat_system("Spanish (Spain)", "EUR", "Spain");
        for must in [
            "personal finance",
            "not a licensed adviser",
            "data",
            "Spanish (Spain)",
        ] {
            assert!(system.contains(must), "missing: {must}");
        }
        let context = serde_json::json!({ "net_worth": { "total": 164000 } });
        let messages = vec![
            ChatMessage {
                role: "user".into(),
                content: "How am I doing?".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "Well.".into(),
            },
            ChatMessage {
                role: "user".into(),
                content: "My IBAN is ES91 2100 0418 4502 0005 1332, can I retire?".into(),
            },
        ];
        let (payload, red) = chat_payload(&context, &messages);
        assert!(payload.contains("\"total\": 164000"));
        assert!(payload.contains("Coin Heaven: Well."));
        assert!(
            !payload.contains("2100 0418"),
            "what is typed is masked too"
        );
        assert_eq!(red.ibans, 1);
        assert!(payload.ends_with("Answer the Person's last message.\nCoin Heaven:"));
    }

    #[test]
    fn exa_results_are_read_with_their_highlights() {
        let body = serde_json::json!({ "results": [
            { "title": "Bitcoin\n price", "url": "https://www.coindesk.com/price/bitcoin",
              "highlights": ["BTC is trading at", "93,210.55 EUR"] },
            { "title": "summary only", "url": "https://example.com/x", "summary": "A page." },
            { "title": "no url" },
        ], "requestId": "r", "costDollars": { "total": 0.007 } });
        let hits = exa_hits(&body, 5);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].title, "Bitcoin price");
        assert_eq!(hits[0].snippet, "BTC is trading at … 93,210.55 EUR");
        assert_eq!(hits[1].snippet, "A page.");
        assert!(exa_hits(&serde_json::json!({ "error": "x" }), 5).is_empty());
    }

    #[test]
    fn duckduckgo_results_are_read_ads_dropped_and_text_cleaned() {
        let html = include_str!("../../tests/fixtures/duckduckgo.html");
        let hits = ddg_hits(html, 10);
        assert!(!hits.is_empty(), "the fixture holds organic results");
        assert!(
            hits.iter().all(|h| h.url.starts_with("http")
                && !h.url.contains("duckduckgo.com/y.js")
                && !h.title.contains('<')
                && !h.snippet.contains('<')
                && !h.snippet.contains("&amp;")),
            "{hits:?}"
        );
        assert_eq!(hits[0].url, "https://www.binance.com/en/price/bitcoin/EUR");
        assert!(hits[0].title.starts_with("Bitcoin to EUR (BTC/EUR) Today"));
        assert!(
            hits[0].title.contains("Rate & Calculator"),
            "entities decoded"
        );
        assert!(
            hits[0].snippet.contains("1 BTC = €76.1K"),
            "{}",
            hits[0].snippet
        );
        assert_eq!(ddg_hits(html, 1).len(), 1);

        // A redirect carrying the page is unwrapped; an unrecognised page
        // yields nothing rather than a guess.
        let wrapped = r#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fa%3Fb%3D1&amp;rut=x">Example</a>
            <a class="result__snippet" href="x">Hello &#x27;there&#x27;</a>"#;
        let w = ddg_hits(wrapped, 5);
        assert_eq!(w[0].url, "https://example.com/a?b=1");
        assert_eq!(w[0].snippet, "Hello 'there'");
        assert!(ddg_hits("<html>please complete the captcha</html>", 5).is_empty());
    }

    #[test]
    fn search_results_are_read_and_handed_over_with_their_urls() {
        let body = serde_json::json!({ "results": [
            { "title": "Bitcoin price", "url": "https://www.coindesk.com/price/bitcoin",
              "content": "BTC is trading at 93,210.55 EUR" },
            { "title": "no url" },
            { "title": "ftp", "url": "ftp://x" },
        ]});
        let hits = searx_hits(&body, 5);
        assert_eq!(hits.len(), 1);
        let p = price_from_hits_payload(
            &[(1, "Bitcoin".into(), Some("BTC".into()), hits)],
            "2026-10-05",
            "EUR",
        );
        assert!(p.contains("### id 1: Bitcoin (BTC)"));
        assert!(p.contains("https://www.coindesk.com/price/bitcoin"));
        assert!(p.contains("using only these results"));
        assert_eq!(
            price_query("Bitcoin", Some("BTC"), "EUR"),
            "Bitcoin BTC price EUR today"
        );
    }
}

#[test]
fn a_local_model_is_asked_for_a_context_window_it_can_actually_load() {
    let r = build(Provider::Ollama, &cfg("qwen3:8b"), "", "sys", "hello");
    assert!(r.url.ends_with("/api/chat"), "the native route: {}", r.url);
    assert_eq!(r.body["options"]["num_ctx"], 16_384);
    assert_eq!(r.body["stream"], false);
    assert_eq!(r.body["think"], false);
    assert!(r.headers.iter().all(|(k, _)| k != "authorization"));
    // A model with no reasoning switch gets the same request without it.
    let mut again = r.clone();
    assert!(provider::without_think(&mut again));
    assert!(again.body.get("think").is_none());
    assert!(!provider::without_think(&mut again), "nothing left to drop");
    // And its reply is read from its own envelope, reasoning removed.
    let body = serde_json::json!({ "message": { "role": "assistant",
        "content": "<think>let me see</think>\nA third." } });
    assert_eq!(
        extract_text(Provider::Ollama, &body).as_deref(),
        Some("A third.")
    );
    // A model that prints its end marker and rambles on is cut at the marker,
    // and asked not to ramble in the first place.
    let rambling = serde_json::json!({ "message": {
        "content": "It is 33%.<|endoftext|>To convert a fraction to a percentage..." } });
    assert_eq!(
        extract_text(Provider::Ollama, &rambling).as_deref(),
        Some("It is 33%.")
    );
    assert!(
        r.body["options"]["stop"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m == "<|endoftext|>")
    );
}

mod reading_results {
    use super::super::*;

    fn hit(title: &str, url: &str, snippet: &str) -> SearchHit {
        SearchHit {
            title: title.into(),
            url: url.into(),
            snippet: snippet.into(),
        }
    }

    #[test]
    fn numbers_are_read_both_ways_where_the_separators_are_ambiguous() {
        let has = |text: &str, v: f64| numbers_in(text).iter().any(|n| (n - v).abs() < 1e-6);
        assert!(has("1 BTC = €76,006.63 today", 76006.63));
        assert!(has("precio 1.234,56 €", 1234.56));
        assert!(has("cotiza a 173,64", 173.64));
        assert!(has("173,64", 173.64));
        assert!(has("EUR 76,006", 76006.0));
        assert!(has("1 234,56", 1234.56));
        assert!(has("12.500", 12500.0) && has("12.500", 12.5));
        assert!(has("1.234.567", 1_234_567.0));
        assert!(!has("up 0.25% in 24h", 76006.63));
    }

    #[test]
    fn a_price_must_be_quoted_from_a_result_and_be_in_the_quote() {
        let hits = vec![
            hit(
                "Bitcoin to EUR",
                "https://a.example/btc",
                "The BTC to EUR conversion rate today is €76,006.63. Up 0.31% in 24h.",
            ),
            hit("Other", "https://b.example/x", "Nothing useful here."),
        ];
        // Right words, right number: accepted, and the source is the hit.
        let ok = check_evidence(76006.63, "conversion rate today is €76,006.63", &hits);
        assert_eq!(ok.map(|h| h.url.as_str()), Some("https://a.example/btc"));
        // Case, spacing and a trailing ellipsis do not matter.
        assert!(check_evidence(76006.63, "  RATE today is €76,006.63 …", &hits).is_some());
        // The number given is not the one in the quote.
        assert!(check_evidence(76100.0, "conversion rate today is €76,006.63", &hits).is_none());
        // A tidied quote still passes when the number is on a page it was shown…
        assert!(check_evidence(76006.63, "BTC is priced at €76,006.63", &hits).is_some());
        // …and a number on no page does not, however it is quoted.
        assert!(check_evidence(80000.0, "BTC is priced at €80,000", &hits).is_none());
        // A percentage is not a price.
        assert!(check_evidence(0.31, "Up 0.31% in 24h", &hits).is_some());
        assert!(check_evidence(76006.63, "", &hits).is_none());
        // An abbreviated figure cannot stand in for an exact one.
        let k = vec![hit("t", "https://c.example", "1 BTC = €76.1K")];
        assert!(check_evidence(76100.0, "1 BTC = €76.1K", &k).is_none());
    }

    #[test]
    fn the_reader_is_told_to_reply_with_json_only() {
        for must in [
            "exactly one JSON object and nothing else",
            "no markdown",
            "Never calculate",
            "evidence",
            "A guess is a failure",
        ] {
            assert!(READER_SYSTEM.contains(must), "missing: {must}");
        }
        let p = price_from_hits_payload(&[], "2026-10-05", "EUR");
        assert!(p.contains("\"evidence\"") && p.contains("{\"prices\":"));
        assert!(!p.contains("\"note\"") && !p.contains("\"date\""));
        assert!(!p.contains("Search the web"));
    }
}

mod untidy_replies {
    use super::super::*;

    #[test]
    fn answers_are_found_however_the_model_wrapped_them() {
        let ids = |s: &str| -> Vec<i64> {
            answer_items(s)
                .iter()
                .filter_map(|v| v["id"].as_i64())
                .collect()
        };
        // Clean.
        assert_eq!(
            ids(r#"[{"id":1,"price":1.5},{"id":2,"price":null}]"#),
            [1, 2]
        );
        // Fenced, with a sentence before and a stray bracket after.
        assert_eq!(
            ids("Sure! ```json\n[{\"id\":1,\"note\":\"see [1]\"}]\n``` Done [ok]"),
            [1]
        );
        // Wrapped in an object.
        assert_eq!(ids(r#"{"prices":[{"id":1},{"id":2}]}"#), [1, 2]);
        // Cut off mid-way: the finished rows survive.
        assert_eq!(
            ids(r#"[{"id":1,"price":9.5,"note":"ok"},{"id":2,"price":3.2,"evid"#),
            [1]
        );
        // Nothing at all.
        assert!(answer_items("I could not find any prices.").is_empty());
        assert!(answer_items("").is_empty());
    }
}

mod json_mode {
    use super::super::provider::*;

    fn cfg(model: &str) -> ProviderConfig {
        ProviderConfig {
            provider: "x".into(),
            display_name: None,
            model: model.into(),
            base_url: None,
            is_enabled: true,
            max_output_tokens: Some(8000),
            temperature: Some(0.0),
            system_prompt: None,
        }
    }

    #[test]
    fn the_provider_is_asked_to_answer_in_json_and_the_ask_can_be_withdrawn() {
        for (p, key, val) in [
            (Provider::Deepseek, "/response_format/type", "json_object"),
            (Provider::Openai, "/response_format/type", "json_object"),
            (Provider::Ollama, "/format", "json"),
            (
                Provider::Gemini,
                "/generationConfig/responseMimeType",
                "application/json",
            ),
        ] {
            let mut r = build(p, &cfg("m"), "k", "sys", "user");
            assert!(json_mode(p, &mut r));
            assert_eq!(
                r.body.pointer(key).and_then(|v| v.as_str()),
                Some(val),
                "{p:?}"
            );
            assert!(without_json_mode(&mut r), "{p:?}");
            assert!(r.body.pointer(key).is_none(), "{p:?}");
            assert!(!without_json_mode(&mut r), "nothing left to take out");
        }
        let mut r = build(Provider::Anthropic, &cfg("m"), "k", "sys", "user");
        assert!(
            !json_mode(Provider::Anthropic, &mut r),
            "Anthropic has no such switch"
        );
    }
}

mod decoding {
    use super::*;

    #[test]
    fn percent_decoding_never_panics_on_multibyte_text() {
        assert_eq!(percent_decode("a%41b"), "aAb");
        assert_eq!(percent_decode("100%é price"), "100%é price");
        assert_eq!(percent_decode("%4é1"), "%4é1");
        assert_eq!(percent_decode("%é"), "%é");
    }
}

mod endpoints {
    use super::*;
    use provider::{HostClass, check_endpoint, host_class};

    #[test]
    fn only_this_machine_counts_as_local() {
        assert!(Provider::Ollama.is_local_at(None));
        assert!(Provider::Ollama.is_local_at(Some("http://localhost:11434")));
        assert!(Provider::Ollama.is_local_at(Some("http://127.0.0.1:11434")));
        assert!(Provider::Ollama.is_local_at(Some("http://[::1]:11434")));
        assert!(!Provider::Ollama.is_local_at(Some("http://192.168.1.20:11434")));
        assert!(!Provider::Ollama.is_local_at(Some("https://gpu.example.com")));
        assert!(!Provider::Openai.is_local_at(None));
    }

    #[test]
    fn plain_http_is_for_this_machine_and_private_networks_only() {
        assert!(check_endpoint("https://api.example.com").is_ok());
        assert!(check_endpoint("http://localhost:11434").is_ok());
        assert!(check_endpoint("http://192.168.1.20:8080").is_ok());
        assert!(check_endpoint("http://server.local:8080").is_ok());
        assert!(check_endpoint("http://api.example.com").is_err());
        assert!(check_endpoint("http://8.8.8.8").is_err());
        assert!(check_endpoint("file:///etc/passwd").is_err());
        assert!(check_endpoint("not a url").is_err());
        assert_eq!(host_class("http://10.0.0.5"), HostClass::PrivateNetwork);
    }
}
