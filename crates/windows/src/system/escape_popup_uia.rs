//! The UI Automation half of `escape_popup`: the popup search, the collapse and the two entry points.

use super::{PopupScope, is_plain_escape, opens_a_popup, refused_for_focus};
use agent_desktop_core::{
    Action, ActionResult, AdapterError, Deadline, DeliveryOutcome, InteractionPolicy, KeyCombo,
    ProcessIdentity, WindowInfo,
};
use std::time::Duration;
use uiautomation::patterns::UIExpandCollapsePattern;
use uiautomation::types::{TreeScope, UIProperty};
use uiautomation::variants::Variant;

use crate::actions::chain::{offered, poll_until, rung};
use crate::actions::mutation::{classify_success, classify_write};
use crate::tree::element::UIAElement;
use crate::tree::properties::property_ids::TreeProperty;
use crate::tree::properties::read_one;

const COLLAPSE_LABEL: &str = "ExpandCollapsePattern.Collapse";
const COLLAPSE_SUGGESTION: &str = "Close the popup with the key while its window is in front.";
const VERIFY_TIMEOUT: Duration = Duration::from_millis(600);
const EXPANDED: i32 = 1;
const COLLAPSED: i32 = 0;

/// The deepest expanded popup owner among the elements below `roots`: a
/// document-order search, so the last match is the innermost level.
fn deepest_popup_owner(roots: &[UIAElement]) -> Option<UIAElement> {
    let client = crate::tree::automation::automation_client().ok()?;
    let expanded = client
        .create_property_condition(
            UIProperty::ExpandCollapseExpandCollapseState,
            Variant::from(EXPANDED),
            None,
        )
        .ok()?;
    let mut owner = None;
    for root in roots {
        let Ok(found) = root.0.find_all(TreeScope::Descendants, &expanded) else {
            continue;
        };
        for element in found {
            let element = UIAElement::from(element);
            if read_one(&element, TreeProperty::ControlType)
                .number()
                .is_some_and(opens_a_popup)
            {
                owner = Some(element);
            }
        }
    }
    owner
}

fn window_roots(window: &WindowInfo, deadline: Deadline) -> Vec<UIAElement> {
    crate::tree::surfaces::surface_root(
        agent_desktop_core::ObservationRoot::Window(window),
        agent_desktop_core::SnapshotSurface::Window,
        deadline,
    )
    .into_iter()
    .collect()
}

fn process_roots(process: &ProcessIdentity) -> Vec<UIAElement> {
    let Ok(client) = crate::tree::automation::automation_client() else {
        return Vec::new();
    };
    let Ok(root) = client.get_root_element() else {
        return Vec::new();
    };
    let Ok(condition) = client.create_property_condition(
        UIProperty::ProcessId,
        Variant::from(i32::try_from(u32::from(process.pid)).unwrap_or(0)),
        None,
    ) else {
        return Vec::new();
    };
    root.find_all(TreeScope::Children, &condition)
        .map(|found| found.into_iter().map(UIAElement::from).collect())
        .unwrap_or_default()
}

fn collapse(owner: &UIAElement) -> Result<bool, AdapterError> {
    match owner.0.get_pattern::<UIExpandCollapsePattern>() {
        Ok(pattern) => match pattern.collapse() {
            Ok(()) => classify_success(),
            Err(error) => classify_write("Collapse", COLLAPSE_LABEL, &error),
        },
        Err(error) => classify_write("get_pattern", COLLAPSE_LABEL, &error),
    }
}

fn close_popup(
    owner: &UIAElement,
    combo: &KeyCombo,
    policy: InteractionPolicy,
    deadline: Deadline,
) -> Result<ActionResult, AdapterError> {
    let mut run = || {
        if !collapse(owner)? {
            return Ok(DeliveryOutcome::NotDelivered);
        }
        let closed = poll_until(deadline, VERIFY_TIMEOUT, || {
            read_one(owner, TreeProperty::ExpandCollapseState).number() == Some(COLLAPSED)
        });
        Ok(DeliveryOutcome::from_delivery(true, closed))
    };
    let steps = agent_desktop_core::execute_chain(
        &mut offered([rung(COLLAPSE_LABEL, true, false, &mut run)]),
        policy,
        deadline,
        COLLAPSE_SUGGESTION,
    )?;
    Ok(ActionResult::from_execution(
        &Action::PressKey(combo.clone()),
        steps,
    ))
}

/// The key route's result, or, when it refused a plain Escape for focus and
/// the scope shows an open popup, the collapse of that popup's owner.
pub(crate) fn after_key_route(
    result: Result<ActionResult, AdapterError>,
    scope: PopupScope<'_>,
    combo: &KeyCombo,
    policy: InteractionPolicy,
    deadline: Deadline,
) -> Result<ActionResult, AdapterError> {
    let Err(refusal) = result else {
        return result;
    };
    let window_scope = matches!(scope, PopupScope::Window(_));
    if !is_plain_escape(combo) || !refused_for_focus(window_scope, &refusal) {
        return Err(refusal);
    }
    let roots = match scope {
        PopupScope::Window(window) => window_roots(window, deadline),
        PopupScope::Process(process) => process_roots(process),
    };
    match deepest_popup_owner(&roots) {
        Some(owner) => close_popup(&owner, combo, policy, deadline),
        None => Err(refusal),
    }
}
