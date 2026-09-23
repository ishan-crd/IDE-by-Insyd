//! Live smoke test: `cargo run -p insyde-core --example acp_smoke -- claude "prompt"`.
use insyde_core::agents::{
    AgentSpec,
    acp::{AcpSession, Block, Policy},
};
use std::sync::Arc;

fn main() {
    let mut args = std::env::args().skip(1);
    let key = args.next().unwrap_or_else(|| "claude".into());
    let prompt = args
        .next()
        .unwrap_or_else(|| "Reply with exactly: pong".into());
    let spec = AgentSpec::by_key(&key).expect("agent key");
    let cwd = std::env::current_dir().unwrap();
    let s = AcpSession::start(
        spec.acp.unwrap(),
        cwd,
        None,
        Default::default(),
        None,
        Policy::Ask,
        Arc::new(|| {}),
    );
    let t0 = std::time::Instant::now();
    while !s.transcript.lock().ready {
        if let Some(e) = s.transcript.lock().error.clone() {
            eprintln!("error: {e}");
            return;
        }
        if t0.elapsed().as_secs() > 120 {
            eprintln!("timeout starting");
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    {
        let t = s.transcript.lock();
        eprintln!(
            "ready in {:?}; models={:?} modes={:?}",
            t0.elapsed(),
            t.models,
            t.modes
        );
    }
    s.prompt(vec![Block::Text(prompt)]);
    std::thread::sleep(std::time::Duration::from_millis(300));
    while s.transcript.lock().running {
        let p = s.transcript.lock().permission.clone();
        if let Some(p) = p {
            eprintln!("permission: {p:?}");
            s.answer(None);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let t = s.transcript.lock();
    for it in &t.items {
        println!("{it:?}");
    }
    println!("usage={:?} error={:?}", t.usage, t.error);
}
