use super::application_surface_tests::MenuAdapter;
use crate::{Deadline, RefEntry, SnapshotOwner, SnapshotSurface, WindowInfo};
fn popover_owner_surface_window() -> WindowInfo {
    WindowInfo {
        id: "w-77".into(),
        title: "Filters".into(),
        app: "MenuApp".into(),
        pid: crate::ProcessId::new(42),
        process_instance: Some("instance-42".into()),
        bounds: None,
        state: Default::default(),
    }
}

fn popover_owners(adapter: &MenuAdapter) -> Vec<SnapshotOwner> {
    crate::snapshot::surface_target::resolve_owners(
        adapter,
        Some("MenuApp"),
        None,
        SnapshotSurface::Popover,
        Deadline::standard().unwrap(),
    )
    .unwrap()
}

#[test]
fn a_surface_the_adapter_presents_in_its_own_window_is_owned_by_that_window() {
    let mut adapter = MenuAdapter::windowless();
    adapter.surface_window = Some(popover_owner_surface_window());

    let owners = popover_owners(&adapter);

    assert!(
        matches!(&owners[..], [SnapshotOwner::Window(window)] if window.id == "w-77"),
        "a surface presented by a window is captured at that window: {owners:?}"
    );
}

#[test]
fn a_surface_with_no_window_of_its_own_stays_application_owned() {
    let owners = popover_owners(&MenuAdapter::windowless());

    assert!(
        matches!(&owners[..], [SnapshotOwner::Application(_)]),
        "a surface with no window stays application-owned: {owners:?}"
    );
}

fn popover_entry(window_id: &str) -> RefEntry {
    let mut entry = crate::adapter::minimal_ref_entry("button", None);
    entry.process.pid = crate::ProcessId::new(42);
    entry.process.process_instance = Some("instance-42".into());
    entry.source.source_app = Some("MenuApp".into());
    entry.source.source_surface = SnapshotSurface::Popover;
    entry.source.source_window_id = Some(window_id.into());
    entry.source.source_window_title = Some("Filters".into());
    entry
}

#[test]
fn a_recorded_popover_reference_resolves_to_the_popover_window_it_named() {
    let mut adapter = MenuAdapter::windowless();
    adapter.surface_window = Some(popover_owner_surface_window());

    let window = crate::refs::resolve_saved_ref::recorded_window(
        &adapter,
        &popover_entry("w-77"),
        Deadline::standard().unwrap(),
    )
    .unwrap();

    assert_eq!(window.id, "w-77");
    assert_eq!(window.pid, crate::ProcessId::new(42));
    assert_eq!(window.process_instance.as_deref(), Some("instance-42"));
}

#[test]
fn a_recorded_popover_reference_to_a_closed_popover_is_stale() {
    let mut adapter = MenuAdapter::windowless();
    adapter.surface_window = Some(popover_owner_surface_window());

    let error = crate::refs::resolve_saved_ref::recorded_window(
        &adapter,
        &popover_entry("w-99"),
        Deadline::standard().unwrap(),
    )
    .unwrap_err();

    assert_eq!(error.code.as_str(), "STALE_REF");
}

#[test]
fn naming_the_surface_window_reads_it_even_though_it_is_not_a_listed_window() {
    let mut adapter = MenuAdapter::windowless();
    adapter.surface_window = Some(popover_owner_surface_window());

    let owners = crate::snapshot::surface_target::resolve_owners(
        &adapter,
        Some("MenuApp"),
        Some("w-77"),
        SnapshotSurface::Popover,
        Deadline::standard().unwrap(),
    )
    .unwrap();

    assert!(
        matches!(&owners[..], [SnapshotOwner::Window(window)] if window.id == "w-77"),
        "the named surface window is captured at that window: {owners:?}"
    );
}
