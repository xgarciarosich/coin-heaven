//! Every command the interface calls is one the app has registered.
//!
//! A command that exists in Rust but is missing from `generate_handler!` fails
//! only when someone clicks the button that calls it ("command … not found"),
//! which no other test sees. This reads both lists and compares them.

use std::collections::BTreeSet;

fn called_by_the_interface() -> BTreeSet<String> {
    let ts = std::fs::read_to_string("../src/lib/ipc.ts").expect("src/lib/ipc.ts");
    let mut found = BTreeSet::new();
    for line in ts.lines() {
        let mut rest = line;
        while let Some(i) = rest.find("call<") {
            rest = &rest[i + 5..];
            // call<Type>("name"  — the name follows the closing `>(` of the type.
            if let Some(j) = rest.find(">(\"") {
                let after = &rest[j + 3..];
                if let Some(end) = after.find('"') {
                    found.insert(after[..end].to_string());
                }
            }
        }
    }
    found
}

fn registered() -> BTreeSet<String> {
    let lib = std::fs::read_to_string("src/lib.rs").expect("src/lib.rs");
    let start = lib.find("generate_handler![").expect("handler list");
    let body = &lib[start..];
    let body = &body[..body.find("])").expect("end of handler list")];
    body.lines()
        .map(|l| l.trim().trim_end_matches(',').trim())
        .filter(|l| l.starts_with("commands::"))
        .map(|l| l.rsplit("::").next().unwrap().to_string())
        .collect()
}

#[test]
fn every_command_the_interface_calls_is_registered() {
    let called = called_by_the_interface();
    assert!(
        called.len() > 100,
        "the scan found only {} calls",
        called.len()
    );
    let registered = registered();
    let missing: Vec<_> = called.difference(&registered).collect();
    assert!(missing.is_empty(), "called but not registered: {missing:?}");
}
