//! The macOS swap of the downloaded app at quit. A stub for now: nothing blocks an update.

/// A reason this Mac cannot install updates (not in /Applications, ...), shown in Settings.
#[allow(dead_code)]
pub fn blocker() -> Option<&'static str> {
    None
}
