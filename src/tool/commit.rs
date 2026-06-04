// SPDX-License-Identifier: AGPL-3.0-only
use crate::provider::Message;
use crate::tool::Tool;

pub const MAX_DIFF_CHARS: usize = 12_000;

pub struct CommitTool;

impl Tool for CommitTool {
    fn name(&self) -> &'static str {
        "git.commit.message.generate"
    }
}

/// Largest index <= max that is a UTF-8 char boundary of `s`.
fn floor_char_boundary(s: &str, max: usize) -> usize {
    if max >= s.len() {
        return s.len();
    }
    let mut i = max;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Build the system+user messages for commit-message generation.
pub fn build_messages(style: &str, language: &str, diff: &str) -> Vec<Message> {
    let system = format!(
        "You write git commit messages.\n\
         Style: {style} (when 'conventional', use Conventional Commits: \
         `type(scope): subject`, types feat|fix|refactor|docs|test|chore|perf|ci).\n\
         Language: {language}.\n\
         Output ONLY the commit message. Subject <= 50 chars, imperative mood. \
         No backticks, no explanation, no surrounding quotes."
    );
    let diff = if diff.len() > MAX_DIFF_CHARS {
        let cut = floor_char_boundary(diff, MAX_DIFF_CHARS);
        format!("{}\n[diff truncated]", &diff[..cut])
    } else {
        diff.to_string()
    };
    let user = format!("Generate a commit message for this staged diff:\n\n{diff}");
    vec![Message::system(system), Message::user(user)]
}

/// Clean a raw model response: strip surrounding ``` fences and trim.
pub fn postprocess(raw: &str) -> String {
    let mut s = raw.trim();
    if s.starts_with("```") {
        if let Some(idx) = s.find('\n') {
            s = &s[idx + 1..];
        }
        if let Some(idx) = s.rfind("```") {
            s = &s[..idx];
        }
    }
    s.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_includes_style_language_and_diff() {
        let msgs = build_messages("conventional", "en", "diff --git a/x");
        assert!(matches!(msgs[0].role, crate::provider::Role::System));
        assert!(msgs[0].content.to_lowercase().contains("conventional"));
        assert!(msgs[0].content.contains("en"));
        assert!(msgs[1].content.contains("diff --git a/x"));
    }

    #[test]
    fn strips_code_fences_and_trims() {
        let raw = "```\nfeat: add x\n```\n";
        assert_eq!(postprocess(raw), "feat: add x");
    }

    #[test]
    fn strips_language_tagged_fence() {
        let raw = "```text\nfix: y\n```";
        assert_eq!(postprocess(raw), "fix: y");
    }

    #[test]
    fn rejects_empty_output() {
        assert!(postprocess("   \n  ").is_empty());
    }

    #[test]
    fn caps_huge_diff() {
        let big = "x".repeat(MAX_DIFF_CHARS + 500);
        let msgs = build_messages("conventional", "en", &big);
        assert!(msgs[1].content.contains("[diff truncated]"));
        // short prompt prefix + capped diff (<= MAX_DIFF_CHARS) + marker
        assert!(msgs[1].content.len() < MAX_DIFF_CHARS + 200);
    }

    #[test]
    fn caps_huge_diff_on_char_boundary_without_panic() {
        // 'あ' is 3 bytes; a diff of these straddles MAX_DIFF_CHARS mid-character.
        // Must cap on a char boundary, not panic.
        let big = "あ".repeat(MAX_DIFF_CHARS);
        let msgs = build_messages("conventional", "en", &big);
        assert!(msgs[1].content.contains("[diff truncated]"));
    }
}
