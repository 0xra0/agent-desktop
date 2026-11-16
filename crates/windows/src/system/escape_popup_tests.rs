use agent_desktop_core::{AdapterError, DeliverySemantics, ErrorCode, KeyCombo, Modifier};

use super::{is_plain_escape, opens_a_popup, refused_for_focus};

fn key(name: &str, modifiers: Vec<Modifier>) -> KeyCombo {
    KeyCombo {
        key: name.to_owned(),
        modifiers,
    }
}

#[test]
fn only_a_plain_escape_may_close_a_popup() {
    assert!(is_plain_escape(&key("escape", vec![])));
    assert!(is_plain_escape(&key("Esc", vec![])));
    assert!(!is_plain_escape(&key("escape", vec![Modifier::Shift])));
    assert!(!is_plain_escape(&key("return", vec![])));
}

#[test]
fn each_scope_answers_only_its_own_focus_refusal() {
    let window = AdapterError::stale_ref_because("not focused")
        .with_details(
            serde_json::json!({ "check": "window_active", "physical_delivery_started": false }),
        )
        .with_disposition(DeliverySemantics::not_delivered());
    let process = AdapterError::new(ErrorCode::ActionFailed, "not foreground")
        .with_details(serde_json::json!({ "pid": 4, "physical_delivery_started": false }))
        .with_disposition(DeliverySemantics::not_delivered());
    let sent = AdapterError::new(ErrorCode::ActionFailed, "sent")
        .with_details(serde_json::json!({ "physical_delivery_started": true }))
        .with_disposition(DeliverySemantics::delivered_unverified());

    assert!(refused_for_focus(true, &window));
    assert!(!refused_for_focus(false, &window));
    assert!(refused_for_focus(false, &process));
    assert!(!refused_for_focus(true, &process));
    assert!(!refused_for_focus(false, &sent));
}

#[test]
fn a_tree_item_or_a_group_is_never_a_popup_owner() {
    assert!(opens_a_popup(50011) && opens_a_popup(50003) && opens_a_popup(50031));
    assert!(!opens_a_popup(50024) && !opens_a_popup(50026) && !opens_a_popup(50029));
}
