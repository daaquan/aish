// SPDX-License-Identifier: AGPL-3.0-only
pub mod commit;

/// Internal tool abstraction. In v0.1 only `CommitTool` implements it; the
/// registry exists so a future external (subprocess) loader can register tools
/// without restructuring callers. No external ABI is defined yet.
pub trait Tool {
    /// Canonical name, e.g. `git.commit.message.generate`.
    fn name(&self) -> &'static str;
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
