// SPDX-License-Identifier: MIT
pub mod changelog;
pub mod commit;
pub mod pr;
pub mod review;

/// Internal tool abstraction. In v0.1 only `CommitTool` implements it; the
/// registry exists so a future external (subprocess) loader can register tools
/// without restructuring callers. No external ABI is defined yet.
pub trait Tool {
    /// Canonical name, e.g. `git.commit.message.generate`.
    fn name(&self) -> &'static str;
}

/// Strip only an outer ``` fence from a model reply, preserving inner code
/// blocks. Returns the trimmed remainder ("" when nothing is left).
pub(crate) fn strip_outer_fence(raw: &str) -> &str {
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
    s.trim()
}

#[derive(Default)]
pub struct Registry {
    tools: Vec<Box<dyn Tool>>,
}

impl Registry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.push(tool);
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.tools.iter().map(|t| t.name()).collect()
    }
}
