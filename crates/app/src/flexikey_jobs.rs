//! Serial profile transactions and hook configuration outside the event loop.
//!
//! Each save carries the complete visible draft. A later draft includes earlier
//! edits; replies identify their request so only the latest intent updates UI.
//! The worker retains the last committed store for disk/runtime rollback.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};

use ram_control_center_protocol::{FlexiKeyProfile, FlexiKeyStore};

use crate::diagnostics::log_action;
use crate::flexikey_engine::{EngineReport, FlexiKeyEngine};
use crate::flexikey_store::{FlexiKeyViewState, load_flexikey_view, save_flexikey_store};
use crate::localization::tr;

#[cfg(test)]
mod tests;

const QUEUE_CAPACITY: usize = 32;

enum Job {
    Load(u64),
    Save {
        id: u64,
        store: FlexiKeyStore,
        notice: String,
        activated: Option<usize>,
    },
    Capture(u64),
    Reconcile,
}

#[derive(Default)]
pub(crate) struct Replies {
    pub(crate) profile: Option<(u64, FlexiKeyViewState)>,
    pub(crate) capture: Option<(u64, Result<(), String>)>,
    notified: bool,
}

/// UI-owned sender; callbacks enqueue work without waiting for disk or Win32.
pub(crate) struct FlexiKeyJobs {
    sender: SyncSender<Job>,
    replies: Arc<Mutex<Replies>>,
    latest: Cell<u64>,
    capture_epoch: Arc<AtomicU64>,
    engine: FlexiKeyEngine,
}

impl FlexiKeyJobs {
    pub(crate) fn start(
        engine: FlexiKeyEngine,
        path: PathBuf,
        notify: impl Fn() + Send + 'static,
    ) -> Result<Self, String> {
        let jobs = Self::start_with_backend(engine.clone(), path, engine.clone(), notify)?;
        let wake = jobs.sender.clone();
        engine.set_hook_wake(move || {
            // A full queue already wakes the worker, which reconciles after
            // every job. A hook callback must never wait for queue capacity.
            let _ = wake.try_send(Job::Reconcile);
        });
        Ok(jobs)
    }

    fn start_with_backend(
        engine: FlexiKeyEngine,
        path: PathBuf,
        backend: impl Backend,
        notify: impl Fn() + Send + 'static,
    ) -> Result<Self, String> {
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let replies = Arc::new(Mutex::new(Replies::default()));
        let capture_epoch = Arc::new(AtomicU64::new(0));
        let thread_replies = Arc::clone(&replies);
        let thread_epoch = Arc::clone(&capture_epoch);
        std::thread::Builder::new()
            .name("flexikey-config".into())
            .spawn(move || {
                run(
                    receiver,
                    thread_replies,
                    thread_epoch,
                    path,
                    backend,
                    notify,
                )
            })
            .map_err(|error| format!("start FlexiKey configuration worker: {error}"))?;
        Ok(Self {
            sender,
            replies,
            latest: Cell::new(0),
            capture_epoch,
            engine,
        })
    }

    fn request(&self, job: impl FnOnce(u64) -> Job) -> Result<(), String> {
        let id = self.latest.get() + 1;
        self.sender
            .try_send(job(id))
            .map_err(|error| format!("FlexiKey: {error}"))?;
        self.latest.set(id);
        Ok(())
    }

    pub(crate) fn save(
        &self,
        store: FlexiKeyStore,
        notice: String,
        activated: Option<usize>,
    ) -> Result<(), String> {
        self.request(|id| Job::Save {
            id,
            store,
            notice,
            activated,
        })
    }

    pub(crate) fn reload(&self) -> Result<(), String> {
        self.cancel_capture();
        self.request(Job::Load)
    }

    pub(crate) fn is_current(&self, id: u64) -> bool {
        self.latest.get() == id
    }

    pub(crate) fn take_replies(&self) -> Replies {
        std::mem::take(
            &mut *self
                .replies
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        )
    }

    pub(crate) fn arm_capture(&self) -> Result<u64, String> {
        self.cancel_capture();
        let epoch = self.capture_epoch.load(Ordering::Acquire);
        self.sender
            .try_send(Job::Capture(epoch))
            .map_err(|error| format!("FlexiKey: {error}"))?;
        Ok(epoch)
    }

    pub(crate) fn capture_is_current(&self, epoch: u64) -> bool {
        self.capture_epoch.load(Ordering::Acquire) == epoch
    }

    pub(crate) fn cancel_capture(&self) {
        self.capture_epoch.fetch_add(1, Ordering::AcqRel);
        self.engine.cancel_capture();
    }
}

impl Drop for FlexiKeyJobs {
    fn drop(&mut self) {
        self.cancel_capture();
        self.engine.set_hook_wake(|| {});
    }
}

trait Backend: Send + 'static {
    fn activate(&mut self, profile: Option<&FlexiKeyProfile>) -> Result<EngineReport, String>;
    fn save(&mut self, path: &Path, store: &FlexiKeyStore) -> Result<(), String>;
    fn arm_capture(&mut self) -> Result<(), String>;
    fn cancel_capture(&mut self);
    fn reconcile(&mut self) -> Result<(), String>;
}

impl Backend for FlexiKeyEngine {
    fn activate(&mut self, profile: Option<&FlexiKeyProfile>) -> Result<EngineReport, String> {
        self.update_profile(profile)
    }
    fn save(&mut self, path: &Path, store: &FlexiKeyStore) -> Result<(), String> {
        save_flexikey_store(path, store)
    }
    fn arm_capture(&mut self) -> Result<(), String> {
        FlexiKeyEngine::arm_capture(self)
    }
    fn cancel_capture(&mut self) {
        FlexiKeyEngine::cancel_capture(self);
    }
    fn reconcile(&mut self) -> Result<(), String> {
        self.reconcile_hooks()
    }
}

fn publish(replies: &Mutex<Replies>, notify: &impl Fn(), update: impl FnOnce(&mut Replies)) {
    let mut replies = replies.lock().unwrap_or_else(|error| error.into_inner());
    update(&mut replies);
    let should_notify = !replies.notified;
    replies.notified = true;
    drop(replies);
    if should_notify {
        notify();
    }
}

pub(crate) fn empty_state(path: PathBuf) -> FlexiKeyViewState {
    FlexiKeyViewState {
        path,
        store: FlexiKeyStore::default(),
        load_error: None,
        activated_index: None,
        operation_error: None,
        operation_notice: None,
        engine_report: None,
        engine_error: None,
    }
}

fn run(
    receiver: Receiver<Job>,
    replies: Arc<Mutex<Replies>>,
    epoch: Arc<AtomicU64>,
    path: PathBuf,
    mut backend: impl Backend,
    notify: impl Fn(),
) {
    let mut confirmed = empty_state(path);
    while let Ok(job) = receiver.recv() {
        match job {
            Job::Load(id) => {
                let mut loaded = load_flexikey_view(&confirmed.path);
                if loaded.load_error.is_some() {
                    confirmed.load_error = loaded.load_error;
                    confirmed.operation_notice = None;
                    confirmed.operation_error = None;
                } else {
                    match backend.activate(loaded.store.active()) {
                        Ok(report) => {
                            loaded.engine_report = Some(report);
                            confirmed = loaded;
                        }
                        Err(error) => restore_after_error(&mut backend, &mut confirmed, error),
                    }
                }
                publish(&replies, &notify, |r| {
                    r.profile = Some((id, confirmed.clone()))
                });
            }
            Job::Save {
                id,
                store,
                notice,
                activated,
            } => {
                match backend.activate(store.active()) {
                    Ok(report) => match backend.save(&confirmed.path, &store) {
                        Ok(()) => {
                            confirmed.store = store;
                            confirmed.load_error = None;
                            confirmed.operation_error = None;
                            confirmed.operation_notice = (!notice.is_empty()).then_some(notice);
                            confirmed.activated_index = activated;
                            confirmed.engine_report = Some(report);
                            confirmed.engine_error = None;
                        }
                        Err(error) => restore_after_error(&mut backend, &mut confirmed, error),
                    },
                    Err(error) => restore_after_error(&mut backend, &mut confirmed, error),
                }
                publish(&replies, &notify, |r| {
                    r.profile = Some((id, confirmed.clone()))
                });
            }
            Job::Capture(request_epoch) => {
                if epoch.load(Ordering::Acquire) == request_epoch {
                    let result = backend.arm_capture();
                    if epoch.load(Ordering::Acquire) == request_epoch {
                        publish(&replies, &notify, |r| {
                            r.capture = Some((request_epoch, result))
                        });
                    } else {
                        backend.cancel_capture();
                    }
                }
            }
            Job::Reconcile => {}
        }
        if let Err(error) = backend.reconcile() {
            log_action(&format!("FlexiKey hook reconciliation failed: {error}"));
        }
    }
    backend.cancel_capture();
    let _ = backend.reconcile();
}

fn restore_after_error(
    backend: &mut impl Backend,
    confirmed: &mut FlexiKeyViewState,
    error: String,
) {
    confirmed.operation_error = Some(error);
    confirmed.operation_notice = None;
    confirmed.activated_index = None;
    match backend.activate(confirmed.store.active()) {
        Ok(report) => {
            confirmed.engine_report = Some(report);
            confirmed.engine_error = None;
        }
        Err(error) => {
            confirmed.engine_report = None;
            confirmed.engine_error = Some(format!("{}: {error}", tr("Action engine unavailable")));
        }
    }
}
