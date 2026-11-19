use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

pub mod ids {
    pub const START_25: &str = "start_25";
    pub const START_50: &str = "start_50";
    pub const STOP: &str = "stop";
    pub const PASS: &str = "pass";
    pub const STATS: &str = "stats";
    pub const SETTINGS: &str = "settings";
    pub const WELCOME: &str = "welcome";
    pub const QUIT: &str = "quit";
}

pub const IDLE_TITLE: &str = "🎯";

#[derive(Debug, PartialEq)]
pub struct View {
    pub title: String,
    pub running: bool,
    pub pass_label: String,
    pub pass_enabled: bool,
}

pub struct Tray {
    icon: TrayIcon,
    start_25: MenuItem,
    start_50: MenuItem,
    stop: MenuItem,
    pass: MenuItem,
    shown: Option<View>,
}

impl Tray {
    pub fn new() -> Self {
        let start_25 = MenuItem::with_id(ids::START_25, "Start 25 min", true, None);
        let start_50 = MenuItem::with_id(ids::START_50, "Start 50 min", true, None);
        let stop = MenuItem::with_id(ids::STOP, "Stop session", false, None);
        let pass = MenuItem::with_id(ids::PASS, "I need something else…", false, None);
        let stats = MenuItem::with_id(ids::STATS, "Today's stats", true, None);
        let settings = MenuItem::with_id(ids::SETTINGS, "Settings…", true, None);
        let welcome = MenuItem::with_id(ids::WELCOME, "Welcome & help", true, None);
        let quit = MenuItem::with_id(ids::QUIT, "Quit", true, None);

        let menu = Menu::new();
        menu.append_items(&[
            &start_25,
            &start_50,
            &stop,
            &PredefinedMenuItem::separator(),
            &pass,
            &PredefinedMenuItem::separator(),
            &stats,
            &settings,
            &welcome,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .expect("failed to build tray menu");

        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_title(IDLE_TITLE)
            .with_tooltip("FocusLock")
            .build()
            .expect("failed to create menu bar item");

        Self { icon, start_25, start_50, stop, pass, shown: None }
    }

    pub fn show(&mut self, view: View) {
        let old = self.shown.as_ref();
        if old.map(|o| &o.title) != Some(&view.title) {
            self.icon.set_title(Some(&view.title));
        }
        if old.map(|o| o.running) != Some(view.running) {
            self.start_25.set_enabled(!view.running);
            self.start_50.set_enabled(!view.running);
            self.stop.set_enabled(view.running);
        }
        if old.map(|o| &o.pass_label) != Some(&view.pass_label) {
            self.pass.set_text(&view.pass_label);
        }
        if old.map(|o| o.pass_enabled) != Some(view.pass_enabled) {
            self.pass.set_enabled(view.pass_enabled);
        }
        self.shown = Some(view);
    }
}
