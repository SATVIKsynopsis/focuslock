use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, SystemTime};

use tao::event_loop::EventLoopProxy;
use url::Url;

use crate::AppEvent;

pub const CHROME_ID: &str = "com.google.Chrome";

#[derive(Debug)]
pub enum Cmd {
    SetActive(bool),
    SetInterval(Duration),
    Redirect { from: String, to: String },
}

#[derive(Debug, Clone)]
pub enum ChromeError {
    NotAuthorized,
    Other(String),
}

pub fn spawn_poller(proxy: EventLoopProxy<AppEvent>, interval: Duration) -> Sender<Cmd> {
    let (tx, rx) = mpsc::channel::<Cmd>();
    thread::Builder::new()
        .name("chrome-poller".into())
        .spawn(move || {
            let mut active = false;
            let mut interval = interval;
            loop {
                let cmd = if active {
                    match rx.recv_timeout(interval) {
                        Ok(c) => Some(c),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                } else {
                    match rx.recv() {
                        Ok(c) => Some(c),
                        Err(_) => return,
                    }
                };

                match cmd {
                    Some(Cmd::SetActive(a)) => {
                        active = a;
                    }
                    Some(Cmd::SetInterval(d)) => {
                        interval = d;
                        continue;
                    }
                    Some(Cmd::Redirect { from, to }) => {
                        if let Err(e) = set_url_if(&from, &to) {
                            let _ = proxy.send_event(AppEvent::ChromeError(e));
                        }
                        continue;
                    }
                    None => {}
                }

                if !active {
                    continue;
                }
                match active_tab_url() {
                    Ok(Some(url)) => {
                        let _ = proxy.send_event(AppEvent::ChromeUrl(url));
                    }
                    Ok(None) => {}
                    Err(e) => {
                        let _ = proxy.send_event(AppEvent::ChromeError(e));
                    }
                }
            }
        })
        .expect("failed to spawn chrome poller thread");
    tx
}

fn active_tab_url() -> Result<Option<String>, ChromeError> {
    const SCRIPT: &str = r#"
if application id "com.google.Chrome" is not running then return "!skip"
tell application id "com.google.Chrome"
    if not frontmost then return "!skip"
    if (count of windows) is 0 then return "!skip"
    return URL of active tab of front window
end tell
"#;
    let out = run_osascript(SCRIPT, &[])?;
    Ok((out != "!skip" && !out.is_empty()).then_some(out))
}

fn set_url_if(from: &str, to: &str) -> Result<(), ChromeError> {
    const SCRIPT: &str = r#"
on run argv
    if application id "com.google.Chrome" is not running then return "!skip"
    tell application id "com.google.Chrome"
        if not frontmost then return "!skip"
        if (count of windows) is 0 then return "!skip"
        set t to active tab of front window
        if (URL of t) is (item 1 of argv) then set URL of t to (item 2 of argv)
    end tell
    return "ok"
end run
"#;
    run_osascript(SCRIPT, &[from, to]).map(|_| ())
}

fn run_osascript(script: &str, args: &[&str]) -> Result<String, ChromeError> {
    let out = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .args(args)
        .output()
        .map_err(|e| ChromeError::Other(format!("can't run osascript: {e}")))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned());
    }
    let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
    if err.contains("-1743") || err.contains("Not authorized") {
        Err(ChromeError::NotAuthorized)
    } else if err.contains("-600") || err.contains("-609") {
        Ok(String::new())
    } else {
        Err(ChromeError::Other(err))
    }
}

#[derive(Debug, PartialEq)]
pub enum Verdict {
    Allowed,
    Blocked { domain: String },
}

pub fn check_url(url: &str, allowed_domains: &[String]) -> Verdict {
    let Ok(parsed) = Url::parse(url) else { return Verdict::Allowed };
    if !matches!(parsed.scheme(), "http" | "https") {
        return Verdict::Allowed;
    }
    let Some(host) = parsed.host_str() else { return Verdict::Allowed };
    let host = host.trim_end_matches('.').to_ascii_lowercase();

    let ok = allowed_domains.iter().any(|d| {
        host == *d || (host.len() > d.len() && host.ends_with(d.as_str()) && host[..host.len() - d.len()].ends_with('.'))
    });
    if ok {
        Verdict::Allowed
    } else {
        Verdict::Blocked { domain: host.trim_start_matches("www.").to_owned() }
    }
}

const BLOCKED_HTML: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Blocked: FocusLock</title>
<style>
  :root { color-scheme: light dark; }
  body { font: 16px -apple-system, system-ui, sans-serif; display: grid; place-items: center;
         min-height: 90vh; margin: 0; text-align: center; }
  .t { font-size: 64px; font-variant-numeric: tabular-nums; margin: .2em 0; }
  .m { opacity: .7; }
  code { font-size: 15px; }
</style></head>
<body><div>
  <div style="font-size:48px">🔒</div>
  <h1>Focus session in progress</h1>
  <p><code id="site"></code> isn't on your allowed list.</p>
  <div class="t" id="left">--:--</div>
  <p class="m" id="what"></p>
  <p class="m">Really need it? Use <b>"I need something else…"</b> in the menu bar.</p>
</div>
<script>
  const q = new URLSearchParams(location.search);
  document.getElementById('site').textContent = q.get('site') || 'This site';
  document.getElementById('what').textContent = q.get('session') || '';
  const until = Number(q.get('until') || 0);
  function tick() {
    const s = Math.max(0, Math.ceil((until - Date.now()) / 1000));
    document.getElementById('left').textContent =
      s > 0 ? String(Math.floor(s / 60)).padStart(2, '0') + ':' + String(s % 60).padStart(2, '0') + ' left'
            : 'Session over. You can go back now.';
  }
  tick(); setInterval(tick, 1000);
</script></body></html>
"#;

pub fn write_blocked_page(dir: &Path) -> PathBuf {
    let path = dir.join("blocked.html");
    if let Err(e) = std::fs::write(&path, BLOCKED_HTML) {
        crate::log::error(&format!("can't write blocked page: {e}"));
    }
    path
}

pub fn blocked_page_url(page: &Path, domain: &str, session: &str, ends_at: SystemTime) -> String {
    let until_ms = ends_at.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_millis();
    let mut url = Url::from_file_path(page).expect("blocked page path must be absolute");
    url.query_pairs_mut()
        .append_pair("site", domain)
        .append_pair("session", session)
        .append_pair("until", &until_ms.to_string());
    url.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed() -> Vec<String> {
        ["docs.rs", "github.com", "localhost"].iter().map(|s| s.to_string()).collect()
    }

    fn blocked(d: &str) -> Verdict {
        Verdict::Blocked { domain: d.into() }
    }

    #[test]
    fn domains_and_subdomains() {
        let a = allowed();
        assert_eq!(check_url("https://docs.rs/tao", &a), Verdict::Allowed);
        assert_eq!(check_url("https://gist.github.com/x", &a), Verdict::Allowed);
        assert_eq!(check_url("https://GitHub.com./x", &a), Verdict::Allowed);
        assert_eq!(check_url("http://localhost:3000/", &a), Verdict::Allowed);
        assert_eq!(check_url("https://evilgithub.com/", &a), blocked("evilgithub.com"));
        assert_eq!(check_url("https://github.com.evil.io/", &a), blocked("github.com.evil.io"));
        assert_eq!(check_url("https://www.youtube.com/watch", &a), blocked("youtube.com"));
        assert_eq!(check_url("https://github.com@youtube.com/", &a), blocked("youtube.com"));
    }

    #[test]
    fn non_web_schemes_allowed() {
        let a = allowed();
        assert_eq!(check_url("chrome://newtab/", &a), Verdict::Allowed);
        assert_eq!(check_url("file:///Users/me/paper.pdf", &a), Verdict::Allowed);
        assert_eq!(check_url("not a url", &a), Verdict::Allowed);
    }

    #[test]
    fn blocked_url_is_escaped_and_allowed() {
        let u = blocked_page_url(
            Path::new("/tmp/blocked.html"),
            "youtube.com",
            "25 min & focus",
            SystemTime::UNIX_EPOCH,
        );
        assert!(u.starts_with("file:///tmp/blocked.html?site=youtube.com&session=25+min+%26+focus"), "{u}");
        assert_eq!(check_url(&u, &allowed()), Verdict::Allowed);
    }
}
