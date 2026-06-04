```markdown
# aish Development Patterns

> Auto-generated skill from repository analysis

## Overview
This skill covers the core development patterns and workflows for the `aish` TypeScript codebase. It documents coding conventions, commit practices, and governance workflows to help contributors maintain consistency and quality. Whether you're updating documentation, writing code, or managing project governance, this guide provides actionable steps and code examples.

## Coding Conventions

### File Naming
- Use **snake_case** for all file names.
  - Example:  
    ```
    user_profile.ts
    data_manager.test.ts
    ```

### Imports
- Use **relative imports** for referencing local modules.
  - Example:
    ```typescript
    import { fetch_data } from './utils/fetch_data';
    ```

### Exports
- Use **named exports** rather than default exports.
  - Example:
    ```typescript
    // In utils/math_tools.ts
    export function add(a: number, b: number): number {
      return a + b;
    }
    // Usage
    import { add } from './utils/math_tools';
    ```

### Commit Messages
- Follow **conventional commit** style.
- Common prefixes: `docs:`, `chore:`, `fix:`
  - Example:
    ```
    fix: correct typo in fetch_data function
    docs: update contributing guidelines
    ```

## Workflows

### Project Governance Foundation Update
**Trigger:** When someone wants to set up or revise project governance policies and documentation.  
**Command:** `/update-governance`

1. **Add or update LICENSE file**
   - Ensure the LICENSE file reflects the current licensing terms.
2. **Add or update CODE_OF_CONDUCT.md**
   - Define or revise standards for community behavior.
3. **Add or update CONTRIBUTING.md**
   - Provide guidelines for contributing to the project.
4. **Add or update PR and issue templates**
   - Update files in `.github/ISSUE_TEMPLATE/` and `.github/PULL_REQUEST_TEMPLATE.md` to standardize contributions.
5. **Update README.md and CLAUDE.md**
   - Reflect any governance changes in project documentation.
6. **Add or update governance design spec**
   - Place or revise design specs in `docs/superpowers/specs/`, e.g., `governance-foundation-design.md`.
7. **Optionally add or update workflow files**
   - Update or add workflow YAML files in `.github/workflows/` as needed.

**Example Directory Structure:**
```
.github/
  ISSUE_TEMPLATE/
    bug_report.md
    feature_request.md
    config.yml
  PULL_REQUEST_TEMPLATE.md
  workflows/
    ci.yml
LICENSE
CODE_OF_CONDUCT.md
CONTRIBUTING.md
README.md
CLAUDE.md
docs/
  superpowers/
    specs/
      governance-foundation-design.md
```

## Testing Patterns

- **Test file naming:** Use `*.test.*` pattern for test files.
  - Example: `data_manager.test.ts`
- **Testing framework:** Not explicitly defined; check test files for framework usage.
- **Test location:** Tests are typically placed alongside the modules they test or in a dedicated test directory.

**Example Test File:**
```typescript
// data_manager.test.ts
import { fetch_data } from './fetch_data';

describe('fetch_data', () => {
  it('should return data for valid input', () => {
    // test implementation
  });
});
```

## Commands

| Command            | Purpose                                                  |
|--------------------|----------------------------------------------------------|
| /update-governance | Initiate or update project governance documentation      |
```
