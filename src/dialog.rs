use objc2::MainThreadMarker;
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSApplication, NSTextField};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("dialogs must be shown from the main thread")
}

pub fn ask_text(title: &str, message: &str, placeholder: &str) -> Option<String> {
    let mtm = mtm();
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(message));
    alert.addButtonWithTitle(&NSString::from_str("OK"));
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));

    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(320.0, 24.0));
    let field = NSTextField::initWithFrame(mtm.alloc(), frame);
    field.setPlaceholderString(Some(&NSString::from_str(placeholder)));
    alert.setAccessoryView(Some(&field));
    alert.window().setInitialFirstResponder(Some(&field));

    let response = run(&alert, mtm);
    (response == NSAlertFirstButtonReturn).then(|| field.stringValue().to_string())
}

fn run(alert: &NSAlert, mtm: MainThreadMarker) -> isize {
    let app = NSApplication::sharedApplication(mtm);
    app.activate();
    let response = alert.runModal();
    app.hide(None);
    response
}
