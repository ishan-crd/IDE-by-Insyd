//! Builds a [`Graph`] from a branch without checking it out.
//!
//! Pipeline (percentages drive the top-bar progress):
//! 1. `git ls-tree` the branch and pick source/doc/config files.
//! 2. Stream contents through one `git cat-file --batch` process; extract a
//!    summary, exported symbols, imports and API routes per file.
//! 3. Mine merged PRs and decision-like commits from `git log`.
//! 4. Infer conventions from config files and history.
//! 5. Group files into feature areas and link everything.

use super::{EdgeKind, Graph, Kind, Node, Progress};
use crate::git;
use anyhow::{Context, Result};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::LazyLock;

const MAX_FILE_BYTES: u64 = 200 * 1024;
const MAX_FILES: usize = 420;
const MAX_SYMBOLS_PER_FILE: usize = 3;
const MAX_GROUPS: usize = 12;

const SKIP_DIRS: &[&str] = &["node_modules", "vendor", "dist", "build", "target", ".next", "out", "Pods", ".git", "coverage", "__pycache__", ".turbo", "third_party", "generated", "fixtures", "snapshots"];
const CODE_EXT: &[&str] = &["rs", "ts", "tsx", "js", "jsx", "mjs", "py", "go", "swift", "kt", "java", "rb", "ex", "c", "cc", "cpp", "h", "hpp", "cs", "php", "vue", "svelte", "dart", "scala", "zig", "lua"];
const CONTAINERS: &[&str] = &["src", "app", "lib", "packages", "crates", "apps", "internal", "pkg", "cmd", "source", "sources", "Sources", "modules", "services", "components"];

fn ext(p: &str) -> &str {
    p.rsplit_once('.').map(|(_, e)| e).unwrap_or("")
}

fn is_test(p: &str) -> bool {
    let l = p.to_lowercase();
    l.contains(".test.") || l.contains(".spec.") || l.contains("_test.") || l.starts_with("test") || l.contains("/tests/") || l.contains("/__tests__/") || l.contains(".e2e.")
}

/// Feature area of a path: the first segment that is not a generic container.
pub fn area_of(path: &str) -> String {
    let segs: Vec<&str> = path.split('/').collect();
    if segs.len() <= 1 {
        return "Root".into();
    }
    let dirs = &segs[..segs.len() - 1];
    for (i, s) in dirs.iter().enumerate() {
        if !CONTAINERS.contains(s) {
            // "packages/<name>" style: the name itself is the area.
            return if i > 0 && matches!(dirs[i - 1], "packages" | "crates" | "apps" | "services" | "modules") {
                s.to_string()
            } else {
                capitalize(s)
            };
        }
    }
    capitalize(dirs.last().unwrap_or(&"Root"))
}

fn capitalize(s: &str) -> String {
    let s = s.replace(['_', '-'], " ");
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

struct Parsed {
    summary: String,
    exports: Vec<String>,
    imports: Vec<String>,
    routes: Vec<String>,
    lines: usize,
}

static RE_RS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*pub(?:\([^)]*\))? (?:async )?(?:unsafe )?(fn|struct|enum|trait|type) ([A-Za-z_]\w*)").unwrap());
static RE_TS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^export (?:default )?(?:async )?(function|class|const|let|interface|type|enum) ([A-Za-z_$][\w$]*)").unwrap());
static RE_PY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^(?:async )?(def|class) ([A-Za-z_]\w*)").unwrap());
static RE_GO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^(?:func (?:\([^)]*\) )?|type )([A-Z]\w*)").unwrap());
static RE_JVM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?:public |open |internal )?(?:final |data |abstract )?(class|struct|func|fun|interface|enum|protocol|object) ([A-Za-z_]\w*)").unwrap()
});
static RE_IMPORT_JS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?:from\s+|require\(|import\()\s*['"](\.{1,2}/[^'"]+)['"]"#).unwrap());
static RE_MOD_RS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*(?:pub )?mod ([a-z_]\w*);").unwrap());
static RE_USE_RS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*(?:pub )?use crate::([a-z_]\w*)").unwrap());
static RE_PY_IMPORT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^from (\.+\w*|[\w.]+) import").unwrap());
static RE_ROUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:\.|@(?:app|router|api|bp)\.)(get|post|put|patch|delete)\(\s*['"](/[^'"\s]*)['"]|\.route\(\s*"(/[^"]*)"\s*,\s*(get|post|put|patch|delete)|HandleFunc\(\s*"(/[^"]*)"|\.(GET|POST|PUT|PATCH|DELETE)\(\s*"(/[^"]*)""#).unwrap()
});
static RE_NEXT_METHOD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^export (?:async )?(?:function|const) (GET|POST|PUT|PATCH|DELETE)\b").unwrap());
static RE_DECISION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(switch(?:ed|es)? (?:to|from)|migrat(?:e|ed|es|ion) (?:to|from|off)|replac(?:e|ed|es) .{2,40} with|adopt(?:ed|s)?|mov(?:e|ed|es) (?:to|from)|in favou?r of|instead of| over (?:direct|raw|custom)|drop(?:ped|s)? .{2,30} for|deprecat(?:e|ed|es))\b").unwrap()
});
static RE_MERGE_PR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^Merge pull request #(\d+) from \S+").unwrap());
static RE_SQUASH_PR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(.*?) \(#(\d+)\)$").unwrap());
static RE_CONVENTIONAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(feat|fix|chore|docs|refactor|test|perf|build|ci|style|revert)(\([^)]+\))?!?: ").unwrap());

fn leading_comment(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines().take(40) {
        let l = line.trim();
        if l.is_empty() {
            if out.is_empty() {
                continue;
            }
            break;
        }
        let stripped = ["//!", "///", "//", "#!", "# ", "*/", "/**", "/*", "* ", "--", "\"\"\""]
            .iter()
            .find_map(|p| l.strip_prefix(p))
            .map(str::trim);
        match stripped {
            Some(s) if !s.starts_with("eslint") && !s.starts_with("@ts-") && !s.starts_with("!/usr") && !s.starts_with("use ") => {
                if !s.is_empty() {
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    out.push_str(s);
                }
            }
            _ if l == "*" => {}
            _ => {
                if !out.is_empty() {
                    break;
                }
                if l.starts_with("import") || l.starts_with("use ") || l.starts_with("package") || l.starts_with("from ") || l.starts_with("#include") || l.starts_with('"') || l.starts_with('\'') {
                    continue;
                }
                break;
            }
        }
        if out.len() > 280 {
            break;
        }
    }
    out.chars().take(280).collect()
}

fn parse(path: &str, text: &str) -> Parsed {
    let e = ext(path);
    let re: Option<&Regex> = match e {
        "rs" => Some(&RE_RS),
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "vue" | "svelte" => Some(&RE_TS),
        "py" => Some(&RE_PY),
        "go" => Some(&RE_GO),
        "swift" | "kt" | "java" | "scala" | "dart" | "cs" => Some(&RE_JVM),
        _ => None,
    };
    let mut exports: Vec<String> = Vec::new();
    if let Some(re) = re {
        for c in re.captures_iter(text).take(40) {
            let name = c.get(c.len() - 1).map(|m| m.as_str().to_string()).unwrap_or_default();
            if !name.is_empty() && !name.starts_with('_') && !exports.contains(&name) {
                exports.push(name);
            }
        }
    }
    let mut imports = Vec::new();
    match e {
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "vue" | "svelte" => {
            imports.extend(RE_IMPORT_JS.captures_iter(text).filter_map(|c| c.get(1)).map(|m| m.as_str().to_string()))
        }
        "rs" => {
            imports.extend(RE_MOD_RS.captures_iter(text).filter_map(|c| c.get(1)).map(|m| format!("mod:{}", m.as_str())));
            imports.extend(RE_USE_RS.captures_iter(text).filter_map(|c| c.get(1)).map(|m| format!("crate:{}", m.as_str())));
        }
        "py" => imports.extend(RE_PY_IMPORT.captures_iter(text).filter_map(|c| c.get(1)).map(|m| format!("py:{}", m.as_str()))),
        _ => {}
    }
    let mut routes = Vec::new();
    for c in RE_ROUTE.captures_iter(text).take(12) {
        let (m, p) = if let (Some(m), Some(p)) = (c.get(1), c.get(2)) {
            (m.as_str(), p.as_str())
        } else if let (Some(p), Some(m)) = (c.get(3), c.get(4)) {
            (m.as_str(), p.as_str())
        } else if let Some(p) = c.get(5) {
            ("ANY", p.as_str())
        } else if let (Some(m), Some(p)) = (c.get(6), c.get(7)) {
            (m.as_str(), p.as_str())
        } else {
            continue;
        };
        routes.push(format!("{} {}", m.to_uppercase(), p));
    }
    // Next.js app-router route handlers: app/api/x/route.ts → "GET /api/x".
    if path.ends_with("/route.ts") || path.ends_with("/route.js") {
        if let Some(i) = path.find("app/") {
            let url = path[i + 3..].trim_end_matches("/route.ts").trim_end_matches("/route.js");
            let url = url.split('/').filter(|s| !(s.starts_with('(') && s.ends_with(')'))).collect::<Vec<_>>().join("/");
            for c in RE_NEXT_METHOD.captures_iter(text) {
                routes.push(format!("{} {}", &c[1], if url.is_empty() { "/".into() } else { url.clone() }));
            }
        }
    }
    let lines = text.lines().count();
    let head = leading_comment(text);
    let mut summary = String::new();
    if !head.is_empty() {
        summary.push_str(&head);
        summary.push('\n');
    }
    if !exports.is_empty() {
        summary.push_str(&format!("Exports: {}.\n", exports.iter().take(10).cloned().collect::<Vec<_>>().join(", ")));
    }
    summary.push_str(&format!("{lines} lines."));
    Parsed { summary, exports, imports, routes, lines }
}

fn md_summary(text: &str) -> (Option<String>, String) {
    let mut title = None;
    let mut para = String::new();
    for l in text.lines() {
        let t = l.trim();
        if title.is_none() && t.starts_with('#') {
            title = Some(t.trim_start_matches('#').trim().to_string());
            continue;
        }
        if t.is_empty() {
            if !para.is_empty() {
                break;
            }
            continue;
        }
        if t.starts_with('#') || t.starts_with("<") || t.starts_with("[!") || t.starts_with("![") || t.starts_with("```") {
            if !para.is_empty() {
                break;
            }
            continue;
        }
        if !para.is_empty() {
            para.push(' ');
        }
        para.push_str(t);
        if para.len() > 320 {
            break;
        }
    }
    (title, para.chars().take(320).collect())
}

/// Read many blobs through one `git cat-file --batch` process.
fn read_blobs(repo: &Path, base: &str, paths: &[String]) -> Result<Vec<Option<String>>> {
    let mut child = Command::new("git")
        .args(["cat-file", "--batch"])
        .current_dir(repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("git cat-file")?;
    let mut stdin = child.stdin.take().unwrap();
    let base_owned = base.to_string();
    let reqs: Vec<String> = paths.iter().map(|p| format!("{base_owned}:{p}\n")).collect();
    let writer = std::thread::spawn(move || {
        for r in reqs {
            if stdin.write_all(r.as_bytes()).is_err() {
                break;
            }
        }
    });
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut res = Vec::with_capacity(paths.len());
    let mut header = String::new();
    for _ in paths {
        header.clear();
        if out.read_line(&mut header)? == 0 {
            break;
        }
        let parts: Vec<&str> = header.split_whitespace().collect();
        if parts.len() < 3 || parts[1] == "missing" {
            res.push(None);
            continue;
        }
        let size: usize = parts[2].parse().unwrap_or(0);
        let mut buf = vec![0u8; size + 1];
        out.read_exact(&mut buf)?;
        buf.pop();
        res.push(if buf.contains(&0) { None } else { Some(String::from_utf8_lossy(&buf).into_owned()) });
    }
    let _ = writer.join();
    let _ = child.wait();
    Ok(res)
}

struct Builder {
    g: Graph,
    keys: HashMap<String, usize>,
}

impl Builder {
    #[allow(clippy::too_many_arguments)]
    fn add(&mut self, key: String, kind: Kind, name: String, group: Option<usize>, summary: String, path: Option<String>, tokens: u32, changed: Option<i64>) -> usize {
        if let Some(&id) = self.keys.get(&key) {
            return id;
        }
        let id = self.g.nodes.len();
        self.keys.insert(key.clone(), id);
        self.g.nodes.push(Node { id, key, kind, name, group, summary, path, tokens, changed, pinned: kind.pinned_by_default(), uses: 0, note: String::new() });
        id
    }
    fn link(&mut self, a: usize, b: usize, k: EdgeKind) {
        if a != b {
            self.g.edges.push((a, b, k));
        }
    }
}

pub fn build(repo: &Path, base: &str, progress: Progress) -> Result<Graph> {
    let sha = git::head_sha(repo, base).unwrap_or_else(|| base.to_string());
    progress(2, format!("Reading {base} @ {sha}…"));
    let listing = git::run_capped(repo, &["ls-tree", "-r", "-z", "--long", base], 64 << 20)?;
    let mut all: Vec<(String, u64)> = Vec::new();
    for rec in listing.split('\0').filter(|r| !r.is_empty()) {
        // "<mode> blob <sha> <size>\t<path>"
        let Some((meta, path)) = rec.split_once('\t') else { continue };
        let size = meta.split_whitespace().nth(3).and_then(|s| s.parse().ok()).unwrap_or(0);
        if path.split('/').any(|s| SKIP_DIRS.contains(&s) || (s.starts_with('.') && s != ".github")) {
            continue;
        }
        all.push((path.to_string(), size));
    }

    // Last-changed time per path (one log pass).
    progress(6, format!("Scanning history of {} files…", all.len()));
    let mut changed: HashMap<String, i64> = HashMap::new();
    if let Ok(log) = git::run_capped(repo, &["log", base, "--format=@%ct", "--name-only", "-n", "1500"], 16 << 20) {
        let mut ts = 0i64;
        for l in log.lines() {
            if let Some(t) = l.strip_prefix('@') {
                ts = t.parse().unwrap_or(0);
            } else if !l.is_empty() {
                changed.entry(l.to_string()).or_insert(ts);
            }
        }
    }
    let churn = |p: &str| changed.get(p).copied().unwrap_or(0);

    let code: Vec<&(String, u64)> = all.iter().filter(|(p, s)| CODE_EXT.contains(&ext(p)) && *s <= MAX_FILE_BYTES && !is_test(p)).collect();
    let tests: Vec<&String> = all.iter().filter(|(p, _)| CODE_EXT.contains(&ext(p)) && is_test(p)).map(|(p, _)| p).collect();
    let docs: Vec<&(String, u64)> = all.iter().filter(|(p, s)| (ext(p) == "md" || ext(p) == "mdx") && *s <= MAX_FILE_BYTES).collect();

    // Pick the files that matter most: recently changed first, then larger.
    let mut ranked: Vec<&(String, u64)> = code.clone();
    ranked.sort_by_key(|(p, s)| std::cmp::Reverse((churn(p), *s)));
    ranked.truncate(MAX_FILES);
    ranked.sort_by(|a, b| a.0.cmp(&b.0));

    progress(10, format!("Parsing {} files…", ranked.len()));
    let paths: Vec<String> = ranked.iter().map(|(p, _)| p.clone()).collect();
    let contents = read_blobs(repo, base, &paths)?;
    let mut parsed: Vec<(String, u64, Parsed)> = Vec::with_capacity(paths.len());
    for (i, ((p, size), text)) in ranked.iter().zip(contents.iter()).enumerate() {
        if let Some(t) = text {
            parsed.push((p.clone(), *size, parse(p, t)));
        }
        if i % 40 == 0 {
            progress(10 + (i * 40 / paths.len().max(1)) as u8, format!("Parsing {} files…", paths.len()));
        }
    }

    // Feature areas → groups (top N by file count).
    let mut counts: HashMap<String, usize> = HashMap::new();
    for (p, _, _) in &parsed {
        *counts.entry(area_of(p)).or_default() += 1;
    }
    let mut areas: Vec<(String, usize)> = counts.into_iter().collect();
    areas.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut groups: Vec<String> = areas.iter().take(MAX_GROUPS - 2).map(|a| a.0.clone()).collect();
    let other = areas.len() > groups.len();
    if other {
        groups.push("Other".into());
    }
    groups.push("Conventions".into());
    let conv_group = groups.len() - 1;
    let group_of = |p: &str| -> usize {
        let a = area_of(p);
        groups.iter().position(|g| *g == a).unwrap_or_else(|| groups.iter().position(|g| g == "Other").unwrap_or(0))
    };

    let project = repo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into());
    let mut b = Builder { g: Graph { sha: sha.clone(), built_at: crate::store::now(), groups: groups.clone(), ..Default::default() }, keys: HashMap::new() };
    let root = b.add("root".into(), Kind::Root, project.clone(), None, format!("Everything the brain knows about {project} on {base}, rolled up by feature area."), None, 0, None);
    let hubs: Vec<usize> = groups
        .iter()
        .enumerate()
        .map(|(gi, name)| {
            let id = b.add(format!("module:{name}"), Kind::Module, name.clone(), Some(gi), String::new(), None, 0, None);
            b.link(root, id, EdgeKind::Hub);
            id
        })
        .collect();

    // Files and symbols.
    let mut file_ids: HashMap<String, usize> = HashMap::new();
    let mut routes: Vec<(String, usize, usize)> = Vec::new();
    for (p, size, pf) in &parsed {
        let gi = group_of(p);
        let name = p.rsplit('/').next().unwrap_or(p).to_string();
        let id = b.add(format!("file:{p}"), Kind::File, name.clone(), Some(gi), format!("{p}\n{}", pf.summary), Some(p.clone()), (*size / 4) as u32, changed.get(p).copied());
        b.link(hubs[gi], id, EdgeKind::Child);
        file_ids.insert(p.clone(), id);
        let stem = name.split('.').next().unwrap_or(&name).to_string();
        for s in pf.exports.iter().take(MAX_SYMBOLS_PER_FILE) {
            let label = format!("{stem}.{s}()");
            let sid = b.add(format!("sym:{p}:{s}"), Kind::Symbol, label, Some(gi), format!("`{s}` exported from {p}."), Some(p.clone()), 40, None);
            b.link(id, sid, EdgeKind::Sym);
        }
        for r in &pf.routes {
            routes.push((r.clone(), id, gi));
        }
        let _ = pf.lines;
    }
    progress(52, "Linking imports…".into());
    // Import edges (cross links).
    let known: HashSet<&String> = file_ids.keys().collect();
    let resolve = |from: &str, spec: &str| -> Option<String> {
        let dir = from.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        if let Some(m) = spec.strip_prefix("mod:") {
            for c in [format!("{dir}/{m}.rs"), format!("{dir}/{m}/mod.rs")] {
                if known.contains(&c) {
                    return Some(c);
                }
            }
            return None;
        }
        if let Some(m) = spec.strip_prefix("crate:") {
            let src = from.find("src/").map(|i| &from[..i + 3]).unwrap_or("src");
            for c in [format!("{src}/{m}.rs"), format!("{src}/{m}/mod.rs")] {
                if known.contains(&c) {
                    return Some(c);
                }
            }
            return None;
        }
        if let Some(m) = spec.strip_prefix("py:") {
            let rel = m.trim_start_matches('.').replace('.', "/");
            for c in [format!("{dir}/{rel}.py"), format!("{rel}.py"), format!("{dir}/{rel}/__init__.py")] {
                if known.contains(&c) {
                    return Some(c);
                }
            }
            return None;
        }
        // JS-style relative import.
        let mut parts: Vec<&str> = if dir.is_empty() { vec![] } else { dir.split('/').collect() };
        for seg in spec.split('/') {
            match seg {
                "." => {}
                ".." => {
                    parts.pop();
                }
                s => parts.push(s),
            }
        }
        let base = parts.join("/");
        for sfx in ["", ".ts", ".tsx", ".js", ".jsx", ".mjs", "/index.ts", "/index.tsx", "/index.js", ".vue", ".svelte"] {
            let c = format!("{base}{sfx}");
            if known.contains(&c) {
                return Some(c);
            }
        }
        None
    };
    let mut import_edges = Vec::new();
    for (p, _, pf) in &parsed {
        let Some(&a) = file_ids.get(p) else { continue };
        for spec in &pf.imports {
            if let Some(t) = resolve(p, spec).and_then(|t| file_ids.get(&t).copied()) {
                import_edges.push((a, t));
            }
        }
    }
    for (a, t) in import_edges {
        b.link(a, t, EdgeKind::Cross);
    }

    // API endpoints.
    let mut seen_routes = HashSet::new();
    for (r, fid, gi) in routes.into_iter().take(60) {
        if !seen_routes.insert(r.clone()) {
            continue;
        }
        let src = b.g.nodes[fid].path.clone().unwrap_or_default();
        let id = b.add(format!("api:{r}"), Kind::Api, r.clone(), Some(gi), format!("Endpoint `{r}` defined in {src}."), Some(src), 60, None);
        b.link(hubs[gi], id, EdgeKind::Child);
        b.link(id, fid, EdgeKind::Cross);
    }

    // History: merged PRs and decisions.
    progress(60, "Mining pull requests…".into());
    // One pass over history: each record is "\x1e<sha>\x1f<subject>\x1f<body>\x1f<ts>\x1f" followed by
    // the files it touched (vs. its first parent), so no per-commit subprocesses.
    let log = git::run_capped(
        repo,
        &["log", base, "--first-parent", "-m", "-n", "600", "--name-only", "--format=%x1e%H%x1f%s%x1f%b%x1f%ct%x1f"],
        24 << 20,
    )
    .unwrap_or_default();
    let mut subjects: Vec<String> = Vec::new();
    let mut prs = 0usize;
    let mut decisions = 0usize;
    let records: Vec<&str> = log.split('\x1e').filter(|r| !r.trim().is_empty()).collect();
    let n_commits = records.len().max(1);
    for (i, rec) in records.iter().enumerate() {
        let f: Vec<&str> = rec.splitn(5, '\x1f').collect();
        if f.len() < 5 {
            continue;
        }
        let (sha_c, subj, body, ts) = (f[0].trim(), f[1].trim(), f[2].trim(), f[3].trim().parse::<i64>().ok());
        let files: Vec<String> = f[4].lines().map(str::trim).filter(|l| !l.is_empty()).take(200).map(str::to_string).collect();
        subjects.push(subj.to_string());
        let group_for = |files: &[String], b: &Builder| -> usize {
            files.iter().filter_map(|p| file_ids.get(p)).map(|&id| b.g.nodes[id].group.unwrap_or(0)).next().unwrap_or(0)
        };
        let pr = RE_MERGE_PR
            .captures(subj)
            .map(|c| (c[1].to_string(), body.lines().next().unwrap_or(subj).to_string()))
            .or_else(|| RE_SQUASH_PR.captures(subj).map(|c| (c[2].to_string(), c[1].to_string())));
        if let Some((num, title)) = pr {
            if prs < 40 {
                prs += 1;
                let gi = group_for(&files, &b);
                let desc: String = body.lines().skip(if RE_MERGE_PR.is_match(subj) { 1 } else { 0 }).collect::<Vec<_>>().join(" ").chars().take(240).collect();
                let summary = format!("{title}\n{}{} files touched.", if desc.is_empty() { String::new() } else { format!("{desc}\n") }, files.len());
                let id = b.add(format!("pr:{num}"), Kind::Pr, format!("PR #{num} · {}", short(&title, 38)), Some(gi), summary, None, (body.len() / 4 + 80) as u32, ts);
                b.link(hubs[gi], id, EdgeKind::Child);
                for p in &files {
                    if let Some(&fid) = file_ids.get(p) {
                        b.link(id, fid, EdgeKind::Cross);
                    }
                }
            }
        }
        if decisions < 25 && RE_DECISION.is_match(subj) && !subj.starts_with("Merge ") && !subj.to_lowercase().starts_with("bump") {
            decisions += 1;
            let gi = group_for(&files, &b);
            let clean = RE_CONVENTIONAL.replace(subj, "").to_string();
            let clean = RE_SQUASH_PR.captures(&clean).map(|c| c[1].to_string()).unwrap_or(clean);
            let summary = format!(
                "Captured from commit {} on {base}; agents treat it as a constraint unless you override it.{}",
                &sha_c[..7.min(sha_c.len())],
                if body.is_empty() { String::new() } else { format!("\n{}", short(body, 240)) }
            );
            let id = b.add(format!("decision:{clean}"), Kind::Decision, capitalize_first(&clean), Some(gi), summary, None, 60, ts);
            b.link(hubs[gi], id, EdgeKind::Child);
            for p in files.iter().take(8) {
                if let Some(&fid) = file_ids.get(p) {
                    b.link(id, fid, EdgeKind::Cross);
                }
            }
        }
        if i % 50 == 0 {
            progress(60 + (i * 20 / n_commits) as u8, format!("Mining {prs} PRs…"));
        }
    }

    // ADR / decision docs and other docs.
    progress(82, "Linking decisions…".into());
    let doc_paths: Vec<String> = docs.iter().map(|(p, _)| p.clone()).take(80).collect();
    let doc_texts = read_blobs(repo, base, &doc_paths).unwrap_or_default();
    let mut n_docs = 0;
    for (p, text) in doc_paths.iter().zip(doc_texts.iter()) {
        let Some(text) = text else { continue };
        let (title, para) = md_summary(text);
        let name = title.clone().unwrap_or_else(|| p.rsplit('/').next().unwrap_or(p).to_string());
        let lower = p.to_lowercase();
        let is_adr = lower.contains("adr") || lower.contains("decision") || lower.contains("rfc");
        let gi = if p.contains('/') { group_of(p) } else { 0 };
        let kind = if is_adr { Kind::Decision } else { Kind::Doc };
        if kind == Kind::Doc && n_docs >= 40 {
            continue;
        }
        n_docs += 1;
        let id = b.add(format!("doc:{p}"), kind, short(&name, 60), Some(gi), format!("{p}\n{para}"), Some(p.clone()), (text.len() / 4) as u32, changed.get(p).copied());
        b.link(hubs[gi], id, EdgeKind::Child);
    }

    // Conventions inferred from config + history.
    progress(88, "Inferring conventions…".into());
    let has = |name: &str| all.iter().any(|(p, _)| p == name || p.ends_with(&format!("/{name}")));
    let root_has = |name: &str| all.iter().any(|(p, _)| p == name);
    let conv = |b: &mut Builder, name: &str, evidence: &str| {
        let id = b.add(format!("conv:{name}"), Kind::Convention, name.to_string(), Some(conv_group), format!("{name}. Evidence: {evidence}."), None, 30, None);
        b.link(hubs[conv_group], id, EdgeKind::Child);
    };
    if has("biome.json") || has("biome.jsonc") {
        conv(&mut b, "Biome for lint & format", "biome.json");
    }
    if all.iter().any(|(p, _)| p.contains(".eslintrc") || p.contains("eslint.config")) {
        conv(&mut b, "ESLint rules enforced", "eslint config");
    }
    if all.iter().any(|(p, _)| p.contains(".prettierrc") || p.contains("prettier.config")) {
        conv(&mut b, "Prettier formatting", "prettier config");
    }
    if root_has("rustfmt.toml") || root_has(".rustfmt.toml") {
        conv(&mut b, "rustfmt formatting", "rustfmt.toml");
    }
    if root_has("clippy.toml") {
        conv(&mut b, "Clippy lints", "clippy.toml");
    }
    if root_has("rust-toolchain.toml") || root_has("rust-toolchain") {
        conv(&mut b, "Pinned Rust toolchain", "rust-toolchain.toml");
    }
    if root_has("pnpm-lock.yaml") {
        conv(&mut b, "pnpm as package manager", "pnpm-lock.yaml");
    } else if root_has("bun.lockb") || root_has("bun.lock") {
        conv(&mut b, "Bun as package manager", "bun.lock");
    } else if root_has("yarn.lock") {
        conv(&mut b, "Yarn as package manager", "yarn.lock");
    }
    if root_has(".editorconfig") {
        conv(&mut b, "EditorConfig whitespace rules", ".editorconfig");
    }
    if all.iter().any(|(p, _)| p.starts_with(".github/workflows/")) {
        conv(&mut b, "CI on GitHub Actions", ".github/workflows");
    }
    if let Some(Some(ts)) = read_blobs(repo, base, &["tsconfig.json".to_string()]).ok().and_then(|v| v.into_iter().next()) {
        if ts.contains("\"strict\": true") || ts.contains("\"strict\":true") {
            conv(&mut b, "TypeScript strict mode", "tsconfig.json");
        }
    }
    if subjects.len() >= 20 {
        let n = subjects.iter().filter(|s| RE_CONVENTIONAL.is_match(s)).count();
        if n * 100 / subjects.len() >= 50 {
            conv(&mut b, "Conventional commits", &format!("{n} of last {} commits", subjects.len()));
        }
    }
    if !tests.is_empty() {
        let colocated = tests.iter().filter(|t| !t.contains("/tests/") && !t.contains("/__tests__/") && !t.starts_with("test")).count();
        if colocated * 2 > tests.len() {
            conv(&mut b, "Tests live next to source", &format!("{colocated} colocated test files"));
        } else {
            conv(&mut b, "Tests in dedicated test folders", &format!("{} test files", tests.len()));
        }
    }
    for (name, label) in [("Dockerfile", "Docker image build"), ("docker-compose.yml", "Docker Compose for local services"), (".nvmrc", "Node version pinned (.nvmrc)")] {
        if root_has(name) {
            conv(&mut b, label, name);
        }
    }

    // Hub summaries now that membership is known.
    for (gi, &h) in hubs.iter().enumerate() {
        let members: Vec<&Node> = b.g.nodes.iter().filter(|n| n.group == Some(gi) && n.kind != Kind::Module && n.kind != Kind::Symbol).collect();
        let files = members.iter().filter(|n| n.kind == Kind::File).count();
        let dec = members.iter().filter(|n| n.kind == Kind::Decision).count();
        let apis = members.iter().filter(|n| n.kind == Kind::Api).count();
        let prs_n = members.iter().filter(|n| n.kind == Kind::Pr).count();
        let key: Vec<String> = members.iter().filter(|n| n.kind == Kind::File).take(5).map(|n| n.name.clone()).collect();
        let tokens = members.iter().map(|n| n.tokens).sum::<u32>() / 20 + 40;
        let summary = format!(
            "Feature area rollup: {files} files, {dec} decisions, {apis} endpoints, {prs_n} merged PRs.{}",
            if key.is_empty() { String::new() } else { format!("\nKey files: {}.", key.join(", ")) }
        );
        b.g.nodes[h].summary = summary;
        b.g.nodes[h].tokens = tokens;
    }
    // Drop empty hubs' edges only visually: keep them, they are cheap.
    progress(94, "Embedding notes…".into());
    b.g.edges.sort_unstable_by_key(|e| (e.0, e.1));
    b.g.edges.dedup_by_key(|e| (e.0, e.1));
    Ok(b.g)
}

fn short(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n).collect::<String>().trim_end()) }
}

fn capitalize_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn areas() {
        assert_eq!(area_of("src/screens/Onboarding.tsx"), "Screens");
        assert_eq!(area_of("crates/insyde-core/src/git.rs"), "insyde-core");
        assert_eq!(area_of("README.md"), "Root");
        assert_eq!(area_of("src/main.rs"), "Src");
    }
    #[test]
    fn parses() {
        let p = parse("src/push.ts", "// Push token refresh.\nimport x from './tokens';\nexport async function refreshToken() {}\nexport const A = 1;\napp.post('/devices', h)\n");
        assert_eq!(p.exports, vec!["refreshToken", "A"]);
        assert_eq!(p.imports, vec!["./tokens"]);
        assert_eq!(p.routes, vec!["POST /devices"]);
        assert!(p.summary.starts_with("Push token refresh."));
    }
}
