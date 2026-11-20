//! Closing the open popup of a window that does not hold the keyboard.
//!
//! Core asks for it after a plain Escape that the key route refused for focus.
//! A popup a person opened (a menu, a drop-down, a split-button list) can sit
//! in a window that is not in front. It has a semantic close: collapsing its
//! owner, through the same primitive `collapse` uses (`disclosure_steps`).

#[cfg(any(target_os = "windows", test))]
use agent_desktop_core::DeliverySemantics;
#[cfg(not(target_os = "windows"))]
use agent_desktop_core::{ActionResult, InteractionPolicy, ProcessIdentity, WindowInfo};
use agent_desktop_core::{AdapterError, Deadline};

#[cfg(any(target_os = "windows", test))]
pub(super) const MAX_POPUP_ROOTS: usize = 32;
#[cfg(any(target_os = "windows", test))]
pub(super) const MAX_POPUP_NODES: usize = 600;
#[cfg(any(target_os = "windows", test))]
pub(super) const MAX_POPUP_DEPTH: usize = 32;
#[cfg(any(target_os = "windows", test))]
pub(super) const MAX_POPUP_CHILDREN: usize = 64;

/// Outcome of a bounded background-Escape popup scan.
#[cfg(any(target_os = "windows", test))]
#[derive(Debug, PartialEq, Eq)]
pub(super) enum PopupScan<N> {
    Found(N),
    None,
    TimedOut,
}

/// Incremental tree reader used by [`scan_popup_owners`] so the bounded walk
/// is tested without live UI Automation.
#[cfg(any(target_os = "windows", test))]
pub(super) trait PopupTree {
    type Node;

    fn is_open_popup_owner(&self, node: &Self::Node) -> bool;
    fn first_child(&self, node: &Self::Node) -> Option<Self::Node>;
    fn next_sibling(&self, node: &Self::Node) -> Option<Self::Node>;
}

#[cfg(any(target_os = "windows", test))]
pub(super) fn scan_timeout_error(deadline: Deadline) -> AdapterError {
    deadline
        .timeout_error()
        .with_disposition(DeliverySemantics::not_delivered())
}

/// Finds the deepest open popup owner across at most `MAX_POPUP_ROOTS` roots
/// and `MAX_POPUP_NODES` nodes, consulting `deadline` before every step.
#[cfg(any(target_os = "windows", test))]
pub(super) fn scan_popup_owners<T: PopupTree>(
    tree: &T,
    roots: Vec<T::Node>,
    deadline: Deadline,
) -> PopupScan<T::Node> {
    if deadline.is_expired() {
        return PopupScan::TimedOut;
    }
    let mut visited = 0usize;
    let mut best: Option<(usize, T::Node)> = None;
    for root in roots.into_iter().take(MAX_POPUP_ROOTS) {
        let mut stack = vec![(root, 0usize)];
        while let Some((node, depth)) = stack.pop() {
            if deadline.is_expired() {
                return PopupScan::TimedOut;
            }
            if visited >= MAX_POPUP_NODES {
                return best.map_or(PopupScan::None, |(_, owner)| PopupScan::Found(owner));
            }
            visited += 1;
            let is_owner = tree.is_open_popup_owner(&node);
            if depth < MAX_POPUP_DEPTH && visited < MAX_POPUP_NODES {
                let mut children = Vec::new();
                if deadline.is_expired() {
                    return PopupScan::TimedOut;
                }
                let mut child = tree.first_child(&node);
                while let Some(current) = child {
                    let next = if children.len() + 1 < MAX_POPUP_CHILDREN {
                        if deadline.is_expired() {
                            return PopupScan::TimedOut;
                        }
                        tree.next_sibling(&current)
                    } else {
                        None
                    };
                    children.push(current);
                    child = next;
                }
                for child in children.into_iter().rev() {
                    stack.push((child, depth + 1));
                }
            }
            if is_owner && best.as_ref().is_none_or(|(deepest, _)| depth >= *deepest) {
                best = Some((depth, node));
            }
        }
    }
    best.map_or(PopupScan::None, |(_, owner)| PopupScan::Found(owner))
}

/// The control types whose expansion is a popup: menu item (50011), combo box
/// (50003) and split button (50031). A tree item or a group is never one.
fn opens_a_popup(control_type: i32) -> bool {
    [50011, 50003, 50031].contains(&control_type)
}

#[cfg(target_os = "windows")]
#[path = "escape_popup_uia.rs"]
mod uia;

#[cfg(target_os = "windows")]
pub(crate) use uia::close_transient_surface;

/// Off Windows no popup can be open.
#[cfg(not(target_os = "windows"))]
pub(crate) fn close_transient_surface(
    _process: &ProcessIdentity,
    _window: Option<&WindowInfo>,
    _policy: InteractionPolicy,
    _deadline: Deadline,
) -> Result<Option<ActionResult>, AdapterError> {
    Ok(None)
}

#[cfg(test)]
#[path = "escape_popup_tests.rs"]
mod tests;
