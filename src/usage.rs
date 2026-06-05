// SPDX-License-Identifier: AGPL-3.0-only
//! Token usage and cost analytics over the JSONL audit log.
use crate::config::ModelPricing;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct LogLine {
    provider: String,
    model: String,
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
}

/// Aggregated counters for one model (or the grand total).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    pub calls: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    /// Estimated USD cost; `None` when pricing for the model is unknown.
    pub cost: Option<f64>,
}

/// A usage summary: per-model rows (keyed `provider/model`) plus a grand total.
#[derive(Debug, Default)]
pub struct Summary {
    pub by_model: BTreeMap<String, Stats>,
    pub total: Stats,
}

fn cost_of(p: &ModelPricing, prompt: u64, completion: u64) -> f64 {
    (prompt as f64 * p.input_per_mtok + completion as f64 * p.output_per_mtok) / 1_000_000.0
}

/// Aggregate audit-log lines into a [`Summary`]. Malformed lines are skipped.
/// Cost is summed only over models with a pricing entry; the total cost is
/// `None` when no priced calls were seen.
pub fn summarize<I, S>(lines: I, pricing: &BTreeMap<String, ModelPricing>) -> Summary
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut summary = Summary::default();
    for line in lines {
        let line = line.as_ref().trim();
        if line.is_empty() {
            continue;
        }
        let Ok(rec) = serde_json::from_str::<LogLine>(line) else {
            continue;
        };
        let key = format!("{}/{}", rec.provider, rec.model);
        let row = summary.by_model.entry(key).or_default();
        row.calls += 1;
        row.prompt_tokens += rec.prompt_tokens;
        row.completion_tokens += rec.completion_tokens;

        summary.total.calls += 1;
        summary.total.prompt_tokens += rec.prompt_tokens;
        summary.total.completion_tokens += rec.completion_tokens;

        if let Some(price) = pricing.get(&rec.model) {
            let c = cost_of(price, rec.prompt_tokens, rec.completion_tokens);
            row.cost = Some(row.cost.unwrap_or(0.0) + c);
            summary.total.cost = Some(summary.total.cost.unwrap_or(0.0) + c);
        }
    }
    summary
}

/// Read the audit log at `path`, returning its lines. A missing file yields an
/// empty summary input rather than an error.
pub fn read_log(path: &Path) -> std::io::Result<Vec<String>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s.lines().map(str::to_owned).collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Render a summary as a plain-text table for the terminal.
pub fn render(summary: &Summary) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{:<32} {:>6} {:>10} {:>11} {:>12}\n",
        "MODEL", "CALLS", "PROMPT", "COMPLETION", "COST(USD)"
    ));
    if summary.by_model.is_empty() {
        out.push_str("(no usage recorded yet)\n");
        return out;
    }
    for (model, s) in &summary.by_model {
        out.push_str(&format!(
            "{:<32} {:>6} {:>10} {:>11} {:>12}\n",
            model,
            s.calls,
            s.prompt_tokens,
            s.completion_tokens,
            fmt_cost(s.cost),
        ));
    }
    out.push_str(&format!(
        "{:<32} {:>6} {:>10} {:>11} {:>12}\n",
        "TOTAL",
        summary.total.calls,
        summary.total.prompt_tokens,
        summary.total.completion_tokens,
        fmt_cost(summary.total.cost),
    ));
    out
}

/// Serialize a summary as JSON for `aish usage --json`. Per-model rows are keyed
/// `provider/model`; `cost` is `null` when the model has no pricing entry.
pub fn to_json(summary: &Summary) -> serde_json::Value {
    fn stats(s: &Stats) -> serde_json::Value {
        serde_json::json!({
            "calls": s.calls,
            "prompt_tokens": s.prompt_tokens,
            "completion_tokens": s.completion_tokens,
            "cost": s.cost,
        })
    }
    let by_model: serde_json::Map<String, serde_json::Value> = summary
        .by_model
        .iter()
        .map(|(k, s)| (k.clone(), stats(s)))
        .collect();
    serde_json::json!({
        "by_model": by_model,
        "total": stats(&summary.total),
    })
}

fn fmt_cost(cost: Option<f64>) -> String {
    match cost {
        Some(c) => format!("${c:.4}"),
        None => "—".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pricing() -> BTreeMap<String, ModelPricing> {
        let mut m = BTreeMap::new();
        m.insert(
            "gpt-5-mini".to_string(),
            ModelPricing {
                input_per_mtok: 1.0,
                output_per_mtok: 2.0,
            },
        );
        m
    }

    #[test]
    fn aggregates_tokens_and_calls_by_model() {
        let lines = [
            r#"{"provider":"openai","model":"gpt-5-mini","prompt_tokens":100,"completion_tokens":50,"decision":"applied"}"#,
            r#"{"provider":"openai","model":"gpt-5-mini","prompt_tokens":200,"completion_tokens":10,"decision":"rejected"}"#,
        ];
        let s = summarize(lines, &pricing());
        let row = &s.by_model["openai/gpt-5-mini"];
        assert_eq!(row.calls, 2);
        assert_eq!(row.prompt_tokens, 300);
        assert_eq!(row.completion_tokens, 60);
        assert_eq!(s.total.calls, 2);
    }

    #[test]
    fn computes_cost_from_pricing() {
        let lines = [
            r#"{"provider":"openai","model":"gpt-5-mini","prompt_tokens":1000000,"completion_tokens":1000000}"#,
        ];
        let s = summarize(lines, &pricing());
        // 1M input * $1 + 1M output * $2 = $3
        assert_eq!(s.by_model["openai/gpt-5-mini"].cost, Some(3.0));
        assert_eq!(s.total.cost, Some(3.0));
    }

    #[test]
    fn unknown_model_has_no_cost() {
        let lines = [
            r#"{"provider":"anthropic","model":"claude-x","prompt_tokens":10,"completion_tokens":5}"#,
        ];
        let s = summarize(lines, &pricing());
        assert_eq!(s.by_model["anthropic/claude-x"].cost, None);
        assert_eq!(s.total.cost, None);
    }

    #[test]
    fn skips_blank_and_malformed_lines() {
        let lines = [
            "",
            "not json",
            r#"{"provider":"openai","model":"gpt-5-mini","prompt_tokens":5,"completion_tokens":1}"#,
        ];
        let s = summarize(lines, &pricing());
        assert_eq!(s.total.calls, 1);
        assert_eq!(s.total.prompt_tokens, 5);
    }

    #[test]
    fn read_log_missing_file_is_empty() {
        let p = Path::new("/nonexistent/aish/audit.log");
        assert!(read_log(p).unwrap().is_empty());
    }

    #[test]
    fn render_shows_total_and_handles_empty() {
        assert!(render(&Summary::default()).contains("no usage recorded"));
        let s = summarize(
            [
                r#"{"provider":"openai","model":"gpt-5-mini","prompt_tokens":1000000,"completion_tokens":0}"#,
            ],
            &pricing(),
        );
        let out = render(&s);
        assert!(out.contains("openai/gpt-5-mini"));
        assert!(out.contains("TOTAL"));
        assert!(out.contains("$1.0000"));
    }

    #[test]
    fn to_json_emits_rows_total_and_null_cost() {
        let s = summarize(
            [
                r#"{"provider":"openai","model":"gpt-5-mini","prompt_tokens":1000000,"completion_tokens":0}"#,
                r#"{"provider":"anthropic","model":"claude-x","prompt_tokens":10,"completion_tokens":5}"#,
            ],
            &pricing(),
        );
        let v = to_json(&s);
        assert_eq!(v["by_model"]["openai/gpt-5-mini"]["cost"], 1.0);
        // Unknown model -> cost null, not absent.
        assert!(v["by_model"]["anthropic/claude-x"]["cost"].is_null());
        assert_eq!(v["total"]["calls"], 2);
        assert_eq!(v["total"]["prompt_tokens"], 1000010);
    }
}
