// SPDX-License-Identifier: MIT
use crate::provider::Message;
use crate::tool::Tool;

pub struct ChangelogTool;

impl Tool for ChangelogTool {
    fn name(&self) -> &'static str {
        "git.changelog.generate"
    }
}

/// Build the system+user messages for changelog generation over `commits`
/// (one subject per line) in the range labeled `range`.
pub fn build_messages(language: &str, range: &str, commits: &str) -> Vec<Message> {
    let system = format!(
        "You write CHANGELOG entries.\n\
         Language: {language}.\n\
         Group entries under markdown headings in Keep-a-Changelog style: \
         Added, Changed, Fixed, Removed (omit empty groups). Use Conventional \
         Commit prefixes (feat/fix/refactor/...) as grouping hints and drop \
         the prefixes from the entries. One concise bullet per change, \
         user-facing wording. No fencing around the whole reply."
    );
    let user = format!("Generate changelog entries for the commits in {range}:\n\n{commits}");
    vec![Message::system(system), Message::user(user)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_asks_for_grouped_entries_with_range_and_commits() {
        let msgs = build_messages("en", "v0.5.0..HEAD", "feat: add pr\nfix: cache key");
        assert!(matches!(msgs[0].role, crate::provider::Role::System));
        let sys = msgs[0].content.to_lowercase();
        assert!(sys.contains("changelog"));
        assert!(sys.contains("added"));
        assert!(msgs[0].content.contains("en"));
        assert!(msgs[1].content.contains("v0.5.0..HEAD"));
        assert!(msgs[1].content.contains("feat: add pr"));
        assert!(msgs[1].content.contains("fix: cache key"));
    }
}
