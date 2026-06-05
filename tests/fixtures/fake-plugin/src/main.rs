// SPDX-License-Identifier: MIT
//! Minimal ABI-speaking plugin for host tests. Behavior is driven by env:
//!   FAKE_MODE=echo_model        -> request model.chat, put content in result
//!   FAKE_MODE=crash             -> exit 3 before sending a result
//!   FAKE_MODE=ok                -> just send result ok exit 0
//!   FAKE_MODE=hang_after_result -> send result ok, then sleep forever (never exit)
//!   FAKE_MODE=slow_service      -> request model.chat; host must time the call out
//!   FAKE_MODE=stderr_flood      -> spew lots of stderr, then crash before a result
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

    if mode == "stderr_flood" {
        // Far more than the host's bounded stderr tail, so the host must not
        // buffer it all. Then crash before a result so the error path drains it.
        let chunk = "x".repeat(1024);
        for _ in 0..512 {
            eprintln!("{chunk}");
        }
        std::process::exit(7);
    }

    let mut content = String::from("(none)");
    if mode == "echo_model" || mode == "slow_service" {
        let req = serde_json::json!({
            "type": "request", "id": 2, "op": "model.chat",
            "payload": {"model": "default", "messages": [{"role":"user","content":"hi"}]}
        });
        writeln!(stdout, "{req}").unwrap();
        stdout.flush().unwrap();
        // In slow_service the host should time the call out and kill us before a
        // response ever arrives, so this read may never return — that is the point.
        let resp = lines.next().expect("response").expect("read");
        let resp: serde_json::Value = serde_json::from_str(&resp).unwrap();
        content = resp["payload"]["content"]
            .as_str()
            .unwrap_or("(none)")
            .to_string();
    }

    let result = serde_json::json!({
        "type": "result", "id": 1, "ok": true, "payload": {"exit": 0, "content": content}
    });
    writeln!(stdout, "{result}").unwrap();
    stdout.flush().unwrap();

    if mode == "hang_after_result" {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    }
}
