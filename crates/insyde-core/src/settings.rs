//! User settings: one typed struct, persisted as human-editable JSON at
//! `<data dir>/settings.json`. Unknown or missing keys fall back to defaults,
//! so the file survives upgrades. A process-wide copy is readable from any
//! thread (`get()`); the UI updates it with `update()`, which saves the file.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    Dark,
    Light,
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Accent {
    Blue,
    Violet,
    Green,
    Orange,
    Pink,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpenAs {
    Chat,
    Terminal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Approval {
    Ask,
    AcceptEdits,
    FullAccess,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // Look
    pub theme: ThemeChoice,
    pub accent: Accent,
    pub reduce_motion: bool,
    pub chat_text_size: f32,
    /// Translucent, blurred window chrome with solid working sheets.
    pub glass: bool,
    /// How much of the chrome color covers the blurred desktop, in percent.
    pub glass_tint: u32,

    // Agents
    pub default_agent: String,
    pub open_agents_as: OpenAs,
    pub approval: Approval,
    pub start_agents_eagerly: bool,
    pub auto_approve_insy: bool,
    pub context_warning_pct: u32,
    /// Per-agent launch command overrides, e.g. `"codex": "npx -y @agentclientprotocol/codex-acp@1.13.1"`.
    pub agent_commands: BTreeMap<String, String>,
    /// Extra environment for agents and terminals, one `KEY=value` per line.
    pub agent_env: String,

    // Project Brain
    pub brain_by_default: bool,
    pub brain_budget_tokens: u32,
    pub brain_auto_update: bool,

    // Worktrees & Git
    /// Where task worktrees go; `{repo}` is replaced by the repository name.
    /// Empty = next to the repo in `.insyde-worktrees/{repo}`.
    pub worktree_root: String,
    pub branch_prefix: String,
    /// Runs in the first terminal of every new worktree (e.g. `pnpm install`).
    pub setup_command: String,
    /// Files copied from the main checkout into new worktrees (comma-separated).
    pub copy_into_worktrees: String,
    pub refresh_secs: u32,
    pub pr_draft: bool,
    pub confirm_worktree_delete: bool,

    // Terminal
    /// Terminals opened in the bottom panel when a worktree is first shown.
    pub default_terminals: u32,
    /// Command behind the top bar's Run button. Empty detects one per worktree.
    pub run_command: String,
    /// Commands offered in the Run menu, one per line.
    pub quick_commands: String,
    pub term_font: String,
    pub term_font_size: f32,
    pub term_line_height: f32,
    pub term_scrollback: u32,
    /// Empty = the login shell from `$SHELL`.
    pub shell: String,
    pub copy_on_select: bool,
    pub option_as_meta: bool,

    // Editor
    pub editor_soft_wrap: bool,
    pub editor_line_numbers: bool,
    pub editor_tab_size: u32,
    pub editor_autosave: bool,

    // Notifications
    pub notify_turn_done: bool,
    pub notify_permission: bool,
    pub notify_only_background: bool,

    // Privacy & data
    pub store_transcripts: bool,
    pub control_socket: bool,

    // Web access
    /// Serve the InsyDE web client so a browser can drive this machine.
    pub web_access: bool,
    pub web_port: u16,
    /// Listen on every network interface (LAN, Tailscale) instead of this machine only.
    pub web_network: bool,
    /// Also publish a temporary public https link through `cloudflared`, if installed.
    pub web_tunnel: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::Dark,
            accent: Accent::Blue,
            reduce_motion: false,
            chat_text_size: 13.,
            glass: false,
            glass_tint: 85,
            default_agent: "claude".into(),
            open_agents_as: OpenAs::Chat,
            approval: Approval::AcceptEdits,
            start_agents_eagerly: false,
            auto_approve_insy: true,
            context_warning_pct: 85,
            agent_commands: BTreeMap::new(),
            agent_env: String::new(),
            brain_by_default: true,
            brain_budget_tokens: 18_000,
            brain_auto_update: false,
            worktree_root: String::new(),
            branch_prefix: "feat".into(),
            setup_command: String::new(),
            copy_into_worktrees: ".env, .env.local".into(),
            refresh_secs: 20,
            pr_draft: true,
            confirm_worktree_delete: true,
            default_terminals: 3,
            run_command: String::new(),
            quick_commands: String::new(),
            term_font: "Menlo".into(),
            term_font_size: 11.5,
            term_line_height: 1.7,
            term_scrollback: 10_000,
            shell: String::new(),
            copy_on_select: false,
            option_as_meta: true,
            editor_soft_wrap: false,
            editor_line_numbers: true,
            editor_tab_size: 4,
            editor_autosave: false,
            notify_turn_done: true,
            notify_permission: true,
            notify_only_background: true,
            store_transcripts: true,
            control_socket: true,
            web_access: false,
            web_port: 7788,
            web_network: false,
            web_tunnel: false,
        }
    }
}

impl Settings {
    /// Extra environment parsed from `agent_env`.
    pub fn env_pairs(&self) -> Vec<(String, String)> {
        self.agent_env
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|l| {
                l.split_once('=')
                    .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            })
            .filter(|(k, _)| {
                !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
            .collect()
    }

    /// Commands offered in the Run menu.
    pub fn quick_list(&self) -> Vec<String> {
        self.quick_commands
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// Files to copy into new worktrees.
    pub fn copy_list(&self) -> Vec<String> {
        self.copy_into_worktrees
            .split([',', '\n'])
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.contains(".."))
            .map(str::to_string)
            .collect()
    }

    /// A launch-command override for `agent`, split into program and args.
    pub fn agent_command(&self, agent: &str) -> Option<(String, Vec<String>)> {
        let cmd = self.agent_commands.get(agent)?.trim();
        let mut parts = split_words(cmd);
        if parts.is_empty() {
            return None;
        }
        let program = parts.remove(0);
        Some((program, parts))
    }
}

/// Shell-like word splitting with single and double quotes.
pub fn split_words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut any = false;
    for c in s.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                any = true;
            }
            (None, c) if c.is_whitespace() => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn path() -> PathBuf {
    crate::store::data_dir().join("settings.json")
}

static SETTINGS: LazyLock<RwLock<Settings>> = LazyLock::new(|| RwLock::new(load()));

fn load() -> Settings {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// A snapshot of the current settings.
pub fn get() -> Settings {
    SETTINGS.read().clone()
}

/// Re-read `settings.json` (after it was edited by hand).
pub fn reload() -> Settings {
    let s = load();
    *SETTINGS.write() = s.clone();
    s
}

/// Change settings and save them. Returns the new settings.
pub fn update(f: impl FnOnce(&mut Settings)) -> Settings {
    let mut w = SETTINGS.write();
    f(&mut w);
    let s = w.clone();
    drop(w);
    save(&s);
    s
}

fn save(s: &Settings) {
    if let Ok(json) = serde_json::to_string_pretty(s) {
        let tmp = path().with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(tmp, path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_parsing() {
        // Missing keys default; unknown keys are ignored.
        let s: Settings =
            serde_json::from_str(r#"{"theme":"light","branch_prefix":"fix","bogus":1}"#).unwrap();
        assert_eq!(s.theme, ThemeChoice::Light);
        assert_eq!(s.branch_prefix, "fix");
        assert_eq!(s.term_scrollback, 10_000);
        let s = Settings {
            agent_env: "A=1\n# c\nBAD KEY=2\nB = two words\n".into(),
            ..Default::default()
        };
        assert_eq!(
            s.env_pairs(),
            vec![("A".into(), "1".into()), ("B".into(), "two words".into())]
        );
        assert_eq!(
            split_words(r#"npx -y "@scope/pkg@1.0" --flag='a b'"#),
            vec!["npx", "-y", "@scope/pkg@1.0", "--flag=a b"]
        );
        assert_eq!(Settings::default().copy_list(), vec![".env", ".env.local"]);
    }
}
