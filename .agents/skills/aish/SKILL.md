```markdown
# aish Development Patterns

> Auto-generated skill from repository analysis

## Overview
This skill teaches you how to contribute to the `aish` Rust codebase, which is focused on AI provider integration and tooling. You'll learn the project's coding conventions, how to add new AI providers, extend configuration logic, and build or extend CLI tools. The guide also covers commit conventions, file structure, and testing patterns to ensure consistency and maintainability.

## Coding Conventions

- **File Naming:** Use `camelCase` for file names.
  - Example: `providerAdapter.rs`, `resolveConfig.rs`
- **Import Style:** Use relative imports.
  - Example:
    ```rust
    use super::providerAdapter;
    use crate::config::resolveConfig;
    ```
- **Export Style:** Use named exports.
  - Example:
    ```rust
    pub fn new_provider() { ... }
    pub struct ProviderConfig { ... }
    ```
- **Commit Messages:** Follow [Conventional Commits](https://www.conventionalcommits.org/) with these prefixes:
  - `feat`: New features
  - `fix`: Bug fixes
  - `chore`: Maintenance
  - `test`: Testing
  - Example: `feat: add Gemini provider adapter`

## Workflows

### Add Provider Adapter
**Trigger:** When you want to add a new provider backend for chat/AI functionality.  
**Command:** `/add-provider`

1. Create or update `src/provider/{provider}.rs` with the new provider implementation.
   - Example: `src/provider/gemini.rs`
2. Update `src/provider/mod.rs` to register or expose the new provider.
   - Example:
     ```rust
     pub mod gemini;
     ```
3. Commit your changes with a message like:  
   `feat: add Gemini provider adapter`

### Extend Config Logic
**Trigger:** When you need to support new config options or update model/provider resolution logic.  
**Command:** `/update-config`

1. Update `src/config/mod.rs` with new structs, logic, or loading mechanisms.
2. Optionally, create or update `src/config/{feature}.rs` for specific config features (e.g., `resolve.rs`).
   - Example:
     ```rust
     // src/config/resolve.rs
     pub fn resolve_provider(name: &str) -> Option<ProviderConfig> { ... }
     ```
3. Commit with a message like:  
   `feat: support custom model resolution in config`

### Add Tool or Extend Tooling
**Trigger:** When you want to add a new CLI tool or extend tool functionality.  
**Command:** `/add-tool`

1. Create or update `src/tool/{tool}.rs` with the tool's logic.
   - Example: `src/tool/commitTool.rs`
2. Update `src/tool/mod.rs` to register or expose the new tool.
   - Example:
     ```rust
     pub mod commitTool;
     ```
3. Commit with a message like:  
   `feat: add commit tool for CLI`

## Testing Patterns

- **Test File Naming:** Test files follow the pattern `*.test.*`
  - Example: `providerAdapter.test.rs`
- **Testing Framework:** Not explicitly specified; use Rust's built-in test framework.
  - Example:
    ```rust
    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_provider_response() {
            // test logic here
        }
    }
    ```

## Commands

| Command        | Purpose                                            |
|----------------|---------------------------------------------------|
| /add-provider  | Add a new AI provider adapter                     |
| /update-config | Add or update configuration logic                  |
| /add-tool      | Add a new CLI tool or extend existing tooling      |
```
