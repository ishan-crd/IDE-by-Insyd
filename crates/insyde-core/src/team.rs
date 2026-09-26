//! Agent teams: a lead agent plus specialists, each specialist in its own
//! worktree. Coordination happens through the `insy` CLI (messages, waits,
//! shared state), so any ACP agent can take part without a new protocol.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Member {
    /// Agent key ("claude", "codex", …).
    pub agent: String,
    /// Short role name ("implementer", "tests", "review").
    #[serde(default)]
    pub role: String,
    /// Assignment for this member (specialists only).
    #[serde(default)]
    pub prompt: String,
    /// Give the specialist its own worktree (default true).
    #[serde(default = "yes")]
    pub worktree: bool,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TeamSpec {
    #[serde(default)]
    pub name: String,
    pub task: String,
    pub lead: Member,
    #[serde(default, rename = "specialist")]
    pub specialists: Vec<Member>,
}

impl TeamSpec {
    pub fn parse(toml_text: &str) -> Result<Self> {
        let spec: TeamSpec = toml::from_str(toml_text)?;
        if spec.task.trim().is_empty() {
            bail!("team spec needs a task");
        }
        if spec.specialists.is_empty() {
            bail!("team spec needs at least one [[specialist]]");
        }
        Ok(spec)
    }

    /// The team used by the UI's "Team…" form.
    pub fn default_for(task: &str, specialists: &[(&str, &str)]) -> Self {
        TeamSpec {
            name: String::new(),
            task: task.to_string(),
            lead: Member {
                agent: "claude".into(),
                role: "lead".into(),
                prompt: String::new(),
                worktree: false,
            },
            specialists: specialists
                .iter()
                .map(|(agent, role)| Member {
                    agent: agent.to_string(),
                    role: role.to_string(),
                    prompt: default_assignment(role),
                    worktree: true,
                })
                .collect(),
        }
    }

    /// Branch for a specialist's worktree.
    pub fn branch_for(&self, role: &str) -> String {
        let base = if self.name.trim().is_empty() {
            self.task.as_str()
        } else {
            self.name.as_str()
        };
        let slug = crate::git::slugify_branch(base);
        let slug = slug.split_once('/').map(|(_, s)| s).unwrap_or(&slug);
        let short: String = slug.chars().take(28).collect();
        format!(
            "team/{}-{}",
            short.trim_end_matches('-'),
            role.to_lowercase().replace(' ', "-")
        )
    }
}

pub fn default_assignment(role: &str) -> String {
    match role {
        "implementer" => "Implement the change. Keep commits small and focused.".into(),
        "tests" => "Write or update tests that prove the change works, and run them.".into(),
        "review" => {
            "Review the other members' branches for bugs and risky changes; report findings.".into()
        }
        other => format!("Handle the {other} part of the task."),
    }
}

/// A started member: its role, agent tab id, worktree and branch.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Started {
    pub role: String,
    pub agent: String,
    pub id: u64,
    pub worktree: String,
    pub branch: String,
}

pub fn lead_prompt(spec: &TeamSpec, members: &[Started]) -> String {
    let mut s = format!(
        "You are the lead of an IDE by Insyd agent team.\n\nTask: {}\n\nYour team (already assigned and working):\n",
        spec.task.trim()
    );
    for m in members {
        s.push_str(&format!(
            "- agent {} · {} · role: {} · branch `{}` in {}\n",
            m.id, m.agent, m.role, m.branch, m.worktree
        ));
    }
    s.push_str(
        "\nCoordinate with the `insy` CLI from your shell:\n\
         - `insy agent read <id>` to see a member's latest reply, `insy agent wait <id>` to wait for its turn to end\n\
         - `insy agent send <id> \"...\"` to give feedback or new instructions\n\
         - `insy coord list` / `insy coord set <key> <value>` for shared status (members set `<role>.status`)\n\
         When members are done, review their branches, merge what's good into this worktree, run the checks, and summarize the result.",
    );
    s
}

pub fn member_prompt(spec: &TeamSpec, m: &Member, lead_id: u64, branch: &str) -> String {
    format!(
        "You are the {role} on an IDE by Insyd agent team led by agent {lead_id}.\n\nTeam task: {task}\n\nYour assignment: {assign}\n\n\
         You work in your own worktree on branch `{branch}`; commit your work there. \
         When you finish, run `insy coord set {role}.status done` (or `blocked: <why>`) and reply with a short summary of what you changed. \
         Use `insy agent send {lead_id} \"...\"` if you need a decision from the lead.",
        role = m.role,
        task = spec.task.trim(),
        assign = if m.prompt.trim().is_empty() {
            default_assignment(&m.role)
        } else {
            m.prompt.trim().to_string()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_spec() {
        let s = TeamSpec::parse(
            r#"
task = "Fix the login redirect"
[lead]
agent = "claude"
[[specialist]]
agent = "codex"
role = "implementer"
[[specialist]]
agent = "claude"
role = "tests"
prompt = "Cover the redirect with an e2e test"
worktree = false
"#,
        )
        .unwrap();
        assert_eq!(s.specialists.len(), 2);
        assert!(s.specialists[0].worktree);
        assert!(!s.specialists[1].worktree);
        assert_eq!(s.branch_for("tests"), "team/fix-the-login-redirect-tests");
        let started = vec![Started {
            role: "tests".into(),
            agent: "claude".into(),
            id: 7,
            worktree: "/w".into(),
            branch: "b".into(),
        }];
        assert!(lead_prompt(&s, &started).contains("agent 7"));
        assert!(member_prompt(&s, &s.specialists[1], 3, "b").contains("led by agent 3"));
    }
}
