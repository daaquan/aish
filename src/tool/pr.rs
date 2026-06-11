// SPDX-License-Identifier: MIT
use crate::provider::Message;
use crate::tool::Tool;

pub use crate::tool::commit::MAX_DIFF_CHARS;

pub struct PrTool;

impl Tool for PrTool {
    fn name(&self) -> &'static str {
        "git.pr.description.generate"
    }
}

/// A generated pull-request title and markdown body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrDescription {
    pub title: String,
    pub body: String,
}

/// Build the system+user messages for PR title/body generation.
pub fn build_messages(language: &str, commits: &str, diff: &str) -> Vec<Message> {
    let system = format!(
        "You write pull request descriptions.\n\
         Language: {language}.\n\
         Output format: the first line is the PR title (<= 70 chars, \
         Conventional Commit style `type: subject`), followed by a blank line, \
         then a markdown body with a short summary and a bullet list of notable \
         changes. No backticks fencing the whole reply, no surrounding quotes."
    );
    let diff = crate::tool::commit::truncate_diff(diff);
    let user = format!(
        "Generate a pull request title and description for this branch.\n\n\
         Commits:\n{commits}\n\nDiff:\n\n{diff}"
    );
    vec![Message::system(system), Message::user(user)]
}

/// Parse a raw model reply into title (first non-empty line) and body (rest).
/// Returns None when the reply is empty or only code fences.
pub fn parse_response(raw: &str) -> Option<PrDescription> {
    // Strip only an outer ``` fence; the body may legitimately contain
    // inner code blocks, so a full fence rejection (like commit's
    // postprocess) would be wrong here.
    let mut s = raw.trim();
    if s.starts_with("```") {
        if let Some(idx) = s.find('\n') {
            s = &s[idx + 1..];
        } else {
            s = "";
        }
        if let Some(idx) = s.rfind("```") {
            s = &s[..idx];
        }
    }
    let cleaned = s.trim();
    if cleaned.is_empty() {
        return None;
    }
    let mut lines = cleaned.lines();
    let title = lines.next()?.trim().to_string();
    let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    Some(PrDescription { title, body })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_includes_language_commits_and_diff() {
        let msgs = build_messages("en", "feat: add x\nfix: y", "diff --git a/x");
        assert!(matches!(msgs[0].role, crate::provider::Role::System));
        assert!(msgs[0].content.contains("en"));
        assert!(msgs[1].content.contains("feat: add x"));
        assert!(msgs[1].content.contains("diff --git a/x"));
    }

    #[test]
    fn caps_huge_diff() {
        let big = "x".repeat(MAX_DIFF_CHARS + 500);
        let msgs = build_messages("en", "feat: z", &big);
        assert!(msgs[1].content.contains("[diff truncated]"));
    }

    #[test]
    fn parses_title_and_body() {
        let raw = "feat: add pr command\n\nAdds a new subcommand.\n\n- detail one\n";
        let pr = parse_response(raw).unwrap();
        assert_eq!(pr.title, "feat: add pr command");
        assert_eq!(pr.body, "Adds a new subcommand.\n\n- detail one");
    }

    #[test]
    fn strips_code_fences_before_parsing() {
        let raw = "```markdown\nfeat: fenced title\n\nBody text.\n```";
        let pr = parse_response(raw).unwrap();
        assert_eq!(pr.title, "feat: fenced title");
        assert_eq!(pr.body, "Body text.");
    }

    #[test]
    fn title_only_reply_yields_empty_body() {
        let pr = parse_response("feat: tiny change\n").unwrap();
        assert_eq!(pr.title, "feat: tiny change");
        assert_eq!(pr.body, "");
    }

    #[test]
    fn keeps_inner_code_blocks_in_body() {
        let raw = "feat: add api\n\nUsage:\n\n```rust\nlet x = 1;\n```";
        let pr = parse_response(raw).unwrap();
        assert_eq!(pr.title, "feat: add api");
        assert!(pr.body.contains("```rust"));
        assert!(pr.body.contains("let x = 1;"));
    }

    #[test]
    fn rejects_empty_or_fence_only_reply() {
        assert!(parse_response("   \n ").is_none());
        assert!(parse_response("```\n```").is_none());
    }
}
