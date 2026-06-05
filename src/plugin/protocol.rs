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
            Frame::Request {
                id: 2,
                op: "model.chat".into(),
                payload: serde_json::json!({}),
            },
            Frame::Response {
                id: 2,
                ok: true,
                payload: Some(serde_json::json!({"content": "x"})),
                error: None,
            },
            Frame::Result {
                id: 1,
                ok: true,
                payload: serde_json::json!({"exit": 0}),
            },
        ] {
            let line = f.to_line().unwrap();
            assert_eq!(Frame::from_line(&line).unwrap(), f);
        }
    }

    #[test]
    fn tag_field_selects_variant() {
        let line = r#"{"type":"result","id":1,"ok":true,"payload":{"exit":0}}"#;
        assert!(matches!(
            Frame::from_line(line).unwrap(),
            Frame::Result {
                id: 1,
                ok: true,
                ..
            }
        ));
    }

    #[test]
    fn unknown_fields_are_ignored() {
        // Additive-compatible: a future field must not break an older parser.
        let line = r#"{"type":"request","id":3,"op":"audit.record","payload":{},"future":42}"#;
        assert!(matches!(
            Frame::from_line(line).unwrap(),
            Frame::Request { id: 3, .. }
        ));
    }
}
