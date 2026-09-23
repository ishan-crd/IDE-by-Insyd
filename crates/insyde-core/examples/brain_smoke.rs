//! `cargo run -p insyde-core --example brain_smoke -- <repo> [base] [task]`
use insyde_core::brain::{Brain, Kind};
use std::sync::Arc;
fn main() {
    let mut a = std::env::args().skip(1);
    let repo = std::path::PathBuf::from(a.next().expect("repo"));
    let base = a
        .next()
        .unwrap_or_else(|| insyde_core::git::default_branch(&repo));
    let task = a.next().unwrap_or_else(|| "fix the terminal layout".into());
    let b = Brain::open(&repo, &base).unwrap();
    let t = std::time::Instant::now();
    let g = b.build(Arc::new(|p, l| eprintln!("{p:>3}% {l}"))).unwrap();
    eprintln!(
        "built in {:?}: {} nodes, {} edges, {} groups {:?}",
        t.elapsed(),
        g.nodes.len(),
        g.edges.len(),
        g.groups.len(),
        g.groups
    );
    for k in Kind::ALL {
        eprintln!(
            "  {:<10} {}",
            k.label(),
            g.nodes.iter().filter(|n| n.kind == k).count()
        );
    }
    for n in g
        .nodes
        .iter()
        .filter(|n| {
            matches!(
                n.kind,
                Kind::Decision | Kind::Convention | Kind::Api | Kind::Pr
            )
        })
        .take(20)
    {
        eprintln!("  [{}] {}", n.kind.chip(), n.name);
    }
    let d = b.digest(&g, "repo", &task, 18_000);
    println!("{}", &d[..d.len().min(3000)]);
    eprintln!("digest: {} chars (~{} tokens)", d.len(), d.len() / 4);
}
