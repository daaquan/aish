// SPDX-License-Identifier: MIT
use crate::commands::emit_json;
use crate::config::{Config, IssueLevel};
use anyhow::{anyhow, Context, Result};

pub fn init(force: bool, json: bool) -> Result<()> {
    let path = Config::default_path();
    Config::write_template(&path, force)
        .with_context(|| format!("writing config to {}", path.display()))?;
    if json {
        emit_json(&serde_json::json!({ "wrote": path.display().to_string() }));
    } else {
        println!("Wrote config template to {}", path.display());
    }
    Ok(())
}

pub fn check(json: bool) -> Result<()> {
    let cfg = Config::load()?;
    let issues = cfg.validate();
    let errors = issues
        .iter()
        .filter(|i| i.level == IssueLevel::Error)
        .count();

    if json {
        let rows: Vec<_> = issues
            .iter()
            .map(|i| {
                let level = match i.level {
                    IssueLevel::Error => "error",
                    IssueLevel::Warning => "warning",
                };
                serde_json::json!({ "level": level, "message": i.message })
            })
            .collect();
        emit_json(&serde_json::json!({
            "ok": errors == 0,
            "providers": cfg.providers.len(),
            "models": cfg.models.len(),
            "issues": rows,
        }));
    } else if issues.is_empty() {
        println!(
            "Config OK: {} provider(s), {} model alias(es).",
            cfg.providers.len(),
            cfg.models.len()
        );
    } else {
        for issue in &issues {
            let tag = match issue.level {
                IssueLevel::Error => "error",
                IssueLevel::Warning => "warning",
            };
            println!("{tag}: {}", issue.message);
        }
    }

    // Nonzero exit on errors regardless of format, so CI gates fail correctly.
    if errors > 0 {
        return Err(anyhow!("config has {errors} error(s)"));
    }
    Ok(())
}
