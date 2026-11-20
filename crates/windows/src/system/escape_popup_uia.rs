//! The UI Automation half of `escape_popup`: the bounded popup search, the collapse and the entry point.

use super::{
    MAX_POPUP_ROOTS, PopupScan, PopupScope, PopupTree, is_plain_escape, opens_a_popup,
    refused_for_focus, scan_popup_owners, scan_timeout_error,
};
use agent_desktop_core::{
    Action, ActionResult, AdapterError, Deadline, DeliveryOutcome, InteractionPolicy, KeyCombo,
    ProcessIdentity, WindowInfo,
};
use std::time::Duration;
use uiautomation::core::{UICacheRequest, UITreeWalker};
use uiautomation::patterns::UIExpandCollapsePattern;

use crate::actions::chain::{offered, poll_until, rung};
use crate::actions::mutation::{classify_success, classify_write};
use crate::system::window::window_enum::enumerate_top_level;
use crate::system::window::window_identity::live_window_owner;
use crate::tree::automation::{automation_client, root_from_hwnd};
use crate::tree::element::UIAElement;
use crate::tree::properties::property_ids::TreeProperty;
use crate::tree::properties::{read_cached, read_one};
use crate::tree::walk::cache::build_walk_cache_request;

const COLLAPSE_LABEL: &str = "ExpandCollapsePattern.Collapse";
const COLLAPSE_SUGGESTION: &str = "Close the popup with the key while its window is in front.";
const VERIFY_TIMEOUT: Duration = Duration::from_millis(600);
const EXPANDED: i32 = 1;
const COLLAPSED: i32 = 0;
const POPUP_SCAN_PROPERTIES: [TreeProperty; 2] =
    [TreeProperty::ControlType, TreeProperty::ExpandCollapseState];

struct UiaPopupTree {
    walker: UITreeWalker,
    cache: UICacheRequest,
}

impl PopupTree for UiaPopupTree {
    type Node = UIAElement;

    fn is_open_popup_owner(&self, node: &UIAElement) -> bool {
        let (properties, _) = read_cached(node, &POPUP_SCAN_PROPERTIES);
        properties
            .get(TreeProperty::ControlType)
            .number()
            .is_some_and(opens_a_popup)
            && properties.get(TreeProperty::ExpandCollapseState).number() == Some(EXPANDED)
    }

    fn first_child(&self, node: &UIAElement) -> Option<UIAElement> {
        self.walker
            .get_first_child_build_cache(&node.0, &self.cache)
            .ok()
            .map(UIAElement::from)
    }

    fn next_sibling(&self, node: &UIAElement) -> Option<UIAElement> {
        self.walker
            .get_next_sibling_build_cache(&node.0, &self.cache)
            .ok()
            .map(UIAElement::from)
    }
}

fn prefetch_roots(
    roots: Vec<UIAElement>,
    cache: &UICacheRequest,
    deadline: Deadline,
) -> Result<Vec<UIAElement>, AdapterError> {
    let mut cached = Vec::with_capacity(roots.len());
    for root in roots.into_iter().take(MAX_POPUP_ROOTS) {
        if deadline.is_expired() {
            return Err(scan_timeout_error(deadline));
        }
        if let Ok(element) = root.0.build_updated_cache(cache) {
            cached.push(UIAElement::from(element));
        }
    }
    Ok(cached)
}

/// The deepest expanded popup owner among the elements below `roots`, found by
/// a bounded, deadline-checked incremental walk over the control view.
fn deepest_popup_owner(
    roots: Vec<UIAElement>,
    deadline: Deadline,
) -> Result<Option<UIAElement>, AdapterError> {
    if deadline.is_expired() {
        return Err(scan_timeout_error(deadline));
    }
    let Ok(client) = automation_client() else {
        return Ok(None);
    };
    let Ok(walker) = client.get_control_view_walker() else {
        return Ok(None);
    };
    let Ok(cache) = build_walk_cache_request(&client, &POPUP_SCAN_PROPERTIES) else {
        return Ok(None);
    };
    let cached_roots = prefetch_roots(roots, &cache, deadline)?;
    let tree = UiaPopupTree { walker, cache };
    match scan_popup_owners(&tree, cached_roots, deadline) {
        PopupScan::Found(owner) => Ok(Some(owner)),
        PopupScan::None => Ok(None),
        PopupScan::TimedOut => Err(scan_timeout_error(deadline)),
    }
}

fn window_roots(window: &WindowInfo, deadline: Deadline) -> Result<Vec<UIAElement>, AdapterError> {
    if deadline.is_expired() {
        return Err(scan_timeout_error(deadline));
    }
    Ok(crate::tree::surfaces::surface_root(
        agent_desktop_core::ObservationRoot::Window(window),
        agent_desktop_core::SnapshotSurface::Window,
        deadline,
    )
    .into_iter()
    .collect())
}

fn process_roots(
    process: &ProcessIdentity,
    deadline: Deadline,
) -> Result<Vec<UIAElement>, AdapterError> {
    if deadline.is_expired() {
        return Err(scan_timeout_error(deadline));
    }
    let mut handles = Vec::new();
    let _ = enumerate_top_level(|window| {
        if live_window_owner(window.handle) == Some(process.pid) {
            handles.push(window.handle);
        }
        handles.len() < MAX_POPUP_ROOTS
    });
    let mut roots = Vec::with_capacity(handles.len());
    for handle in handles {
        if deadline.is_expired() {
            return Err(scan_timeout_error(deadline));
        }
        if let Ok(element) = root_from_hwnd(handle as isize, deadline) {
            roots.push(element);
        }
    }
    Ok(roots)
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
    if deadline.is_expired() {
        return Err(scan_timeout_error(deadline));
    }
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
        PopupScope::Window(window) => window_roots(window, deadline)?,
        PopupScope::Process(process) => process_roots(process, deadline)?,
    };
    match deepest_popup_owner(roots, deadline)? {
        Some(owner) => close_popup(&owner, combo, policy, deadline),
        None => Err(refusal),
    }
}
