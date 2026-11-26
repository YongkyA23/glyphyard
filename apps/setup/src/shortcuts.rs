//! "Glyphyard" shortcuts in the Start menu and on the desktop (IShellLinkW).

use std::io;
use std::path::{Path, PathBuf};

use windows::core::{Interface, GUID, HSTRING};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Programs, IShellLinkW, SHGetKnownFolderPath, ShellLink, KNOWN_FOLDER_FLAG,
};

#[derive(Clone, Copy)]
pub enum Place {
    StartMenu,
    Desktop,
}

/// The shortcut's file name (what Start and the desktop show), then the name it had before the rename:
/// an update or uninstall removes that one too, and counts it when keeping the user's choices.
const NAMES: [&str; 2] = ["Glyphyard.lnk", "Typeshelf.lnk"];

fn folder(place: Place) -> io::Result<PathBuf> {
    let id: &GUID = match place {
        Place::StartMenu => &FOLDERID_Programs,
        Place::Desktop => &FOLDERID_Desktop,
    };
    unsafe {
        let p = SHGetKnownFolderPath(id, KNOWN_FOLDER_FLAG(0), None)?;
        let dir = PathBuf::from(p.to_string().map_err(io::Error::other)?);
        CoTaskMemFree(Some(p.0 as _));
        Ok(dir)
    }
}

pub fn create(place: Place, target: &Path) -> io::Result<()> {
    let dir = folder(place)?;
    unsafe {
        // Commands run on worker threads; S_FALSE / RPC_E_CHANGED_MODE just mean COM is already up.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(target.as_os_str()))?;
        if let Some(dir) = target.parent() {
            link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()))?;
        }
        link.cast::<IPersistFile>()?.Save(&HSTRING::from(dir.join(NAMES[0]).as_os_str()), true)?;
    }
    NAMES[1..].iter().try_for_each(|n| remove_file(&dir.join(n)))
}

pub fn exists(place: Place) -> bool {
    folder(place).is_ok_and(|dir| NAMES.iter().any(|n| dir.join(n).is_file()))
}

pub fn remove(place: Place) -> io::Result<()> {
    let dir = folder(place)?;
    NAMES.iter().try_for_each(|n| remove_file(&dir.join(n)))
}

fn remove_file(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Creates the shortcut when wanted, removes a leftover one otherwise.
pub fn set(place: Place, target: &Path, wanted: bool) -> io::Result<()> {
    if wanted {
        create(place, target)
    } else {
        remove(place)
    }
}
