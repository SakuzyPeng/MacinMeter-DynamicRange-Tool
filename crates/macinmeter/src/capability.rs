//! Read-only capability query over the native codec catalog.
//!
//! This is an independent application API, separate from the schema-v4
//! analysis wire envelope. Container, codec, and status identifiers are
//! forward-extensible strings: consumers must tolerate values they do not
//! know rather than maintaining their own capability union.

use macinmeter_codecs::NATIVE_CAPABILITY_CATALOG;
use serde::Serialize;
use std::collections::BTreeSet;

/// One native decode route as seen by adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityRoute {
    pub container: String,
    pub codec: String,
    pub status: String,
    pub backend: String,
    pub discovery_extensions: Vec<String>,
    pub limitations: Vec<String>,
}

/// The full read-only capability snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitySnapshot {
    pub routes: Vec<CapabilityRoute>,
    /// Sorted, deduplicated lowercase extensions of stable routes; the only
    /// list adapters may use to seed pickers or discovery filters.
    pub stable_discovery_extensions: Vec<String>,
}

/// Return the current native capability snapshot.
pub fn capabilities() -> CapabilitySnapshot {
    let routes = NATIVE_CAPABILITY_CATALOG
        .iter()
        .map(|route| CapabilityRoute {
            container: route.container.to_owned(),
            codec: route.codec.to_owned(),
            status: route.status.as_str().to_owned(),
            backend: route.backend.to_owned(),
            discovery_extensions: route
                .discovery_extensions
                .iter()
                .map(|extension| (*extension).to_owned())
                .collect(),
            limitations: route
                .limitations
                .iter()
                .map(|limitation| (*limitation).to_owned())
                .collect(),
        })
        .collect();
    let stable_discovery_extensions = macinmeter_codecs::stable_discovery_extensions()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(str::to_owned)
        .collect();
    CapabilitySnapshot {
        routes,
        stable_discovery_extensions,
    }
}

/// True when discovery may consider a file with this extension.
pub(crate) fn is_stable_discovery_extension(extension: &str) -> bool {
    macinmeter_codecs::stable_discovery_extensions()
        .any(|stable| extension.eq_ignore_ascii_case(stable))
}

// Re-exported so library users can reason about catalog statuses without
// depending on macinmeter-codecs directly.
pub use macinmeter_codecs::{CapabilityStatus, NativeRouteCapability};
