use std::process::Command;
use std::thread;

pub fn notify(title: &str, message: &str) {
    let (title, message) = (title.to_owned(), message.to_owned());
    thread::spawn(move || {
        let result = Command::new("/usr/bin/osascript")
            .args([
                "-e",
                "on run argv",
                "-e",
                "display notification (item 2 of argv) with title (item 1 of argv)",
                "-e",
                "end run",
                &title,
                &message,
            ])
            .output();
        match result {
            Ok(out) if !out.status.success() => {
                eprintln!("notify: osascript failed: {}", String::from_utf8_lossy(&out.stderr));
            }
            Err(e) => eprintln!("notify: could not run osascript: {e}"),
            _ => {}
        }
    });
}
