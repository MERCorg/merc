use std::result::Result;

use merc_utilities::MercError;
#[cfg(windows)]
use winapi::um::consoleapi::AllocConsole;

#[cfg(windows)]
use winapi::um::wincon::ATTACH_PARENT_PROCESS;
#[cfg(windows)]
use winapi::um::wincon::AttachConsole;
#[cfg(windows)]
use winapi::um::wincon::FreeConsole;
#[cfg(windows)]
use winapi::um::wincon::GetConsoleWindow;

/// Guard returned by [`init_console`]; dropping it frees a console this call allocated.
pub struct Console {
    #[cfg(windows)]
    attached: bool,
}

/// Attaches to a console so `println!` and panic output are visible from a
/// Windows-subsystem GUI binary, which otherwise starts with no attached stdio.
///
/// On Windows this attaches to the parent process' console, an existing console,
/// or allocates a fresh one when none is available. Dropping the returned guard
/// frees a console that this call allocated. On other platforms it is a no-op and
/// the guard does nothing.
pub fn init_console() -> Result<Console, MercError> {
    #[cfg(windows)]
    unsafe {
        // SAFETY: These console functions take no pointers and are safe to call
        // in any process state; the only soundness obligation is to balance an
        // allocated console with a single `FreeConsole`, which the `Drop` impl
        // does (guarded by `attached` so we never free a pre-existing console).
        // Check if we're attached to an existing Windows console
        if GetConsoleWindow().is_null() {
            // Try to attach to an existing Windows console.
            //
            // It's normally a no-brainer to call this - it just makes println! and friends
            // work as expected, without cluttering the screen with a console in the general
            // case.
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                // Try to attach to a console, and if not, allocate ourselves a new one.
                if AllocConsole() != 0 {
                    Ok(Console { attached: false })
                } else {
                    Err("Failed to attach to a console, and to create one".into())
                }
            } else {
                // We attached to an existing console.
                Ok(Console { attached: true })
            }
        } else {
            // The program was started with a console attached.
            Ok(Console { attached: true })
        }
    }

    #[cfg(not(windows))]
    {
        Ok(Console {})
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        // Free the allocated console, when it was not attached.
        #[cfg(windows)]
        if !self.attached {
            // SAFETY: `FreeConsole` takes no arguments; `attached == false`
            // means `init_console` allocated this console, so freeing it here
            // balances that allocation exactly once.
            unsafe { FreeConsole() };
        }
    }
}
