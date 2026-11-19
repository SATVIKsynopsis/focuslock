use std::process::Command;
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_app_kit::{
    NSApplicationActivationOptions, NSRunningApplication, NSWorkspace, NSWorkspaceApplicationKey,
    NSWorkspaceDidActivateApplicationNotification,
};
use objc2_foundation::{NSNotification, NSObjectProtocol, NSOperationQueue, NSString};

pub const OWN_BUNDLE_ID: &str = "dev.focuslock.FocusLock";

pub const ALWAYS_ALLOWED: &[&str] = &[
    OWN_BUNDLE_ID,
    "com.apple.finder",
    "com.apple.Spotlight",
    "com.apple.systempreferences",
    "com.apple.loginwindow",
    "com.apple.SecurityAgent",
    "com.apple.UserNotificationCenter",
    "com.apple.coreservices.uiagent",
    "com.apple.notificationcenterui",
    "com.apple.controlcenter",
    "com.apple.dock",
];

#[derive(Debug, Clone)]
pub struct ActiveApp {
    pub bundle_id: Option<String>,
    pub pid: i32,
    pub name: String,
}

impl ActiveApp {
    fn from_ns(app: &NSRunningApplication) -> Self {
        Self {
            bundle_id: app.bundleIdentifier().map(|s| s.to_string()),
            pid: app.processIdentifier(),
            name: app.localizedName().map(|s| s.to_string()).unwrap_or_default(),
        }
    }

    pub fn is_self(&self) -> bool {
        self.pid == std::process::id() as i32
    }
}

pub type ObserverToken = Retained<ProtocolObject<dyn NSObjectProtocol>>;

pub fn observe_activations(on_activate: impl Fn(ActiveApp) + 'static) -> ObserverToken {
    let block = RcBlock::new(move |notification: NonNull<NSNotification>| {
        let notification = unsafe { notification.as_ref() };
        let Some(info) = notification.userInfo() else { return };
        let key: &AnyObject = unsafe { NSWorkspaceApplicationKey };
        let Some(obj) = info.objectForKey(key) else { return };
        if let Ok(app) = obj.downcast::<NSRunningApplication>() {
            on_activate(ActiveApp::from_ns(&app));
        }
    });

    let workspace = NSWorkspace::sharedWorkspace();
    let center = workspace.notificationCenter();
    let queue = NSOperationQueue::mainQueue();
    unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceDidActivateApplicationNotification),
            None,
            Some(&queue),
            &block,
        )
    }
}

pub fn frontmost() -> Option<ActiveApp> {
    NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| ActiveApp::from_ns(&a))
}

pub fn is_running(bundle_id: &str) -> bool {
    NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(bundle_id))
        .iter()
        .any(|a| !a.isTerminated())
}

pub fn hide(pid: i32) -> bool {
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid).is_some_and(|a| a.hide())
}

pub fn activate(bundle_id: &str) -> bool {
    let apps =
        NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(bundle_id));
    let Some(app) = apps.iter().find(|a| !a.isTerminated()) else { return false };
    app.activateWithOptions(NSApplicationActivationOptions::empty())
}

pub fn open_by_bundle_id(bundle_id: &str) {
    let id = bundle_id.to_owned();
    std::thread::spawn(move || {
        let _ = Command::new("/usr/bin/open").args(["-b", &id]).status();
    });
}
