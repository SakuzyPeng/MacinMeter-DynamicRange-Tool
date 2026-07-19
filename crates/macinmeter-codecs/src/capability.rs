//! The single Rust source of truth for native decode capabilities.
//!
//! Discovery, the application capability query, the Tauri picker, and the
//! product capability snapshot tests all consume this catalog. Container and
//! codec identifiers are stable strings so `planned` routes can be described
//! without growing the schema-v3 report enums; for `stable` routes they must
//! equal the serde identifiers of the corresponding domain enums, which a
//! product test enforces.

/// Stability status of one native container/codec route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityStatus {
    /// Named as a future evaluation candidate; not decodable today.
    Planned,
    /// Decodable behind explicit opt-in, without a stable support claim.
    Experimental,
    /// Part of the declared, evidence-backed native matrix.
    Stable,
    /// Recognized but deliberately rejected.
    Unavailable,
}

impl CapabilityStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Experimental => "experimental",
            Self::Stable => "stable",
            Self::Unavailable => "unavailable",
        }
    }
}

/// One native decode route in the capability catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeRouteCapability {
    pub container: &'static str,
    pub codec: &'static str,
    pub status: CapabilityStatus,
    pub backend: &'static str,
    /// Lowercase extensions the discovery layer may use to find inputs.
    /// Extensions are never trusted during probing.
    pub discovery_extensions: &'static [&'static str],
    /// Key limitations a caller must know before relying on the route.
    pub limitations: &'static [&'static str],
}

const BACKEND: &str = "symphonia";
const MAX_CHANNELS_LIMIT: &str = "analysis supports at most 64 channels";

/// The full native capability catalog, stable routes first.
///
/// `planned` entries record the ADR-0003 §9 evaluation order only; they never
/// enter default discovery and are not a support commitment.
pub const NATIVE_CAPABILITY_CATALOG: &[NativeRouteCapability] = &[
    NativeRouteCapability {
        container: "wave",
        codec: "pcm_integer",
        status: CapabilityStatus::Stable,
        backend: BACKEND,
        discovery_extensions: &["wav", "wave"],
        limitations: &[
            "classic RIFF format tag 1 only; WAVE_FORMAT_EXTENSIBLE is rejected at probe",
            "8/16/24/32-bit linear PCM",
            MAX_CHANNELS_LIMIT,
        ],
    },
    NativeRouteCapability {
        container: "wave",
        codec: "pcm_float",
        status: CapabilityStatus::Stable,
        backend: BACKEND,
        discovery_extensions: &["wav", "wave"],
        limitations: &[
            "classic RIFF format tag 3 only; WAVE_FORMAT_EXTENSIBLE is rejected at probe",
            "IEEE float32/float64 PCM",
            MAX_CHANNELS_LIMIT,
        ],
    },
    NativeRouteCapability {
        container: "flac",
        codec: "flac",
        status: CapabilityStatus::Stable,
        backend: BACKEND,
        discovery_extensions: &["flac"],
        limitations: &[
            "native FLAC container only; Ogg FLAC is not probed",
            "STREAMINFO must declare a nonzero total sample count",
            MAX_CHANNELS_LIMIT,
        ],
    },
    NativeRouteCapability {
        container: "aiff",
        codec: "pcm_integer",
        status: CapabilityStatus::Stable,
        backend: BACKEND,
        discovery_extensions: &["aif", "aiff"],
        limitations: &[
            "exactly 18-byte COMM and zero SSND offset/block size",
            "finite positive integral sample rates representable as u32",
            "8/16/24/32-bit big-endian linear PCM",
            MAX_CHANNELS_LIMIT,
        ],
    },
    NativeRouteCapability {
        container: "aifc",
        codec: "pcm_integer",
        status: CapabilityStatus::Planned,
        backend: BACKEND,
        discovery_extensions: &[],
        limitations: &["first ADR-0003 §9 evaluation candidate; probe currently rejects AIFC"],
    },
    NativeRouteCapability {
        container: "mp4",
        codec: "alac",
        status: CapabilityStatus::Planned,
        backend: BACKEND,
        discovery_extensions: &[],
        limitations: &["ADR-0003 §9 evaluation candidate; not decodable today"],
    },
    NativeRouteCapability {
        container: "mpeg",
        codec: "mp3",
        status: CapabilityStatus::Planned,
        backend: BACKEND,
        discovery_extensions: &[],
        limitations: &["ADR-0003 §9 evaluation candidate; not decodable today"],
    },
    NativeRouteCapability {
        container: "ogg",
        codec: "vorbis",
        status: CapabilityStatus::Planned,
        backend: BACKEND,
        discovery_extensions: &[],
        limitations: &["ADR-0003 §9 evaluation candidate; not decodable today"],
    },
    NativeRouteCapability {
        container: "mp4",
        codec: "aac",
        status: CapabilityStatus::Planned,
        backend: BACKEND,
        discovery_extensions: &[],
        limitations: &["ADR-0003 §9 evaluation candidate; not decodable today"],
    },
];

/// Lowercase extensions of stable routes, in catalog order and with
/// duplicates preserved; callers that need a set must deduplicate.
pub fn stable_discovery_extensions() -> impl Iterator<Item = &'static str> {
    NATIVE_CAPABILITY_CATALOG
        .iter()
        .filter(|route| route.status == CapabilityStatus::Stable)
        .flat_map(|route| route.discovery_extensions.iter().copied())
}
