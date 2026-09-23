//! Coding agents. Each agent can run as a chat session over the Agent Client
//! Protocol ([`acp`]) and/or as its own TUI inside a terminal tab. Launch
//! commands come from the public ACP registry (agentclientprotocol/registry).

pub mod acp;
pub mod transcript;

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AgentId {
    Super,
    Codex,
    ClaudeCode,
    OpenCode,
    Pi,
    OhMyPi,
    Grok,
    Cursor,
    Antigravity,
    Browser,
    Terminal,
}

/// Brand color slot for the monogram (resolved to a theme palette color by the UI).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    /// Outlined monogram (Super).
    Outline,
    Blue,
    Orange,
    Slate,
    Amber,
    Purple,
    Graphite,
    Stone,
    Teal,
    /// Neutral chip for tools (Browser, Terminal).
    Tool,
}

#[derive(Clone, Copy, Debug)]
pub struct Cmd {
    pub program: &'static str,
    pub args: &'static [&'static str],
}

#[derive(Clone, Copy, Debug)]
pub struct AgentSpec {
    pub id: AgentId,
    pub name: &'static str,
    pub mono: &'static str,
    pub tint: Tint,
    /// How to start an ACP (chat UI) session.
    pub acp: Option<Cmd>,
    /// How to start the agent's own terminal UI.
    pub tui: Option<Cmd>,
    /// Stable key stored with sessions.
    pub key: &'static str,
}

impl AgentSpec {
    pub fn is_tool(&self) -> bool {
        matches!(self.id, AgentId::Browser | AgentId::Terminal)
    }

    pub fn by_key(key: &str) -> Option<&'static AgentSpec> {
        AGENTS.iter().find(|a| a.key == key)
    }

    pub fn get(id: AgentId) -> &'static AgentSpec {
        AGENTS
            .iter()
            .find(|a| a.id == id)
            .expect("agent in registry")
    }

    /// Whether the executable needed for `cmd` is on PATH.
    pub fn available(cmd: &Cmd) -> bool {
        which(cmd.program).is_some()
    }
}

const fn c(program: &'static str, args: &'static [&'static str]) -> Option<Cmd> {
    Some(Cmd { program, args })
}

/// Order matches the design's agent picker (keys 1–9, then tools).
pub static AGENTS: &[AgentSpec] = &[
    AgentSpec {
        id: AgentId::Super,
        key: "super",
        name: "Super",
        mono: "S",
        tint: Tint::Outline,
        acp: c(
            "npx",
            &["-y", "@agentclientprotocol/claude-agent-acp@latest"],
        ),
        tui: c("claude", &[]),
    },
    AgentSpec {
        id: AgentId::Codex,
        key: "codex",
        name: "Codex",
        mono: "Cx",
        tint: Tint::Blue,
        acp: c("npx", &["-y", "@agentclientprotocol/codex-acp@latest"]),
        tui: c("codex", &[]),
    },
    AgentSpec {
        id: AgentId::ClaudeCode,
        key: "claude",
        name: "Claude Code",
        mono: "CC",
        tint: Tint::Orange,
        acp: c(
            "npx",
            &["-y", "@agentclientprotocol/claude-agent-acp@latest"],
        ),
        tui: c("claude", &[]),
    },
    AgentSpec {
        id: AgentId::OpenCode,
        key: "opencode",
        name: "OpenCode",
        mono: "OC",
        tint: Tint::Slate,
        acp: c("opencode", &["acp"]),
        tui: c("opencode", &[]),
    },
    AgentSpec {
        id: AgentId::Pi,
        key: "pi",
        name: "Pi",
        mono: "Pi",
        tint: Tint::Amber,
        acp: c("npx", &["-y", "pi-acp@latest"]),
        tui: c("pi", &[]),
    },
    AgentSpec {
        id: AgentId::OhMyPi,
        key: "omp",
        name: "Oh My Pi",
        mono: "π",
        tint: Tint::Purple,
        acp: None,
        tui: c("omp", &[]),
    },
    AgentSpec {
        id: AgentId::Grok,
        key: "grok",
        name: "Grok",
        mono: "G",
        tint: Tint::Graphite,
        acp: c(
            "npx",
            &["-y", "@xai-official/grok@latest", "agent", "stdio"],
        ),
        tui: c("grok", &[]),
    },
    AgentSpec {
        id: AgentId::Cursor,
        key: "cursor",
        name: "Cursor",
        mono: "Cu",
        tint: Tint::Stone,
        acp: c("cursor-agent", &["acp"]),
        tui: c("cursor-agent", &[]),
    },
    AgentSpec {
        id: AgentId::Antigravity,
        key: "antigravity",
        name: "Antigravity",
        mono: "A",
        tint: Tint::Teal,
        acp: c("agy_acp_server", &[]),
        tui: c("agy", &[]),
    },
    AgentSpec {
        id: AgentId::Browser,
        key: "browser",
        name: "Browser",
        mono: "◎",
        tint: Tint::Tool,
        acp: None,
        tui: None,
    },
    AgentSpec {
        id: AgentId::Terminal,
        key: "terminal",
        name: "Terminal",
        mono: "›_",
        tint: Tint::Tool,
        acp: None,
        tui: None,
    },
];

/// Default agent for quick-add ("+ Agent") — Claude Code, as in the design.
pub const DEFAULT_AGENT: AgentId = AgentId::ClaudeCode;

/// PATH lookup, also checking common install dirs that GUI apps on macOS
/// don't inherit (Homebrew, npm/fnm/volta shims, ~/.local/bin, cargo).
pub fn which(program: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(h) = dirs::home_dir() {
        for d in [
            ".local/bin",
            ".cargo/bin",
            ".volta/bin",
            ".bun/bin",
            ".npm-global/bin",
            ".opencode/bin",
        ] {
            dirs.push(h.join(d));
        }
    }
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    dirs.into_iter()
        .map(|d| d.join(program))
        .find(|p| p.is_file())
}

/// A PATH that includes the common install dirs above, so agents launched
/// from a Finder-started app can find `node`, `git`, `gh`… exactly like a shell.
pub fn augmented_path() -> String {
    let mut parts: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(h) = dirs::home_dir() {
        for d in [
            ".local/bin",
            ".cargo/bin",
            ".volta/bin",
            ".bun/bin",
            ".npm-global/bin",
        ] {
            parts.push(h.join(d));
        }
    }
    for d in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
        parts.push(PathBuf::from(d));
    }
    // Node version managers keep node in a versioned dir; add the one `which node` would find via login shell once.
    if let Some(node_dir) = login_shell_node_dir() {
        parts.insert(0, node_dir);
    }
    let mut seen = std::collections::HashSet::new();
    parts.retain(|p| seen.insert(p.clone()));
    std::env::join_paths(parts)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn login_shell_node_dir() -> Option<PathBuf> {
    use std::sync::OnceLock;
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let out = std::process::Command::new(shell)
            .args(["-lic", "command -v node"])
            .stdin(std::process::Stdio::null())
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout);
        let line = s.lines().rev().find(|l| l.trim_start().starts_with('/'))?;
        PathBuf::from(line.trim()).parent().map(PathBuf::from)
    })
    .clone()
}
