//! The platform's half of two boot-order tests whose shell and splash halves stay in Python
//! (tests/test_boot_order.py, tests/test_reborn_splash.py).
// SPDX-License-Identifier: GPL-2.0-only

/// `test_reborn_is_installed_last_…`: the power-off coordinator and the maintenance service list name
/// Reborn's init script by its last-place name.
#[test]
fn reborn_is_installed_last_and_everything_else_runs_before_it() {
    for (name, text) in [
        (
            "power/coordinator/steps.rs",
            include_str!("power/coordinator/steps.rs"),
        ),
        (
            "reports/maintenance/plan.rs",
            include_str!("reports/maintenance/plan.rs"),
        ),
    ] {
        assert!(text.contains("S90reborn"), "{name}");
        assert!(!text.contains("S05reborn"), "{name}");
    }
}

/// The storage setup's half of tests/test_reborn_splash.py `test_every_milestone_a_producer_…`: every
/// phase it shows during a boot-time setup is in Reborn's boot table, and every `media_setup` entry
/// of the table is one it shows. Skipped without the Reborn checkout beside this one (as there).
#[test]
fn every_milestone_a_producer_reports_is_in_the_table() {
    use crate::commands::storage::internal::splash;
    use crate::testing::{repository, Fixture};
    use std::collections::BTreeSet;
    let layout = repository().join("../Y2Reborn/docs/ui/previews/v3/boot-layout.json");
    let Ok(layout) = std::fs::read_to_string(&layout) else {
        eprintln!("skipped: Reborn boot layout not present");
        return;
    };
    let layout: serde_json::Value = serde_json::from_str(&layout).unwrap();
    let tokens: BTreeSet<&str> = layout["phases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["token"].as_str().unwrap())
        .collect();
    let fixture = Fixture::new();
    fixture.mkdir("/run/reborn-splash");
    let mut reported = BTreeSet::new();
    for step in 0..=110 {
        splash(&fixture.root, step);
        if let Some(phase) = fixture.root.read("/run/reborn-splash/phase") {
            reported.insert(phase);
        }
        fixture.remove("/run/reborn-splash/phase");
    }
    let unknown: Vec<_> = reported
        .iter()
        .filter(|p| !tokens.contains(p.as_str()))
        .collect();
    assert!(
        unknown.is_empty(),
        "the storage setup reports a milestone the splash does not know: {unknown:?}"
    );
    let unreported: Vec<_> = tokens
        .iter()
        .filter(|t| t.starts_with("media_setup") && !reported.contains(**t))
        .collect();
    assert!(
        unreported.is_empty(),
        "table entries nobody reports: {unreported:?}"
    );
}
