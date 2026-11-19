use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub allowed_apps: Vec<String>,
    pub allowed_domains: Vec<String>,
    pub max_passes: u32,
    pub pass_minutes: u32,
    pub min_reason_chars: usize,
    pub chrome_poll_ms: u64,
    pub show_dock_icon: bool,
}

impl Default for Config {
    fn default() -> Self {
        let strings = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
        Self {
            allowed_apps: strings(&[
                "com.microsoft.VSCode",
                "net.sourceforge.skim-app.skim",
                "com.apple.Terminal",
                "com.google.Chrome",
            ]),
            allowed_domains: strings(&[
                "docs.rs",
                "doc.rust-lang.org",
                "github.com",
                "stackoverflow.com",
                "localhost",
            ]),
            max_passes: 3,
            pass_minutes: 2,
            min_reason_chars: 10,
            chrome_poll_ms: 1500,
            show_dock_icon: false,
        }
    }
}

impl Config {
    pub fn normalize(&mut self) {
        let mut apps: Vec<String> = Vec::new();
        for a in &self.allowed_apps {
            let a = a.trim().to_string();
            if !a.is_empty() && !apps.contains(&a) {
                apps.push(a);
            }
        }
        self.allowed_apps = apps;

        let mut domains: Vec<String> = Vec::new();
        for d in &self.allowed_domains {
            if let Some(d) = normalize_domain(d)
                && !domains.contains(&d)
            {
                domains.push(d);
            }
        }
        self.allowed_domains = domains;

        self.max_passes = self.max_passes.min(20);
        self.pass_minutes = self.pass_minutes.clamp(1, 30);
        self.min_reason_chars = self.min_reason_chars.min(200);
        self.chrome_poll_ms = self.chrome_poll_ms.clamp(500, 10_000);
    }

    pub fn to_toml(&self) -> String {
        let s = |v: &str| toml::Value::String(v.to_string()).to_string();
        let list = |v: &[String]| {
            if v.is_empty() {
                "[]".to_string()
            } else {
                let items: String = v.iter().map(|x| format!("    {},\n", s(x))).collect();
                format!("[\n{items}]")
            }
        };
        format!(
            r#"# FocusLock configuration. Edit here or in the app (🎯 → Settings…).
# Changes apply when the next session starts.
#
# Find an app's bundle ID with:
#   osascript -e 'id of app "Visual Studio Code"'

# Apps you may switch to during a session. Everything else is hidden.
# (FocusLock itself, Finder, Spotlight, System Settings and macOS security /
# permission dialogs are always allowed.)
allowed_apps = {apps}

# Websites allowed in Google Chrome during a session.
# Each entry also allows its subdomains: "github.com" allows "gist.github.com".
allowed_domains = {domains}

# Escape hatch ("I need something else…").
max_passes = {max_passes}          # passes per session
pass_minutes = {pass_minutes}        # how long a pass lasts
min_reason_chars = {min_reason_chars}   # minimum length of the reason you type

# How often to check Chrome's active tab, in milliseconds.
chrome_poll_ms = {chrome_poll_ms}

# Show FocusLock in the Dock (it always lives in the menu bar).
show_dock_icon = {show_dock_icon}
"#,
            apps = list(&self.allowed_apps),
            domains = list(&self.allowed_domains),
            max_passes = self.max_passes,
            pass_minutes = self.pass_minutes,
            min_reason_chars = self.min_reason_chars,
            chrome_poll_ms = self.chrome_poll_ms,
            show_dock_icon = self.show_dock_icon,
        )
    }
}

pub fn normalize_domain(input: &str) -> Option<String> {
    let mut d = input.trim().to_ascii_lowercase();
    if let Some(i) = d.find("://") {
        d = d[i + 3..].to_string();
    }
    if let Some(i) = d.find(['/', '?', '#', ':']) {
        d.truncate(i);
    }
    let d = d.trim_matches('.').trim_start_matches("www.").to_string();
    let valid = !d.is_empty()
        && d.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    valid.then_some(d)
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn config_path() -> PathBuf {
    home().join(".config/focuslock/config.toml")
}

pub fn data_dir() -> PathBuf {
    let dir = home().join(".local/share/focuslock");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn ensure_exists() {
    if !config_path().exists() {
        let _ = save(&Config::default());
    }
}

pub fn save(cfg: &Config) -> std::io::Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, cfg.to_toml())?;
    fs::rename(&tmp, &path)
}

pub fn load() -> (Config, Option<String>) {
    ensure_exists();
    let path = config_path();
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => return (Config::default(), Some(format!("can't read {}: {e}", path.display()))),
    };
    match toml::from_str::<Config>(&text) {
        Ok(mut c) => {
            c.normalize();
            (c, None)
        }
        Err(e) => (Config::default(), Some(format!("config.toml error (using defaults): {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_file_roundtrips() {
        let c = Config::default();
        let parsed: Config = toml::from_str(&c.to_toml()).unwrap();
        assert_eq!(parsed, c);

        let mut odd = Config { allowed_apps: vec![], ..Config::default() };
        odd.allowed_domains = vec![r#"we"ird.com"#.into()];
        let parsed: Config = toml::from_str(&odd.to_toml()).unwrap();
        assert_eq!(parsed, odd, "quotes must be escaped");
    }

    #[test]
    fn missing_keys_use_defaults() {
        let c: Config = toml::from_str("max_passes = 5").unwrap();
        assert_eq!(c.max_passes, 5);
        assert_eq!(c.pass_minutes, 2);
        assert!(!c.show_dock_icon);
    }

    #[test]
    fn domains_are_normalized() {
        assert_eq!(normalize_domain("https://www.GitHub.com/foo?x=1"), Some("github.com".into()));
        assert_eq!(normalize_domain("localhost:3000"), Some("localhost".into()));
        assert_eq!(normalize_domain(" .docs.rs. "), Some("docs.rs".into()));
        assert_eq!(normalize_domain("   "), None);
        assert_eq!(normalize_domain("bad domain"), None);

        let mut c = Config {
            allowed_domains: vec!["GitHub.com".into(), "github.com".into(), "".into()],
            allowed_apps: vec![" com.a ".into(), "com.a".into()],
            pass_minutes: 0,
            ..Config::default()
        };
        c.normalize();
        assert_eq!(c.allowed_domains, vec!["github.com"]);
        assert_eq!(c.allowed_apps, vec!["com.a"]);
        assert_eq!(c.pass_minutes, 1);
    }
}
