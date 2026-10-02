/// Keeps the Mac from napping the game or idling to sleep while a game is open. macOS throttles
/// the timers of a window it thinks nobody is using, and a controller doesn't count as using it.
#[cfg(target_os = "macos")]
pub struct Awake(
    objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2::runtime::NSObjectProtocol>>,
);

#[cfg(target_os = "macos")]
impl Awake {
    pub fn begin() -> Self {
        use objc2_foundation::{ns_string, NSActivityOptions, NSProcessInfo};
        let activity = NSProcessInfo::processInfo().beginActivityWithOptions_reason(
            NSActivityOptions::UserInitiated | NSActivityOptions::LatencyCritical,
            ns_string!("Playing a game"),
        );
        Self(activity)
    }
}

#[cfg(target_os = "macos")]
impl Drop for Awake {
    fn drop(&mut self) {
        unsafe { objc2_foundation::NSProcessInfo::processInfo().endActivity(&self.0) };
    }
}

#[cfg(not(target_os = "macos"))]
pub struct Awake;

#[cfg(not(target_os = "macos"))]
impl Awake {
    pub fn begin() -> Self {
        Self
    }
}
