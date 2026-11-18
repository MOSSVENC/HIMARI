//! Owns the thread and Win32 handles for low-level input hooks.
//!
//! The dispatcher supplies callbacks; this module installs and removes them on
//! one message-loop thread. A timed-out thread is retained until it exits, so a
//! retry cannot create a second set of hooks.

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows_sys::Win32::Foundation::WAIT_FAILED;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, HHOOK, HOOKPROC, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx,
    PM_REMOVE, PeekMessageW, QS_ALLINPUT, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    WH_KEYBOARD_LL, WH_MOUSE_LL, WM_QUIT,
};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct HookStatus {
    pub(crate) keyboard: bool,
    pub(crate) mouse: bool,
}

struct Configure {
    keyboard: bool,
    mouse: bool,
    keyboard_proc: HOOKPROC,
    mouse_proc: HOOKPROC,
    result: Sender<Result<HookStatus, String>>,
}

struct RunningThread {
    sender: Sender<Configure>,
    wake: Arc<OwnedHandle>,
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

#[derive(Default)]
pub(crate) struct HookRuntime {
    running: Option<RunningThread>,
    on_activated: Option<fn(HookStatus)>,
}

impl HookRuntime {
    pub(crate) fn with_activation_callback(callback: fn(HookStatus)) -> Self {
        Self {
            running: None,
            on_activated: Some(callback),
        }
    }

    pub(crate) fn is_started(&self) -> bool {
        self.running.is_some()
    }

    pub(crate) fn configure(
        &mut self,
        keyboard: bool,
        mouse: bool,
        keyboard_proc: HOOKPROC,
        mouse_proc: HOOKPROC,
    ) -> Result<HookStatus, String> {
        if self
            .running
            .as_ref()
            .is_some_and(|running| running.thread.is_finished())
            && let Some(running) = self.running.take()
        {
            let _ = running.thread.join();
        }
        if let Some(running) = self.running.as_ref()
            && running.stop.load(Ordering::Acquire)
        {
            return Err("input hook thread is still stopping".to_owned());
        }
        if self.running.is_none() {
            if !keyboard && !mouse {
                return Ok(HookStatus::default());
            }
            let wake = Arc::new(new_wake_event()?);
            let stop = Arc::new(AtomicBool::new(false));
            let (sender, receiver) = mpsc::channel();
            let thread_wake = Arc::clone(&wake);
            let thread_stop = Arc::clone(&stop);
            let on_activated = self.on_activated;
            let thread = thread::Builder::new()
                .name("input-hooks".to_owned())
                .spawn(move || {
                    run_loop(receiver, thread_wake, thread_stop, on_activated);
                })
                .map_err(|error| format!("start input hook thread: {error}"))?;
            self.running = Some(RunningThread {
                sender,
                wake,
                stop,
                thread,
            });
        }
        let running = self
            .running
            .as_ref()
            .expect("hook thread was created above");
        let (result, reply) = mpsc::channel();
        let outcome = running
            .sender
            .send(Configure {
                keyboard,
                mouse,
                keyboard_proc,
                mouse_proc,
                result,
            })
            .map_err(|_| "input hook thread exited".to_owned())
            .and_then(|()| signal(&running.wake))
            .and_then(|()| {
                reply
                    .recv_timeout(COMMAND_TIMEOUT)
                    .map_err(|error| format!("input hook command: {error}"))
            });
        if outcome.is_err() {
            running.stop.store(true, Ordering::Release);
            let _ = signal(&running.wake);
        }
        outcome?
    }
}

impl Drop for HookRuntime {
    fn drop(&mut self) {
        if let Some(running) = self.running.take() {
            running.stop.store(true, Ordering::Release);
            let _ = signal(&running.wake);
            if running.thread.is_finished() {
                let _ = running.thread.join();
            }
        }
    }
}

fn new_wake_event() -> Result<OwnedHandle, String> {
    // SAFETY: null security/name arguments create an unnamed auto-reset event;
    // a successful handle is immediately transferred to its sole Rust owner.
    unsafe {
        let raw = CreateEventW(ptr::null(), 0, 0, ptr::null());
        if raw.is_null() {
            return Err(format!(
                "create hook wake event: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(OwnedHandle::from_raw_handle(raw))
    }
}

fn signal(event: &OwnedHandle) -> Result<(), String> {
    // SAFETY: the borrowed OwnedHandle keeps this event alive through SetEvent.
    if unsafe { SetEvent(event.as_raw_handle()) } == 0 {
        return Err(format!(
            "wake input hook thread: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn run_loop(
    commands: Receiver<Configure>,
    wake: Arc<OwnedHandle>,
    stop: Arc<AtomicBool>,
    on_activated: Option<fn(HookStatus)>,
) {
    let mut hooks = Hooks {
        keyboard: None,
        mouse: None,
    };
    while !stop.load(Ordering::Acquire) {
        while let Ok(command) = commands.try_recv() {
            let previous = hooks.status();
            let result = hooks.configure(
                command.keyboard,
                command.mouse,
                command.keyboard_proc,
                command.mouse_proc,
            );
            let current = hooks.status();
            if let Some(callback) = on_activated {
                callback(HookStatus {
                    keyboard: current.keyboard && !previous.keyboard,
                    mouse: current.mouse && !previous.mouse,
                });
            }
            let _ = command.result.send(result);
        }
        if stop.load(Ordering::Acquire) || !pump_messages(&wake) {
            break;
        }
    }
    // Dropping Hooks releases handles on the thread which installed them.
}

fn pump_messages(wake: &OwnedHandle) -> bool {
    let handle = wake.as_raw_handle();
    let mut message = MSG::default();
    // SAFETY: `handle` remains live, message is writable local storage, and only
    // messages removed from this thread's queue are translated and dispatched.
    unsafe {
        if MsgWaitForMultipleObjectsEx(1, &handle, 1_000, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
            == WAIT_FAILED
        {
            return false;
        }
        while PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            if message.message == WM_QUIT {
                return false;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    true
}

struct Hooks {
    keyboard: Option<Hook>,
    mouse: Option<Hook>,
}

impl Hooks {
    fn status(&self) -> HookStatus {
        HookStatus {
            keyboard: self.keyboard.is_some(),
            mouse: self.mouse.is_some(),
        }
    }

    fn configure(
        &mut self,
        keyboard: bool,
        mouse: bool,
        keyboard_proc: HOOKPROC,
        mouse_proc: HOOKPROC,
    ) -> Result<HookStatus, String> {
        // Stage all additions before changing active handles. An installation
        // error drops only the staged hooks and leaves existing input paired.
        let new_keyboard = if keyboard && self.keyboard.is_none() {
            Some(Hook::install(WH_KEYBOARD_LL, keyboard_proc)?)
        } else {
            None
        };
        let new_mouse = if mouse && self.mouse.is_none() {
            Some(Hook::install(WH_MOUSE_LL, mouse_proc)?)
        } else {
            None
        };
        if new_keyboard.is_some() {
            self.keyboard = new_keyboard;
        }
        if new_mouse.is_some() {
            self.mouse = new_mouse;
        }
        if !keyboard {
            Hook::remove(&mut self.keyboard)?;
        }
        if !mouse {
            Hook::remove(&mut self.mouse)?;
        }
        Ok(self.status())
    }
}

struct Hook(HHOOK);

impl Hook {
    fn install(kind: i32, callback: HOOKPROC) -> Result<Self, String> {
        if callback.is_none() {
            return Err("input hook callback is missing".to_owned());
        }
        // SAFETY: callbacks are static functions in this process and the current
        // module remains loaded; the resulting hook is owned by this thread.
        let handle = unsafe { SetWindowsHookExW(kind, callback, GetModuleHandleW(ptr::null()), 0) };
        if handle.is_null() {
            return Err(format!(
                "install input hook {kind}: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self(handle))
    }
}

impl Hook {
    fn remove(slot: &mut Option<Self>) -> Result<(), String> {
        if let Some(hook) = slot {
            hook.uninstall()?;
        }
        // The handle is already released; taking the value must not unhook twice.
        if let Some(mut hook) = slot.take() {
            hook.0 = ptr::null_mut();
        }
        Ok(())
    }

    fn uninstall(&self) -> Result<(), String> {
        // SAFETY: this handle was created by Hook::install on this thread and
        // this owner removes it exactly once before the thread exits.
        if unsafe { UnhookWindowsHookEx(self.0) } == 0 {
            return Err(format!(
                "remove input hook: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}

impl Drop for Hook {
    fn drop(&mut self) {
        if !self.0.is_null()
            && let Err(error) = self.uninstall()
        {
            crate::diagnostics::log_action(&error);
        }
    }
}
