use std::sync::atomic::{AtomicUsize, Ordering};

use agent_desktop_core::{ErrorCode, Modifier, ProcessId, ProcessIdentity, WindowState};

use super::{
    ActionResult, AdapterError, CANCEL, Deadline, DeliverySemantics, InteractionLease,
    InteractionPolicy, KeyCombo, SystemOps, WindowInfo, answer_with_surface_close,
    answers_with_surface_close, delivery_result, is_focus_refusal, press_for_window, semantic_step,
};

struct WindowRoute {
    focused: Option<WindowInfo>,
    dispatched: AtomicUsize,
}

impl SystemOps for WindowRoute {
    fn focused_window(&self, _deadline: Deadline) -> Result<Option<WindowInfo>, AdapterError> {
        Ok(self.focused.clone())
    }

    fn press_key_for_app(
        &self,
        _process: ProcessIdentity,
        _combo: &KeyCombo,
        _policy: InteractionPolicy,
        _lease: &InteractionLease,
    ) -> Result<ActionResult, AdapterError> {
        self.dispatched.fetch_add(1, Ordering::SeqCst);
        Ok(ActionResult::delivered_unverified("press_key"))
    }
}

fn window(id: &str) -> WindowInfo {
    WindowInfo {
        id: id.into(),
        title: format!("Window {id}"),
        app: "Fixture".into(),
        pid: ProcessId::new(std::process::id()),
        process_instance: Some("generation-1".into()),
        bounds: None,
        state: WindowState::default(),
    }
}

fn route(focused: &str) -> WindowRoute {
    WindowRoute {
        focused: Some(window(focused)),
        dispatched: AtomicUsize::new(0),
    }
}

fn key(name: &str, modifiers: Vec<Modifier>) -> KeyCombo {
    KeyCombo {
        key: name.into(),
        modifiers,
    }
}

fn lease() -> InteractionLease {
    InteractionLease::guarded(Deadline::standard().unwrap(), ()).unwrap()
}

fn focus_refusal(adapter: &WindowRoute) -> AdapterError {
    agent_desktop_core::press_key_in_focused_window(
        adapter,
        &window("w-1"),
        &key("a", Vec::new()),
        InteractionPolicy::headless(),
        &lease(),
    )
    .unwrap_err()
}

#[test]
fn a_window_that_holds_the_focus_takes_the_key_itself() {
    let adapter = route("w-1");
    let result = press_for_window(
        &adapter,
        &window("w-1"),
        &key("escape", Vec::new()),
        InteractionPolicy::headless(),
        &lease(),
    )
    .unwrap();

    assert_eq!(result.action, "press_key");
    assert_eq!(adapter.dispatched.load(Ordering::SeqCst), 1);
}

#[test]
fn only_a_plain_escape_answers_the_window_focus_refusal() {
    let refusal = focus_refusal(&route("w-2"));

    assert!(is_focus_refusal(&refusal));
    assert!(answers_with_surface_close(
        &key("escape", Vec::new()),
        &refusal
    ));
    assert!(answers_with_surface_close(
        &key("esc", Vec::new()),
        &refusal
    ));
    assert!(!answers_with_surface_close(&key("a", Vec::new()), &refusal));
    assert!(!answers_with_surface_close(
        &key("escape", vec![Modifier::Shift]),
        &refusal
    ));
}

#[test]
fn a_window_press_that_is_not_a_plain_escape_keeps_its_refusal() {
    let adapter = route("w-2");
    let error = press_for_window(
        &adapter,
        &window("w-1"),
        &key("a", Vec::new()),
        InteractionPolicy::headless(),
        &lease(),
    )
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::StaleRef);
    assert_eq!(error.details.unwrap()["check"], "window_active");
    assert_eq!(adapter.dispatched.load(Ordering::SeqCst), 0);
}

#[test]
fn a_closed_surface_answers_the_refused_window_escape() {
    let closed = delivery_result(semantic_step(CANCEL).with_verified(true));
    assert_eq!(
        closed.disposition(),
        DeliverySemantics::delivered_verified()
    );

    let answer =
        answer_with_surface_close(focus_refusal(&route("w-2")), || Ok(Some(closed))).unwrap();
    assert_eq!(
        answer.disposition(),
        DeliverySemantics::delivered_verified()
    );

    let unread = delivery_result(semantic_step(CANCEL).with_verified(false));
    let answer =
        answer_with_surface_close(focus_refusal(&route("w-2")), || Ok(Some(unread))).unwrap();
    assert_eq!(
        answer.disposition(),
        DeliverySemantics::delivered_unverified()
    );
}

#[test]
fn a_refused_window_escape_keeps_the_refusal_when_no_surface_is_open() {
    let answer = answer_with_surface_close(focus_refusal(&route("w-2")), || Ok(None)).unwrap_err();

    assert!(is_focus_refusal(&answer));
    assert_eq!(answer.disposition, DeliverySemantics::not_delivered());
}

#[test]
fn a_surface_close_that_failed_keeps_its_own_error() {
    let answer = answer_with_surface_close(focus_refusal(&route("w-2")), || {
        Err(AdapterError::new(ErrorCode::ActionFailed, "cancel failed"))
    })
    .unwrap_err();

    assert_eq!(answer.message, "cancel failed");
}
