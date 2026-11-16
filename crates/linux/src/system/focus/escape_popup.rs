//! Escape for an application that does not own the active window.
//!
//! The key route sends the key only to the active window, so a key for a
//! background application is refused. A popup that the application opened
//! stays open in the background on some toolkits, and the key cannot reach
//! it. The popup has an owner: a combo box, or a button that has a popup. The
//! owner's own action toggles the popup, so the action is sent only when a
//! read shows the popup open, and the result is verified by reading it gone.
//!
//! Three states, in this order: the key route is allowed and nothing here
//! runs; the key route is refused for focus and an open popup owner exists
//! in the target window, whose own action closes it; otherwise the refusal
//! stays as it was.

use std::time::Duration;

use agent_desktop_core::{
    Action, ActionResult, AdapterError, ChainRung, Deadline, DeliveryOutcome, KeyCombo,
    StepMechanism, WindowInfo,
};

use crate::actions::action_call::resolve_verb;
use crate::actions::chain::run_semantic_chain;
use crate::system::window::window_id;
use crate::system::window::window_ops::windows_of_process;
use crate::system::{SharedBus, within};
use crate::tree::capabilities::action_names::click_verbs;
use crate::tree::menu::observe_menu::{rooted, scan_walk_structure};
use crate::tree::vocabulary::role_numbers::{
    COMBO_BOX, MENU, POPUP_MENU, PUSH_BUTTON, PUSH_BUTTON_MENU, TOGGLE_BUTTON,
};
use crate::tree::vocabulary::state_bits::{EXPANDED, HAS_POPUP, SHOWING, StateBits};
use crate::tree::{AtspiElement, ObservedRead, WalkedNode, call, get_live_actions};

const ACTION_INTERFACE: &str = "org.a11y.atspi.Action";
const SCAN_NODES: usize = 600;
const OWNER_NODES: usize = 64;
const POPUP_CONTAINER_CHILDREN: usize = 16;
const CLOSE_SETTLE: Duration = Duration::from_millis(1000);
const CLOSE_STEP: Duration = Duration::from_millis(50);
const FOCUS_CHECKS: [&str; 2] = ["window_active", "exact_window_focus"];
const SUGGESTION: &str = "Refresh the snapshot and check whether the popup is still open.";
const CLOSE_LABEL: &str = "Action.DoAction";

pub(super) fn is_plain_escape(combo: &KeyCombo) -> bool {
    combo.modifiers.is_empty() && combo.key.eq_ignore_ascii_case("escape")
}

pub(super) fn is_focus_refusal(error: &AdapterError) -> bool {
    error
        .details
        .as_ref()
        .and_then(|details| details.get("check"))
        .and_then(|check| check.as_str())
        .is_some_and(|check| FOCUS_CHECKS.contains(&check))
}

fn bits(node: &WalkedNode<ObservedRead>) -> Option<StateBits<'_>> {
    node.payload
        .node
        .traits
        .states
        .present()
        .map(|words| StateBits::new(words))
}

fn role(node: &WalkedNode<ObservedRead>) -> Option<u32> {
    node.payload.node.traits.role.present().copied()
}

fn showing_popup(node: &WalkedNode<ObservedRead>) -> bool {
    matches!(role(node), Some(MENU | POPUP_MENU))
        && bits(node).is_some_and(|bits| bits.has(SHOWING))
}

/// Whether the node owns a popup and the popup is open now. The owner roles
/// are the ones whose expansion is a popup: a combo box, and a button that
/// has the popup state. An owner is open when it reports `EXPANDED` or when a
/// popup menu below it shows. A tree row, an expander or a table cell is
/// never an owner: Escape does not collapse a tree.
pub(super) fn is_open_owner(node: &WalkedNode<ObservedRead>) -> bool {
    let Some(bits) = bits(node) else {
        return false;
    };
    let owner = match role(node) {
        Some(COMBO_BOX) => true,
        Some(PUSH_BUTTON | PUSH_BUTTON_MENU | TOGGLE_BUTTON) => bits.has(HAS_POPUP),
        _ => false,
    };
    owner && (bits.has(EXPANDED) || node.children.iter().any(showing_popup))
}

fn deepest_open_owner<'a>(
    node: &'a WalkedNode<ObservedRead>,
    depth: usize,
    best: &mut Option<(usize, &'a WalkedNode<ObservedRead>)>,
) {
    if is_open_owner(node) && best.is_none_or(|(deepest, _)| depth > deepest) {
        *best = Some((depth, node));
    }
    for child in &node.children {
        deepest_open_owner(child, depth + 1, best);
    }
}

fn frames_of(
    bus: &SharedBus,
    pid: u32,
    window: Option<&WindowInfo>,
    deadline: Deadline,
) -> Result<Vec<AtspiElement>, AdapterError> {
    let windows = match window {
        Some(window) => vec![window.clone()],
        None => windows_of_process(bus, pid, deadline)?.0,
    };
    let mut frames = Vec::new();
    for window in windows {
        let (name, path) = window_id::decode(&window.id)?;
        frames.push(rooted(&AtspiElement::new(name, path, pid)));
    }
    Ok(frames)
}

fn still_open(bus: &SharedBus, owner: &AtspiElement, deadline: Deadline) -> Option<bool> {
    let walk = scan_walk_structure(
        bus,
        &rooted(owner),
        OWNER_NODES,
        Some(POPUP_CONTAINER_CHILDREN),
        deadline,
    )
    .ok()?;
    Some(is_open_owner(&walk))
}

fn press_owner(
    bus: &SharedBus,
    owner: &AtspiElement,
    role: Option<u32>,
    deadline: Deadline,
) -> Result<bool, AdapterError> {
    let handle = owner.clone().into_native_handle();
    let names = get_live_actions(bus, &handle, deadline)?.unwrap_or_default();
    let Some(index) = resolve_verb(&names, click_verbs(role)) else {
        return Ok(false);
    };
    let connection = bus.get(deadline)?;
    within(deadline, async {
        call(
            connection.inner(),
            owner,
            ACTION_INTERFACE,
            "DoAction",
            &(index,),
        )
        .await
        .map(|_| true)
        .map_err(|error| crate::system::classify(&error))
    })
}

/// The refused Escape of a background application, answered by closing the
/// open popup its target window shows. `None` leaves the refusal as it was.
pub(super) fn close_open_popup(
    bus: &SharedBus,
    pid: u32,
    window: Option<&WindowInfo>,
    combo: &KeyCombo,
    deadline: Deadline,
) -> Result<Option<ActionResult>, AdapterError> {
    let mut best: Option<(usize, AtspiElement, Option<u32>)> = None;
    for frame in frames_of(bus, pid, window, deadline)? {
        let walk = scan_walk_structure(
            bus,
            &frame,
            SCAN_NODES,
            Some(POPUP_CONTAINER_CHILDREN),
            deadline,
        )?;
        let mut found = None;
        deepest_open_owner(&walk, 0, &mut found);
        if let Some((depth, node)) = found
            && best.as_ref().is_none_or(|(deepest, _, _)| depth > *deepest)
        {
            best = Some((depth, node.payload.node.element.clone(), role(node)));
        }
    }
    let Some((_, owner, owner_role)) = best else {
        return Ok(None);
    };
    let run = || {
        if !press_owner(bus, &owner, owner_role, deadline)? {
            return Ok(DeliveryOutcome::NotDelivered);
        }
        let closed = deadline.poll_until(
            CLOSE_SETTLE,
            CLOSE_STEP,
            || still_open(bus, &owner, deadline).map(|open| !open),
            |closed| *closed == Some(true),
        );
        Ok(DeliveryOutcome::observed(closed))
    };
    let mut rungs = [ChainRung {
        label: CLOSE_LABEL,
        effects: &[],
        mechanism: StepMechanism::SemanticApi,
        run: Box::new(run),
    }];
    run_semantic_chain(
        &Action::PressKey(combo.clone()),
        &mut rungs,
        deadline,
        SUGGESTION,
    )
    .map(Some)
}

#[cfg(test)]
#[path = "escape_popup_tests.rs"]
mod tests;
