//! Regression gate for the committed environment surface.
//!
//! The surface is the shim specification for a browser-free signer: every property here must be
//! provided by any embedded-JS or native-interpreter implementation. It was produced by a headless
//! run (`scripts/headless/emit-surface.mjs`), not by a browser.

use std::path::PathBuf;

use ttl_sign_lab::{
    environment_surface_json, missing_shim_coverage, read_environment_surface, SurfaceRoot,
    ENVIRONMENT_SURFACE_VERSION,
};

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/research/environment-surface-v1.json")
}

#[test]
fn the_committed_surface_parses_and_is_canonical() {
    let document = read_environment_surface(fixture_path()).expect("surface fixture parses");
    assert_eq!(document.surface_version, ENVIRONMENT_SURFACE_VERSION);
    let committed = std::fs::read_to_string(fixture_path()).unwrap();
    assert_eq!(
        environment_surface_json(&document),
        committed,
        "the committed surface is not in canonical form"
    );
}

/// A surface with a failed trap is not a shim specification. This asserts the committed one is
/// complete, so it can be used as the Phase 2 gate.
#[test]
fn every_root_was_instrumented() {
    let document = read_environment_surface(fixture_path()).unwrap();
    assert!(
        document.uninstrumented_roots().is_empty(),
        "an uninstrumented root makes the surface unusable as a shim spec: {:?}",
        document.uninstrumented_roots()
    );
}

/// The properties the signing path is known to depend on must be present. These are the ones a
/// shim cannot omit without changing the signature.
#[test]
fn the_known_signing_inputs_are_recorded() {
    let document = read_environment_surface(fixture_path()).unwrap();
    let paths = document.paths();
    for required in [
        // msToken is read verbatim from this key.
        "localStorage.getItem",
        "document.cookie",
        "navigator.userAgent",
        "location.href",
    ] {
        assert!(
            paths.contains(&required),
            "{required} is missing from the surface"
        );
    }
}

/// The gate Phase 2 runs against: a shim implementing nothing is missing everything, and one
/// implementing the recorded surface is missing nothing.
#[test]
fn coverage_is_measured_against_the_recorded_surface() {
    let document = read_environment_surface(fixture_path()).unwrap();
    assert_eq!(
        missing_shim_coverage(&document, &[] as &[&str]).len(),
        document.properties.len(),
        "an empty shim must be missing every property"
    );

    let everything: Vec<String> = document.paths().iter().map(|p| p.to_string()).collect();
    assert!(
        missing_shim_coverage(&document, &everything).is_empty(),
        "a shim covering the surface must be missing nothing"
    );
}

/// The surface is small enough to be worth stating: this is the whole browser dependency.
#[test]
fn the_surface_is_bounded() {
    let document = read_environment_surface(fixture_path()).unwrap();
    assert!(
        document.properties.len() < 200,
        "surface grew to {}; re-read it before treating it as a small shim",
        document.properties.len()
    );
    let non_window = document
        .properties
        .iter()
        .filter(|p| !matches!(p.root, SurfaceRoot::Window | SurfaceRoot::Other))
        .count();
    assert!(non_window > 0 && non_window < 40);
}

/// No recorded value may reach the fixture — only paths, counts, types, and lengths.
#[test]
fn the_surface_carries_no_values() {
    let committed = std::fs::read_to_string(fixture_path()).unwrap();
    for forbidden in [
        "msToken=",
        "ttwid=",
        "sessionid",
        "X-Gnarly=",
        "X-Dynosaur=",
    ] {
        assert!(
            !committed.contains(forbidden),
            "surface contains {forbidden}"
        );
    }
}
