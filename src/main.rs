mod apps;
mod chrome;
mod config;
mod dialog;
mod log;
mod notify;
mod permission;
mod session;
mod tray;
mod ui;
mod watcher;

use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use chrome::{CHROME_ID, ChromeError, Verdict};
use config::Config;
use log::Event as LogEvent;
use session::{Session, SystemClock, Tick, Timer, format_remaining, minutes_left};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
use tray::{Tray, View, ids};
use tray_icon::menu::{Menu, MenuEvent, PredefinedMenuItem, Submenu};
use ui::{Page, Push, UiRequest};
use watcher::{ALWAYS_ALLOWED, ActiveApp};

#[derive(Debug)]
pub enum AppEvent {
    Menu(MenuEvent),
    AppActivated(ActiveApp),
    ChromeUrl(String),
    ChromeError(ChromeError),
    Ui(String),
    ChromePermission(permission::Status),
}

enum AppClass {
    Allowed,
    Ignored,
    Blocked,
}

const NOTICE_COOLDOWN: Duration = Duration::from_secs(10);
const URL_LOG_COOLDOWN: Duration = Duration::from_secs(5);
const BOUNCE_GRACE: Duration = Duration::from_millis(600);

struct App {
    cfg: Config,
    timer: Timer<SystemClock>,
    minute: Duration,
    tray: Option<Tray>,

    frontmost: Option<ActiveApp>,
    last_allowed: Option<String>,
    pending_bounce: Option<(String, Instant)>,
    last_notice: Option<Instant>,

    chrome: Sender<chrome::Cmd>,
    chrome_polling: bool,
    blocked_page: PathBuf,
    last_blocked_url: Option<(String, Instant)>,
    chrome_error_notified: bool,

    ui: ui::Ui,
    pending_page: Option<Page>,
    proxy: EventLoopProxy<AppEvent>,

    _observer: Option<watcher::ObserverToken>,
    _main_menu: Option<Menu>,
}

impl App {
    fn on_menu(&mut self, event: &MenuEvent) -> bool {
        let id = &event.id;
        if id == ids::START_25 {
            self.start_session(25);
        } else if id == ids::START_50 {
            self.start_session(50);
        } else if id == ids::STOP {
            self.end_session("stopped");
        } else if id == ids::PASS {
            self.request_pass();
        } else if id == ids::STATS {
            self.pending_page = Some(Page::Stats);
        } else if id == ids::SETTINGS {
            self.pending_page = Some(Page::Settings);
        } else if id == ids::WELCOME {
            self.pending_page = Some(Page::Welcome);
        } else if id == ids::QUIT {
            return self.confirm_quit();
        }
        false
    }

    fn start_session(&mut self, minutes: u32) {
        let (cfg, err) = config::load();
        if let Some(err) = err {
            log::error(&err);
            notify::notify("FocusLock: config problem", &err);
        }
        let _ = self.chrome.send(chrome::Cmd::SetInterval(Duration::from_millis(cfg.chrome_poll_ms)));
        self.cfg = cfg;

        self.timer.start(minutes);
        log::record(LogEvent::SessionStart { minutes });
        notify::notify("FocusLock", &format!("{minutes}-minute focus session started."));

        self.recheck_frontmost();
    }

    fn end_session(&mut self, how: &str) {
        if let Some(s) = self.timer.stop() {
            self.log_session_end(&s, how);
        }
    }

    fn log_session_end(&self, s: &Session, how: &str) {
        log::record(LogEvent::SessionEnd {
            planned_minutes: s.minutes,
            focused_minutes: self.timer.elapsed_minutes(s),
            how: how.into(),
        });
    }

    fn ask_reason(&self, title: &str, message: &str) -> Option<String> {
        let reason = dialog::ask_text(title, message, "e.g. need the AWS console to check a log")?;
        let reason = reason.trim().to_owned();
        let min = self.cfg.min_reason_chars;
        if reason.chars().count() < min {
            notify::notify("FocusLock", &format!("Reason too short (at least {min} characters). Denied."));
            return None;
        }
        Some(reason)
    }

    fn request_pass(&mut self) {
        let (max, used) = match self.timer.session() {
            Some(s) => (self.cfg.max_passes, s.passes_used),
            None => return,
        };
        if used >= max {
            notify::notify("FocusLock", "No passes left this session.");
            return;
        }
        let msg = format!(
            "A {}-minute pass unblocks everything. You have {} of {max} left this session.",
            self.cfg.pass_minutes,
            max - used
        );
        let Some(reason) = self.ask_reason("Why do you need to leave?", &msg) else { return };

        if self.timer.grant_pass(self.cfg.pass_minutes, max).is_ok() {
            log::record(LogEvent::PassGranted { reason });
            notify::notify("FocusLock", &format!("Pass granted: {} min. Be quick!", self.cfg.pass_minutes));
        }
    }

    fn confirm_quit(&mut self) -> bool {
        let Some(left) = self.timer.remaining() else { return true };
        let msg = format!(
            "A focus session is running ({} min left). Why do you need to quit?\n\n\
             (\"Stop session\" ends the session without quitting.)",
            minutes_left(left, self.minute)
        );
        let Some(reason) = self.ask_reason("Quit FocusLock?", &msg) else { return false };
        log::record(LogEvent::QuitWithReason { reason });
        self.end_session("quit");
        true
    }

    fn on_ui(&mut self, text: &str) {
        let request: UiRequest = match serde_json::from_str(text) {
            Ok(r) => r,
            Err(e) => {
                log::error(&format!("ui: bad message {text:?}: {e}"));
                return;
            }
        };
        match request {
            UiRequest::Ready => {
                self.push_state();
                self.push_config(&self.saved_config());
                self.push_stats();
                self.check_chrome_permission(false);
            }
            UiRequest::GetStats => self.push_stats(),
            UiRequest::ListApps => self.ui.send(&Push::Apps { apps: &apps::scan() }),
            UiRequest::SaveConfig { mut config } => self.save_config_from_ui(&mut config),
            UiRequest::StartSession { minutes } => {
                if !self.timer.is_running() && (1..=240).contains(&minutes) {
                    self.start_session(minutes);
                }
            }
            UiRequest::ChromePermission { ask } => self.check_chrome_permission(ask),
            UiRequest::OpenAutomationSettings => open(&[
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation",
            ]),
            UiRequest::OpenConfigFile => {
                config::ensure_exists();
                open(&["-t", &config::config_path().to_string_lossy()]);
            }
            UiRequest::RevealData => open(&[&config::data_dir().to_string_lossy()]),
            UiRequest::Close => self.ui.close(),
        }
    }

    fn saved_config(&self) -> Config {
        config::load().0
    }

    fn save_config_from_ui(&mut self, config: &mut Config) {
        config.normalize();
        let result = config::save(config);
        let running = self.timer.is_running();
        if result.is_ok() {
            ui::set_dock_icon(config.show_dock_icon);
            self.cfg.show_dock_icon = config.show_dock_icon;
            if !running {
                self.cfg = config.clone();
                let _ = self.chrome.send(chrome::Cmd::SetInterval(Duration::from_millis(config.chrome_poll_ms)));
            }
        }
        self.ui.send(&Push::Saved {
            ok: result.is_ok(),
            error: result.err().map(|e| e.to_string()),
            applies_next_session: running,
        });
        self.push_config(config);
    }

    fn push_state(&self) {
        let session = self.timer.session().map(|s| ui::SessionInfo {
            minutes: s.minutes,
            left_secs: self.timer.remaining().unwrap_or_default().as_secs_f64(),
            pass_left_secs: self.timer.pass_left().map(|d| d.as_secs_f64()),
            passes_left: self.cfg.max_passes.saturating_sub(s.passes_used),
        });
        self.ui.send(&Push::State {
            session,
            version: env!("CARGO_PKG_VERSION"),
            fast: self.minute < Duration::from_secs(60),
        });
    }

    fn push_config(&self, config: &Config) {
        self.ui.send(&Push::Config {
            config,
            always_allowed: ALWAYS_ALLOWED,
            config_path: config::config_path().to_string_lossy().into_owned(),
        });
    }

    fn push_stats(&self) {
        if !self.ui.is_open() {
            return;
        }
        let names = apps::scan().into_iter().map(|a| (a.bundle_id, a.name)).collect();
        self.ui.send(&Push::Stats { report: log::report(), names });
    }

    fn check_chrome_permission(&self, ask: bool) {
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let status = permission::automation(CHROME_ID, ask);
            let _ = proxy.send_event(AppEvent::ChromePermission(status));
        });
    }

    fn classify(&self, app: &ActiveApp) -> AppClass {
        if app.is_self() {
            return AppClass::Ignored;
        }
        match app.bundle_id.as_deref() {
            None => AppClass::Ignored,
            Some(id) if ALWAYS_ALLOWED.contains(&id) => AppClass::Ignored,
            Some(id) if self.cfg.allowed_apps.iter().any(|a| a == id) => AppClass::Allowed,
            Some(_) => AppClass::Blocked,
        }
    }

    fn on_app_activated(&mut self, app: ActiveApp) {
        if self.pending_bounce.as_ref().map(|(id, _)| Some(id)) == Some(app.bundle_id.as_ref()) {
            self.pending_bounce = None;
        }
        match self.classify(&app) {
            AppClass::Allowed => self.last_allowed = app.bundle_id.clone(),
            AppClass::Ignored => {}
            AppClass::Blocked if self.timer.is_blocking() => self.block_app(&app),
            AppClass::Blocked => {}
        }
        self.frontmost = Some(app);
    }

    fn block_app(&mut self, app: &ActiveApp) {
        let bundle_id = app.bundle_id.clone().unwrap_or_default();
        watcher::hide(app.pid);
        if let Some(target) = self.last_allowed.clone() {
            watcher::activate(&target);
            self.pending_bounce = Some((target, Instant::now() + BOUNCE_GRACE));
        }
        log::record(LogEvent::BlockedApp { bundle_id });
        self.notice(&format!("{} is blocked", display_name(app)));
    }

    fn recheck_frontmost(&mut self) {
        if let Some(app) = watcher::frontmost() {
            self.on_app_activated(app);
        }
    }

    fn check_pending_bounce(&mut self) {
        let Some((target, deadline)) = &self.pending_bounce else { return };
        if Instant::now() < *deadline {
            return;
        }
        let target = target.clone();
        self.pending_bounce = None;
        let still_blocked = watcher::frontmost().is_some_and(|a| matches!(self.classify(&a), AppClass::Blocked));
        if still_blocked && self.timer.is_blocking() && watcher::is_running(&target) {
            watcher::open_by_bundle_id(&target);
        }
    }

    fn notice(&mut self, title: &str) {
        let now = Instant::now();
        if self.last_notice.is_some_and(|t| now - t < NOTICE_COOLDOWN) {
            return;
        }
        self.last_notice = Some(now);
        if let Some(left) = self.timer.remaining() {
            notify::notify(title, &format!("Focus session: {} min left", minutes_left(left, self.minute)));
        }
    }

    fn update_chrome_polling(&mut self) {
        let chrome_front = self.frontmost.as_ref().and_then(|a| a.bundle_id.as_deref()) == Some(CHROME_ID);
        let want = self.timer.is_blocking() && chrome_front;
        if want != self.chrome_polling {
            self.chrome_polling = want;
            let _ = self.chrome.send(chrome::Cmd::SetActive(want));
        }
    }

    fn on_chrome_url(&mut self, url: String) {
        if !self.chrome_polling {
            return;
        }
        let Verdict::Blocked { domain } = chrome::check_url(&url, &self.cfg.allowed_domains) else { return };
        let Some(s) = self.timer.session() else { return };

        let session_label = format!("{}-minute focus session", s.minutes);
        let to = chrome::blocked_page_url(&self.blocked_page, &domain, &session_label, s.ends_at);
        let _ = self.chrome.send(chrome::Cmd::Redirect { from: url, to });

        let now = Instant::now();
        let repeat = self
            .last_blocked_url
            .as_ref()
            .is_some_and(|(d, t)| *d == domain && now - *t < URL_LOG_COOLDOWN);
        if !repeat {
            log::record(LogEvent::BlockedUrl { domain: domain.clone() });
            self.notice(&format!("{domain} is blocked"));
        }
        self.last_blocked_url = Some((domain, now));
    }

    fn on_chrome_error(&mut self, err: ChromeError) {
        let msg = match &err {
            ChromeError::NotAuthorized => "FocusLock isn't allowed to control Chrome. Enable it in \
                System Settings → Privacy & Security → Automation."
                .to_string(),
            ChromeError::Other(e) => format!("Chrome check failed: {e}"),
        };
        log::error(&msg);
        if !self.chrome_error_notified {
            self.chrome_error_notified = true;
            notify::notify("FocusLock: can't check Chrome", &msg);
        }
    }

    fn refresh(&mut self) -> Option<Duration> {
        let tick = self.timer.tick();
        self.check_pending_bounce();

        let (view, wake) = match tick {
            Tick::Finished(s) => {
                self.log_session_end(&s, "completed");
                self.push_stats();
                notify::notify(
                    "FocusLock",
                    &format!("Session complete: {} minutes of focus. Nice work!", s.minutes),
                );
                (self.idle_view(), None)
            }
            Tick::Idle => (self.idle_view(), None),
            Tick::Running(status) => {
                if status.pass_just_ended {
                    notify::notify("FocusLock", "Pass over. Back to focus.");
                    self.recheck_frontmost();
                }
                let lock = format!("🔒 {}", format_remaining(status.left));
                let title = match status.pass_left {
                    Some(p) => format!("🎟 {} · {lock}", format_remaining(p)),
                    None => lock,
                };
                let used = self.timer.session().map_or(0, |s| s.passes_used);
                let left = self.cfg.max_passes.saturating_sub(used);
                let view = View {
                    title,
                    running: true,
                    pass_label: format!("I need something else… ({left} left)"),
                    pass_enabled: left > 0 && status.pass_left.is_none(),
                };
                let next = |d: Duration| Duration::from_nanos(u64::from(d.subsec_nanos()));
                let mut wake = next(status.left);
                if let Some(p) = status.pass_left {
                    wake = wake.min(next(p));
                }
                (view, Some(wake + Duration::from_millis(5)))
            }
        };

        self.update_chrome_polling();
        if let Some(tray) = self.tray.as_mut() {
            tray.show(view);
        }
        if self.ui.is_open() {
            self.push_state();
        }
        wake
    }

    fn idle_view(&self) -> View {
        View {
            title: tray::IDLE_TITLE.into(),
            running: false,
            pass_label: "I need something else…".into(),
            pass_enabled: false,
        }
    }
}

fn open(args: &[&str]) {
    let _ = Command::new("/usr/bin/open").args(args).spawn();
}

fn build_main_menu() -> Option<Menu> {
    let app = Submenu::with_items("FocusLock", true, &[&PredefinedMenuItem::hide(None)]).ok()?;
    let edit = Submenu::with_items(
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(None),
            &PredefinedMenuItem::redo(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::cut(None),
            &PredefinedMenuItem::copy(None),
            &PredefinedMenuItem::paste(None),
            &PredefinedMenuItem::select_all(None),
        ],
    )
    .ok()?;
    let window = Submenu::with_items(
        "Window",
        true,
        &[&PredefinedMenuItem::minimize(None), &PredefinedMenuItem::close_window(None)],
    )
    .ok()?;
    let menu = Menu::new();
    menu.append_items(&[&app, &edit, &window]).ok()?;
    menu.init_for_nsapp();
    Some(menu)
}

fn first_launch() -> bool {
    let marker = config::data_dir().join(".welcome-shown");
    if marker.exists() {
        return false;
    }
    let _ = std::fs::write(&marker, "");
    true
}

fn display_name(app: &ActiveApp) -> String {
    if app.name.is_empty() { app.bundle_id.clone().unwrap_or_default() } else { app.name.clone() }
}

fn main() {
    let mut event_loop = EventLoopBuilder::<AppEvent>::with_user_event().build();

    event_loop.set_activation_policy(ActivationPolicy::Accessory);

    let minute = if std::env::var_os("FOCUSLOCK_FAST").is_some() {
        Duration::from_secs(5)
    } else {
        Duration::from_secs(60)
    };

    let (cfg, cfg_err) = config::load();
    let blocked_page = chrome::write_blocked_page(&config::data_dir());

    let proxy = event_loop.create_proxy();
    let menu_proxy = proxy.clone();
    MenuEvent::set_event_handler(Some(move |e| {
        let _ = menu_proxy.send_event(AppEvent::Menu(e));
    }));
    let chrome = chrome::spawn_poller(proxy.clone(), Duration::from_millis(cfg.chrome_poll_ms));

    let mut app = App {
        cfg,
        timer: Timer::new(SystemClock, minute),
        minute,
        tray: None,
        frontmost: watcher::frontmost(),
        last_allowed: None,
        pending_bounce: None,
        last_notice: None,
        chrome,
        chrome_polling: false,
        blocked_page,
        last_blocked_url: None,
        chrome_error_notified: false,
        ui: ui::Ui::new(proxy.clone()),
        pending_page: None,
        proxy: proxy.clone(),
        _observer: None,
        _main_menu: None,
    };
    if let Some(front) = app.frontmost.clone() {
        app.on_app_activated(front);
    }

    event_loop.run(move |event, target, control_flow| {
        match event {
            Event::NewEvents(StartCause::Init) => {
                app.tray = Some(Tray::new());
                app._main_menu = build_main_menu();
                if app.cfg.show_dock_icon {
                    ui::set_dock_icon(true);
                }
                if first_launch() {
                    app.pending_page = Some(Page::Welcome);
                }
                let proxy = proxy.clone();
                app._observer = Some(watcher::observe_activations(move |a| {
                    let _ = proxy.send_event(AppEvent::AppActivated(a));
                }));
                if let Some(err) = &cfg_err {
                    log::error(err);
                    notify::notify("FocusLock: config problem", err);
                }
            }
            Event::UserEvent(AppEvent::Menu(e)) => {
                if app.on_menu(&e) {
                    *control_flow = ControlFlow::Exit;
                    return;
                }
            }
            Event::UserEvent(AppEvent::AppActivated(a)) => app.on_app_activated(a),
            Event::UserEvent(AppEvent::ChromeUrl(url)) => app.on_chrome_url(url),
            Event::UserEvent(AppEvent::ChromeError(e)) => app.on_chrome_error(e),
            Event::UserEvent(AppEvent::Ui(text)) => app.on_ui(&text),
            Event::UserEvent(AppEvent::ChromePermission(status)) => {
                app.ui.send(&Push::ChromePermission { status });
            }
            Event::WindowEvent { window_id, event: WindowEvent::CloseRequested, .. }
                if Some(window_id) == app.ui.window_id() =>
            {
                app.ui.close();
            }
            Event::Reopen { .. } => app.pending_page = Some(Page::Stats),
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {}
            _ => return,
        }

        if let Some(page) = app.pending_page.take() {
            app.ui.show(target, page);
        }

        *control_flow = match app.refresh() {
            Some(sleep) => ControlFlow::WaitUntil(Instant::now() + sleep),
            None => ControlFlow::Wait,
        };
    });
}
