//! The web client, compiled into the binary: no files to install or find at
//! runtime, and `insy serve` works from any directory.

use std::borrow::Cow;

macro_rules! files {
    ($($url:literal => $file:literal),* $(,)?) => {
        &[$( ($url, $file, include_bytes!(concat!("../../../../", $file)) as &[u8]) ),*]
    };
}

static FILES: &[(&str, &str, &[u8])] = files![
    "/" => "web/index.html",
    "/app.css" => "web/app.css",
    "/app.js" => "web/app.js",
    "/md.js" => "web/md.js",
    "/rpc.js" => "web/rpc.js",
    "/term.js" => "web/term.js",
    "/ui.js" => "web/ui.js",
    "/manifest.json" => "web/manifest.json",
    "/icon.svg" => "web/icon.svg",
    "/fonts/InstrumentSerif-Italic.ttf" => "assets/fonts/InstrumentSerif-Italic.ttf",
    "/vendor/xterm.mjs" => "web/vendor/xterm.mjs",
    "/vendor/xterm.css" => "web/vendor/xterm.css",
    "/vendor/addon-fit.mjs" => "web/vendor/addon-fit.mjs",
    "/vendor/addon-web-links.mjs" => "web/vendor/addon-web-links.mjs",
    "/icons/brain.svg" => "assets/icons/brain.svg",
    "/icons/refresh.svg" => "assets/icons/refresh.svg",
    "/icons/play.svg" => "assets/icons/play.svg",
    "/icons/layout.svg" => "assets/icons/layout.svg",
    "/icons/moon.svg" => "assets/icons/moon.svg",
    "/icons/sun.svg" => "assets/icons/sun.svg",
    "/icons/pr.svg" => "assets/icons/pr.svg",
    "/icons/branch.svg" => "assets/icons/branch.svg",
    "/icons/plus.svg" => "assets/icons/plus.svg",
    "/icons/minus.svg" => "assets/icons/minus.svg",
    "/icons/chevron-down.svg" => "assets/icons/chevron-down.svg",
    "/icons/chevron-right.svg" => "assets/icons/chevron-right.svg",
    "/icons/chevron-left.svg" => "assets/icons/chevron-left.svg",
    "/icons/history.svg" => "assets/icons/history.svg",
    "/icons/settings.svg" => "assets/icons/settings.svg",
    "/icons/search.svg" => "assets/icons/search.svg",
    "/icons/send.svg" => "assets/icons/send.svg",
    "/icons/stop.svg" => "assets/icons/stop.svg",
    "/icons/check.svg" => "assets/icons/check.svg",
    "/icons/running.svg" => "assets/icons/running.svg",
    "/icons/cross.svg" => "assets/icons/cross.svg",
    "/icons/close.svg" => "assets/icons/close.svg",
    "/icons/folder.svg" => "assets/icons/folder.svg",
    "/icons/file.svg" => "assets/icons/file.svg",
    "/icons/trash.svg" => "assets/icons/trash.svg",
    "/icons/popout.svg" => "assets/icons/popout.svg",
    "/icons/agents/claude.svg" => "assets/icons/agents/claude.svg",
    "/icons/agents/codex.svg" => "assets/icons/agents/codex.svg",
    "/icons/agents/opencode.svg" => "assets/icons/agents/opencode.svg",
    "/icons/agents/pi.svg" => "assets/icons/agents/pi.svg",
    "/icons/agents/cursor.svg" => "assets/icons/agents/cursor.svg",
    "/icons/agents/grok.svg" => "assets/icons/agents/grok.svg",
    "/icons/agents/antigravity.svg" => "assets/icons/agents/antigravity.svg",
];

/// Content type and bytes for a request path.
///
/// Developers can set `INSYDE_WEB_DIR` to the repo's `web/` folder to serve
/// the client from disk and reload without rebuilding.
pub fn get(path: &str) -> Option<(&'static str, Cow<'static, [u8]>)> {
    let path = if path == "/index.html" { "/" } else { path };
    let (url, file, bytes) = FILES.iter().find(|(p, _, _)| *p == path)?;
    if let (Ok(dir), Some(rel)) = (std::env::var("INSYDE_WEB_DIR"), file.strip_prefix("web/"))
        && let Ok(b) = std::fs::read(std::path::Path::new(&dir).join(rel))
    {
        return Some((content_type(url), Cow::Owned(b)));
    }
    Some((content_type(url), Cow::Borrowed(*bytes)))
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        _ if path == "/" => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("ttf") => "font/ttf",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/manifest+json",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn serves_shell_and_vendor() {
        assert!(super::get("/").unwrap().0.starts_with("text/html"));
        assert!(
            super::get("/vendor/xterm.mjs")
                .unwrap()
                .0
                .starts_with("text/javascript")
        );
        assert!(super::get("/../Cargo.toml").is_none());
    }
}
