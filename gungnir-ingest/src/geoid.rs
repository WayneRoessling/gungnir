// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A geoid model a host lends the ingest path, so a height stated above mean sea level
//! can be placed on the WGS-84 ellipsoid the picture reads (GAP-196, D-123).
//!
//! **Why an interface here and not a call into the grid.** The verified EGM2008 grid is
//! `gungnir-data`'s (`gungnir_data::geoid`, D-121), read through PROJ behind that crate's
//! `crs` feature, and `ARCHITECTURE.md` §7.1 gives this crate no edge to it -- nor should
//! it: the ingest boundary would then link libproj into every binary that ingests
//! anything. So this crate names what it needs, [`GeoidSeparation`], and a host that
//! holds a grid lends one through a [`GeoidHandle`]; the desktop does, from its
//! start-up grid check (`gungnir_app::geoid`). A host that lends none leaves every
//! handle unavailable **with a reason**, and a height it cannot correct is carried as
//! mean sea level, flagged and counted, never as an ellipsoidal one (D-124).
//!
//! **Why a handle that changes after the feed is bound.** The desktop binds its feeds
//! when it starts, and verifies the 80 MB grid on a thread of its own on the first tick.
//! The handle is how a feed bound before the answer learns it: the host sets the handle
//! once the check settles, and every adapter holding a clone reads the new state on its
//! next report. Until then the reason says the grid is still being checked.

use std::sync::{Arc, PoisonError, RwLock};

/// The geoid separation (undulation) N of one model: the height of that geoid above the
/// WGS-84 ellipsoid, so a height H above it is the ellipsoidal height H + N.
///
/// `Send + Sync` because the adapter that asks is owned by the ingest gateway and the
/// model is shared by every feed a host binds.
pub trait GeoidSeparation: Send + Sync {
    /// The model's name as a report states it, for example `"EGM2008"`.
    fn model(&self) -> &'static str;

    /// N at a WGS-84 latitude and longitude, degrees.
    ///
    /// # Errors
    ///
    /// Why there is no N there, in words an operator reads: the position is not finite
    /// or lies off the model's grid, or the model could not be read. **Never a zero**:
    /// a separation that is not known is an error, not the absence of one.
    fn separation_m(&self, lat_deg: f64, lon_deg: f64) -> Result<f64, String>;
}

/// What a handle holds: a model, or why there is none.
type Lent = Result<Arc<dyn GeoidSeparation>, String>;

/// A host's geoid model as the feeds it bound see it: available, or unavailable with
/// the reason, and changeable after binding (module documentation).
#[derive(Clone)]
pub struct GeoidHandle {
    lent: Arc<RwLock<Lent>>,
}

/// The reason a handle nobody set gives: the host attached no geoid model at all.
pub const NO_GEOID_ATTACHED: &str = "no geoid model is attached to this feed";

impl GeoidHandle {
    /// A handle with no model, and why.
    #[must_use]
    pub fn unavailable(reason: impl Into<String>) -> Self {
        GeoidHandle {
            lent: Arc::new(RwLock::new(Err(reason.into()))),
        }
    }

    /// Lend `model` to every adapter holding a clone of this handle.
    pub fn set_available(&self, model: Arc<dyn GeoidSeparation>) {
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
    pub fn current(&self) -> Result<Arc<dyn GeoidSeparation>, String> {
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

impl Default for GeoidHandle {
    /// Unavailable, with [`NO_GEOID_ATTACHED`]: a host that never lends a model gets
    /// every height it cannot correct flagged, not silently passed through.
    fn default() -> Self {
        GeoidHandle::unavailable(NO_GEOID_ATTACHED)
    }
}

impl std::fmt::Debug for GeoidHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.current() {
            Ok(model) => write!(f, "GeoidHandle(available: {})", model.model()),
            Err(reason) => write!(f, "GeoidHandle(unavailable: {reason})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Flat;
    impl GeoidSeparation for Flat {
        fn model(&self) -> &'static str {
            "flat"
        }
        fn separation_m(&self, _: f64, _: f64) -> Result<f64, String> {
            Ok(10.0)
        }
    }

    #[test]
    fn a_default_handle_is_unavailable_and_says_why() {
        let handle = GeoidHandle::default();
        assert_eq!(handle.current().err().as_deref(), Some(NO_GEOID_ATTACHED));
        assert!(!handle.is_available());
    }

    /// A clone taken before the model was lent sees it afterwards: the desktop binds its
    /// feeds before the grid check settles.
    #[test]
    fn a_clone_taken_before_the_model_is_lent_sees_it_and_its_withdrawal() {
        let host = GeoidHandle::unavailable("still checking");
        let feed = host.clone();
        assert_eq!(feed.current().err().as_deref(), Some("still checking"));
        host.set_available(Arc::new(Flat));
        let model = feed.current().expect("lent");
        assert_eq!(model.model(), "flat");
        assert_eq!(format!("{feed:?}"), "GeoidHandle(available: flat)");
        host.set_unavailable("grid refused");
        assert_eq!(feed.current().err().as_deref(), Some("grid refused"));
    }
}
