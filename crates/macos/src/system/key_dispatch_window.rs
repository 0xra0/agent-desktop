//! Key delivery to a named window, and the surface close a refused Escape
//! answers.
//!
//! Core's window route needs the named window to hold the exact focus. A menu
//! or a popover a person opened can sit in a window that is not focused, and
//! the key is then refused before anything is sent. The surface still has a
//! semantic close: `AXCancel` on the menu or popover itself. The states, in
//! this order: the key route delivers, unchanged; it refuses for focus and a
//! plain Escape finds an open surface in that window or its application, which
//! is cancelled and read back; otherwise the refusal stays as it was.

use agent_desktop_core::{
    ActionResult, AdapterError, Deadline, DeliverySemantics, InteractionLease, InteractionPolicy,
    KeyCombo, SystemOps, WindowInfo,
};

use super::{CANCEL, delivery_result, semantic_step, simple_key_action, surface_closed};
use crate::tree::AXElement;

/// A key for a named window: core's focus-gated delivery, then, for a plain
/// Escape that gate refused, the close of the menu or popover showing in that
/// window or its application.
pub(crate) fn press_for_window<A: SystemOps + ?Sized>(
    adapter: &A,
    window: &WindowInfo,
    combo: &KeyCombo,
    policy: InteractionPolicy,
    lease: &InteractionLease,
) -> Result<ActionResult, AdapterError> {
    match agent_desktop_core::press_key_in_focused_window(adapter, window, combo, policy, lease) {
        Err(refusal) if answers_with_surface_close(combo, &refusal) => {
            answer_with_surface_close(refusal, || close_window_surface(window, lease.deadline()))
        }
        other => other,
    }
}

/// Whether a surface close answers this refusal: the key is a plain Escape,
/// which cancels the open surface, and the window focus gate refused before
/// anything was sent.
fn answers_with_surface_close(combo: &KeyCombo, refusal: &AdapterError) -> bool {
    simple_key_action(combo) == Some(CANCEL) && is_focus_refusal(refusal)
}

fn is_focus_refusal(error: &AdapterError) -> bool {
    let details = error.details.as_ref();
    error.disposition == DeliverySemantics::not_delivered()
        && details.and_then(|details| details["physical_delivery_started"].as_bool()) == Some(false)
        && details.and_then(|details| details["check"].as_str()) == Some("window_active")
}

/// The surface close is the answer when it closed something; with nothing open
/// the refusal stands, and an attempt that failed keeps its own error.
fn answer_with_surface_close(
    refusal: AdapterError,
    close: impl FnOnce() -> Result<Option<ActionResult>, AdapterError>,
) -> Result<ActionResult, AdapterError> {
    match close()? {
        Some(closed) => Ok(closed),
        None => Err(refusal),
    }
}

/// Closes the menu or popover the named window or its application shows, and
/// reads that it closed. `Ok(None)` when none is open, or when the surface does
/// not take a cancel action.
fn close_window_surface(
    window: &WindowInfo,
    deadline: Deadline,
) -> Result<Option<ActionResult>, AdapterError> {
    let Some(surface) = open_surface_in_window(window, deadline) else {
        return Ok(None);
    };
    if !crate::actions::ax_helpers::try_ax_action_or_err(&surface, CANCEL, deadline)? {
        return Ok(None);
    }
    let closed = surface_closed(&surface, deadline);
    Ok(Some(delivery_result(
        semantic_step(CANCEL).with_verified(closed),
    )))
}

/// The menu or popover the named window or its application shows. A read that
/// cannot finish finds none, so the Escape keeps the refusal it had.
fn open_surface_in_window(window: &WindowInfo, deadline: Deadline) -> Option<AXElement> {
    let pid = crate::system::process::process_identity::to_pid_t(window.pid).ok()?;
    crate::tree::surfaces::transient_surface_for_window(pid, window, deadline)
        .ok()
        .flatten()
}

#[cfg(test)]
#[path = "key_dispatch_window_tests.rs"]
mod tests;
