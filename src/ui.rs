use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tao::dpi::LogicalSize;
use tao::event_loop::{EventLoopProxy, EventLoopWindowTarget};
use tao::window::{Theme, Window, WindowBuilder, WindowId};
use wry::http::{Request, Response, header::CONTENT_TYPE};
use wry::{WebView, WebViewBuilder};

use crate::AppEvent;
use crate::config::Config;

const INDEX_HTML: &str = include_str!("../assets/ui/index.html");
const APP_ICON_PNG: &[u8] = include_bytes!("../assets/icon-1024.png");

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Page {
    Welcome,
    Stats,
    Settings,
}

impl Page {
    fn as_str(self) -> &'static str {
        match self {
            Page::Welcome => "welcome",
            Page::Stats => "stats",
            Page::Settings => "settings",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiRequest {
    Ready,
    GetStats,
    ListApps,
    SaveConfig { config: Config },
    StartSession { minutes: u32 },
    ChromePermission { ask: bool },
    OpenAutomationSettings,
    OpenConfigFile,
    RevealData,
    Close,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Push<'a> {
    State { session: Option<SessionInfo>, version: &'static str, fast: bool },
    Config { config: &'a Config, always_allowed: &'a [&'a str], config_path: String },
    Stats { report: crate::log::Report, names: HashMap<String, String> },
    Apps { apps: &'a [crate::apps::InstalledApp] },
    Saved { ok: bool, error: Option<String>, applies_next_session: bool },
    ChromePermission { status: crate::permission::Status },
    Go { page: &'static str },
}

#[derive(Debug, Serialize)]
pub struct SessionInfo {
    pub minutes: u32,
    pub left_secs: f64,
    pub pass_left_secs: Option<f64>,
    pub passes_left: u32,
}

struct Open {
    webview: WebView,
    window: Window,
}

pub struct Ui {
    open: Option<Open>,
    proxy: EventLoopProxy<AppEvent>,
}

impl Ui {
    pub fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self { open: None, proxy }
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn window_id(&self) -> Option<WindowId> {
        self.open.as_ref().map(|o| o.window.id())
    }

    pub fn show(&mut self, target: &EventLoopWindowTarget<AppEvent>, page: Page) {
        if self.open.is_none() {
            match self.create(target, page) {
                Ok(open) => self.open = Some(open),
                Err(e) => {
                    crate::log::error(&format!("can't open window: {e}"));
                    return;
                }
            }
        } else {
            self.send(&Push::Go { page: page.as_str() });
        }
        if let Some(o) = &self.open {
            o.window.set_visible(true);
            o.window.set_focus();
        }
        activate_app();
    }

    fn create(
        &self,
        target: &EventLoopWindowTarget<AppEvent>,
        page: Page,
    ) -> Result<Open, Box<dyn std::error::Error>> {
        let window = WindowBuilder::new()
            .with_title("FocusLock")
            .with_inner_size(LogicalSize::new(920.0, 640.0))
            .with_min_inner_size(LogicalSize::new(760.0, 520.0))
            .with_theme(Some(Theme::Dark))
            .build(target)?;

        let proxy = self.proxy.clone();
        let webview = WebViewBuilder::new()
            .with_custom_protocol("focuslock".into(), |_id, request| serve(request))
            .with_url(format!("focuslock://localhost/#{}", page.as_str()))
            .with_ipc_handler(move |request: Request<String>| {
                let _ = proxy.send_event(AppEvent::Ui(request.into_body()));
            })
            .with_navigation_handler(|url| url.starts_with("focuslock://"))
            .with_background_color((10, 10, 11, 255))
            .with_devtools(cfg!(debug_assertions))
            .build(&window)?;
        Ok(Open { webview, window })
    }

    pub fn close(&mut self) {
        if self.open.take().is_some() {
            hide_app();
        }
    }

    pub fn send(&self, msg: &Push) {
        let Some(o) = &self.open else { return };
        match serde_json::to_string(msg) {
            Ok(json) => {
                let _ = o.webview.evaluate_script(&format!("window.FL && FL.receive({json})"));
            }
            Err(e) => crate::log::error(&format!("ui: can't encode message: {e}")),
        }
    }
}

fn serve(request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let uri = request.uri();
    let respond = |mime: &str, body: Cow<'static, [u8]>| {
        Response::builder()
            .header(CONTENT_TYPE, mime)
            .header(
                "Content-Security-Policy",
                "default-src 'none'; img-src focuslock:; style-src 'unsafe-inline'; script-src 'unsafe-inline'",
            )
            .body(body)
            .unwrap()
    };
    match uri.path() {
        "/" | "/index.html" => respond("text/html; charset=utf-8", Cow::Borrowed(INDEX_HTML.as_bytes())),
        "/app-icon.png" => respond("image/png", Cow::Borrowed(APP_ICON_PNG)),
        "/icon" => {
            let path = uri
                .query()
                .and_then(|q| url::form_urlencoded::parse(q.as_bytes()).find(|(k, _)| k == "path"))
                .map(|(_, v)| v.into_owned())
                .unwrap_or_default();
            match cached_icon(&path) {
                Some(png) => respond("image/png", Cow::Owned(png)),
                None => not_found(),
            }
        }
        _ => not_found(),
    }
}

fn not_found() -> Response<Cow<'static, [u8]>> {
    Response::builder().status(404).body(Cow::Borrowed(&b""[..])).unwrap()
}

fn cached_icon(path: &str) -> Option<Vec<u8>> {
    if !path.ends_with(".app") {
        return None;
    }
    static CACHE: OnceLock<Mutex<HashMap<String, Vec<u8>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(png) = cache.lock().ok()?.get(path) {
        return Some(png.clone());
    }
    let png = crate::apps::icon_png(path, 64)?;
    cache.lock().ok()?.insert(path.to_string(), png.clone());
    Some(png)
}

fn activate_app() {
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        objc2_app_kit::NSApplication::sharedApplication(mtm).activate();
    }
}

fn hide_app() {
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        objc2_app_kit::NSApplication::sharedApplication(mtm).hide(None);
    }
}

pub fn set_dock_icon(visible: bool) {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        let policy = if visible {
            NSApplicationActivationPolicy::Regular
        } else {
            NSApplicationActivationPolicy::Accessory
        };
        NSApplication::sharedApplication(mtm).setActivationPolicy(policy);
    }
}
