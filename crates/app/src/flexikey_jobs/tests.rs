use super::*;
use std::collections::HashSet;
use std::sync::atomic::AtomicUsize;
use std::time::{Duration, Instant};

#[derive(Default)]
struct State {
    active: Mutex<Option<String>>,
    saved: Mutex<Vec<String>>,
    reject_save: Mutex<HashSet<String>>,
    reject_activate: Mutex<HashSet<String>>,
    arm_calls: AtomicUsize,
    cancel_calls: AtomicUsize,
}

struct TestBackend {
    state: Arc<State>,
    activation_gate: Option<(mpsc::Sender<()>, Receiver<()>)>,
    capture_gate: Option<(mpsc::Sender<()>, Receiver<()>)>,
}

impl Backend for TestBackend {
    fn activate(&mut self, profile: Option<&FlexiKeyProfile>) -> Result<EngineReport, String> {
        if let Some((entered, proceed)) = self.activation_gate.take() {
            entered.send(()).unwrap();
            proceed.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        let name = profile.map(|p| p.name.clone());
        if name
            .as_ref()
            .is_some_and(|name| self.state.reject_activate.lock().unwrap().contains(name))
        {
            return Err("activation failure".into());
        }
        *self.state.active.lock().unwrap() = name;
        Ok(EngineReport::default())
    }
    fn save(&mut self, _: &Path, store: &FlexiKeyStore) -> Result<(), String> {
        let name = store.active().unwrap().name.clone();
        if self.state.reject_save.lock().unwrap().contains(&name) {
            return Err("write failure".into());
        }
        self.state.saved.lock().unwrap().push(name);
        Ok(())
    }
    fn arm_capture(&mut self) -> Result<(), String> {
        self.state.arm_calls.fetch_add(1, Ordering::SeqCst);
        if let Some((entered, proceed)) = self.capture_gate.take() {
            entered.send(()).unwrap();
            proceed.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        Ok(())
    }
    fn cancel_capture(&mut self) {
        self.state.cancel_calls.fetch_add(1, Ordering::SeqCst);
    }
    fn reconcile(&mut self) -> Result<(), String> {
        Ok(())
    }
}

fn setup(
    activation_gate: Option<(mpsc::Sender<()>, Receiver<()>)>,
    capture_gate: Option<(mpsc::Sender<()>, Receiver<()>)>,
) -> (FlexiKeyJobs, Arc<State>, Receiver<()>) {
    let state = Arc::new(State::default());
    let backend = TestBackend {
        state: Arc::clone(&state),
        activation_gate,
        capture_gate,
    };
    let (sender, notified) = mpsc::channel();
    let jobs = FlexiKeyJobs::start_with_backend(
        FlexiKeyEngine {},
        PathBuf::from("unused.xml"),
        backend,
        move || {
            let _ = sender.send(());
        },
    )
    .unwrap();
    (jobs, state, notified)
}

fn store(name: &str) -> FlexiKeyStore {
    let mut store = FlexiKeyStore::default();
    store.add_profile(name.into());
    store
}

fn wait_for_current(jobs: &FlexiKeyJobs, notified: &Receiver<()>) -> FlexiKeyViewState {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some((id, state)) = jobs.take_replies().profile
            && jobs.is_current(id)
        {
            return state;
        }
        notified
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("worker completion");
    }
}

#[test]
fn blocked_configuration_keeps_submission_and_cancellation_nonblocking() {
    let (entered, entering) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let (jobs, state, notified) = setup(Some((entered, gate)), None);
    jobs.save(store("first"), "".into(), None).unwrap();
    entering.recv_timeout(Duration::from_secs(5)).unwrap();
    for i in 0..QUEUE_CAPACITY {
        jobs.save(store(&format!("draft-{i}")), "".into(), None)
            .unwrap();
    }
    let accepted = jobs.latest.get();
    assert!(jobs.save(store("overflow"), "".into(), None).is_err());
    assert_eq!(jobs.latest.get(), accepted);
    jobs.cancel_capture();
    assert!(state.active.lock().unwrap().is_none());
    release.send(()).unwrap();
    let result = wait_for_current(&jobs, &notified);
    assert_eq!(result.store.active().unwrap().name, "draft-31");
    assert!(result.operation_error.is_none());
    assert_eq!(state.saved.lock().unwrap().len(), QUEUE_CAPACITY + 1);
}

#[test]
fn latest_save_failure_restores_last_success_even_before_ui_consumes_it() {
    let (jobs, state, notified) = setup(None, None);
    state.reject_save.lock().unwrap().insert("second".into());
    jobs.save(store("first"), "first notice".into(), None)
        .unwrap();
    let first_id = jobs.latest.get();
    jobs.save(store("second"), "second notice".into(), None)
        .unwrap();
    assert!(!jobs.is_current(first_id));
    let result = wait_for_current(&jobs, &notified);
    assert_eq!(result.store.active().unwrap().name, "first");
    assert_eq!(state.active.lock().unwrap().as_deref(), Some("first"));
    assert_eq!(state.saved.lock().unwrap().as_slice(), ["first"]);
    assert_eq!(result.operation_error.as_deref(), Some("write failure"));
    assert!(result.engine_error.is_none());
    assert!(result.operation_notice.is_none());
}

#[test]
fn later_complete_draft_preserves_intent_after_an_intermediate_failure() {
    let (jobs, state, notified) = setup(None, None);
    state.reject_save.lock().unwrap().insert("second".into());
    jobs.save(store("first"), "".into(), None).unwrap();
    jobs.save(store("second"), "".into(), None).unwrap();
    let mut last = store("third");
    last.profiles[0].mouse_enabled = false;
    jobs.save(last, "saved".into(), None).unwrap();
    let result = wait_for_current(&jobs, &notified);
    assert_eq!(result.store.active().unwrap().name, "third");
    assert!(!result.store.active().unwrap().mouse_enabled);
    assert!(result.operation_error.is_none());
    assert_eq!(state.saved.lock().unwrap().as_slice(), ["first", "third"]);
}

#[test]
fn runtime_rollback_failure_is_visible_separately_from_disk_failure() {
    let (jobs, state, notified) = setup(None, None);
    jobs.save(store("first"), "".into(), None).unwrap();
    wait_for_current(&jobs, &notified);
    state.reject_activate.lock().unwrap().insert("first".into());
    state.reject_save.lock().unwrap().insert("second".into());
    jobs.save(store("second"), "".into(), None).unwrap();
    let result = wait_for_current(&jobs, &notified);
    assert_eq!(result.operation_error.as_deref(), Some("write failure"));
    assert!(result.engine_error.unwrap().contains("activation failure"));
    assert!(result.engine_report.is_none());
}

#[test]
fn canceled_queued_capture_never_arms_later() {
    let (entered, entering) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let (jobs, state, notified) = setup(Some((entered, gate)), None);
    jobs.save(store("first"), "".into(), None).unwrap();
    entering.recv_timeout(Duration::from_secs(5)).unwrap();
    let old = jobs.arm_capture().unwrap();
    jobs.cancel_capture();
    assert!(!jobs.capture_is_current(old));
    jobs.save(store("last"), "".into(), None).unwrap();
    release.send(()).unwrap();
    wait_for_current(&jobs, &notified);
    assert_eq!(state.arm_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn cancellation_during_slow_capture_setup_disarms_before_next_command() {
    let (entered, entering) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let (jobs, state, notified) = setup(None, Some((entered, gate)));
    let old = jobs.arm_capture().unwrap();
    entering.recv_timeout(Duration::from_secs(5)).unwrap();
    jobs.cancel_capture();
    jobs.save(store("last"), "".into(), None).unwrap();
    release.send(()).unwrap();
    wait_for_current(&jobs, &notified);
    assert!(!jobs.capture_is_current(old));
    assert_eq!(state.arm_calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.cancel_calls.load(Ordering::SeqCst), 1);
    assert!(jobs.take_replies().capture.is_none());
}
