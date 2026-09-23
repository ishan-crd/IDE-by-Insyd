//! GitHub integration through the `gh` CLI (reuses the user's login, never
//! stores a token). Every function is blocking and tolerant: when `gh` is
//! missing or unauthenticated they return empty results instead of errors.

use serde::Deserialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

fn gh(cwd: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = match crate::remote::split(cwd) {
        // Remote repos: run `gh` on the host, in the repo.
        Some((host, dir)) => crate::remote::command(
            &host,
            &format!(
                "GH_PROMPT_DISABLED=1 NO_COLOR=1 {}",
                crate::remote::script_in(&dir, "gh", args)
            ),
            false,
        ),
        None => {
            let mut c = Command::new("gh");
            c.args(args).current_dir(cwd);
            c
        }
    };
    let mut child = cmd
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut out = Vec::new();
    child
        .stdout
        .take()?
        .take(4 << 20)
        .read_to_end(&mut out)
        .ok()?;
    child
        .wait()
        .ok()?
        .success()
        .then(|| String::from_utf8_lossy(&out).into_owned())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckState {
    Ok,
    Running,
    Failed,
    Skipped,
}

#[derive(Clone, Debug)]
pub struct Check {
    pub name: String,
    pub detail: String,
    pub state: CheckState,
    pub duration: String,
    pub url: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PullRequest {
    pub number: u32,
    pub title: String,
    pub branch: String,
    pub base: String,
    pub draft: bool,
    pub url: String,
    pub mergeable: Option<bool>,
    pub checks: Vec<Check>,
}

impl PullRequest {
    pub fn counts(&self) -> (usize, usize, usize) {
        let ok = self
            .checks
            .iter()
            .filter(|c| c.state == CheckState::Ok)
            .count();
        let run = self
            .checks
            .iter()
            .filter(|c| c.state == CheckState::Running)
            .count();
        let bad = self
            .checks
            .iter()
            .filter(|c| c.state == CheckState::Failed)
            .count();
        (ok, run, bad)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPr {
    number: u32,
    #[serde(default)]
    title: String,
    head_ref_name: String,
    #[serde(default)]
    base_ref_name: String,
    #[serde(default)]
    is_draft: bool,
    #[serde(default)]
    url: String,
    #[serde(default)]
    mergeable: Option<String>,
    #[serde(default)]
    status_check_rollup: Vec<RawCheck>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCheck {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    context: Option<String>,
    #[serde(default)]
    workflow_name: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    started_at: Option<String>,
    #[serde(default)]
    completed_at: Option<String>,
    #[serde(default)]
    details_url: Option<String>,
    #[serde(default)]
    target_url: Option<String>,
}

fn parse_ts(s: &str) -> Option<i64> {
    // "2026-09-23T10:11:12Z" → unix seconds, without pulling a date crate.
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (
        n(0..4)?,
        n(5..7)?,
        n(8..10)?,
        n(11..13)?,
        n(14..16)?,
        n(17..19)?,
    );
    let (y2, m2) = if mo <= 2 {
        (y - 1, mo + 9)
    } else {
        (y, mo - 3)
    };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146_097 + doe - 719_468) * 86_400 + h * 3600 + mi * 60 + se)
}

fn fmt_dur(secs: i64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {}s", secs / 60, secs % 60)
    }
}

impl RawCheck {
    fn into_check(self) -> Check {
        let state = match (
            self.status.as_deref(),
            self.conclusion.as_deref(),
            self.state.as_deref(),
        ) {
            (Some("COMPLETED"), Some("SUCCESS" | "NEUTRAL"), _) | (_, _, Some("SUCCESS")) => {
                CheckState::Ok
            }
            (Some("COMPLETED"), Some("SKIPPED" | "CANCELLED"), _) => CheckState::Skipped,
            (Some("COMPLETED"), Some(_), _) | (_, _, Some("FAILURE" | "ERROR")) => {
                CheckState::Failed
            }
            _ => CheckState::Running,
        };
        let now = crate::store::now();
        let start = self.started_at.as_deref().and_then(parse_ts);
        let end = self
            .completed_at
            .as_deref()
            .and_then(parse_ts)
            .filter(|&e| e > 0)
            .unwrap_or(now);
        let duration = start.map(|s| fmt_dur((end - s).max(0))).unwrap_or_default();
        let name = self.name.or(self.context).unwrap_or_else(|| "check".into());
        let detail = match (&state, self.workflow_name) {
            (_, Some(w)) if !w.is_empty() && w != name => w,
            (CheckState::Ok, _) => "passed".into(),
            (CheckState::Running, _) => "running".into(),
            (CheckState::Failed, _) => "failed".into(),
            (CheckState::Skipped, _) => "skipped".into(),
        };
        Check {
            name,
            detail,
            state,
            duration,
            url: self.details_url.or(self.target_url),
        }
    }
}

const PR_FIELDS: &str =
    "number,title,headRefName,baseRefName,isDraft,url,mergeable,statusCheckRollup";

fn to_pr(r: RawPr) -> PullRequest {
    PullRequest {
        number: r.number,
        title: r.title,
        branch: r.head_ref_name,
        base: r.base_ref_name,
        draft: r.is_draft,
        url: r.url,
        mergeable: r.mergeable.map(|m| m == "MERGEABLE"),
        checks: r
            .status_check_rollup
            .into_iter()
            .map(RawCheck::into_check)
            .collect(),
    }
}

/// Open PRs by head branch: `branch → (number, has_failing_checks)`.
pub fn open_prs(repo: &Path) -> HashMap<String, (u32, bool)> {
    let Some(out) = gh(
        repo,
        &[
            "pr",
            "list",
            "--state",
            "open",
            "--limit",
            "100",
            "--json",
            "number,headRefName,statusCheckRollup",
        ],
    ) else {
        return HashMap::new();
    };
    let prs: Vec<RawPr> = serde_json::from_str(&out).unwrap_or_default();
    prs.into_iter()
        .map(|r| {
            let failing = r
                .status_check_rollup
                .into_iter()
                .map(RawCheck::into_check)
                .any(|c| c.state == CheckState::Failed);
            (r.head_ref_name, (r.number, failing))
        })
        .collect()
}

/// The PR for the worktree's current branch, with checks.
pub fn pr_for(worktree: &Path) -> Option<PullRequest> {
    let out = gh(worktree, &["pr", "view", "--json", PR_FIELDS])?;
    serde_json::from_str::<RawPr>(&out).ok().map(to_pr)
}

/// Push the branch and open a PR (draft), filling title/body from commits.
pub fn create_pr(worktree: &Path, base: &str, draft: bool) -> anyhow::Result<String> {
    let branch =
        crate::git::current_branch(worktree).ok_or_else(|| anyhow::anyhow!("detached HEAD"))?;
    crate::git::run(worktree, &["push", "-u", "origin", &branch])?;
    let mut args = vec!["pr", "create", "--fill", "--base", base];
    if draft {
        args.push("--draft");
    }
    gh(worktree, &args)
        .map(|s| s.trim().to_string())
        .ok_or_else(|| anyhow::anyhow!("gh pr create failed (is gh installed and logged in?)"))
}

pub fn rerun_failed(worktree: &Path) -> bool {
    // Re-run the latest failed workflow run for this branch.
    let Some(branch) = crate::git::current_branch(worktree) else {
        return false;
    };
    let Some(id) = gh(
        worktree,
        &[
            "run",
            "list",
            "--branch",
            &branch,
            "--limit",
            "1",
            "--json",
            "databaseId",
            "--jq",
            ".[0].databaseId",
        ],
    ) else {
        return false;
    };
    gh(worktree, &["run", "rerun", id.trim(), "--failed"]).is_some()
}

#[cfg(test)]
mod tests {
    #[test]
    fn ts() {
        assert_eq!(super::parse_ts("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(super::parse_ts("2026-09-23T00:00:00Z"), Some(1_790_121_600));
    }
}
