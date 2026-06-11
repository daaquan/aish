// SPDX-License-Identifier: MIT
use crate::provider::Message;
use crate::tool::Tool;

pub struct AskTool;

impl Tool for AskTool {
    fn name(&self) -> &'static str {
        "ask.answer"
    }
}

/// Build the system+user messages for a one-shot question, with optional
/// piped-in context (e.g. a build log) capped at the shared input limit.
pub fn build_messages(language: &str, question: &str, context: Option<&str>) -> Vec<Message> {
    let system = format!(
        "You are a concise assistant for software developers working in a \
         terminal. Answer questions about errors, commands, code, and tooling \
         directly and practically.\n\
         Language: {language}.\n\
         Plain text or minimal markdown; no fencing around the whole reply."
    );
    let user = match context {
        Some(ctx) if !ctx.trim().is_empty() => {
            let ctx = crate::tool::commit::truncate_input(ctx, "[input truncated]");
            format!("{question}\n\nContext (piped input):\n\n{ctx}")
        }
        _ => question.to_string(),
    };
    vec![Message::system(system), Message::user(user)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::commit::MAX_DIFF_CHARS;

    #[test]
    fn prompt_is_developer_focused_and_includes_question() {
        let msgs = build_messages("en", "what does EXDEV mean?", None);
        assert!(matches!(msgs[0].role, crate::provider::Role::System));
        assert!(msgs[0].content.to_lowercase().contains("developer"));
        assert!(msgs[0].content.contains("en"));
        assert!(msgs[1].content.contains("what does EXDEV mean?"));
    }

    #[test]
    fn piped_context_is_included() {
        let msgs = build_messages("en", "explain this error", Some("error[E0382]: borrow"));
        assert!(msgs[1].content.contains("error[E0382]: borrow"));
        assert!(msgs[1].content.contains("explain this error"));
    }

    #[test]
    fn huge_context_is_truncated() {
        let big = "x".repeat(MAX_DIFF_CHARS + 500);
        let msgs = build_messages("en", "explain", Some(&big));
        assert!(msgs[1].content.contains("[input truncated]"));
        assert!(msgs[1].content.len() < MAX_DIFF_CHARS + 300);
    }
}
