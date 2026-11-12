//! Whether anyone can be at the screen: is the session unlocked, is the
//! display awake. The daily briefing speaks only when both are true.

use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGSessionCopyCurrentDictionary() -> CFDictionaryRef;
    fn CGMainDisplayID() -> u32;
    fn CGDisplayIsAsleep(display: u32) -> u32;
}

fn flag(session: &CFDictionary<CFString, CFType>, key: &'static str) -> Option<bool> {
    let value = session.find(CFString::from_static_string(key))?;
    if let Some(boolean) = value.downcast::<CFBoolean>() {
        return Some(boolean.into());
    }
    value.downcast::<CFNumber>().and_then(|number| number.to_i64()).map(|number| number != 0)
}

/// Whether the screen is locked, or this user's session is not the one on the
/// screen (fast user switching, the login window). With no window-server
/// session at all the answer is yes: nobody is looking.
pub fn screen_locked() -> bool {
    // SAFETY: the dictionary is checked for null and follows the Copy rule,
    // so the wrapper owns it and releases it.
    let session: CFDictionary<CFString, CFType> = unsafe {
        let dictionary = CGSessionCopyCurrentDictionary();
        if dictionary.is_null() {
            return true;
        }
        CFDictionary::wrap_under_create_rule(dictionary)
    };
    let locked = flag(&session, "CGSSessionScreenIsLocked").unwrap_or(false);
    let on_console = flag(&session, "kCGSSessionOnConsoleKey").unwrap_or(true);
    locked || !on_console
}

/// Whether the main display is asleep. It is during a dark wake (Power Nap,
/// lid closed), when the Mac runs code with nobody in front of it.
pub fn display_asleep() -> bool {
    // SAFETY: plain value calls with no pointers.
    unsafe { CGDisplayIsAsleep(CGMainDisplayID()) != 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the real session and display once. The answers depend on the
    /// machine running the tests, so nothing is asserted about them; this
    /// proves the calls link and return. Run with `--nocapture` to see what
    /// this Mac reports.
    #[test]
    fn the_real_session_and_display_can_be_read() {
        println!("screen_locked = {}, display_asleep = {}", screen_locked(), display_asleep());
    }
}
