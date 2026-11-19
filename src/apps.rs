use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct InstalledApp {
    pub name: String,
    pub bundle_id: String,
    pub path: String,
}

fn app_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = [
        "/Applications",
        "/Applications/Utilities",
        "/System/Applications",
        "/System/Applications/Utilities",
        "/System/Volumes/Preboot/Cryptexes/App/System/Applications",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join("Applications"));
    }
    dirs
}

pub fn scan() -> Vec<InstalledApp> {
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in app_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "app")
                && let Some(app) = read_app(&path)
                && seen.insert(app.bundle_id.clone())
            {
                apps.push(app);
            }
        }
    }
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

fn read_app(path: &Path) -> Option<InstalledApp> {
    let plist = plist::Value::from_file(path.join("Contents/Info.plist")).ok()?;
    let info = plist.as_dictionary()?;
    if info.get("LSBackgroundOnly").and_then(|v| v.as_boolean()) == Some(true) {
        return None;
    }
    let bundle_id = info.get("CFBundleIdentifier")?.as_string()?.to_string();
    let name = path.file_stem()?.to_string_lossy().into_owned();
    Some(InstalledApp { name, bundle_id, path: path.to_string_lossy().into_owned() })
}

pub fn icon_png(path: &str, px: usize) -> Option<Vec<u8>> {
    use objc2::AllocAnyThread;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSBitmapImageFileType, NSBitmapImageRep, NSDeviceRGBColorSpace, NSGraphicsContext, NSWorkspace,
    };
    use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};

    MainThreadMarker::new()?;
    let image = NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(path));

    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            px as isize,
            px as isize,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
    }?;
    let ctx = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&ctx));
    image.drawInRect(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(px as f64, px as f64)));
    NSGraphicsContext::restoreGraphicsState_class();

    let data = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }?;
    Some(data.to_vec())
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_system_apps() {
        let apps = super::scan();
        assert!(apps.iter().any(|a| a.bundle_id == "com.apple.Terminal"), "Terminal should be installed");
        assert!(apps.windows(2).all(|w| w[0].name.to_lowercase() <= w[1].name.to_lowercase()));
    }
}
