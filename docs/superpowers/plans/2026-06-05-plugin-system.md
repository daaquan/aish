# aish Plugin System + `commit` Extraction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn aish into a subprocess-plugin host — install/enable/disable external tool plugins over a stdio JSON ABI — and move the built-in `commit` command out of core into the first plugin.

**Architecture:** Core ships zero tools. Unknown subcommands are routed to an installed+enabled plugin, spawned as a child process. The plugin (a "fat" tool) owns the whole flow and calls back to the host over newline-delimited JSON on stdin/stdout for two services — `model.chat` (keeps API keys/network in core) and `audit.record`. Human UI (the `[Y/n]` prompt and rendering) goes to `/dev/tty`, since stdin/stdout are the protocol channel. Plugins are installed by building source crates from a registry git repo and are treated as fully trusted native code.

**Tech Stack:** Rust, tokio (async process + io + time), serde / serde_json, toml, sha2, fs2 (file lock), clap (external subcommands), thiserror/anyhow. Plugins live in a separate cargo workspace repo (`aish-plugins`).

---

## Reference: spec

Design spec: `docs/superpowers/specs/2026-06-05-plugin-system-design.md`. Read it before starting. This plan implements it phase-by-phase.

## File Structure

### aish core (`/opt/aish`)

| File | Responsibility | Action |
|------|----------------|--------|
| `Cargo.toml` | deps: add `toml`, `sha2`, `fs2`; extend `tokio` features | Modify |
| `src/lib.rs` | module list: drop `tool`, add `plugin` | Modify |
| `src/plugin/mod.rs` | module exports + `aish_home()` path helper | Create |
| `src/plugin/protocol.rs` | ABI frame types, serde, constants | Create |
| `src/plugin/manifest.rs` | `aish-plugin.toml` + `plugins.toml` (installed-state registry), conflict check, file lock | Create |
| `src/plugin/install.rs` | registry resolution, build-on-install, atomic install, sha256 | Create |
| `src/plugin/host.rs` | spawn plugin, run the stdio loop, timeouts, error mapping | Create |
| `src/plugin/services.rs` | `model.chat` + `audit.record` service handlers, config scoping | Create |
| `src/cli.rs` | drop `Commit`; add `Plugin{action}` + external-subcommand catch-all | Modify |
| `src/main.rs` | drop `run_commit`; dispatch `plugin` + external subcommands | Modify |
| `src/audit.rs` | add `Deserialize` to `AuditEntry` (host decodes it from a frame) | Modify |
| `src/tool/mod.rs`, `src/tool/commit.rs` | obsolete in-process seam | Delete |
| `tests/commit_e2e.rs` | built-in commit gone | Delete |
| `tests/plugin_e2e.rs` | install + run a plugin end-to-end | Create |
| `tests/fixtures/fake-plugin/` | tiny plugin binary used by host/integration tests | Create |

### aish-plugins (`/opt/aish-plugins`)

| File | Responsibility | Action |
|------|----------------|--------|
| `Cargo.toml` | workspace manifest | Create |
| `commit/Cargo.toml` | the commit plugin crate | Create |
| `commit/aish-plugin.toml` | plugin manifest | Create |
| `commit/src/protocol.rs` | ABI frame types (plugin-side copy of the contract) | Create |
| `commit/src/message.rs` | `build_messages` + `postprocess` (moved verbatim from core) | Create |
| `commit/src/main.rs` | stdio loop: diff → model.chat → prompt on /dev/tty → git commit → audit | Create |

---

# Phase 1 — Protocol + manifest types

### Task 1: Add core dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Edit `Cargo.toml` dependencies**

Replace the `tokio` line and add three crates:

```toml
[dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread", "process", "io-util", "time"] }
async-trait = "0.1"
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
serde = { version = "1", features = ["derive"] }
serde_yaml = "0.9"
serde_json = "1"
toml = "0.8"
sha2 = "0.10"
fs2 = "0.4"
clap = { version = "4", features = ["derive"] }
thiserror = "1"
anyhow = "1"
dirs = "5"
```

- [ ] **Step 2: Verify it builds**

Run: `cargo build`
Expected: PASS (compiles; new crates downloaded).

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add toml, sha2, fs2 deps and tokio process/io/time features"
```

---

### Task 2: Protocol frame types

**Files:**
- Create: `src/plugin/mod.rs`
- Create: `src/plugin/protocol.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Wire the module into the crate**

Edit `src/lib.rs` — remove `pub mod tool;` (deleted in Phase 4; if it still exists leave it for now and remove in Task 13) and add:

```rust
pub mod plugin;
```

Create `src/plugin/mod.rs`:

```rust
// SPDX-License-Identifier: MIT
pub mod protocol;

use std::path::PathBuf;

/// Root for installed plugins + state. `~/.aish`, overridable via `$AISH_HOME`.
pub fn aish_home() -> PathBuf {
    if let Ok(p) = std::env::var("AISH_HOME") {
        return PathBuf::from(p);
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".aish")
}
```

- [ ] **Step 2: Write the failing test**

Create `src/plugin/protocol.rs`:

```rust
// SPDX-License-Identifier: MIT
//! The aish plugin ABI. Newline-delimited JSON frames over stdin/stdout.
//! This is a *contract*, not a shared library: the plugin side re-declares the
//! same types. Keep changes additive within an ABI major.
use serde::{Deserialize, Serialize};

/// Protocol major version. A plugin manifest's `abi` must match this major.
pub const ABI_MAJOR: u32 = 1;
/// Largest accepted frame (one JSON line) in bytes.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProtoError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Frame {
    Invoke {
        id: u64,
        subcommand: String,
        #[serde(default)]
        args: Vec<String>,
        cwd: String,
        #[serde(default)]
        config: serde_json::Value,
        #[serde(default)]
        services: Vec<String>,
    },
    Request {
        id: u64,
        op: String,
        #[serde(default)]
        payload: serde_json::Value,
    },
    Response {
        id: u64,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        payload: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<ProtoError>,
    },
    Result {
        id: u64,
        ok: bool,
        #[serde(default)]
        payload: serde_json::Value,
    },
}

impl Frame {
    pub fn to_line(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
    pub fn from_line(line: &str) -> serde_json::Result<Frame> {
        serde_json::from_str(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invoke_roundtrips() {
        let f = Frame::Invoke {
            id: 1,
            subcommand: "commit".into(),
            args: vec!["--apply".into()],
            cwd: "/repo".into(),
            config: serde_json::json!({"style": "conventional"}),
            services: vec!["model.chat".into()],
        };
        let line = f.to_line().unwrap();
        assert_eq!(Frame::from_line(&line).unwrap(), f);
    }

    #[test]
    fn request_response_result_roundtrip() {
        for f in [
            Frame::Request { id: 2, op: "model.chat".into(), payload: serde_json::json!({}) },
            Frame::Response { id: 2, ok: true, payload: Some(serde_json::json!({"content": "x"})), error: None },
            Frame::Result { id: 1, ok: true, payload: serde_json::json!({"exit": 0}) },
        ] {
            let line = f.to_line().unwrap();
            assert_eq!(Frame::from_line(&line).unwrap(), f);
        }
    }

    #[test]
    fn tag_field_selects_variant() {
        let line = r#"{"type":"result","id":1,"ok":true,"payload":{"exit":0}}"#;
        assert!(matches!(Frame::from_line(line).unwrap(), Frame::Result { id: 1, ok: true, .. }));
    }

    #[test]
    fn unknown_fields_are_ignored() {
        // Additive-compatible: a future field must not break an older parser.
        let line = r#"{"type":"request","id":3,"op":"audit.record","payload":{},"future":42}"#;
        assert!(matches!(Frame::from_line(line).unwrap(), Frame::Request { id: 3, .. }));
    }
}
```

- [ ] **Step 3: Run the tests, verify they fail then pass**

Run: `cargo test --lib plugin::protocol`
Expected: compiles and PASSES (these are pure serde tests). If `tool` module references cause build errors, they are unrelated — leave `tool` untouched until Phase 4.

- [ ] **Step 4: Commit**

```bash
git add src/lib.rs src/plugin/mod.rs src/plugin/protocol.rs
git commit -m "feat: add plugin ABI frame types (protocol)"
```

---

### Task 3: Manifest (`aish-plugin.toml`) parsing

**Files:**
- Create: `src/plugin/manifest.rs`
- Modify: `src/plugin/mod.rs`

- [ ] **Step 1: Register the module**

Add to `src/plugin/mod.rs`:

```rust
pub mod manifest;
```

- [ ] **Step 2: Write the failing test**

Create `src/plugin/manifest.rs`:

```rust
// SPDX-License-Identifier: MIT
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub abi: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub subcommands: Vec<String>,
    #[serde(default)]
    pub permissions: Permissions,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Permissions {
    #[serde(default)]
    pub model: bool,
    #[serde(default)]
    pub audit: bool,
}

impl Manifest {
    pub fn from_toml(s: &str) -> Result<Manifest, toml::de::Error> {
        toml::from_str(s)
    }
    /// Major component of the declared `abi` string ("1" or "1.2" -> 1).
    pub fn abi_major(&self) -> Option<u32> {
        self.abi.split('.').next()?.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
name = "commit"
version = "0.1.0"
abi = "1"
description = "AI commit message"
subcommands = ["commit"]
[permissions]
model = true
audit = true
"#;

    #[test]
    fn parses_full_manifest() {
        let m = Manifest::from_toml(SAMPLE).unwrap();
        assert_eq!(m.name, "commit");
        assert_eq!(m.subcommands, vec!["commit".to_string()]);
        assert!(m.permissions.model);
        assert!(m.permissions.audit);
        assert_eq!(m.abi_major(), Some(1));
    }

    #[test]
    fn permissions_default_to_false() {
        let m = Manifest::from_toml("name=\"x\"\nversion=\"0.1.0\"\nabi=\"1\"\n").unwrap();
        assert!(!m.permissions.model);
        assert!(!m.permissions.audit);
        assert!(m.subcommands.is_empty());
    }

    #[test]
    fn abi_major_parses_dotted() {
        let m = Manifest::from_toml("name=\"x\"\nversion=\"0.1.0\"\nabi=\"2.5\"\n").unwrap();
        assert_eq!(m.abi_major(), Some(2));
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib plugin::manifest`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/plugin/mod.rs src/plugin/manifest.rs
git commit -m "feat: add plugin manifest parsing"
```

---

### Task 4: Installed-state registry (`plugins.toml`) + conflict check

**Files:**
- Modify: `src/plugin/manifest.rs`

- [ ] **Step 1: Write the failing test**

Append to `src/plugin/manifest.rs` (above the existing `#[cfg(test)]` module, add the types; then add tests inside the test module):

Add near the top (after the `Permissions` block):

```rust
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One installed plugin's recorded state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginEntry {
    pub version: String,
    pub abi: String,
    pub enabled: bool,
    pub path: PathBuf,
    #[serde(default)]
    pub subcommands: Vec<String>,
    pub source: String,
    pub revision: String,
    pub binary_sha256: String,
}

/// The whole `~/.aish/plugins.toml`: name -> entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstalledRegistry {
    #[serde(flatten, default)]
    pub plugins: BTreeMap<String, PluginEntry>,
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("io error on {0}: {1}")]
    Io(PathBuf, String),
    #[error("invalid plugins.toml: {0}")]
    Parse(String),
    #[error("serialize plugins.toml: {0}")]
    Serialize(String),
    #[error("subcommand `{sub}` already provided by enabled plugin `{owner}`")]
    SubcommandConflict { sub: String, owner: String },
}

impl InstalledRegistry {
    pub fn load(path: &Path) -> Result<Self, RegistryError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path)
            .map_err(|e| RegistryError::Io(path.to_path_buf(), e.to_string()))?;
        toml::from_str(&raw).map_err(|e| RegistryError::Parse(e.to_string()))
    }

    pub fn save(&self, path: &Path) -> Result<(), RegistryError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| RegistryError::Io(parent.to_path_buf(), e.to_string()))?;
        }
        let s = toml::to_string_pretty(self).map_err(|e| RegistryError::Serialize(e.to_string()))?;
        std::fs::write(path, s).map_err(|e| RegistryError::Io(path.to_path_buf(), e.to_string()))
    }

    /// Find the enabled plugin (name, entry) that provides `subcommand`.
    pub fn find_by_subcommand(&self, subcommand: &str) -> Option<(&str, &PluginEntry)> {
        self.plugins
            .iter()
            .filter(|(_, e)| e.enabled)
            .find(|(_, e)| e.subcommands.iter().any(|s| s == subcommand))
            .map(|(n, e)| (n.as_str(), e))
    }

    /// Reject if inserting/enabling `name` with `subcommands` would make two
    /// *enabled* plugins claim the same subcommand.
    pub fn check_conflicts(&self, name: &str, subcommands: &[String]) -> Result<(), RegistryError> {
        for sub in subcommands {
            if let Some((owner, _)) = self.find_by_subcommand(sub) {
                if owner != name {
                    return Err(RegistryError::SubcommandConflict {
                        sub: sub.clone(),
                        owner: owner.to_string(),
                    });
                }
            }
        }
        Ok(())
    }
}
```

Add these tests inside the existing `mod tests`:

```rust
    use tempfile::tempdir;

    fn entry(enabled: bool, subs: &[&str]) -> PluginEntry {
        PluginEntry {
            version: "0.1.0".into(),
            abi: "1".into(),
            enabled,
            path: PathBuf::from("/x/bin"),
            subcommands: subs.iter().map(|s| s.to_string()).collect(),
            source: "local".into(),
            revision: "0".into(),
            binary_sha256: "deadbeef".into(),
        }
    }

    #[test]
    fn registry_roundtrips_through_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins.toml");
        let mut reg = InstalledRegistry::default();
        reg.plugins.insert("commit".into(), entry(true, &["commit"]));
        reg.save(&path).unwrap();
        let loaded = InstalledRegistry::load(&path).unwrap();
        assert_eq!(loaded.plugins["commit"].subcommands, vec!["commit".to_string()]);
        assert!(loaded.plugins["commit"].enabled);
    }

    #[test]
    fn missing_file_loads_empty() {
        let dir = tempdir().unwrap();
        let reg = InstalledRegistry::load(&dir.path().join("nope.toml")).unwrap();
        assert!(reg.plugins.is_empty());
    }

    #[test]
    fn find_by_subcommand_ignores_disabled() {
        let mut reg = InstalledRegistry::default();
        reg.plugins.insert("commit".into(), entry(false, &["commit"]));
        assert!(reg.find_by_subcommand("commit").is_none());
        reg.plugins.get_mut("commit").unwrap().enabled = true;
        assert_eq!(reg.find_by_subcommand("commit").unwrap().0, "commit");
    }

    #[test]
    fn conflict_detected_between_enabled_plugins() {
        let mut reg = InstalledRegistry::default();
        reg.plugins.insert("commit".into(), entry(true, &["commit"]));
        let err = reg.check_conflicts("other", &["commit".to_string()]).unwrap_err();
        assert!(matches!(err, RegistryError::SubcommandConflict { .. }));
        // Re-checking the same plugin name is allowed (idempotent install).
        assert!(reg.check_conflicts("commit", &["commit".to_string()]).is_ok());
    }
```

- [ ] **Step 2: Run tests**

Run: `cargo test --lib plugin::manifest`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/plugin/manifest.rs
git commit -m "feat: add installed-plugin registry (plugins.toml) with conflict check"
```

---

# Phase 2 — Plugin host + services

### Task 5: `model.chat` + `audit.record` service handlers

**Files:**
- Modify: `src/audit.rs`
- Create: `src/plugin/services.rs`
- Modify: `src/plugin/mod.rs`

- [ ] **Step 1: Make `AuditEntry` decodable**

Edit `src/audit.rs`, change the derive on `AuditEntry`:

```rust
use serde::{Deserialize, Serialize};
```

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct AuditEntry {
```

Run: `cargo test --lib audit`
Expected: PASS (existing audit tests still pass).

- [ ] **Step 2: Register the module**

Add to `src/plugin/mod.rs`:

```rust
pub mod services;
```

- [ ] **Step 3: Write the failing test**

Create `src/plugin/services.rs`:

```rust
// SPDX-License-Identifier: MIT
use crate::audit::{self, AuditEntry};
use crate::config::resolve::resolve_model;
use crate::config::Config;
use crate::plugin::manifest::Manifest;
use crate::plugin::protocol::{ProtoError, WireMessage};
use crate::provider::{build_provider, ChatRequest, Message, Provider, Role};
use serde::Deserialize;

/// Service ops a plugin may call given its declared permissions.
pub fn available_services(m: &Manifest) -> Vec<String> {
    let mut v = Vec::new();
    if m.permissions.model {
        v.push("model.chat".to_string());
    }
    if m.permissions.audit {
        v.push("audit.record".to_string());
    }
    v
}

/// The sanitized config slice forwarded to a plugin in the `invoke` frame.
/// NEVER includes provider keys. v0.2 forwards the commit settings for all tools.
pub fn scoped_config(cfg: &Config) -> serde_json::Value {
    serde_json::json!({
        "style": cfg.commit.style,
        "language": cfg.commit.language,
        "model": cfg.commit.model,
    })
}

fn role_from_wire(r: &str) -> Role {
    match r {
        "system" => Role::System,
        "assistant" => Role::Assistant,
        _ => Role::User,
    }
}

fn err(code: &str, message: impl Into<String>) -> ProtoError {
    ProtoError { code: code.into(), message: message.into() }
}

/// Dispatch one host service request. `Ok(payload)` -> Response ok:true,
/// `Err(proto)` -> Response ok:false with that error.
pub async fn handle(
    op: &str,
    payload: serde_json::Value,
    manifest: &Manifest,
    cfg: &Config,
) -> Result<serde_json::Value, ProtoError> {
    match op {
        "model.chat" => {
            if !manifest.permissions.model {
                return Err(err("permission_denied", "manifest does not grant `model`"));
            }
            model_chat(payload, cfg).await
        }
        "audit.record" => {
            if !manifest.permissions.audit {
                return Err(err("permission_denied", "manifest does not grant `audit`"));
            }
            audit_record(payload)
        }
        other => Err(err("unknown_op", format!("unknown service op `{other}`"))),
    }
}

async fn model_chat(payload: serde_json::Value, cfg: &Config) -> Result<serde_json::Value, ProtoError> {
    #[derive(Deserialize)]
    struct Req {
        model: String,
        messages: Vec<WireMessage>,
        #[serde(default)]
        temperature: Option<f32>,
    }
    let req: Req = serde_json::from_value(payload).map_err(|e| err("bad_request", e.to_string()))?;
    let resolved = resolve_model(cfg, &req.model).map_err(|e| err("resolve", e.to_string()))?;
    let messages: Vec<Message> = req
        .messages
        .into_iter()
        .map(|m| Message { role: role_from_wire(&m.role), content: m.content })
        .collect();

    // Test hook: AISH_PROVIDER=mock returns canned text without network.
    let provider: Box<dyn Provider> = if std::env::var("AISH_PROVIDER").as_deref() == Ok("mock") {
        Box::new(crate::provider::mock::MockProvider::new(
            std::env::var("AISH_MOCK_REPLY").unwrap_or_else(|_| "feat: add thing".into()),
        ))
    } else {
        build_provider(&resolved.provider_name, &resolved).map_err(|e| err("provider", e.to_string()))?
    };

    let resp = provider
        .chat(ChatRequest { model: resolved.model.clone(), messages, temperature: req.temperature })
        .await
        .map_err(|e| err("provider", e.to_string()))?;
    let usage = resp.usage.unwrap_or_default();
    Ok(serde_json::json!({
        "content": resp.content,
        "usage": { "prompt_tokens": usage.prompt_tokens, "completion_tokens": usage.completion_tokens }
    }))
}

fn audit_record(payload: serde_json::Value) -> Result<serde_json::Value, ProtoError> {
    let entry: AuditEntry = serde_json::from_value(payload).map_err(|e| err("bad_request", e.to_string()))?;
    audit::record(&entry).map_err(|e| err("audit", e.to_string()))?;
    Ok(serde_json::json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::manifest::Permissions;

    fn manifest(model: bool, audit: bool) -> Manifest {
        Manifest {
            name: "commit".into(),
            version: "0.1.0".into(),
            abi: "1".into(),
            description: None,
            subcommands: vec!["commit".into()],
            permissions: Permissions { model, audit },
        }
    }

    fn cfg() -> Config {
        Config::from_yaml(
            "providers:\n  openai: { api_key: sk-x }\nmodels:\n  default: { provider: openai, model: gpt-5-mini }\ncommit: { style: conventional, language: en, model: default }\n",
        )
        .unwrap()
    }

    #[tokio::test]
    async fn model_chat_uses_mock_provider() {
        std::env::set_var("AISH_PROVIDER", "mock");
        std::env::set_var("AISH_MOCK_REPLY", "feat: hello");
        let payload = serde_json::json!({
            "model": "default",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let out = handle("model.chat", payload, &manifest(true, false), &cfg()).await.unwrap();
        assert_eq!(out["content"], "feat: hello");
        std::env::remove_var("AISH_PROVIDER");
    }

    #[tokio::test]
    async fn model_chat_denied_without_permission() {
        let payload = serde_json::json!({"model": "default", "messages": []});
        let e = handle("model.chat", payload, &manifest(false, false), &cfg()).await.unwrap_err();
        assert_eq!(e.code, "permission_denied");
    }

    #[tokio::test]
    async fn unknown_op_errors() {
        let e = handle("bogus.op", serde_json::json!({}), &manifest(true, true), &cfg()).await.unwrap_err();
        assert_eq!(e.code, "unknown_op");
    }

    #[tokio::test]
    async fn scoped_config_excludes_secrets() {
        let v = scoped_config(&cfg());
        assert_eq!(v["style"], "conventional");
        assert!(v.get("providers").is_none());
        assert!(!v.to_string().contains("sk-x"));
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib plugin::services`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/audit.rs src/plugin/mod.rs src/plugin/services.rs
git commit -m "feat: add plugin host services (model.chat, audit.record)"
```

---

### Task 6: A fake plugin fixture for host tests

**Files:**
- Create: `tests/fixtures/fake-plugin/Cargo.toml`
- Create: `tests/fixtures/fake-plugin/src/main.rs`

This is a standalone binary the host integration tests spawn. It is NOT part of the aish workspace (keep it out so `cargo test` of core doesn't build it implicitly); tests build it on demand.

- [ ] **Step 1: Create the fixture crate**

`tests/fixtures/fake-plugin/Cargo.toml`:

```toml
[package]
name = "fake-plugin"
version = "0.0.0"
edition = "2021"

[dependencies]
serde_json = "1"

[[bin]]
name = "fake-plugin"
path = "src/main.rs"
```

`tests/fixtures/fake-plugin/src/main.rs`:

```rust
// SPDX-License-Identifier: MIT
//! Minimal ABI-speaking plugin for host tests. Behavior is driven by env:
//!   FAKE_MODE=echo_model  -> request model.chat, put content in result
//!   FAKE_MODE=crash        -> exit 3 before sending a result
//!   FAKE_MODE=ok           -> just send result ok exit 0
use std::io::{BufRead, Write};

fn main() {
    let mode = std::env::var("FAKE_MODE").unwrap_or_else(|_| "ok".into());
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut lines = stdin.lock().lines();

    // Read the invoke frame.
    let invoke = lines.next().expect("invoke").expect("read");
    let invoke: serde_json::Value = serde_json::from_str(&invoke).unwrap();
    assert_eq!(invoke["type"], "invoke");

    if mode == "crash" {
        eprintln!("boom");
        std::process::exit(3);
    }

    let mut content = String::from("(none)");
    if mode == "echo_model" {
        let req = serde_json::json!({
            "type": "request", "id": 2, "op": "model.chat",
            "payload": {"model": "default", "messages": [{"role":"user","content":"hi"}]}
        });
        writeln!(stdout, "{req}").unwrap();
        stdout.flush().unwrap();
        let resp = lines.next().expect("response").expect("read");
        let resp: serde_json::Value = serde_json::from_str(&resp).unwrap();
        content = resp["payload"]["content"].as_str().unwrap_or("(none)").to_string();
    }

    let result = serde_json::json!({
        "type": "result", "id": 1, "ok": true, "payload": {"exit": 0, "content": content}
    });
    writeln!(stdout, "{result}").unwrap();
    stdout.flush().unwrap();
}
```

- [ ] **Step 2: Verify it builds**

Run: `cargo build --manifest-path tests/fixtures/fake-plugin/Cargo.toml`
Expected: PASS; binary at `tests/fixtures/fake-plugin/target/debug/fake-plugin`.

- [ ] **Step 3: Commit**

```bash
git add tests/fixtures/fake-plugin/Cargo.toml tests/fixtures/fake-plugin/src/main.rs
git commit -m "test: add fake-plugin fixture for host tests"
```

---

### Task 7: The plugin host (spawn + stdio loop)

**Files:**
- Create: `src/plugin/host.rs`
- Modify: `src/plugin/mod.rs`

- [ ] **Step 1: Register the module**

Add to `src/plugin/mod.rs`:

```rust
pub mod host;
```

- [ ] **Step 2: Write the host implementation**

Create `src/plugin/host.rs`:

```rust
// SPDX-License-Identifier: MIT
use crate::config::Config;
use crate::plugin::manifest::{Manifest, PluginEntry};
use crate::plugin::protocol::{Frame, ProtoError, ABI_MAJOR, MAX_FRAME_BYTES};
use crate::plugin::services::{available_services, scoped_config, handle};
use anyhow::{anyhow, Context, Result};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);

/// Spawn an installed plugin and drive one invocation to completion.
/// Returns the plugin's reported exit code.
pub async fn run_plugin(
    entry: &PluginEntry,
    manifest: &Manifest,
    subcommand: &str,
    args: &[String],
    cwd: &Path,
    cfg: &Config,
) -> Result<i32> {
    if manifest.abi_major() != Some(ABI_MAJOR) {
        return Err(anyhow!(
            "plugin `{}` speaks ABI {} but this aish supports major {}",
            manifest.name, manifest.abi, ABI_MAJOR
        ));
    }

    let mut child = Command::new(&entry.path)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawning plugin `{}`", entry.path.display()))?;

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // Always drain stderr so a chatty plugin cannot deadlock the pipe.
    let stderr_handle = tokio::spawn(async move {
        let mut buf = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut buf).await;
        buf
    });

    let invoke = Frame::Invoke {
        id: 1,
        subcommand: subcommand.to_string(),
        args: args.to_vec(),
        cwd: cwd.display().to_string(),
        config: scoped_config(cfg),
        services: available_services(manifest),
    };
    write_frame(&mut stdin, &invoke).await?;

    let mut first = true;
    loop {
        let timeout = if first { STARTUP_TIMEOUT } else { REQUEST_TIMEOUT };
        let maybe_line = tokio::time::timeout(timeout, read_frame_line(&mut reader))
            .await
            .map_err(|_| anyhow!("plugin `{}` timed out", manifest.name))??;
        first = false;

        let Some(line) = maybe_line else {
            // EOF before a result frame => crash.
            let stderr_tail = tail(&stderr_handle.await.unwrap_or_default(), 2000);
            let status = child.wait().await?;
            return Err(anyhow!(
                "plugin `{}` exited before sending a result (status {status}).\n{stderr_tail}",
                manifest.name
            ));
        };

        match Frame::from_line(line.trim_end()) {
            Ok(Frame::Request { id, op, payload }) => {
                let resp = match handle(&op, payload, manifest, cfg).await {
                    Ok(payload) => Frame::Response { id, ok: true, payload: Some(payload), error: None },
                    Err(ProtoError { code, message }) => Frame::Response {
                        id, ok: false, payload: None, error: Some(ProtoError { code, message }),
                    },
                };
                write_frame(&mut stdin, &resp).await?;
            }
            Ok(Frame::Result { ok, payload, .. }) => {
                let _ = child.wait().await?;
                if ok {
                    return Ok(payload.get("exit").and_then(|v| v.as_i64()).unwrap_or(0) as i32);
                }
                return Err(anyhow!("plugin `{}` reported failure: {payload}", manifest.name));
            }
            Ok(other) => {
                let _ = child.kill().await;
                return Err(anyhow!("protocol error: unexpected frame from plugin: {other:?}"));
            }
            Err(e) => {
                let _ = child.kill().await;
                return Err(anyhow!("protocol error: malformed frame: {e}"));
            }
        }
    }
}

async fn write_frame<W: AsyncWriteExt + Unpin>(w: &mut W, frame: &Frame) -> Result<()> {
    let line = frame.to_line()?;
    w.write_all(line.as_bytes()).await?;
    w.write_all(b"\n").await?;
    w.flush().await?;
    Ok(())
}

/// Read one newline-delimited frame, enforcing the size cap. `Ok(None)` on EOF.
async fn read_frame_line<R: AsyncBufRead + AsyncBufReadExt + Unpin>(
    reader: &mut R,
) -> Result<Option<String>> {
    let mut line = String::new();
    let n = reader.read_line(&mut line).await?;
    if n == 0 {
        return Ok(None);
    }
    if line.len() > MAX_FRAME_BYTES {
        return Err(anyhow!("protocol error: frame exceeds {MAX_FRAME_BYTES} bytes"));
    }
    Ok(Some(line))
}

fn tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let start = s.len() - max;
    let start = (start..s.len()).find(|i| s.is_char_boundary(*i)).unwrap_or(s.len());
    format!("…{}", &s[start..])
}

use tokio::io::AsyncBufRead;
```

- [ ] **Step 3: Verify it builds**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/plugin/mod.rs src/plugin/host.rs
git commit -m "feat: add plugin host (spawn + stdio protocol loop)"
```

---

### Task 8: Host integration tests against the fake plugin

**Files:**
- Create: `tests/host_loop.rs`

- [ ] **Step 1: Write the failing test**

Create `tests/host_loop.rs`:

```rust
// SPDX-License-Identifier: MIT
use aish::config::Config;
use aish::plugin::host::run_plugin;
use aish::plugin::manifest::{Manifest, Permissions, PluginEntry};
use std::path::PathBuf;
use std::process::Command;

fn build_fake() -> PathBuf {
    let status = Command::new(env!("CARGO"))
        .args(["build", "--manifest-path", "tests/fixtures/fake-plugin/Cargo.toml"])
        .status()
        .unwrap();
    assert!(status.success(), "fake-plugin build failed");
    PathBuf::from("tests/fixtures/fake-plugin/target/debug/fake-plugin")
}

fn manifest() -> Manifest {
    Manifest {
        name: "fake".into(),
        version: "0.0.0".into(),
        abi: "1".into(),
        description: None,
        subcommands: vec!["fake".into()],
        permissions: Permissions { model: true, audit: true },
    }
}

fn entry(bin: PathBuf) -> PluginEntry {
    PluginEntry {
        version: "0.0.0".into(),
        abi: "1".into(),
        enabled: true,
        path: bin,
        subcommands: vec!["fake".into()],
        source: "local".into(),
        revision: "0".into(),
        binary_sha256: String::new(),
    }
}

fn cfg() -> Config {
    Config::from_yaml("providers:\n  openai: { api_key: sk-x }\nmodels:\n  default: { provider: openai, model: gpt-5-mini }\ncommit: { style: conventional, language: en, model: default }\n").unwrap()
}

#[tokio::test]
async fn plugin_ok_returns_exit_zero() {
    let bin = build_fake();
    std::env::set_var("FAKE_MODE", "ok");
    let code = run_plugin(&entry(bin), &manifest(), "fake", &[], &std::env::current_dir().unwrap(), &cfg()).await.unwrap();
    assert_eq!(code, 0);
}

#[tokio::test]
async fn plugin_model_chat_roundtrips_through_host() {
    let bin = build_fake();
    std::env::set_var("FAKE_MODE", "echo_model");
    std::env::set_var("AISH_PROVIDER", "mock");
    std::env::set_var("AISH_MOCK_REPLY", "feat: from host");
    let code = run_plugin(&entry(bin), &manifest(), "fake", &[], &std::env::current_dir().unwrap(), &cfg()).await.unwrap();
    assert_eq!(code, 0);
    std::env::remove_var("AISH_PROVIDER");
}

#[tokio::test]
async fn plugin_crash_before_result_is_an_error() {
    let bin = build_fake();
    std::env::set_var("FAKE_MODE", "crash");
    let err = run_plugin(&entry(bin), &manifest(), "fake", &[], &std::env::current_dir().unwrap(), &cfg()).await.unwrap_err();
    assert!(err.to_string().contains("before sending a result"));
}

#[tokio::test]
async fn abi_major_mismatch_is_rejected() {
    let bin = build_fake();
    let mut m = manifest();
    m.abi = "2".into();
    let err = run_plugin(&entry(bin), &m, "fake", &[], &std::env::current_dir().unwrap(), &cfg()).await.unwrap_err();
    assert!(err.to_string().contains("ABI"));
}
```

> Note: these tests set process-global env vars; run the suite single-threaded if flaky: `cargo test --test host_loop -- --test-threads=1`.

- [ ] **Step 2: Run tests**

Run: `cargo test --test host_loop -- --test-threads=1`
Expected: PASS (4 tests).

- [ ] **Step 3: Commit**

```bash
git add tests/host_loop.rs
git commit -m "test: host loop integration tests (ok, model.chat, crash, abi mismatch)"
```

---

# Phase 3 — Install / registry

### Task 9: sha256 + atomic install from a built binary

**Files:**
- Create: `src/plugin/install.rs`
- Modify: `src/plugin/mod.rs`

- [ ] **Step 1: Register the module**

Add to `src/plugin/mod.rs`:

```rust
pub mod install;
```

- [ ] **Step 2: Write install primitives + failing test**

Create `src/plugin/install.rs`:

```rust
// SPDX-License-Identifier: MIT
use crate::plugin::aish_home;
use crate::plugin::manifest::{InstalledRegistry, Manifest, PluginEntry};
use anyhow::{anyhow, Context, Result};
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub fn plugins_dir() -> PathBuf {
    aish_home().join("plugins")
}
pub fn plugins_toml() -> PathBuf {
    aish_home().join("plugins.toml")
}
fn lock_path() -> PathBuf {
    aish_home().join("plugins.lock")
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Ok(format!("{:x}", h.finalize()))
}

/// Install an already-built plugin binary + manifest atomically and record it.
/// `source`/`revision` describe provenance for `plugins.toml`.
pub fn install_built(
    manifest: &Manifest,
    built_binary: &Path,
    source: &str,
    revision: &str,
) -> Result<PluginEntry> {
    let home = aish_home();
    std::fs::create_dir_all(&home)?;

    // Serialize concurrent installs.
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(lock_path())?;
    lock.lock_exclusive()?;
    let result = (|| {
        let mut reg = InstalledRegistry::load(&plugins_toml())?;
        reg.check_conflicts(&manifest.name, &manifest.subcommands)?;

        let final_dir = plugins_dir().join(&manifest.name);
        let staging = plugins_dir().join(format!(".{}.staging", manifest.name));
        if staging.exists() {
            std::fs::remove_dir_all(&staging)?;
        }
        std::fs::create_dir_all(&staging)?;

        let bin_name = manifest.name.clone();
        let staged_bin = staging.join(&bin_name);
        std::fs::copy(built_binary, &staged_bin)
            .with_context(|| format!("copying {} -> {}", built_binary.display(), staged_bin.display()))?;
        make_executable(&staged_bin)?;
        std::fs::write(staging.join("aish-plugin.toml"), toml::to_string_pretty(manifest)?)?;

        let sha = sha256_file(&staged_bin)?;

        // Atomic-ish replace of the install dir.
        if final_dir.exists() {
            std::fs::remove_dir_all(&final_dir)?;
        }
        std::fs::rename(&staging, &final_dir)
            .with_context(|| format!("renaming staging into {}", final_dir.display()))?;

        let entry = PluginEntry {
            version: manifest.version.clone(),
            abi: manifest.abi.clone(),
            enabled: true,
            path: final_dir.join(&bin_name),
            subcommands: manifest.subcommands.clone(),
            source: source.to_string(),
            revision: revision.to_string(),
            binary_sha256: sha,
        };
        reg.plugins.insert(manifest.name.clone(), entry.clone());
        reg.save(&plugins_toml())?;
        Ok(entry)
    })();
    let _ = FileExt::unlock(&lock);
    result
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}
#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

/// Verify a plugin binary still matches its recorded hash (tamper check).
pub fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    if expected.is_empty() {
        return Ok(()); // unrecorded (e.g. test entries) — skip
    }
    let actual = sha256_file(path)?;
    if actual != expected {
        return Err(anyhow!("plugin binary at {} failed integrity check", path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn manifest() -> Manifest {
        Manifest::from_toml("name=\"demo\"\nversion=\"0.1.0\"\nabi=\"1\"\nsubcommands=[\"demo\"]\n").unwrap()
    }

    #[test]
    fn install_built_records_and_hashes() {
        let home = tempdir().unwrap();
        std::env::set_var("AISH_HOME", home.path());
        let src = tempdir().unwrap();
        let bin = src.path().join("prebuilt");
        std::fs::write(&bin, b"#!/bin/sh\nexit 0\n").unwrap();

        let entry = install_built(&manifest(), &bin, "local", "rev1").unwrap();
        assert!(entry.path.exists());
        assert_eq!(entry.binary_sha256, sha256_file(&entry.path).unwrap());
        assert!(entry.enabled);

        let reg = InstalledRegistry::load(&plugins_toml()).unwrap();
        assert_eq!(reg.plugins["demo"].source, "local");
        verify_sha256(&entry.path, &entry.binary_sha256).unwrap();
        std::env::remove_var("AISH_HOME");
    }

    #[test]
    fn tamper_check_fails_on_modified_binary() {
        let home = tempdir().unwrap();
        std::env::set_var("AISH_HOME", home.path());
        let src = tempdir().unwrap();
        let bin = src.path().join("prebuilt");
        std::fs::write(&bin, b"original").unwrap();
        let entry = install_built(&manifest(), &bin, "local", "rev1").unwrap();
        std::fs::write(&entry.path, b"tampered").unwrap();
        assert!(verify_sha256(&entry.path, &entry.binary_sha256).is_err());
        std::env::remove_var("AISH_HOME");
    }
}
```

> Note: tests set the global `AISH_HOME`; run install tests single-threaded.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib plugin::install -- --test-threads=1`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/plugin/mod.rs src/plugin/install.rs
git commit -m "feat: atomic plugin install from built binary + sha256 integrity"
```

---

### Task 10: Registry resolution + build-on-install

**Files:**
- Modify: `src/plugin/install.rs`

- [ ] **Step 1: Write resolution + build, with a failing test**

Append to `src/plugin/install.rs` (before the `#[cfg(test)]` module):

```rust
use std::process::Command;

/// Where the registry lives. A local filesystem path (tests/dev) or a git URL.
#[derive(Debug, Clone)]
pub enum RegistrySource {
    Local(PathBuf),
    Git { url: String },
}

impl RegistrySource {
    /// Parse a config/CLI value: an existing path or a `file://` is Local,
    /// anything containing `://` or `git@` is Git.
    pub fn parse(value: &str) -> RegistrySource {
        let trimmed = value.trim();
        if let Some(rest) = trimmed.strip_prefix("file://") {
            return RegistrySource::Local(PathBuf::from(rest));
        }
        if trimmed.contains("://") || trimmed.starts_with("git@") {
            return RegistrySource::Git { url: trimmed.to_string() };
        }
        RegistrySource::Local(PathBuf::from(trimmed))
    }
}

/// Ensure a local checkout of the registry exists, returning (dir, revision).
/// Local sources are used in place; git sources are cloned/updated under
/// `~/.aish/registry`.
pub fn ensure_registry(source: &RegistrySource) -> Result<(PathBuf, String)> {
    match source {
        RegistrySource::Local(dir) => {
            if !dir.exists() {
                return Err(anyhow!("registry path does not exist: {}", dir.display()));
            }
            Ok((dir.clone(), "local".to_string()))
        }
        RegistrySource::Git { url } => {
            let dir = aish_home().join("registry");
            if dir.join(".git").exists() {
                run_git(&dir, &["fetch", "--quiet", "origin"])?;
                run_git(&dir, &["reset", "--quiet", "--hard", "origin/HEAD"])?;
            } else {
                std::fs::create_dir_all(aish_home())?;
                run_git(Path::new("."), &["clone", "--quiet", url, &dir.display().to_string()])?;
            }
            let rev = run_git(&dir, &["rev-parse", "HEAD"])?.trim().to_string();
            Ok((dir, rev))
        }
    }
}

fn run_git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").current_dir(dir).args(args).output()
        .with_context(|| format!("running git {args:?}"))?;
    if !out.status.success() {
        return Err(anyhow!("git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Build the named plugin crate in the registry checkout and return the path to
/// the produced release binary. Uses an isolated CARGO_HOME under ~/.aish.
pub fn build_plugin(registry_dir: &Path, name: &str) -> Result<PathBuf> {
    let crate_dir = registry_dir.join(name);
    let manifest_path = crate_dir.join("Cargo.toml");
    if !manifest_path.exists() {
        return Err(anyhow!("plugin `{name}` not found in registry ({})", manifest_path.display()));
    }
    let cargo_home = aish_home().join("cargo-home");
    std::fs::create_dir_all(&cargo_home)?;
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .env("CARGO_HOME", &cargo_home)
        .args(["build", "--release", "--locked", "--manifest-path"])
        .arg(&manifest_path)
        .output()
        .with_context(|| format!("building plugin `{name}`"))?;
    if !out.status.success() {
        return Err(anyhow!("cargo build failed for `{name}`: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let bin = crate_dir.join("target").join("release").join(name);
    if !bin.exists() {
        return Err(anyhow!("expected built binary at {}", bin.display()));
    }
    Ok(bin)
}

/// Read the plugin's manifest from the registry checkout.
pub fn read_manifest(registry_dir: &Path, name: &str) -> Result<Manifest> {
    let path = registry_dir.join(name).join("aish-plugin.toml");
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("reading manifest {}", path.display()))?;
    Manifest::from_toml(&raw).map_err(|e| anyhow!("invalid manifest {}: {e}", path.display()))
}

/// Full install pipeline: resolve registry -> read manifest -> build -> install.
pub fn install_from_registry(source: &RegistrySource, name: &str) -> Result<PluginEntry> {
    let (dir, revision) = ensure_registry(source)?;
    let manifest = read_manifest(&dir, name)?;
    if manifest.name != name {
        return Err(anyhow!("manifest name `{}` != requested `{name}`", manifest.name));
    }
    let bin = build_plugin(&dir, name)?;
    let source_str = match source {
        RegistrySource::Local(p) => p.display().to_string(),
        RegistrySource::Git { url } => url.clone(),
    };
    install_built(&manifest, &bin, &source_str, &revision)
}
```

Add this test to the `#[cfg(test)]` module:

```rust
    #[test]
    fn registry_source_parsing() {
        assert!(matches!(RegistrySource::parse("/opt/aish-plugins"), RegistrySource::Local(_)));
        assert!(matches!(RegistrySource::parse("file:///tmp/x"), RegistrySource::Local(_)));
        assert!(matches!(RegistrySource::parse("git@github.com:u/r.git"), RegistrySource::Git { .. }));
        assert!(matches!(RegistrySource::parse("https://github.com/u/r"), RegistrySource::Git { .. }));
    }

    #[test]
    fn ensure_local_registry_missing_errors() {
        let s = RegistrySource::Local(PathBuf::from("/no/such/registry/xyz"));
        assert!(ensure_registry(&s).is_err());
    }
```

- [ ] **Step 2: Run tests**

Run: `cargo test --lib plugin::install -- --test-threads=1`
Expected: PASS (parsing + missing-registry; the build path is covered by the E2E in Phase 6 to avoid a slow cargo build here).

- [ ] **Step 3: Commit**

```bash
git add src/plugin/install.rs
git commit -m "feat: registry resolution + build-on-install pipeline"
```

---

# Phase 4 — CLI rewire (remove built-in commit)

### Task 11: Tamper-check before spawn in the host

**Files:**
- Modify: `src/plugin/host.rs`

- [ ] **Step 1: Wire verify_sha256 into run_plugin**

In `src/plugin/host.rs`, add the import and the check at the top of `run_plugin` (right after the ABI check):

Add import near the others:

```rust
use crate::plugin::install::verify_sha256;
```

Insert before the `Command::new(&entry.path)` spawn:

```rust
    verify_sha256(&entry.path, &entry.binary_sha256)?;
```

- [ ] **Step 2: Verify build + existing host tests still pass**

Run: `cargo test --test host_loop -- --test-threads=1`
Expected: PASS (fixture entries use empty `binary_sha256`, so the check is skipped).

- [ ] **Step 3: Commit**

```bash
git add src/plugin/host.rs
git commit -m "feat: verify plugin binary hash before spawn"
```

---

### Task 12: CLI surface — `plugin` command + external subcommands

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Replace the command enum**

Edit `src/cli.rs`. Remove the `Commit { .. }` variant. Add the `plugin` command and an external catch-all. The full file becomes:

```rust
// SPDX-License-Identifier: MIT
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "aish",
    version,
    about = "AI-powered extensible shell for developers"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
    /// Print detailed error context.
    #[arg(long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Command {
    /// Write a commented config template to ~/.aish/config.yaml.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// List configured providers.
    Providers {
        #[command(subcommand)]
        action: ProvidersAction,
    },
    /// List model aliases.
    Models {
        #[command(subcommand)]
        action: ModelsAction,
    },
    /// Manage tool plugins (install / list / enable / disable / uninstall).
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
    /// Any other subcommand is dispatched to an installed plugin.
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Subcommand)]
pub enum ConfigAction {
    Init {
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
pub enum ProvidersAction {
    List,
}

#[derive(Subcommand)]
pub enum ModelsAction {
    List,
}

#[derive(Subcommand)]
pub enum PluginAction {
    /// Build and install a plugin from the registry.
    Install {
        /// Plugin name (directory in the registry).
        name: String,
        /// Skip the trusted-code confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
    /// List installed plugins.
    List,
    /// Enable an installed plugin.
    Enable { name: String },
    /// Disable an installed plugin (keeps it installed).
    Disable { name: String },
    /// Remove an installed plugin.
    Uninstall { name: String },
}
```

- [ ] **Step 2: Verify it builds (main.rs will break — fixed in Task 13)**

Run: `cargo build 2>&1 | head -20`
Expected: FAIL — `main.rs` still references `Command::Commit` / `run_commit`. That's expected; fixed next task.

- [ ] **Step 3: Do NOT commit yet** (commit together with Task 13 so the tree stays buildable).

---

### Task 13: main.rs dispatch + delete the in-process tool module

**Files:**
- Modify: `src/main.rs`
- Modify: `src/lib.rs`
- Delete: `src/tool/commit.rs`, `src/tool/mod.rs`
- Delete: `tests/commit_e2e.rs`

- [ ] **Step 1: Delete the obsolete module + its e2e**

```bash
git rm src/tool/commit.rs src/tool/mod.rs tests/commit_e2e.rs
rmdir src/tool 2>/dev/null || true
```

- [ ] **Step 2: Drop the `tool` module from lib.rs**

Edit `src/lib.rs` to remove the `pub mod tool;` line (if still present). Final `src/lib.rs`:

```rust
// SPDX-License-Identifier: MIT
pub mod audit;
pub mod cli;
pub mod config;
pub mod git;
pub mod plugin;
pub mod provider;
```

- [ ] **Step 3: Rewrite `src/main.rs`**

Replace the whole file:

```rust
// SPDX-License-Identifier: MIT
use aish::cli::{Cli, Command, ConfigAction, ModelsAction, PluginAction, ProvidersAction};
use aish::config::Config;
use aish::plugin::host::run_plugin;
use aish::plugin::install::{self, RegistrySource};
use aish::plugin::manifest::{InstalledRegistry, Manifest};
use anyhow::{anyhow, Context, Result};
use clap::Parser;
use std::io::Write;

const DEFAULT_REGISTRY: &str = "git@github.com:daaquan/aish-plugins.git";

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let verbose = cli.verbose;
    if let Err(e) = run(cli).await {
        if verbose {
            eprintln!("error: {e:?}");
        } else {
            eprintln!("error: {e}");
        }
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Config { action: ConfigAction::Init { force } } => {
            let path = Config::default_path();
            Config::write_template(&path, force)
                .with_context(|| format!("writing config to {}", path.display()))?;
            println!("Wrote config template to {}", path.display());
            Ok(())
        }
        Command::Providers { action: ProvidersAction::List } => {
            let cfg = Config::load()?;
            for (name, p) in &cfg.providers {
                let status = if p.api_key.is_some() {
                    "key set"
                } else if p.base_url.is_some() {
                    "endpoint set"
                } else {
                    "unconfigured"
                };
                println!("{name:12} {status}");
            }
            Ok(())
        }
        Command::Models { action: ModelsAction::List } => {
            let cfg = Config::load()?;
            for (alias, m) in &cfg.models {
                println!("{alias:10} -> {}/{}", m.provider, m.model);
            }
            Ok(())
        }
        Command::Plugin { action } => run_plugin_cmd(action).await,
        Command::External(args) => dispatch_external(args).await,
    }
}

fn registry_source() -> RegistrySource {
    let value = std::env::var("AISH_REGISTRY").unwrap_or_else(|_| DEFAULT_REGISTRY.to_string());
    RegistrySource::parse(&value)
}

async fn run_plugin_cmd(action: PluginAction) -> Result<()> {
    match action {
        PluginAction::Install { name, yes } => {
            let source = registry_source();
            if !yes {
                println!(
                    "Installing `{name}` builds and runs code from:\n  {source:?}\n\
                     Plugins are trusted native executables (install runs build scripts).\n\
                     Continue? [y/N] "
                );
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !matches!(input.trim().to_lowercase().as_str(), "y" | "yes") {
                    println!("Aborted.");
                    return Ok(());
                }
            }
            let entry = install::install_from_registry(&source, &name)?;
            println!("Installed `{name}` {} (revision {}).", entry.version, entry.revision);
            Ok(())
        }
        PluginAction::List => {
            let reg = InstalledRegistry::load(&install::plugins_toml())?;
            if reg.plugins.is_empty() {
                println!("No plugins installed. Try `aish plugin install commit`.");
            }
            for (name, e) in &reg.plugins {
                let state = if e.enabled { "enabled" } else { "disabled" };
                println!("{name:14} {:8} {state:8} [{}]", e.version, e.subcommands.join(","));
            }
            Ok(())
        }
        PluginAction::Enable { name } => set_enabled(&name, true),
        PluginAction::Disable { name } => set_enabled(&name, false),
        PluginAction::Uninstall { name } => {
            let path = install::plugins_toml();
            let mut reg = InstalledRegistry::load(&path)?;
            let entry = reg.plugins.remove(&name).ok_or_else(|| anyhow!("plugin `{name}` is not installed"))?;
            if let Some(dir) = entry.path.parent() {
                let _ = std::fs::remove_dir_all(dir);
            }
            reg.save(&path)?;
            println!("Uninstalled `{name}`.");
            Ok(())
        }
    }
}

fn set_enabled(name: &str, enabled: bool) -> Result<()> {
    let path = install::plugins_toml();
    let mut reg = InstalledRegistry::load(&path)?;
    let subs = reg
        .plugins
        .get(name)
        .ok_or_else(|| anyhow!("plugin `{name}` is not installed"))?
        .subcommands
        .clone();
    if enabled {
        reg.check_conflicts(name, &subs)?;
    }
    reg.plugins.get_mut(name).unwrap().enabled = enabled;
    reg.save(&path)?;
    println!("{} `{name}`.", if enabled { "Enabled" } else { "Disabled" });
    Ok(())
}

async fn dispatch_external(args: Vec<String>) -> Result<()> {
    let subcommand = args.first().cloned().ok_or_else(|| anyhow!("no subcommand given"))?;
    let rest = &args[1..];
    let cfg = Config::load()?;
    let reg = InstalledRegistry::load(&install::plugins_toml())?;
    let (_name, entry) = reg.find_by_subcommand(&subcommand).ok_or_else(|| {
        anyhow!("no enabled plugin provides `{subcommand}` — try `aish plugin install {subcommand}`")
    })?;
    // Load the installed manifest for permission + abi info.
    let manifest_path = entry.path.parent().unwrap().join("aish-plugin.toml");
    let manifest = Manifest::from_toml(&std::fs::read_to_string(&manifest_path)?)
        .map_err(|e| anyhow!("reading installed manifest: {e}"))?;
    let cwd = std::env::current_dir()?;
    let code = run_plugin(entry, &manifest, &subcommand, rest, &cwd, &cfg).await?;
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}
```

- [ ] **Step 4: Build the whole workspace**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 5: Lint + full test (minus E2E that needs the plugin repo)**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: PASS (fix any clippy/fmt issues).

Run: `cargo test --all -- --test-threads=1`
Expected: PASS (protocol/manifest/services/install/host tests green; no commit_e2e).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: route external subcommands to plugins; remove built-in commit"
```

---

# Phase 5 — The `commit` plugin (aish-plugins repo)

> Worktree note: these tasks operate in `/opt/aish-plugins` (already cloned, empty). `cd` there for this phase.

### Task 14: Scaffold the aish-plugins workspace + commit crate skeleton

**Files (in `/opt/aish-plugins`):**
- Create: `Cargo.toml`
- Create: `commit/Cargo.toml`
- Create: `commit/aish-plugin.toml`
- Create: `LICENSE`, `README.md`

- [ ] **Step 1: Workspace manifest**

`/opt/aish-plugins/Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["commit"]
```

`/opt/aish-plugins/commit/Cargo.toml`:

```toml
[package]
name = "commit"
version = "0.1.0"
edition = "2021"
license = "MIT"
description = "aish commit plugin: AI commit message from the staged diff"

[[bin]]
name = "commit"
path = "src/main.rs"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[dev-dependencies]
tempfile = "3"
```

`/opt/aish-plugins/commit/aish-plugin.toml`:

```toml
name = "commit"
version = "0.1.0"
abi = "1"
description = "AI commit message from the staged diff"
subcommands = ["commit"]

[permissions]
model = true
audit = true
```

`/opt/aish-plugins/README.md`:

```markdown
# aish-plugins

Plugins for [aish](https://github.com/daaquan/aish). Each directory is a source
crate built on `aish plugin install <name>`. Plugins speak the aish stdio ABI
(see `commit/src/protocol.rs`).
```

Copy the aish `LICENSE` (MIT) into `/opt/aish-plugins/LICENSE`.

- [ ] **Step 2: Commit (in the plugins repo)**

```bash
cd /opt/aish-plugins
git add -A
git commit -m "chore: scaffold aish-plugins workspace + commit crate"
```

---

### Task 15: Plugin-side protocol + message logic (moved from core)

**Files (in `/opt/aish-plugins`):**
- Create: `commit/src/protocol.rs`
- Create: `commit/src/message.rs`

- [ ] **Step 1: Protocol contract copy**

`/opt/aish-plugins/commit/src/protocol.rs` — the plugin-side mirror of the ABI. Only what the plugin needs:

```rust
// SPDX-License-Identifier: MIT
//! Plugin-side copy of the aish ABI contract (must match aish core's
//! src/plugin/protocol.rs for ABI major 1).
use serde::{Deserialize, Serialize};

pub const ABI_MAJOR: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtoError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Frame {
    Invoke {
        id: u64,
        subcommand: String,
        #[serde(default)]
        args: Vec<String>,
        cwd: String,
        #[serde(default)]
        config: serde_json::Value,
        #[serde(default)]
        services: Vec<String>,
    },
    Request {
        id: u64,
        op: String,
        #[serde(default)]
        payload: serde_json::Value,
    },
    Response {
        id: u64,
        ok: bool,
        #[serde(default)]
        payload: Option<serde_json::Value>,
        #[serde(default)]
        error: Option<ProtoError>,
    },
    Result {
        id: u64,
        ok: bool,
        #[serde(default)]
        payload: serde_json::Value,
    },
}
```

- [ ] **Step 2: Move message logic with its tests**

`/opt/aish-plugins/commit/src/message.rs` — copy `build_messages` + `postprocess` + their unit tests from the old core `src/tool/commit.rs`, but return `WireMessage` (the plugin's wire type) instead of core's `Message`:

```rust
// SPDX-License-Identifier: MIT
use crate::protocol::WireMessage;

pub const MAX_DIFF_CHARS: usize = 12_000;

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
pub fn build_messages(style: &str, language: &str, diff: &str) -> Vec<WireMessage> {
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
    vec![
        WireMessage { role: "system".into(), content: system },
        WireMessage { role: "user".into(), content: user },
    ]
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
        let msgs = build_messages("conventional", "en", "diff --git a/x");
        assert_eq!(msgs[0].role, "system");
        assert!(msgs[0].content.to_lowercase().contains("conventional"));
        assert!(msgs[0].content.contains("en"));
        assert!(msgs[1].content.contains("diff --git a/x"));
    }

    #[test]
    fn strips_code_fences_and_trims() {
        assert_eq!(postprocess("```\nfeat: add x\n```\n"), "feat: add x");
    }

    #[test]
    fn strips_language_tagged_fence() {
        assert_eq!(postprocess("```text\nfix: y\n```"), "fix: y");
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
        let msgs = build_messages("conventional", "en", &big);
        assert!(msgs[1].content.contains("[diff truncated]"));
        assert!(msgs[1].content.len() < MAX_DIFF_CHARS + 200);
    }

    #[test]
    fn caps_huge_diff_on_char_boundary_without_panic() {
        let big = "あ".repeat(MAX_DIFF_CHARS);
        let msgs = build_messages("conventional", "en", &big);
        assert!(msgs[1].content.contains("[diff truncated]"));
    }
}
```

- [ ] **Step 3: Run plugin unit tests**

Run: `cd /opt/aish-plugins && cargo test -p commit message`
Expected: PASS (will fail to compile until `main.rs` declares the modules — add a stub `src/main.rs` with `mod protocol; mod message; fn main(){}` to compile, then proceed to Task 16).

- [ ] **Step 4: Commit**

```bash
cd /opt/aish-plugins
git add commit/src/protocol.rs commit/src/message.rs
git commit -m "feat: commit plugin protocol contract + message logic (moved from aish core)"
```

---

### Task 16: Commit plugin main — stdio loop + /dev/tty UI

**Files (in `/opt/aish-plugins`):**
- Create: `commit/src/git.rs`
- Create: `commit/src/main.rs`

- [ ] **Step 1: Git helpers (plugin runs git itself)**

`/opt/aish-plugins/commit/src/git.rs`:

```rust
// SPDX-License-Identifier: MIT
use std::path::Path;
use std::process::Command;

pub fn staged_diff(dir: &Path) -> Result<String, String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(["diff", "--cached"])
        .output()
        .map_err(|e| format!("running git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn commit(dir: &Path, message: &str, signoff: bool) -> Result<(), String> {
    let mut args = vec!["commit", "-m", message];
    if signoff {
        args.push("-s");
    }
    let out = Command::new("git")
        .current_dir(dir)
        .args(&args)
        .output()
        .map_err(|e| format!("running git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    Ok(())
}
```

- [ ] **Step 2: The stdio loop**

`/opt/aish-plugins/commit/src/main.rs`:

```rust
// SPDX-License-Identifier: MIT
mod git;
mod message;
mod protocol;

use protocol::{Frame, WireMessage};
use std::io::{BufRead, Write};
use std::path::PathBuf;

fn main() {
    if let Err(e) = run() {
        // Errors go to the result frame; this is a last resort.
        eprintln!("commit plugin error: {e}");
        std::process::exit(1);
    }
}

struct Host {
    stdin: std::io::Stdin,
    stdout: std::io::Stdout,
    next_id: u64,
}

impl Host {
    fn new() -> Self {
        Host { stdin: std::io::stdin(), stdout: std::io::stdout(), next_id: 100 }
    }

    fn send(&mut self, frame: &Frame) -> Result<(), String> {
        let line = serde_json::to_string(frame).map_err(|e| e.to_string())?;
        let mut out = self.stdout.lock();
        writeln!(out, "{line}").map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())
    }

    fn read_frame(&mut self) -> Result<Frame, String> {
        let mut line = String::new();
        let n = self.stdin.lock().read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("host closed stdin".into());
        }
        serde_json::from_str(line.trim_end()).map_err(|e| e.to_string())
    }

    /// Send a request and block for its response payload.
    fn request(&mut self, op: &str, payload: serde_json::Value) -> Result<serde_json::Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&Frame::Request { id, op: op.to_string(), payload })?;
        match self.read_frame()? {
            Frame::Response { ok: true, payload: Some(p), .. } => Ok(p),
            Frame::Response { ok: false, error, .. } => {
                Err(error.map(|e| format!("{}: {}", e.code, e.message)).unwrap_or_else(|| "service error".into()))
            }
            other => Err(format!("expected response, got {other:?}")),
        }
    }
}

fn run() -> Result<(), String> {
    let mut host = Host::new();

    let (cwd, args, config) = match host.read_frame()? {
        Frame::Invoke { cwd, args, config, .. } => (PathBuf::from(cwd), args, config),
        other => return Err(format!("expected invoke, got {other:?}")),
    };

    let apply = args.iter().any(|a| a == "--apply");
    let signoff = args.iter().any(|a| a == "--signoff");
    let style = config.get("style").and_then(|v| v.as_str()).unwrap_or("conventional").to_string();
    let language = config.get("language").and_then(|v| v.as_str()).unwrap_or("en").to_string();
    let model = config.get("model").and_then(|v| v.as_str()).unwrap_or("default").to_string();

    let diff = git::staged_diff(&cwd)?;
    if diff.trim().is_empty() {
        tty_print("Nothing staged. Run `git add` first.\n");
        return finish(&mut host, 0);
    }

    let messages = message::build_messages(&style, &language, &diff);
    let payload = serde_json::json!({
        "model": model,
        "messages": messages.iter().map(|m: &WireMessage| serde_json::json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
        "temperature": 0.2,
    });
    let resp = host.request("model.chat", payload)?;
    let raw = resp.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let msg = message::postprocess(raw);
    if msg.is_empty() {
        return Err(format!("model returned empty/unusable message; not committing. raw: {raw:?}"));
    }

    tty_print(&format!("\nSuggested commit:\n\n{msg}\n\n"));

    let decision = if apply || tty_confirm("Accept? [Y/n] ") {
        git::commit(&cwd, &msg, signoff)?;
        tty_print("Committed.\n");
        "applied"
    } else {
        tty_print("Aborted.\n");
        "rejected"
    };

    let usage = resp.get("usage").cloned().unwrap_or_else(|| serde_json::json!({}));
    let _ = host.request("audit.record", serde_json::json!({
        "tool": "git.commit.message.generate",
        "provider": "",
        "model": model,
        "prompt_tokens": usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
        "completion_tokens": usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
        "decision": decision,
    }));

    finish(&mut host, 0)
}

fn finish(host: &mut Host, exit: i64) -> Result<(), String> {
    host.send(&Frame::Result { id: 1, ok: true, payload: serde_json::json!({"exit": exit}) })
}

/// Human output goes to /dev/tty, never to protocol stdout.
fn tty_print(s: &str) {
    if let Ok(mut tty) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
        let _ = tty.write_all(s.as_bytes());
    }
}

/// Prompt on /dev/tty. Returns true on Y/empty. Non-interactive (no tty) = false.
fn tty_confirm(prompt: &str) -> bool {
    use std::io::{BufRead, BufReader};
    let Ok(tty_w) = std::fs::OpenOptions::new().write(true).open("/dev/tty") else {
        return false;
    };
    let Ok(tty_r) = std::fs::OpenOptions::new().read(true).open("/dev/tty") else {
        return false;
    };
    {
        let mut w = tty_w;
        let _ = w.write_all(prompt.as_bytes());
        let _ = w.flush();
    }
    let mut line = String::new();
    if BufReader::new(tty_r).read_line(&mut line).unwrap_or(0) == 0 {
        return false;
    }
    let a = line.trim().to_lowercase();
    a.is_empty() || a == "y" || a == "yes"
}
```

- [ ] **Step 3: Build + unit tests**

Run: `cd /opt/aish-plugins && cargo build -p commit && cargo test -p commit`
Expected: PASS (message tests; the loop is exercised by the core E2E in Phase 6).

- [ ] **Step 4: Commit + push the plugins repo**

```bash
cd /opt/aish-plugins
git add commit/src/git.rs commit/src/main.rs
git commit -m "feat: commit plugin stdio loop with /dev/tty UI"
git push -u origin HEAD
```

---

# Phase 6 — End-to-end + docs

### Task 17: Core E2E — install commit from a local registry and run it

**Files (in `/opt/aish`):**
- Create: `tests/plugin_e2e.rs`

- [ ] **Step 1: Write the E2E**

This builds the real `commit` plugin from a local checkout of the plugins repo (use `/opt/aish-plugins` as a local registry), installs it into a temp `$AISH_HOME`, then runs `aish commit --apply` against a temp git repo with the mock provider.

`/opt/aish/tests/plugin_e2e.rs`:

```rust
// SPDX-License-Identifier: MIT
use assert_cmd::Command;
use std::path::Path;
use std::process::Command as Std;
use tempfile::tempdir;

fn git(dir: &Path, args: &[&str]) {
    Std::new("git").current_dir(dir).args(args).status().unwrap();
}

/// Path to a local checkout of the plugins repo used as the registry.
/// Override with AISH_PLUGINS_DIR; defaults to /opt/aish-plugins.
fn plugins_registry() -> String {
    std::env::var("AISH_PLUGINS_DIR").unwrap_or_else(|_| "/opt/aish-plugins".into())
}

#[test]
fn install_then_commit_end_to_end() {
    let registry = plugins_registry();
    if !Path::new(&registry).join("commit/Cargo.toml").exists() {
        eprintln!("skipping: plugins registry not present at {registry}");
        return;
    }

    let home = tempdir().unwrap(); // AISH_HOME
    let cfgdir = tempdir().unwrap();
    let cfg_path = cfgdir.path().join("config.yaml");
    std::fs::write(
        &cfg_path,
        "providers:\n  openai: { api_key: sk-x }\nmodels:\n  default: { provider: openai, model: gpt-5-mini }\ncommit: { style: conventional, language: en, model: default }\n",
    )
    .unwrap();

    // Install the commit plugin (build-on-install from the local registry).
    Command::cargo_bin("aish")
        .unwrap()
        .env("AISH_HOME", home.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_REGISTRY", &registry)
        .args(["plugin", "install", "commit", "--yes"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Installed `commit`"));

    // It shows up enabled.
    Command::cargo_bin("aish")
        .unwrap()
        .env("AISH_HOME", home.path())
        .env("AISH_CONFIG", &cfg_path)
        .args(["plugin", "list"])
        .assert()
        .success()
        .stdout(predicates::str::contains("commit"));

    // Run `aish commit --apply` in a temp repo via the mock provider.
    let repo = tempdir().unwrap();
    git(repo.path(), &["init", "-q"]);
    git(repo.path(), &["config", "user.email", "t@e.st"]);
    git(repo.path(), &["config", "user.name", "t"]);
    git(repo.path(), &["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.path().join("a.txt"), "hello").unwrap();
    git(repo.path(), &["add", "a.txt"]);

    Command::cargo_bin("aish")
        .unwrap()
        .current_dir(repo.path())
        .env("AISH_HOME", home.path())
        .env("AISH_CONFIG", &cfg_path)
        .env("AISH_PROVIDER", "mock")
        .env("AISH_MOCK_REPLY", "feat: add greeting file")
        .args(["commit", "--apply"])
        .assert()
        .success();

    let log = Std::new("git").current_dir(repo.path()).args(["log", "--oneline"]).output().unwrap();
    assert!(String::from_utf8_lossy(&log.stdout).contains("feat: add greeting file"));
}

#[test]
fn unknown_subcommand_without_plugin_errors() {
    let home = tempdir().unwrap();
    let cfgdir = tempdir().unwrap();
    let cfg_path = cfgdir.path().join("config.yaml");
    std::fs::write(&cfg_path, "providers: {}\nmodels: {}\ncommit: { style: conventional, language: en, model: default }\n").unwrap();

    Command::cargo_bin("aish")
        .unwrap()
        .env("AISH_HOME", home.path())
        .env("AISH_CONFIG", &cfg_path)
        .args(["commit"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("no enabled plugin provides `commit`"));
}
```

- [ ] **Step 2: Run the E2E**

Run: `cargo test --test plugin_e2e -- --test-threads=1`
Expected: PASS. The first test is skipped (prints a skip line, still passes) if `/opt/aish-plugins` is absent; on CI, ensure the plugins repo is checked out or set `AISH_PLUGINS_DIR`.

- [ ] **Step 3: Commit**

```bash
git add tests/plugin_e2e.rs
git commit -m "test: e2e install + run commit plugin"
```

---

### Task 18: Docs — README, CHANGELOG, flip the spec's deferred item

**Files (in `/opt/aish`):**
- Modify: `README.md`
- Modify: `CHANGELOG.md`
- Modify: `docs/superpowers/specs/2026-06-04-aish-mvp-design.md`

- [ ] **Step 1: README — replace built-in commit usage with plugin usage**

In `README.md`, update the commands/usage section so it reads (adjust to match the surrounding style):

```markdown
## Plugins

aish ships no tools by default. Install them from the plugin registry:

```
aish plugin install commit     # build + install the commit plugin
aish plugin list               # show installed plugins + state
aish plugin disable commit     # turn it off without uninstalling
aish plugin enable commit
aish plugin uninstall commit
```

Once installed:

```
git add .
aish commit            # suggest a message, then [Y/n]
aish commit --apply    # generate and commit without prompting
```

Plugins are trusted native executables built from source on install.
The default registry is `git@github.com:daaquan/aish-plugins.git` (override with
`AISH_REGISTRY`).
```

- [ ] **Step 2: CHANGELOG entry**

Add to `CHANGELOG.md` under a new `## [Unreleased]` (or next-version) section:

```markdown
### Added
- Subprocess plugin system: `aish plugin install/list/enable/disable/uninstall`.
  Tools are external binaries spoken to over a stdio JSON ABI; core exposes
  `model.chat` (keys stay in core) and `audit.record` services.

### Changed
- `commit` is no longer built in. Install it as a plugin:
  `aish plugin install commit`.
```

- [ ] **Step 3: Flip the MVP spec's deferred item**

In `docs/superpowers/specs/2026-06-04-aish-mvp-design.md`, in the "Deferred to v0.2+" list, mark the plugin-loader line as done:

```markdown
- ~~External plugin loader + manifest + ABI (subprocess plugins over stdio).~~
  **Done in v0.2** — see `docs/superpowers/specs/2026-06-05-plugin-system-design.md`.
```

- [ ] **Step 4: Final full verification**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: PASS.

Run: `cargo test --all -- --test-threads=1`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add README.md CHANGELOG.md docs/superpowers/specs/2026-06-04-aish-mvp-design.md
git commit -m "docs: document plugin system; mark v0.2 plugin loader done"
```

---

## Self-Review (completed by plan author)

**Spec coverage:**
- Topology / zero built-in tools → Tasks 12, 13. ✓
- Stdio ABI frames, abi major, frame cap → Tasks 2, 7. ✓
- Host services model.chat/audit.record + permission gating + sanitized config → Task 5. ✓
- /dev/tty human UI → Task 16. ✓
- Concurrency (stderr drain), timeouts, error precedence (crash before result), malformed-frame kill → Tasks 7, 8. ✓
- Manifest + plugins.toml state + conflict policy → Tasks 3, 4. ✓
- Install: registry resolution (local + git SHA), build-on-install, isolated CARGO_HOME, atomic rename + file lock, binary_sha256 + tamper check, trust confirmation → Tasks 9, 10, 11, 13. ✓
- commit plugin extraction (build_messages/postprocess moved verbatim with tests) → Tasks 15, 16. ✓
- Testing: protocol/manifest unit, install unit (local fixture, no network), host integration vs fake plugin, one full-build E2E → Tasks 2–10, 17. ✓

**Known scope notes:**
- The `MAX_FRAME_BYTES` enforcement is a post-read length check (sound for trusted plugins; a bounded streaming reader is a future hardening, consistent with the spec's "trusted native code" stance).
- `id` correlation: the plugin uses ids ≥100 for its requests and the host echoes them; invoke/result use id 1. Single in-flight request at a time (no concurrent plugin requests in v0.2), so no id collision.
- Type names verified consistent across tasks: `Frame`, `WireMessage`, `ProtoError`, `Manifest`, `Permissions`, `PluginEntry`, `InstalledRegistry`, `RegistrySource`; functions `run_plugin`, `handle`, `available_services`, `scoped_config`, `install_built`, `install_from_registry`, `ensure_registry`, `build_plugin`, `verify_sha256`, `find_by_subcommand`, `check_conflicts`.

**Placeholder scan:** none — every code step contains full code.
