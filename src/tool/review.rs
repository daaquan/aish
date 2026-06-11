// SPDX-License-Identifier: MIT
use crate::provider::Message;
use crate::tool::Tool;

pub struct ReviewTool;

impl Tool for ReviewTool {
    fn name(&self) -> &'static str {
        "git.diff.review"
    }
}

/// Build the system+user messages for a code review of `diff`.
pub fn build_messages(language: &str, diff: &str) -> Vec<Message> {
    let system = format!(
        "You are a rigorous code reviewer.\n\
         Language: {language}.\n\
         Review the diff for bugs, security issues, and maintainability \
         problems. Output markdown grouped by severity (CRITICAL, HIGH, \
         MEDIUM, LOW), each finding referencing the file (and line where \
         possible) with a concrete suggestion. If there are no findings, \
         say so in one line. No fencing around the whole reply."
    );
    let diff = crate::tool::commit::truncate_diff(diff);
    let user = format!("Review this diff:\n\n{diff}");
    vec![Message::system(system), Message::user(user)]
}

/// Clean a raw review reply: trim and strip an outer ``` fence.
/// Returns None when nothing remains.
pub fn postprocess(raw: &str) -> Option<String> {
    let s = crate::tool::strip_outer_fence(raw);
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::commit::MAX_DIFF_CHARS;

    #[test]
    fn prompt_asks_for_severity_grouped_review_in_language() {
        let msgs = build_messages("en", "diff --git a/x");
        assert!(matches!(msgs[0].role, crate::provider::Role::System));
        let sys = msgs[0].content.to_lowercase();
        assert!(sys.contains("review"));
        assert!(sys.contains("severity"));
        assert!(msgs[0].content.contains("en"));
        assert!(msgs[1].content.contains("diff --git a/x"));
    }

    #[test]
    fn caps_huge_diff() {
        let big = "x".repeat(MAX_DIFF_CHARS + 500);
        let msgs = build_messages("en", &big);
        assert!(msgs[1].content.contains("[diff truncated]"));
    }

    #[test]
    fn postprocess_trims_and_strips_outer_fence() {
        let raw = "```markdown\n## CRITICAL\n\n- finding\n```\n";
        assert_eq!(postprocess(raw).unwrap(), "## CRITICAL\n\n- finding");
    }

    #[test]
    fn postprocess_keeps_inner_code_blocks() {
        let raw = "## HIGH\n\n```rust\nlet x = 1;\n```";
        let out = postprocess(raw).unwrap();
        assert!(out.contains("```rust"));
    }

    #[test]
    fn postprocess_rejects_empty_reply() {
        assert!(postprocess("  \n ").is_none());
        assert!(postprocess("```\n```").is_none());
    }
}
