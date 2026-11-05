use std::path::PathBuf;
use std::sync::Mutex;

static QUEUE: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

#[cfg(any(target_os = "macos", test))]
pub fn push(path: PathBuf) {
    QUEUE.lock().unwrap_or_else(|e| e.into_inner()).push(path);
}

pub fn take() -> Option<PathBuf> {
    std::mem::take(&mut *QUEUE.lock().unwrap_or_else(|e| e.into_inner())).pop()
}

#[cfg(target_os = "macos")]
pub use mac::listen;

#[cfg(not(target_os = "macos"))]
pub fn listen() {}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
    use objc2::{ffi, sel};
    use objc2_foundation::{NSArray, NSURL};
    use std::path::PathBuf;

    type OpenUrls = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut NSArray<NSURL>);

    unsafe extern "C-unwind" fn open_urls(
        _this: *mut AnyObject,
        _cmd: Sel,
        _app: *mut AnyObject,
        urls: *mut NSArray<NSURL>,
    ) {
        let Some(urls) = (unsafe { urls.as_ref() }) else {
            return;
        };
        urls.iter()
            .filter_map(|url| url.path())
            .for_each(|path| super::push(PathBuf::from(path.to_string())));
    }

    pub fn listen() {
        let Some(class) = AnyClass::get(c"NSObject") else {
            return;
        };
        let imp: Imp = unsafe { std::mem::transmute::<OpenUrls, Imp>(open_urls) };
        unsafe {
            ffi::class_addMethod(
                class as *const AnyClass as *mut AnyClass,
                sel!(application:openURLs:),
                imp,
                c"v@:@@".as_ptr(),
            );
        }
    }
}
