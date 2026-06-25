// SPDX-License-Identifier: MIT
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

/// Cap a diff at `MAX_DIFF_CHARS`, cutting on a UTF-8 char boundary and
/// appending a truncation marker.
pub(crate) fn truncate_diff(diff: &str) -> String {
    truncate_input(diff, "[diff truncated]")
}

/// Cap any prompt input at `MAX_DIFF_CHARS`, cutting on a UTF-8 char boundary
/// and appending `marker` when something was dropped.
pub(crate) fn truncate_input(s: &str, marker: &str) -> String {
    if s.len() > MAX_DIFF_CHARS {
        let cut = floor_char_boundary(s, MAX_DIFF_CHARS);
        format!("{}\n{marker}", &s[..cut])
    } else {
        s.to_string()
    }
}

/// Smallest index >= `min` that is a UTF-8 char boundary of `s`.
fn ceil_char_boundary(s: &str, min: usize) -> usize {
    if min >= s.len() {
        return s.len();
    }
    let mut i = min;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Cap `s` at `MAX_DIFF_CHARS` keeping the **tail** (most recent output),
/// cutting on a UTF-8 char boundary and prefixing `marker` when something was
/// dropped. Used for command output, where the failure lives at the end —
/// the opposite of [`truncate_input`], which keeps the head.
pub(crate) fn truncate_tail(s: &str, marker: &str) -> String {
    if s.len() > MAX_DIFF_CHARS {
        let cut = ceil_char_boundary(s, s.len() - MAX_DIFF_CHARS);
        format!("{marker}\n{}", &s[cut..])
    } else {
        s.to_string()
    }
}

/// Build the system+user messages for commit-message generation.
///
/// `instructions`, when non-empty, is appended as extra style guidance. The
/// output-format guardrails (output-only, no fences) stay fixed regardless, so
/// [`postprocess`] keeps working.
pub fn build_messages(
    style: &str,
    language: &str,
    instructions: Option<&str>,
    diff: &str,
) -> Vec<Message> {
    let mut system = format!(
        "You write git commit messages.\n\
         Style: {style} (when 'conventional', use Conventional Commits: \
         `type(scope): subject`, types feat|fix|refactor|docs|test|chore|perf|ci).\n\
         Language: {language}.\n\
         Output ONLY the commit message. Subject <= 50 chars, imperative mood. \
         No backticks, no explanation, no surrounding quotes."
    );
    if let Some(extra) = instructions.map(str::trim).filter(|s| !s.is_empty()) {
        system.push_str("\nAdditional style guidance:\n");
        system.push_str(extra);
    }
    let diff = truncate_diff(diff);
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
    let result = s.trim();
    // A residual fence marker means the input was just fences / malformed — reject it.
    if result.contains("```") || result.is_empty() {
        return String::new();
    }
    result.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_includes_style_language_and_diff() {
        let msgs = build_messages("conventional", "en", None, "diff --git a/x");
        assert!(matches!(msgs[0].role, crate::provider::Role::System));
        assert!(msgs[0].content.to_lowercase().contains("conventional"));
        assert!(msgs[0].content.contains("en"));
        assert!(msgs[1].content.contains("diff --git a/x"));
    }

    #[test]
    fn appends_custom_instructions_to_system_prompt() {
        let msgs = build_messages("conventional", "en", Some("Use a gitmoji prefix."), "diff");
        assert!(msgs[0].content.contains("Use a gitmoji prefix."));
        // Guardrails stay regardless of custom guidance.
        assert!(msgs[0].content.contains("Output ONLY the commit message"));
    }

    #[test]
    fn blank_instructions_add_nothing() {
        let plain = build_messages("conventional", "en", None, "diff");
        let blank = build_messages("conventional", "en", Some("   \n "), "diff");
        assert_eq!(plain[0].content, blank[0].content);
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
    fn bare_fence_becomes_empty() {
        assert!(postprocess("```").is_empty());
        assert!(postprocess("``````").is_empty());
        assert!(postprocess("```\n```").is_empty());
    }

    #[test]
    fn caps_huge_diff() {
        let big = "x".repeat(MAX_DIFF_CHARS + 500);
        let msgs = build_messages("conventional", "en", None, &big);
        assert!(msgs[1].content.contains("[diff truncated]"));
        // short prompt prefix + capped diff (<= MAX_DIFF_CHARS) + marker
        assert!(msgs[1].content.len() < MAX_DIFF_CHARS + 200);
    }

    #[test]
    fn truncate_tail_keeps_the_end_with_marker() {
        let s = format!("HEAD{}TAIL_ERROR", "x".repeat(MAX_DIFF_CHARS));
        let out = super::truncate_tail(&s, "[earlier output truncated]");
        assert!(out.starts_with("[earlier output truncated]"));
        assert!(out.ends_with("TAIL_ERROR"));
        assert!(!out.contains("HEAD"));
    }

    #[test]
    fn truncate_tail_leaves_short_input_untouched() {
        assert_eq!(super::truncate_tail("short", "[m]"), "short");
    }

    #[test]
    fn truncate_tail_cuts_on_char_boundary_without_panic() {
        let s = "あ".repeat(MAX_DIFF_CHARS);
        let out = super::truncate_tail(&s, "[m]");
        assert!(out.starts_with("[m]"));
    }

    #[test]
    fn caps_huge_diff_on_char_boundary_without_panic() {
        // 'あ' is 3 bytes; a diff of these straddles MAX_DIFF_CHARS mid-character.
        // Must cap on a char boundary, not panic.
        let big = "あ".repeat(MAX_DIFF_CHARS);
        let msgs = build_messages("conventional", "en", None, &big);
        assert!(msgs[1].content.contains("[diff truncated]"));
    }
}
