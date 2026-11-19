use std::ffi::c_void;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Granted,
    Denied,
    NotDetermined,
    NotRunning,
    Error,
}

#[repr(C)]
struct AEDesc {
    descriptor_type: u32,
    data_handle: *mut c_void,
}

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn AECreateDesc(type_code: u32, data: *const c_void, size: isize, result: *mut AEDesc) -> i16;
    fn AEDisposeDesc(desc: *mut AEDesc) -> i16;
    fn AEDeterminePermissionToAutomateTarget(
        target: *const AEDesc,
        event_class: u32,
        event_id: u32,
        ask_user_if_needed: u8,
    ) -> i32;
}

const fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}
const TYPE_APPLICATION_BUNDLE_ID: u32 = fourcc(b"bund");
const TYPE_WILDCARD: u32 = fourcc(b"****");

pub fn automation(bundle_id: &str, ask: bool) -> Status {
    let mut desc = AEDesc { descriptor_type: 0, data_handle: std::ptr::null_mut() };
    let created = unsafe {
        AECreateDesc(
            TYPE_APPLICATION_BUNDLE_ID,
            bundle_id.as_ptr().cast(),
            bundle_id.len() as isize,
            &mut desc,
        )
    };
    if created != 0 {
        return Status::Error;
    }
    let status = unsafe {
        AEDeterminePermissionToAutomateTarget(&desc, TYPE_WILDCARD, TYPE_WILDCARD, u8::from(ask))
    };
    unsafe { AEDisposeDesc(&mut desc) };

    match status {
        0 => Status::Granted,
        -1743 => Status::Denied,
        -1744 => Status::NotDetermined,
        -600 => Status::NotRunning,
        _ => Status::Error,
    }
}
