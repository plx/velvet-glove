use clap::ValueEnum;
use hookkit_core::HarnessId;

/// Built-in harness selected explicitly at the CLI boundary.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Harness {
    /// Claude Code.
    Claude,
    /// Codex CLI.
    Codex,
    /// Antigravity.
    Antigravity,
}

impl Harness {
    /// Convert the CLI choice into HookKit's stable harness identity.
    pub const fn id(self) -> HarnessId {
        match self {
            Self::Claude => HarnessId::CLAUDE_CODE,
            Self::Codex => HarnessId::CODEX,
            Self::Antigravity => HarnessId::ANTIGRAVITY,
        }
    }
}
