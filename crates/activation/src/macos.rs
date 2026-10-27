//! Reconcile rule: CoreText registrations carry no owner tag, so we cannot tell ours from another
//! app's. Reconcile therefore only unregisters persistent-scope registrations whose file no
//! longer exists (dead for everyone) and re-registers ledger entries that are missing.
//! Registrations of existing files that left the ledger are removed by `deactivate` only.

use std::collections::HashSet;
use std::ffi::c_void;
use std::path::{Path, PathBuf};

use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::error::{CFError, CFErrorRef};
use core_foundation::string::CFStringRef;
use core_foundation::url::{CFURLRef, CFURL};

use crate::{ActivationError, LedgerEntry, ReconcileReport};

/// kCTFontManagerScopePersistent (same value as kCTFontManagerScopeUser): per user, survives reboots.
const SCOPE_PERSISTENT: u32 = 2;

// CTFontManagerError codes.
const FILE_NOT_FOUND: isize = 101;
const UNRECOGNIZED_FORMAT: isize = 103;
const INVALID_FONT_DATA: isize = 104;
const ALREADY_REGISTERED: isize = 105;
const NOT_REGISTERED: isize = 201;
const IN_USE: isize = 202;

#[link(name = "CoreText", kind = "framework")]
extern "C" {
    static kCTFontURLAttribute: CFStringRef;
    fn CTFontManagerRegisterFontsForURL(url: CFURLRef, scope: u32, error: *mut CFErrorRef) -> bool;
    fn CTFontManagerUnregisterFontsForURL(url: CFURLRef, scope: u32, error: *mut CFErrorRef) -> bool;
    fn CTFontManagerCopyRegisteredFontDescriptors(scope: u32, enabled: bool) -> CFArrayRef;
    fn CTFontDescriptorCopyAttribute(descriptor: *const c_void, attribute: CFStringRef) -> CFTypeRef;
}

type ManagerFn = unsafe extern "C" fn(CFURLRef, u32, *mut CFErrorRef) -> bool;

/// Runs a CTFontManager URL call; `Err` carries the CTFontManagerError code.
fn call(f: ManagerFn, path: &Path) -> Result<(), isize> {
    let url = CFURL::from_path(path, false).ok_or(FILE_NOT_FOUND)?;
    let mut error: CFErrorRef = std::ptr::null_mut();
    if unsafe { f(url.as_concrete_TypeRef(), SCOPE_PERSISTENT, &mut error) } {
        return Ok(());
    }
    if error.is_null() {
        return Err(0);
    }
    Err(unsafe { CFError::wrap_under_create_rule(error) }.code())
}

fn absolute(path: &Path) -> Result<PathBuf, ActivationError> {
    std::path::absolute(path).map_err(|e| ActivationError::Os(e.to_string()))
}

pub fn activate(entry: &LedgerEntry) -> Result<(), ActivationError> {
    let path = absolute(&entry.path)?;
    match call(CTFontManagerRegisterFontsForURL, &path) {
        Ok(()) | Err(ALREADY_REGISTERED) => Ok(()),
        Err(FILE_NOT_FOUND) => Err(ActivationError::FileMissing(path)),
        Err(UNRECOGNIZED_FORMAT | INVALID_FONT_DATA) => Err(ActivationError::InvalidFont(path)),
        Err(IN_USE) => Err(ActivationError::InUse(path)),
        Err(code) => Err(ActivationError::Os(format!("CoreText error {code}"))),
    }
}

pub fn deactivate(entry: &LedgerEntry) -> Result<(), ActivationError> {
    let path = absolute(&entry.path)?;
    match call(CTFontManagerUnregisterFontsForURL, &path) {
        Ok(()) | Err(NOT_REGISTERED | FILE_NOT_FOUND) => Ok(()),
        Err(IN_USE) => Err(ActivationError::InUse(path)),
        Err(code) => Err(ActivationError::Os(format!("CoreText error {code}"))),
    }
}

// ponytail: one by one; CoreText registration is fast. Batch with CTFontManagerRegisterFontURLs if it isn't.
pub fn activate_many(entries: &[LedgerEntry]) -> Vec<Result<(), ActivationError>> {
    entries.iter().map(activate).collect()
}

pub fn deactivate_many(entries: &[LedgerEntry]) -> Vec<Result<(), ActivationError>> {
    entries.iter().map(deactivate).collect()
}

/// File paths of all persistent-scope registrations (one per file, even for collections).
fn registered_paths() -> HashSet<PathBuf> {
    unsafe {
        let array = CTFontManagerCopyRegisteredFontDescriptors(SCOPE_PERSISTENT, true);
        if array.is_null() {
            return HashSet::new();
        }
        let array: CFArray<CFType> = CFArray::wrap_under_create_rule(array);
        array
            .iter()
            .filter_map(|d| {
                let url = CTFontDescriptorCopyAttribute(d.as_CFTypeRef(), kCTFontURLAttribute);
                if url.is_null() {
                    return None;
                }
                CFURL::wrap_under_create_rule(url as CFURLRef).to_path()
            })
            .collect()
    }
}

pub fn reconcile(ledger: &[LedgerEntry]) -> ReconcileReport {
    let mut report = ReconcileReport::default();
    let registered = registered_paths();
    for path in &registered {
        if !path.exists() && call(CTFontManagerUnregisterFontsForURL, path).is_ok() {
            report.removed.push(path.display().to_string());
        }
    }
    for e in ledger {
        if !e.path.is_file() {
            report.missing.push(e.path.clone());
        } else if !registered.contains(&e.path) && activate(e).is_ok() {
            report.reregistered.push(e.path.clone());
        }
    }
    report
}

pub fn installed_ps_names() -> HashSet<String> {
    let mut files = Vec::new();
    crate::walk(Path::new("/System/Library/Fonts"), &mut files);
    crate::walk(Path::new("/Library/Fonts"), &mut files);
    if let Some(home) = std::env::var_os("HOME") {
        crate::walk(&PathBuf::from(home).join("Library/Fonts"), &mut files);
    }
    crate::ps_names_of(&files)
}

/// CoreText posts kCTFontManagerRegisteredFontsChangedNotification itself.
pub fn notify_font_change() {}
