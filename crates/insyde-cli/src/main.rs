//! `insy` — talk to a running InsyDE over its local socket.
//!
//! Works from any shell; inside InsyDE terminals `INSYDE_SOCKET` and
//! `INSYDE_WORKTREE` are already set, so agents can use it too.

use anyhow::{Result, bail};
use insyde_core::rpc::call;
use serde_json::{Value, json};

const HELP: &str = "insy — control IDE by Insyd from the command line

USAGE
  insy status                                  projects, active worktree, open agents
  insy worktree list                           worktrees of the active project
  insy worktree create \"<task or branch>\"      new task worktree (selected when ready)
  insy agent list                              agent tabs in every open worktree
  insy agent new <agent> [--prompt \"...\"]      open an agent chat (claude, codex, opencode, pi, grok, cursor)
                  [--worktree <path>]          in a specific worktree (default: current)
  insy agent send <id> \"<message>\"             send a message to an agent tab
  insy agent wait <id> [--timeout <secs>]      block until the agent finishes its turn
  insy agent read <id>                         print the agent's last reply
  insy brain search \"<query>\"                  full-text search the Project Brain
  insy open <path | user@host:/path>           add a repository (local or over SSH) and switch to it
  insy coord get <key> | set <key> <value> | list
                                               shared state for agent teams (per project)
  insy team run <team.toml>                    start a lead + specialists team
  insy instructions                            how agents should use insy
  insy serve [--port N] [--network] [--tunnel] serve the web client from this machine (no app needed)
  insy serve --new-link                        replace the pairing link (old links stop working)

Add --json to print raw JSON.";

const INSTRUCTIONS: &str = "You are running inside IDE by Insyd. You can coordinate with other agents:
- `insy agent list` shows every agent tab (id, agent, worktree, running).
- `insy agent send <id> \"message\"` messages another agent; `insy agent wait <id>` waits for it; `insy agent read <id>` reads its last reply.
- `insy coord set <key> <value>` / `insy coord get <key>` share state (plans, hand-offs, status) with the team.
- `insy worktree create \"task\"` makes an isolated worktree; `insy agent new codex --worktree <path> --prompt \"...\"` starts a helper there.
- `insy brain search \"query\"` searches the project's persistent context.
Keep messages short and specific. Record decisions with `insy coord set`.";

fn main() {
    if let Err(e) = run() {
        eprintln!("insy: {e}");
        std::process::exit(1);
    }
}

fn flag(args: &mut Vec<String>, name: &str) -> Option<String> {
    let i = args.iter().position(|a| a == name)?;
    args.remove(i);
    (i < args.len()).then(|| args.remove(i))
}

fn run() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let raw = if let Some(i) = args.iter().position(|a| a == "--json") {
        args.remove(i);
        true
    } else {
        false
    };
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let (method, params): (&str, Value) = match a.as_slice() {
        [] | ["help"] | ["-h"] | ["--help"] => {
            println!("{HELP}");
            return Ok(());
        }
        ["instructions"] => {
            println!("{INSTRUCTIONS}");
            return Ok(());
        }
        ["serve", ..] => return serve(args[1..].to_vec()),
        ["status"] => ("status", json!({})),
        ["worktree", "list"] => ("worktree.list", json!({})),
        ["worktree", "create", title] => ("worktree.create", json!({ "title": title })),
        ["agent", "list"] => ("agent.list", json!({})),
        ["agent", "new", agent, ..] => {
            let mut rest = args[3..].to_vec();
            let prompt = flag(&mut rest, "--prompt");
            let worktree =
                flag(&mut rest, "--worktree").or_else(|| std::env::var("INSYDE_WORKTREE").ok());
            (
                "agent.new",
                json!({ "agent": agent, "prompt": prompt, "worktree": worktree }),
            )
        }
        ["agent", "send", id, text] => (
            "agent.send",
            json!({ "id": id.parse::<u64>()?, "text": text }),
        ),
        ["agent", "wait", id, ..] => {
            let mut rest = args[3..].to_vec();
            let timeout = flag(&mut rest, "--timeout")
                .and_then(|t| t.parse::<u64>().ok())
                .unwrap_or(1800);
            let id = id.parse::<u64>()?;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
            loop {
                let v = call("agent.get", json!({ "id": id }))?;
                if !v["running"].as_bool().unwrap_or(false) {
                    return print(raw, &v, |v| {
                        v["last_reply"].as_str().unwrap_or("").to_string()
                    });
                }
                if std::time::Instant::now() > deadline {
                    bail!("timed out waiting for agent {id}");
                }
                std::thread::sleep(std::time::Duration::from_millis(800));
            }
        }
        ["agent", "read", id] => ("agent.get", json!({ "id": id.parse::<u64>()? })),
        ["brain", "search", q] => ("brain.search", json!({ "query": q })),
        ["open", path] => {
            // `user@host:/path` and `ssh://host/path` open over SSH.
            if insyde_core::remote::parse_target(path).is_some()
                && !std::path::Path::new(path).exists()
            {
                ("open", json!({ "path": path }))
            } else {
                ("open", json!({ "path": std::fs::canonicalize(path)? }))
            }
        }
        ["coord", "get", key] => ("coord.get", json!({ "key": key })),
        ["coord", "set", key, value] => ("coord.set", json!({ "key": key, "value": value })),
        ["coord", "list"] => ("coord.list", json!({})),
        ["team", "run", file] => {
            let spec = std::fs::read_to_string(file)?;
            ("team.run", json!({ "spec": spec }))
        }
        _ => bail!("unknown command. Run `insy help`."),
    };
    let v = call(method, params)?;
    print(raw, &v, |v| human(method, v))
}

fn print(raw: bool, v: &Value, f: impl Fn(&Value) -> String) -> Result<()> {
    if raw {
        println!("{}", serde_json::to_string_pretty(v)?);
    } else {
        let s = f(v);
        if !s.is_empty() {
            println!("{s}");
        }
    }
    Ok(())
}

fn human(method: &str, v: &Value) -> String {
    let arr = |k: &str| v[k].as_array().cloned().unwrap_or_default();
    match method {
        "status" => {
            let mut s = format!(
                "project  {}\nworktree {}\nbranch   {}\n",
                v["project"].as_str().unwrap_or("-"),
                v["worktree"].as_str().unwrap_or("-"),
                v["branch"].as_str().unwrap_or("-")
            );
            for a in arr("agents") {
                s.push_str(&agent_line(&a));
                s.push('\n');
            }
            s.trim_end().to_string()
        }
        "worktree.list" => arr("worktrees")
            .iter()
            .map(|w| {
                format!(
                    "{}{}  {}",
                    if w["active"].as_bool() == Some(true) {
                        "* "
                    } else {
                        "  "
                    },
                    w["branch"].as_str().unwrap_or(""),
                    w["path"].as_str().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        "agent.list" => arr("agents")
            .iter()
            .map(agent_line)
            .collect::<Vec<_>>()
            .join("\n"),
        "agent.get" => v["last_reply"].as_str().unwrap_or("").to_string(),
        "brain.search" => arr("results")
            .iter()
            .map(|r| {
                format!(
                    "[{}] {}  {}",
                    r["kind"].as_str().unwrap_or(""),
                    r["name"].as_str().unwrap_or(""),
                    r["summary"]
                        .as_str()
                        .unwrap_or("")
                        .lines()
                        .next()
                        .unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        "coord.get" => v["value"].as_str().unwrap_or("").to_string(),
        "coord.list" => v["entries"]
            .as_object()
            .map(|m| {
                m.iter()
                    .map(|(k, x)| format!("{k} = {}", x.as_str().unwrap_or("")))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default(),
        _ => v
            .get("message")
            .and_then(|m| m.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| v.to_string()),
    }
}

fn agent_line(a: &Value) -> String {
    format!(
        "{:>4}  {:<12} {:<8} {}  {}",
        a["id"].as_u64().unwrap_or(0),
        a["agent"].as_str().unwrap_or(""),
        if a["running"].as_bool() == Some(true) {
            "running"
        } else {
            "idle"
        },
        a["branch"].as_str().unwrap_or(""),
        a["title"].as_str().unwrap_or("")
    )
}

/// Run the web server in the foreground (headless machines, servers, CI boxes).
fn serve(mut args: Vec<String>) -> Result<()> {
    let settings = insyde_core::settings::get();
    let port = match flag(&mut args, "--port") {
        Some(p) => p
            .parse()
            .map_err(|_| anyhow::anyhow!("--port needs a number"))?,
        None => settings.web_port,
    };
    let take = |args: &mut Vec<String>, name: &str| {
        let i = args.iter().position(|a| a == name);
        i.map(|i| args.remove(i)).is_some()
    };
    if take(&mut args, "--new-link") {
        insyde_core::web::rotate_token();
        println!("New pairing link created. Previous links no longer work.");
    }
    let network = take(&mut args, "--network") || settings.web_network;
    let tunnel = take(&mut args, "--tunnel") || settings.web_tunnel;
    if let Some(extra) = args.first() {
        bail!("unknown option {extra} (see insy help)");
    }
    let store = insyde_core::store::Store::open_default()?;
    let server = insyde_core::web::WebServer::start(store, port, network, tunnel)?;
    println!(
        "IDE by Insyd web is running on {}. Press Ctrl-C to stop.\n",
        server.hub.host
    );
    let mut printed = 0;
    loop {
        let links = server.links();
        if links.len() > printed {
            for l in &links[printed..] {
                println!("  {:<14} {}", l.label, l.url);
            }
            // The QR code opens the best link for a phone (public, else network).
            if let Some(best) = links.iter().rev().find(|l| !l.local) {
                println!("\nScan to open on your phone ({}):\n", best.label);
                println!("{}", insyde_core::web::qr_text(&best.url));
            } else if !network && !tunnel {
                println!(
                    "\n  Only this computer can connect. Use --network (LAN, Tailscale) or --tunnel (public https)."
                );
            }
            println!(
                "  The link works like a key to this machine. Share it only with your own devices.\n"
            );
            printed = links.len();
        }
        if let Some(e) = server.tunnel_error() {
            eprintln!("insy: {e}");
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        if printed > 0
            && !server.tunnel_pending()
            && tunnel
            && links.iter().any(|l| l.label == "Public link")
        {
            break;
        }
    }
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
