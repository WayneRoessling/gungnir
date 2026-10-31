// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Every PROJ call this crate makes, one at a time across the process (GAP-261).
//!
//! **Why one lock.** `proj` 0.31's `Drop` for `Proj` calls `proj_destroy`,
//! `proj_context_destroy` and then `proj_cleanup`. PROJ 9.6.2's `proj_cleanup`
//! (`src/malloc.cpp`) closes the database of `pj_get_default_ctx()`'s context and clears
//! PROJ's init-file and grid-name caches, with no lock of its own around the default
//! context; and `proj_context_create`, which every `Proj::new` and `Proj::new_known_crs`
//! starts with, copies that same default context. PROJ documents `proj_cleanup` for
//! process end, once nothing else is using PROJ. Here a `Proj` is built and dropped per
//! conversion, on whatever thread converts, while the geoid lookup thread
//! (`crate::geoid::UndulationService`) holds one for the life of the process, so one
//! thread's drop ran under another's construction or conversion and the heap corrupted:
//! the `geoid` test binary on eight test threads aborted in about a third of its runs,
//! with one thread in `sqlite3BtreeClose` on PROJ's database while another parsed SQL on
//! it.
//!
//! [`LockedProj`] is the only way this crate holds a PROJ object, and its construction,
//! each conversion, its WKT and its drop each hold [`PROJ`] for their whole PROJ call. No
//! two PROJ calls overlap, so no `proj_cleanup` runs under another.
//!
//! **Why a `proj_cleanup` between two calls is harmless to an object kept alive.** What
//! it clears is the default context's database (each `Proj` has a context of its own,
//! with its own database handle), the cache of `+init` files, the network chunk cache
//! (`FileManager::clearMemoryCache`; no grid here comes over the network), and the sets
//! of grid *names* `hgridshift`, `vgridshift` and `gridshift` have opened before. A live
//! pipeline owns its grids, so the geoid lookup thread's pipeline reads its grid as
//! before after another thread's drop.
//!
//! **What it costs, and why the lock is first come, first served.** Conversions on
//! different threads no longer run in parallel. Each lock is held for one call -- one
//! point's conversion, not a whole file's -- and the lock is taken in the order it was
//! asked for. `std`'s `Mutex` makes no such promise: a thread that releases it and asks
//! again at once usually gets it back, and with eight threads converting in a loop the
//! geoid lookup thread waited past `crate::geoid::UNDULATION_LOOKUP_TIMEOUT`'s 250 ms
//! and refused a lookup. Served in turn, a live feed's lookup waits for the calls already
//! queued, one each at most from every other thread using PROJ, however long their
//! files take to convert.
//!
//! The other way out, a teardown that destroys the object and context without
//! `proj_cleanup`, needs `unsafe` calls into `proj-sys` and is human-owned
//! (`docs/agentic-workflow.md`); the lock needs neither.

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

/// The lock every PROJ call this crate makes holds for its length: a ticket lock, so
/// that it is served in the order it was asked for. It guards no data, only the order of
/// calls into libproj.
static PROJ: Turns = Turns {
    tickets: Mutex::new(Tickets {
        next: 0,
        serving: 0,
    }),
    turn: Condvar::new(),
};

struct Turns {
    tickets: Mutex<Tickets>,
    /// Signalled whenever `serving` moves on.
    turn: Condvar,
}

struct Tickets {
    /// The ticket the next caller takes.
    next: u64,
    /// The ticket whose holder may call PROJ now.
    serving: u64,
}

impl Turns {
    /// The ticket counters, poisoned or not: they are only ever incremented whole, so a
    /// panic cannot leave them half-written, and a conversion refused because another
    /// thread once panicked would be a refusal for no reason.
    fn tickets(&self) -> MutexGuard<'_, Tickets> {
        self.tickets.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// One caller's turn at PROJ; the next ticket is served when it drops, unwinding
/// included.
struct Turn;

impl Drop for Turn {
    fn drop(&mut self) {
        PROJ.tickets().serving += 1;
        PROJ.turn.notify_all();
    }
}

/// Take a ticket and wait for it to be served.
fn lock() -> Turn {
    let mut tickets = PROJ.tickets();
    let mine = tickets.next;
    tickets.next += 1;
    while tickets.serving != mine {
        tickets = PROJ
            .turn
            .wait(tickets)
            .unwrap_or_else(PoisonError::into_inner);
    }
    Turn
}

/// A `proj::Proj` whose every PROJ call, its drop included, holds [`PROJ`].
///
/// Neither `Send` nor `Sync`, as `proj::Proj` is not. No method calls another while it
/// holds the lock, which would wait on itself for ever.
pub(crate) struct LockedProj {
    /// `None` only inside `drop`, which takes it to drop it under the lock.
    inner: Option<proj::Proj>,
}

impl LockedProj {
    /// `proj::Proj::new` under the lock: a PROJ string, a pipeline, or a CRS.
    pub(crate) fn new(definition: &str) -> Result<LockedProj, proj::ProjCreateError> {
        let _guard = lock();
        proj::Proj::new(definition).map(|inner| LockedProj { inner: Some(inner) })
    }

    /// `proj::Proj::new_known_crs` under the lock, with no area of use: a transformation
    /// from `from` to `to`, axis order normalised to longitude, latitude or easting,
    /// northing.
    pub(crate) fn new_known_crs(from: &str, to: &str) -> Result<LockedProj, proj::ProjCreateError> {
        let _guard = lock();
        proj::Proj::new_known_crs(from, to, None).map(|inner| LockedProj { inner: Some(inner) })
    }

    /// `proj::Proj::convert` of one `(x, y)` under the lock.
    pub(crate) fn convert(&self, point: (f64, f64)) -> Result<(f64, f64), proj::ProjError> {
        let _guard = lock();
        self.inner()?.convert(point)
    }

    /// `proj::Proj::as_wkt` under the lock, with PROJ's default options.
    pub(crate) fn as_wkt(&self, version: proj::wkt::WktVersion) -> Result<String, proj::ProjError> {
        let _guard = lock();
        self.inner()?.as_wkt(Some(version), None)
    }

    /// The object; always there outside `drop`.
    fn inner(&self) -> Result<&proj::Proj, proj::ProjError> {
        self.inner
            .as_ref()
            .ok_or_else(|| proj::ProjError::Projection("the PROJ object is gone".to_string()))
    }
}

impl Drop for LockedProj {
    fn drop(&mut self) {
        let _guard = lock();
        // `proj::Proj`'s own drop: `proj_destroy`, `proj_context_destroy`, `proj_cleanup`.
        drop(self.inner.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Eight threads building, converting through, describing and dropping their own
    /// PROJ objects -- a compound CRS, a transformation from its WKT, a grid pipeline --
    /// each drop running `proj_cleanup`, while a ninth converts through one pipeline it
    /// keeps throughout, as the geoid lookup thread does: the calls GAP-261's lock
    /// serialises. Every answer is checked, and the kept pipeline must answer between
    /// the others' cleanups.
    #[test]
    fn concurrent_builds_conversions_and_drops_stay_correct() {
        const ROUNDS: usize = 10;
        // The committed EGM2008 clip `geoid`'s own tests read.
        let clip = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../testdata/geoid/egm08_25_clip_53n56n_13e17e.tif");
        let pipeline = |clip: &std::path::Path| {
            LockedProj::new(&format!(
                "+proj=pipeline \
                 +step +proj=unitconvert +xy_in=deg +xy_out=rad \
                 +step +proj=vgridshift +grids=\"{}\" +multiplier=1 \
                 +step +proj=unitconvert +xy_in=rad +xy_out=deg \
                 +step +proj=axisswap +order=1,3,2",
                clip.display()
            ))
            .expect("PROJ opens the clip")
        };
        // One pipeline kept for the whole test and used throughout, as the geoid lookup
        // thread keeps its own, while the others are built and dropped around it.
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let kept = {
            let (clip, done) = (clip.clone(), std::sync::Arc::clone(&done));
            std::thread::spawn(move || {
                let geoid = pipeline(&clip);
                while !done.load(std::sync::atomic::Ordering::Relaxed) {
                    let (_, n) = geoid.convert((15.0, 54.5)).expect("on the clip");
                    assert!(n.is_finite(), "{n}");
                }
            })
        };
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let clip = clip.clone();
                std::thread::spawn(move || {
                    for _ in 0..ROUNDS {
                        // The CRS described as `nad83::horizontal_datum` reads one ...
                        // UTM zone 33N with EGM2008 heights, the geoid fixtures' frame.
                        let crs = LockedProj::new("EPSG:32633+3855").expect("a CRS");
                        let wkt = crs
                            .as_wkt(proj::wkt::WktVersion::Wkt1_Gdal)
                            .expect("describes itself");
                        assert!(wkt.contains("COMPD_CS"), "{wkt}");
                        drop(crs);
                        // ... and that WKT converted from, as `nad83::positions_to_wgs84`
                        // converts a point cloud's.
                        let utm =
                            LockedProj::new_known_crs(&wkt, "EPSG:4979").expect("a transform");
                        // The zone's central meridian on the equator: 15 E, 0 N.
                        let (lon, lat) = utm.convert((500_000.0, 0.0)).expect("converts");
                        assert!((lon - 15.0).abs() < 1e-9, "{lon}");
                        assert!(lat.abs() < 1e-9, "{lat}");
                        drop(utm);
                        // And a grid pipeline, as `geoid::undulations` builds one.
                        let geoid = pipeline(&clip);
                        let (_, n) = geoid.convert((15.0, 54.5)).expect("on the clip");
                        assert!(n.is_finite(), "{n}");
                        drop(geoid);
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().expect("no thread panicked");
        }
        done.store(true, std::sync::atomic::Ordering::Relaxed);
        kept.join()
            .expect("the kept pipeline's thread did not panic");
    }

    /// The lock only holds if nothing builds a PROJ object around it: no code line in
    /// this crate's sources names `proj::Proj` but this module's.
    #[test]
    fn no_other_module_builds_a_proj_object() {
        fn visit(dir: &std::path::Path, found: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).expect("the source directory reads") {
                let path = entry.expect("an entry").path();
                if path.is_dir() {
                    visit(&path, found);
                } else if path.extension().is_some_and(|e| e == "rs")
                    && path.file_name().is_some_and(|n| n != "proj_lock.rs")
                {
                    let text = std::fs::read_to_string(&path).expect("a source file reads");
                    for (n, line) in text.lines().enumerate() {
                        if !line.trim_start().starts_with("//") && line.contains("proj::Proj") {
                            found.push(format!("{}:{}: {line}", path.display(), n + 1));
                        }
                    }
                }
            }
        }
        let mut found = Vec::new();
        visit(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut found,
        );
        assert!(found.is_empty(), "{found:#?}");
    }
}
