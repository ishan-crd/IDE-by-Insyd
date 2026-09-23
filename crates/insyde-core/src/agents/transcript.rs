//! The chat transcript of one agent session: a flat list of items that the
//! UI renders top to bottom. Streaming chunks extend the last item in place,
//! so memory grows with content, not with the number of chunks.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolKind {
    Read,
    Edit,
    Delete,
    Move,
    Search,
    Bash,
    Think,
    Fetch,
    Other,
}

impl ToolKind {
    pub fn label(self) -> &'static str {
        match self {
            ToolKind::Read => "Read",
            ToolKind::Edit => "Edit",
            ToolKind::Delete => "Delete",
            ToolKind::Move => "Move",
            ToolKind::Search => "Search",
            ToolKind::Bash => "Bash",
            ToolKind::Think => "Think",
            ToolKind::Fetch => "Fetch",
            ToolKind::Other => "Tool",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolStatus {
    Pending,
    Running,
    Done,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolItem {
    pub id: String,
    pub kind: ToolKind,
    pub title: String,
    /// Primary argument shown in mono (path or command).
    pub arg: String,
    pub status: ToolStatus,
    /// Right-hand meta ("412 lines", "+18 −9", "7/7 passed · 41s").
    pub meta: String,
    pub added: u32,
    pub removed: u32,
    pub paths: Vec<String>,
    pub started: i64,
    pub ended: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanEntry {
    pub text: String,
    pub done: bool,
    pub active: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Item {
    User {
        text: String,
    },
    Agent {
        text: String,
    },
    Thought {
        text: String,
    },
    /// A run of consecutive tool calls, rendered as one card.
    Tools {
        calls: Vec<ToolItem>,
    },
    Plan {
        entries: Vec<PlanEntry>,
    },
    /// "Worked for 4m 12s" marker at the start of a turn's output.
    Worked {
        secs: i64,
    },
    Notice {
        text: String,
        error: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PermissionPrompt {
    pub title: String,
    pub detail: String,
    pub options: Vec<(String, String, bool)>, // (id, label, is_allow)
}

#[derive(Clone, Debug, Default)]
pub struct Usage {
    pub used: u64,
    pub size: u64,
    pub cost: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Transcript {
    pub items: Vec<Item>,
    /// Bumped on every change so views can skip unchanged frames.
    pub version: u64,
    pub running: bool,
    pub turn_started: Option<i64>,
    pub usage: Usage,
    pub modes: Vec<(String, String)>,
    pub mode: Option<String>,
    pub models: Vec<(String, String)>,
    pub model: Option<String>,
    pub model_config_id: Option<String>,
    pub commands: Vec<(String, String)>,
    pub permission: Option<PermissionPrompt>,
    pub status_line: Option<String>,
    pub ready: bool,
    pub error: Option<String>,
    /// Index of the first item not yet written to the store.
    pub persisted: usize,
}

impl Transcript {
    pub fn from_history(items: Vec<Item>) -> Self {
        let persisted = items.len();
        Self {
            items,
            persisted,
            ..Default::default()
        }
    }

    pub fn touch(&mut self) {
        self.version = self.version.wrapping_add(1);
    }

    pub fn push_text(&mut self, thought: bool, chunk: &str) {
        match (self.items.last_mut(), thought) {
            (Some(Item::Agent { text }), false) | (Some(Item::Thought { text }), true) => {
                text.push_str(chunk)
            }
            _ => self.items.push(if thought {
                Item::Thought { text: chunk.into() }
            } else {
                Item::Agent { text: chunk.into() }
            }),
        }
        self.touch();
    }

    pub fn tool_mut(&mut self, id: &str) -> Option<&mut ToolItem> {
        self.items.iter_mut().rev().find_map(|it| match it {
            Item::Tools { calls } => calls.iter_mut().find(|c| c.id == id),
            _ => None,
        })
    }

    pub fn push_tool(&mut self, t: ToolItem) {
        if let Some(Item::Tools { calls }) = self.items.last_mut() {
            calls.push(t);
        } else {
            self.items.push(Item::Tools { calls: vec![t] });
        }
        self.touch();
    }

    /// Approximate token count of the visible conversation (chars / 4),
    /// used when the agent does not report usage.
    pub fn estimate_tokens(&self) -> u64 {
        let chars: usize = self
            .items
            .iter()
            .map(|i| match i {
                Item::User { text }
                | Item::Agent { text }
                | Item::Thought { text }
                | Item::Notice { text, .. } => text.len(),
                Item::Tools { calls } => calls
                    .iter()
                    .map(|c| c.title.len() + c.arg.len() + 200)
                    .sum(),
                Item::Plan { entries } => entries.iter().map(|e| e.text.len()).sum(),
                Item::Worked { .. } => 0,
            })
            .sum();
        (chars / 4) as u64
    }

    /// Changed files across all edit tool calls: (paths, +, −).
    pub fn edit_totals(&self) -> (Vec<String>, u32, u32) {
        let mut paths: Vec<String> = Vec::new();
        let (mut a, mut r) = (0, 0);
        for it in &self.items {
            if let Item::Tools { calls } = it {
                for c in calls.iter().filter(|c| {
                    matches!(c.kind, ToolKind::Edit | ToolKind::Delete | ToolKind::Move)
                }) {
                    a += c.added;
                    r += c.removed;
                    for p in &c.paths {
                        if !paths.contains(p) {
                            paths.push(p.clone());
                        }
                    }
                }
            }
        }
        (paths, a, r)
    }

    /// Plan entries not yet completed ("Open tasks" in hand-off).
    pub fn open_tasks(&self) -> Vec<String> {
        self.items
            .iter()
            .rev()
            .find_map(|i| match i {
                Item::Plan { entries } => Some(
                    entries
                        .iter()
                        .filter(|e| !e.done)
                        .map(|e| e.text.clone())
                        .collect(),
                ),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// Plain-text rendition of the conversation for hand-off, most recent
    /// content kept when over `max_chars`.
    pub fn summary_text(&self, max_chars: usize) -> String {
        let mut out = String::new();
        for it in &self.items {
            match it {
                Item::User { text } => out.push_str(&format!("\n## User\n{text}\n")),
                Item::Agent { text } => out.push_str(&format!("\n## Agent\n{text}\n")),
                Item::Tools { calls } => {
                    for c in calls {
                        out.push_str(&format!("- {} `{}` {}\n", c.kind.label(), c.arg, c.meta));
                    }
                }
                Item::Plan { entries } => {
                    for e in entries {
                        out.push_str(&format!(
                            "- [{}] {}\n",
                            if e.done { "x" } else { " " },
                            e.text
                        ));
                    }
                }
                _ => {}
            }
        }
        if out.len() > max_chars {
            let mut cut = out.len() - max_chars;
            while !out.is_char_boundary(cut) {
                cut += 1;
            }
            out = format!("[earlier conversation omitted]\n{}", &out[cut..]);
        }
        out
    }
}

/// Line-level +/− counts between two texts (for edit tool calls).
pub fn diff_counts(old: &str, new: &str) -> (u32, u32) {
    let d = similar::TextDiff::from_lines(old, new);
    let (mut a, mut r) = (0, 0);
    for ch in d.iter_all_changes() {
        match ch.tag() {
            similar::ChangeTag::Insert => a += 1,
            similar::ChangeTag::Delete => r += 1,
            similar::ChangeTag::Equal => {}
        }
    }
    (a, r)
}
