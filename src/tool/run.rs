// SPDX-License-Identifier: MIT
use crate::provider::Message;
use crate::tool::Tool;

pub struct RunTool;

impl Tool for RunTool {
    fn name(&self) -> &'static str {
        "command.generate"
    }
}

/// Build the system+user messages for turning a natural-language description
/// into a single shell command, grounded in the current OS and shell.
pub fn build_messages(language: &str, os: &str, shell: &str, prompt: &str) -> Vec<Message> {
    let system = format!(
        "You are a terminal copilot for software developers.\n\
         Target shell: {shell}. Operating system: {os}.\n\
         Given a natural-language description, output EXACTLY ONE shell command \
         that accomplishes it on this shell and OS. Output only the command: no \
         prose, no explanation, no comments, no code fence, and never more than \
         one line. Prefer standard, portable flags for the target OS. \
         Language for any unavoidable text: {language}."
    );
    let user = prompt.to_string();
    vec![Message::system(system), Message::user(user)]
}

/// Clean a raw command reply: strip an outer ``` fence, trim, and strip a
/// single pair of surrounding backticks if present. Returns None when the
/// result is empty or spans more than one line (multi-command output is out of
/// scope).
pub fn postprocess(raw: &str) -> Option<String> {
    let s = crate::tool::strip_outer_fence(raw).trim();
    // A reply like `ls -la` (inline code) loses its single backtick pair.
    let s = s
        .strip_prefix('`')
        .and_then(|s| s.strip_suffix('`'))
        .unwrap_or(s)
        .trim();
    if s.is_empty() || s.contains('\n') {
        None
    } else {
        Some(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_includes_os_shell_and_request() {
        let msgs = build_messages("en", "linux", "zsh", "list all files by size");
        assert!(matches!(msgs[0].role, crate::provider::Role::System));
        assert!(msgs[0].content.contains("linux"));
        assert!(msgs[0].content.contains("zsh"));
        assert!(msgs[0].content.contains("en"));
        assert!(msgs[1].content.contains("list all files by size"));
    }

    #[test]
    fn postprocess_strips_outer_fence() {
        let raw = "```sh\ngit status\n```\n";
        assert_eq!(postprocess(raw).unwrap(), "git status");
    }

    #[test]
    fn postprocess_strips_inline_backticks() {
        assert_eq!(postprocess("`ls -la`").unwrap(), "ls -la");
    }

    #[test]
    fn postprocess_trims_plain_command() {
        assert_eq!(postprocess("  rm -rf build/  \n").unwrap(), "rm -rf build/");
    }

    #[test]
    fn postprocess_rejects_empty() {
        assert!(postprocess("   \n").is_none());
        assert!(postprocess("```\n```").is_none());
    }

    #[test]
    fn postprocess_rejects_multiline() {
        assert!(postprocess("cd src\nmake").is_none());
    }
}
