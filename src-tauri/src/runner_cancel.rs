use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[cfg(windows)]
use windows_sys::Win32::System::Console::{SetConsoleCtrlHandler, CTRL_BREAK_EVENT, CTRL_C_EVENT};

#[cfg(windows)]
static CONSOLE_CANCELLED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug)]
pub struct RunnerCancellationV1 {
    local_cancelled: Arc<AtomicBool>,
    observe_console: bool,
}

impl RunnerCancellationV1 {
    pub fn new() -> Self {
        Self {
            local_cancelled: Arc::new(AtomicBool::new(false)),
            observe_console: false,
        }
    }

    pub fn cancel(&self) {
        self.local_cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        if self.local_cancelled.load(Ordering::Acquire) {
            return true;
        }
        #[cfg(windows)]
        if self.observe_console && CONSOLE_CANCELLED.load(Ordering::Acquire) {
            return true;
        }
        false
    }

    fn observing_console() -> Self {
        Self {
            local_cancelled: Arc::new(AtomicBool::new(false)),
            observe_console: true,
        }
    }
}

impl Default for RunnerCancellationV1 {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct ConsoleCancellationGuardV1 {
    #[cfg(windows)]
    installed: bool,
}

impl ConsoleCancellationGuardV1 {
    pub fn install() -> io::Result<(Self, RunnerCancellationV1)> {
        #[cfg(windows)]
        {
            CONSOLE_CANCELLED.store(false, Ordering::Release);
            let installed = unsafe { SetConsoleCtrlHandler(Some(console_control_handler), 1) };
            if installed == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok((
                Self { installed: true },
                RunnerCancellationV1::observing_console(),
            ))
        }

        #[cfg(not(windows))]
        Ok((Self {}, RunnerCancellationV1::new()))
    }
}

#[cfg(windows)]
impl Drop for ConsoleCancellationGuardV1 {
    fn drop(&mut self) {
        if self.installed {
            let _ = unsafe { SetConsoleCtrlHandler(Some(console_control_handler), 0) };
            self.installed = false;
            CONSOLE_CANCELLED.store(false, Ordering::Release);
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn console_control_handler(control_type: u32) -> i32 {
    match control_type {
        CTRL_C_EVENT | CTRL_BREAK_EVENT => {
            CONSOLE_CANCELLED.store(true, Ordering::Release);
            1
        }
        _ => 0,
    }
}
