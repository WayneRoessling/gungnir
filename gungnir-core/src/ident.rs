//! Identifier and lifecycle-status primitives shared by the whole workspace.
//!
//! These live here, in the lowest crate, so that `gungnir-track` (which owns the
//! lifecycle state machine), `gungnir-allocation` (which only needs to name a track),
//! and `gungnir-model` (the canonical operational model) all use the *same* types
//! rather than each defining a look-alike -- agentic-coding-standards.md §1.2.
//! `gungnir-track` and `gungnir-model` re-export them; nothing redefines them.

/// Session-local track identifier assigned by the track manager. Never reused while
/// the track is active (a `gungnir-testkit` invariant). Cross-session identity is
/// `gungnir_model::identity::GlobalEntityId`, resolved by `gungnir-identity`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct TrackId(pub u64);

/// Track lifecycle state, per the verification table's "Track lifecycle
/// (init/confirm/coast/delete)" row. Transitions are owned by `gungnir-track`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TrackStatus {
    Tentative,
    Confirmed,
    Coasting,
    Deleted,
}

/// Identifier of a taskable resource (an interceptor, a sensor, a platform) as seen
/// by `gungnir-allocation` and everything above it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct ResourceId(pub u32);
