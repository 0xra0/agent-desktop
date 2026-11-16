use agent_desktop_core::{AdapterError, ErrorCode, KeyCombo, Modifier};
use serde_json::json;

use super::{is_focus_refusal, is_open_owner, is_plain_escape};
use crate::tree::test_support::helpers::{read_of, words};
use crate::tree::vocabulary::role_numbers::{COMBO_BOX, MENU, PUSH_BUTTON, TABLE_CELL};
use crate::tree::vocabulary::state_bits::{EXPANDED, HAS_POPUP, SHOWING};
use crate::tree::{Field, ObservedRead, WalkedNode};

fn node(
    role: u32,
    states: &[u32],
    children: Vec<WalkedNode<ObservedRead>>,
) -> WalkedNode<ObservedRead> {
    let mut read = read_of(role, "n", &[]);
    read.node.traits.states = Field::Present(words(states));
    WalkedNode {
        payload: read,
        children,
        truncated: false,
        child_count: None,
        source_index: 0,
        in_web_document: false,
    }
}

fn combo(key: &str, modifiers: Vec<Modifier>) -> KeyCombo {
    KeyCombo {
        key: key.to_owned(),
        modifiers,
    }
}

#[test]
fn only_a_plain_escape_is_an_escape() {
    assert!(is_plain_escape(&combo("escape", Vec::new())));
    assert!(is_plain_escape(&combo("Escape", Vec::new())));
    assert!(!is_plain_escape(&combo("escape", vec![Modifier::Ctrl])));
    assert!(!is_plain_escape(&combo("return", Vec::new())));
}

#[test]
fn only_a_focus_refusal_is_answered_by_closing_a_popup() {
    let refusal = |check: &str| {
        AdapterError::new(ErrorCode::ActionFailed, "refused")
            .with_details(json!({ "check": check }))
    };
    assert!(is_focus_refusal(&refusal("window_active")));
    assert!(is_focus_refusal(&refusal("exact_window_focus")));
    assert!(!is_focus_refusal(&refusal("process_state")));
    assert!(!is_focus_refusal(&AdapterError::new(
        ErrorCode::ActionFailed,
        "no details"
    )));
}

#[test]
fn only_a_combo_box_or_a_popup_button_with_an_open_popup_is_an_open_owner() {
    let shown = || vec![node(MENU, &[SHOWING], Vec::new())];
    let hidden = || vec![node(MENU, &[], Vec::new())];
    assert!(is_open_owner(&node(COMBO_BOX, &[], shown())));
    assert!(!is_open_owner(&node(COMBO_BOX, &[], hidden())));
    assert!(is_open_owner(&node(
        PUSH_BUTTON,
        &[HAS_POPUP, EXPANDED],
        Vec::new()
    )));
    assert!(!is_open_owner(&node(PUSH_BUTTON, &[EXPANDED], Vec::new())));
    assert!(!is_open_owner(&node(TABLE_CELL, &[EXPANDED], shown())));
}
