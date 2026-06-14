// SPDX-License-Identifier: MIT
use crate::provider::Message;
use crate::tool::Tool;

pub struct FixTool;

impl Tool for FixTool {
    fn name(&self) -> &'static str {
        "command.diagnose"
    }
}

/// Build the system+user messages for diagnosing a command invocation.
///
/// `output` is the command's combined stdout+stderr; it is tail-truncated
/// (the failure lives at the end) before being embedded in the prompt.
pub fn build_messages(language: &str, command: &str, exit_code: i32, output: &str) -> Vec<Message> {
    let system = format!(
        "You are a terminal copilot for software developers.\n\
         Language: {language}.\n\
         Given a command, its exit code, and its combined output, explain in \
         one or two sentences why it failed, then give ONE concrete, minimal \
         fix the user can act on (a corrected command, an edit, or an install \
         step). Be specific to the actual error; do not lecture. Plain text or \
         minimal markdown; no fencing around the whole reply."
    );
    let output = crate::tool::commit::truncate_tail(output, "[earlier output truncated]");
    let user = format!("Command:\n    {command}\n\nExit code: {exit_code}\n\nOutput:\n\n{output}");
    vec![Message::system(system), Message::user(user)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::commit::MAX_DIFF_CHARS;

    #[test]
    fn prompt_includes_command_exit_code_and_output() {
        let msgs = build_messages(
            "en",
            "cargo build",
            101,
            "error[E0382]: borrow of moved value",
        );
        assert!(matches!(msgs[0].role, crate::provider::Role::System));
        assert!(msgs[0].content.contains("en"));
        assert!(msgs[0].content.to_lowercase().contains("fix"));
        assert!(msgs[1].content.contains("cargo build"));
        assert!(msgs[1].content.contains("101"));
        assert!(msgs[1].content.contains("error[E0382]"));
    }

    #[test]
    fn long_output_is_tail_truncated() {
        let out = format!("FIRST_LINE{}LAST_ERROR_LINE", "x".repeat(MAX_DIFF_CHARS));
        let msgs = build_messages("en", "make", 2, &out);
        assert!(msgs[1].content.contains("[earlier output truncated]"));
        assert!(msgs[1].content.contains("LAST_ERROR_LINE"));
        assert!(!msgs[1].content.contains("FIRST_LINE"));
    }
}
