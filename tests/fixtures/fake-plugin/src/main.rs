// SPDX-License-Identifier: MIT
//! Minimal ABI-speaking plugin for host tests. Behavior is driven by env:
//!   FAKE_MODE=echo_model  -> request model.chat, put content in result
//!   FAKE_MODE=crash        -> exit 3 before sending a result
//!   FAKE_MODE=ok           -> just send result ok exit 0
use std::io::{BufRead, Write};

fn main() {
    let mode = std::env::var("FAKE_MODE").unwrap_or_else(|_| "ok".into());
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut lines = stdin.lock().lines();

    // Read the invoke frame.
    let invoke = lines.next().expect("invoke").expect("read");
    let invoke: serde_json::Value = serde_json::from_str(&invoke).unwrap();
    assert_eq!(invoke["type"], "invoke");

    if mode == "crash" {
        eprintln!("boom");
        std::process::exit(3);
    }

    let mut content = String::from("(none)");
    if mode == "echo_model" {
        let req = serde_json::json!({
            "type": "request", "id": 2, "op": "model.chat",
            "payload": {"model": "default", "messages": [{"role":"user","content":"hi"}]}
        });
        writeln!(stdout, "{req}").unwrap();
        stdout.flush().unwrap();
        let resp = lines.next().expect("response").expect("read");
        let resp: serde_json::Value = serde_json::from_str(&resp).unwrap();
        content = resp["payload"]["content"].as_str().unwrap_or("(none)").to_string();
    }

    let result = serde_json::json!({
        "type": "result", "id": 1, "ok": true, "payload": {"exit": 0, "content": content}
    });
    writeln!(stdout, "{result}").unwrap();
    stdout.flush().unwrap();
}
