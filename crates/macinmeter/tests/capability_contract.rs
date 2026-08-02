//! Fixes the current stable capability catalog snapshot so support-matrix
//! drift between the Rust catalog, adapters, and documentation is caught by
//! tests instead of review.

use macinmeter::capabilities;
use serde_json::{Value, json};

#[test]
fn stable_catalog_snapshot_is_fixed() {
    let snapshot = capabilities();

    let stable: Vec<(&str, &str)> = snapshot
        .routes
        .iter()
        .filter(|route| route.status == "stable")
        .map(|route| (route.container.as_str(), route.codec.as_str()))
        .collect();
    assert_eq!(
        stable,
        [
            ("wave", "pcm_integer"),
            ("wave", "pcm_float"),
            ("flac", "flac"),
            ("aiff", "pcm_integer"),
            ("mp4", "alac"),
        ]
    );

    assert_eq!(
        snapshot.stable_discovery_extensions,
        ["aif", "aiff", "flac", "m4a", "mp4", "wav", "wave"]
    );

    for route in &snapshot.routes {
        assert!(
            ["planned", "experimental", "stable", "unavailable"].contains(&route.status.as_str()),
            "unknown catalog status {}",
            route.status
        );
        assert_eq!(route.backend, "symphonia");
        if route.status != "stable" {
            assert!(
                route.discovery_extensions.is_empty(),
                "{}/{} must not contribute discovery extensions",
                route.container,
                route.codec
            );
        } else {
            assert!(
                !route.limitations.is_empty(),
                "stable {}/{} must state its limitations",
                route.container,
                route.codec
            );
        }
    }
}

#[test]
fn capability_snapshot_serializes_as_a_forward_extensible_document() {
    let document = serde_json::to_value(capabilities()).expect("snapshot must serialize");

    let extensions = &document["stableDiscoveryExtensions"];
    assert_eq!(
        *extensions,
        json!(["aif", "aiff", "flac", "m4a", "mp4", "wav", "wave"]),
        "picker extension list drifted"
    );

    let routes = document["routes"].as_array().expect("routes array");
    assert!(routes.len() >= 4);
    for route in routes {
        for key in [
            "container",
            "codec",
            "status",
            "backend",
            "discoveryExtensions",
            "limitations",
        ] {
            assert!(
                !matches!(route[key], Value::Null),
                "route field {key} must be present"
            );
        }
        assert!(route["container"].is_string());
        assert!(route["codec"].is_string());
        assert!(route["status"].is_string());
    }
}

#[test]
fn discovery_only_follows_stable_catalog_extensions() {
    let root = tempfile::tempdir().unwrap();
    for name in [
        "a.wav",
        "b.wave",
        "c.flac",
        "d.aif",
        "e.aiff",
        "f.WAV",
        "skip.mp3",
        "skip.aifc",
        "skip.ogg",
        "g.m4a",
        "h.mp4",
        "i.M4A",
    ] {
        std::fs::write(root.path().join(name), b"x").unwrap();
    }
    let discovered = macinmeter::Application::new()
        .discover_inputs(&[root.path().to_path_buf()], false)
        .unwrap();
    let names: Vec<String> = discovered
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        [
            "a.wav", "b.wave", "c.flac", "d.aif", "e.aiff", "f.WAV", "g.m4a", "h.mp4", "i.M4A"
        ]
    );
}
