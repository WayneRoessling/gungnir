// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A ground elevation model a host lends the ingest path, so a height stated above ground
//! level can be placed on the WGS-84 ellipsoid the picture reads (GAP-199, D-130).
//!
//! **Why an interface here, as for the geoid.** A loaded DEM is the desktop's: it is read
//! by `gungnir-data`, placed through PROJ behind that crate's `crs` feature, and held by
//! `gungnir-app`, none of which this crate may reach (`ARCHITECTURE.md` §7.1). So this
//! crate names what it needs, [`GroundElevation`], and a host that holds a placed DEM
//! lends one through a [`TerrainHandle`], the way it lends a geoid model through
//! [`crate::geoid::GeoidHandle`] (D-123). D-130 chose exactly this so no edge is added.
//!
//! A host that lends none leaves every handle unavailable **with a reason**, and a height
//! it cannot place is carried as unknown: held at 0, flagged, counted, and given a
//! vertical variance that says so (`gungnir_model::UNKNOWN_UAS_HEIGHT_BOUND_M`). The node
//! loads no terrain, so it takes that path (D-130).
//!
//! **Why a handle that changes after the feed is bound.** The desktop binds its feeds when
//! it starts, and loads its DEM on the loader thread after the geoid check settles. The
//! host sets the handle when the load settles, and every adapter holding a clone reads
//! the new state on its next report.

use std::sync::{Arc, PoisonError, RwLock};

/// The ground under a position, as a DEM gives it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroundHeight {
    /// The ground's WGS-84 ellipsoidal height, metres.
    pub ellipsoidal_m: f64,
    /// The DEM's vertical error there, one standard deviation in metres.
    pub sigma_m: f64,
}

/// A ground elevation model: the WGS-84 ellipsoidal height of the ground under a WGS-84
/// latitude and longitude.
///
/// `Send + Sync` because the adapter that asks is owned by the ingest gateway and the
/// model is shared by every feed a host binds.
pub trait GroundElevation: Send + Sync {
    /// The model's name as a report states it: the DEM's path, for example.
    fn name(&self) -> &str;

    /// The ground at a WGS-84 latitude and longitude, degrees.
    ///
    /// # Errors
    ///
    /// Why there is no ground height there, in words an operator reads: the position is
    /// not finite, lies off the DEM, or falls in a cell with no data. **Never a zero**: a
    /// ground height that is not known is an error, not sea level.
    fn ground(&self, lat_deg: f64, lon_deg: f64) -> Result<GroundHeight, String>;
}

/// What a handle holds: a model, or why there is none.
type Lent = Result<Arc<dyn GroundElevation>, String>;

/// A host's ground elevation model as the feeds it bound see it: available, or
/// unavailable with the reason, and changeable after binding (module documentation).
#[derive(Clone)]
pub struct TerrainHandle {
    lent: Arc<RwLock<Lent>>,
}

/// The reason a handle nobody set gives: the host attached no ground elevation model.
pub const NO_TERRAIN_ATTACHED: &str = "no ground elevation model is attached to this feed";

impl TerrainHandle {
    /// A handle with no model, and why.
    #[must_use]
    pub fn unavailable(reason: impl Into<String>) -> Self {
        TerrainHandle {
            lent: Arc::new(RwLock::new(Err(reason.into()))),
        }
    }

    /// Lend `model` to every adapter holding a clone of this handle.
    pub fn set_available(&self, model: Arc<dyn GroundElevation>) {
        *self.lent.write().unwrap_or_else(PoisonError::into_inner) = Ok(model);
    }

    /// Withdraw the model, if any, and say why there is none.
    pub fn set_unavailable(&self, reason: impl Into<String>) {
        *self.lent.write().unwrap_or_else(PoisonError::into_inner) = Err(reason.into());
    }

    /// The model now, or why there is none.
    ///
    /// # Errors
    ///
    /// The reason the host gave when it set the handle unavailable.
    pub fn current(&self) -> Result<Arc<dyn GroundElevation>, String> {
        self.lent
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Whether a model is lent now.
    #[must_use]
    pub fn is_available(&self) -> bool {
        self.current().is_ok()
    }
}

impl Default for TerrainHandle {
    /// Unavailable, with [`NO_TERRAIN_ATTACHED`]: a host that never lends a model gets
    /// every height above ground carried as unknown, not placed on an invented ground.
    fn default() -> Self {
        TerrainHandle::unavailable(NO_TERRAIN_ATTACHED)
    }
}

impl std::fmt::Debug for TerrainHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.current() {
            Ok(model) => write!(f, "TerrainHandle(available: {})", model.name()),
            Err(reason) => write!(f, "TerrainHandle(unavailable: {reason})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Plain;
    impl GroundElevation for Plain {
        fn name(&self) -> &'static str {
            "plain"
        }
        fn ground(&self, _: f64, _: f64) -> Result<GroundHeight, String> {
            Ok(GroundHeight {
                ellipsoidal_m: 100.0,
                sigma_m: 2.0,
            })
        }
    }

    #[test]
    fn a_default_handle_is_unavailable_and_says_why() {
        let handle = TerrainHandle::default();
        assert_eq!(handle.current().err().as_deref(), Some(NO_TERRAIN_ATTACHED));
        assert!(!handle.is_available());
    }

    /// A clone taken before the model was lent sees it afterwards: the desktop binds its
    /// feeds before its DEM has loaded.
    #[test]
    fn a_clone_taken_before_the_model_is_lent_sees_it_and_its_withdrawal() {
        let host = TerrainHandle::unavailable("still loading");
        let feed = host.clone();
        assert_eq!(feed.current().err().as_deref(), Some("still loading"));
        host.set_available(Arc::new(Plain));
        let model = feed.current().expect("lent");
        assert_eq!(model.name(), "plain");
        assert_eq!(format!("{feed:?}"), "TerrainHandle(available: plain)");
        host.set_unavailable("the DEM did not load");
        assert_eq!(
            feed.current().err().as_deref(),
            Some("the DEM did not load")
        );
    }
}
