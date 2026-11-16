//! The effect witness around an action whose effect lands outside the
//! target's own state: a click, a select. The adapter reads a witness before
//! the action; the dispatch result chooses what happens next, as one of three
//! explicit steps; a strong change upgrades a delivery to verified, and the
//! verdict and the changes are reported. A delivered result never becomes
//! undelivered, and the witness never makes an error of its own.

use std::time::{Duration, Instant};

use crate::actions::{
    effect_witness::{EffectWitness, verdict},
    observed_effect::ObservedEffect,
    witness_change::WitnessChange,
};
use crate::{
    ActionRequest, ActionResult, AdapterError, DeliverySemantics, ErrorCode, InteractionLease,
    NativeHandle, PlatformAdapter,
};

/// The settle time after an unclear answer, counted from the dispatch.
const SETTLE: Duration = Duration::from_millis(400);
const POLL: Duration = Duration::from_millis(50);

/// What the witness does once the action was dispatched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WitnessStep {
    /// Nothing was sent, or the adapter already proved the effect: no read.
    Leave,
    /// Sent without proof: one read, no wait.
    ReadOnce,
    /// The platform's answer was unclear: read until the settle time ends.
    Settle,
}

pub(crate) fn witness_step(dispatch: &Result<ActionResult, AdapterError>) -> WitnessStep {
    match dispatch {
        Ok(result) if result.disposition() == DeliverySemantics::delivered_unverified() => {
            WitnessStep::ReadOnce
        }
        Err(error) if error.disposition == DeliverySemantics::uncertain() => WitnessStep::Settle,
        Ok(_) | Err(_) => WitnessStep::Leave,
    }
}

pub(crate) fn execute_witnessed_action(
    adapter: &dyn PlatformAdapter,
    handle: &NativeHandle,
    request: ActionRequest,
    lease: &InteractionLease,
) -> Result<ActionResult, AdapterError> {
    let deadline = lease.deadline();
    let before = adapter.read_effect_witness(handle, deadline).ok();
    let dispatched = Instant::now();
    let dispatch = adapter.execute_action(handle, request, lease);
    let Some(before) = before else {
        return dispatch;
    };
    let read = || after_changes(adapter.read_effect_witness(handle, deadline), &before);
    match witness_step(&dispatch) {
        WitnessStep::Leave => dispatch.map(adapter_proof),
        WitnessStep::ReadOnce => match read() {
            Some(changes) => dispatch.map(|result| with_changes(result, &changes)),
            None => dispatch,
        },
        WitnessStep::Settle => {
            let mut changes = read();
            while !changes.as_deref().is_some_and(has_strong)
                && dispatched.elapsed() < SETTLE
                && deadline.remaining() > POLL
            {
                crate::wait::deadline::sleep(POLL);
                changes = read();
            }
            match changes {
                Some(changes) => dispatch.map_err(|error| error_with_changes(error, &changes)),
                None => dispatch,
            }
        }
    }
}

/// The changes since `before`, or none when the witness could not be read.
/// A target that is gone after the action was closed or replaced by it.
fn after_changes(
    after: Result<EffectWitness, AdapterError>,
    before: &EffectWitness,
) -> Option<Vec<WitnessChange>> {
    match after {
        Ok(after) => Some(after.changes_since(before)),
        Err(error) if error.code == ErrorCode::StaleRef => Some(vec![WitnessChange::TargetGone]),
        Err(_) => None,
    }
}

fn has_strong(changes: &[WitnessChange]) -> bool {
    changes.iter().any(|change| change.is_strong())
}

fn adapter_proof(result: ActionResult) -> ActionResult {
    if result.disposition() == DeliverySemantics::delivered_verified() {
        result.with_effect(ObservedEffect::Observed)
    } else {
        result
    }
}

fn with_changes(result: ActionResult, changes: &[WitnessChange]) -> ActionResult {
    let effect = verdict(changes);
    let result = if effect == ObservedEffect::Observed {
        result.with_verified_delivery()
    } else {
        result
    };
    let mut result = result.with_effect(effect);
    if !changes.is_empty() {
        let details = result.details.get_or_insert_with(|| serde_json::json!({}));
        if let Some(details) = details.as_object_mut() {
            details.insert("witness_diff".into(), serde_json::json!(changes));
        }
    }
    result
}

/// An unclear answer with a strong change was delivered: the error stays, as
/// the platform reported it, but its disposition says what the witness saw.
fn error_with_changes(mut error: AdapterError, changes: &[WitnessChange]) -> AdapterError {
    let effect = verdict(changes);
    if effect == ObservedEffect::Observed {
        error.disposition = DeliverySemantics::delivered_verified();
    }
    let details = error.details.get_or_insert_with(|| serde_json::json!({}));
    if let Some(details) = details.as_object_mut() {
        details.insert("effect".into(), serde_json::json!(effect));
        if !changes.is_empty() {
            details.insert("witness_diff".into(), serde_json::json!(changes));
        }
    }
    error
}

#[cfg(test)]
#[path = "witnessed_action_tests.rs"]
mod tests;
