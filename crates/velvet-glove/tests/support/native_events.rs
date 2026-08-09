#![allow(dead_code)]

use hookkit_core::{EnvironmentVariables, HarnessId, Utf8PathBuf};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

const MODELED_CLAUDE_ENVIRONMENT: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_PROJECT_DIR",
    "CLAUDE_ENV_FILE",
    "CLAUDE_EFFORT",
    "TRACEPARENT",
    "CLAUDE_CODE_REMOTE",
    "CLAUDE_CODE_REMOTE_SESSION_ID",
    "CLAUDE_CODE_BRIDGE_SESSION_ID",
    "CLAUDE_PLUGIN_ROOT",
    "CLAUDE_PLUGIN_DATA",
    "PLUGIN_ROOT",
    "PLUGIN_DATA",
];

/// Native post-tool surfaces supported by the unified executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProtocolSurface {
    Claude,
    Codex,
    Antigravity,
}

impl ProtocolSurface {
    pub const ALL: [Self; 3] = [Self::Claude, Self::Codex, Self::Antigravity];

    pub const fn cli_name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Antigravity => "antigravity",
        }
    }

    pub const fn harness_id(self) -> HarnessId {
        match self {
            Self::Claude => HarnessId::CLAUDE_CODE,
            Self::Codex => HarnessId::CODEX,
            Self::Antigravity => HarnessId::ANTIGRAVITY,
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "claude" | "claude-code" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            "antigravity" => Ok(Self::Antigravity),
            _ => Err(format!("unknown protocol surface {value}")),
        }
    }
}

impl fmt::Display for ProtocolSurface {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.cli_name())
    }
}

/// Builder for one valid, harness-native PostToolUse input.
#[derive(Debug, Clone)]
pub struct PostToolUseBuilder {
    surface: ProtocolSurface,
    project: PathBuf,
    relative_path: PathBuf,
    session_id: String,
    turn_id: String,
    tool_use_id: String,
    tool_name: String,
    tool_input: JsonValue,
    tool_response: JsonValue,
}

impl PostToolUseBuilder {
    pub fn new(
        surface: ProtocolSurface,
        project: impl Into<PathBuf>,
        relative_path: impl Into<PathBuf>,
    ) -> Self {
        let relative_path = relative_path.into();
        let relative_text = relative_path.to_string_lossy().into_owned();
        Self {
            surface,
            project: project.into(),
            relative_path,
            session_id: "test-session".to_owned(),
            turn_id: "test-turn".to_owned(),
            tool_use_id: "test-tool".to_owned(),
            tool_name: "Write".to_owned(),
            tool_input: serde_json::json!({
                "file_path": relative_text,
                "content": "<fixture-test-input>"
            }),
            tool_response: JsonValue::Null,
        }
    }

    pub fn identity(
        mut self,
        session_id: impl Into<String>,
        turn_id: impl Into<String>,
        tool_use_id: impl Into<String>,
    ) -> Self {
        self.session_id = session_id.into();
        self.turn_id = turn_id.into();
        self.tool_use_id = tool_use_id.into();
        self
    }

    pub fn tool(mut self, name: impl Into<String>, input: JsonValue, response: JsonValue) -> Self {
        self.tool_name = name.into();
        self.tool_input = input;
        self.tool_response = response;
        self
    }

    pub fn build(mut self) -> Result<NativePostToolInput, String> {
        let project = utf8_path(self.project, "project path")?;
        let relative_path = utf8_path(self.relative_path, "fixture path")?;
        if relative_path.is_absolute() {
            return Err(format!(
                "fixture path must be project-relative: {relative_path}"
            ));
        }
        let target = project.join(&relative_path);
        if self.tool_response.is_null() {
            self.tool_response = serde_json::json!({"filePath": target});
        }

        let payload = match self.surface {
            ProtocolSurface::Claude => {
                serde_json::to_vec(&hookkit_claude::protocol::PostToolUseInput {
                    session_id: self.session_id.clone(),
                    transcript_path: project.join(".fixture-transcript.jsonl"),
                    cwd: project.clone(),
                    hook_event_name: "PostToolUse".to_owned(),
                    tool_name: self.tool_name,
                    tool_input: self.tool_input,
                    tool_use_id: self.tool_use_id,
                    tool_response: self.tool_response,
                    agent_id: None,
                    agent_type: None,
                    duration_ms: None,
                    effort: None,
                    permission_mode: None,
                    prompt_id: None,
                    extra: BTreeMap::new(),
                })
            }
            ProtocolSurface::Codex => {
                serde_json::to_vec(&hookkit_codex::protocol::PostToolUseInput {
                    session_id: self.session_id.clone(),
                    transcript_path: Some(project.join(".fixture-transcript.jsonl")),
                    cwd: project.clone(),
                    hook_event_name: "PostToolUse".to_owned(),
                    model: "fixture-model".to_owned(),
                    turn_id: self.turn_id,
                    permission_mode: hookkit_codex::protocol::PermissionMode::Default,
                    tool_name: self.tool_name,
                    tool_use_id: self.tool_use_id,
                    tool_input: self.tool_input,
                    tool_response: self.tool_response,
                    agent_id: None,
                    agent_type: None,
                    extra: BTreeMap::new(),
                })
            }
            ProtocolSurface::Antigravity => {
                let mut args = serde_json::Map::new();
                args.insert(
                    "CommandLine".to_owned(),
                    JsonValue::String(format!(
                        "printf fixture > {}",
                        shell_quote(relative_path.as_str())
                    )),
                );
                args.insert("Cwd".to_owned(), JsonValue::String(project.to_string()));
                serde_json::to_vec(&hookkit_antigravity::PostToolUseInput {
                    conversation_id: self.session_id.clone(),
                    workspace_paths: vec![project.clone()],
                    transcript_path: project.join(".fixture-transcript.jsonl"),
                    artifact_directory_path: project.join(".fixture-artifacts"),
                    tool_call: hookkit_antigravity::ToolCall {
                        name: "run_command".to_owned(),
                        args,
                        extra: BTreeMap::new(),
                    },
                    step_idx: 1,
                    error: None,
                    extra: BTreeMap::new(),
                })
            }
        }
        .map_err(|error| format!("serialize {} PostToolUse input: {error}", self.surface))?;

        Ok(NativePostToolInput {
            surface: self.surface,
            project,
            session_id: self.session_id,
            payload,
        })
    }
}

/// Serialized native input plus the matching command environment.
#[derive(Debug, Clone)]
pub struct NativePostToolInput {
    surface: ProtocolSurface,
    project: Utf8PathBuf,
    session_id: String,
    payload: Vec<u8>,
}

impl NativePostToolInput {
    pub const fn surface(&self) -> ProtocolSurface {
        self.surface
    }

    pub fn bytes(&self) -> &[u8] {
        &self.payload
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.payload
    }

    pub fn environment_variables(&self) -> EnvironmentVariables {
        EnvironmentVariables::from_pairs(self.environment_pairs())
    }

    pub fn configure_command(&self, command: &mut Command) {
        clear_modeled_hook_environment(command);
        for (name, value) in self.environment_pairs() {
            command.env(name, value);
        }
    }

    fn environment_pairs(&self) -> Vec<(String, String)> {
        match self.surface {
            ProtocolSurface::Claude => vec![
                ("CLAUDECODE".to_owned(), "1".to_owned()),
                ("CLAUDE_CODE_CHILD_SESSION".to_owned(), "1".to_owned()),
                ("CLAUDE_CODE_SESSION_ID".to_owned(), self.session_id.clone()),
                ("CLAUDE_PROJECT_DIR".to_owned(), self.project.to_string()),
            ],
            ProtocolSurface::Codex | ProtocolSurface::Antigravity => Vec::new(),
        }
    }
}

fn utf8_path(path: PathBuf, label: &str) -> Result<Utf8PathBuf, String> {
    Utf8PathBuf::from_path_buf(path).map_err(|path| format!("{label} is not UTF-8: {path:?}"))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn clear_modeled_hook_environment(command: &mut Command) {
    for name in MODELED_CLAUDE_ENVIRONMENT {
        command.env_remove(name);
    }
    for name in std::env::vars_os().filter_map(|(name, _)| name.into_string().ok()) {
        if name.starts_with("CLAUDE_PLUGIN_OPTION_") {
            command.env_remove(name);
        }
    }
}

pub fn canonical_project(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
