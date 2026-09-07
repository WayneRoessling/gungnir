//! PN-03, the track list (GAP-073 added selection and three columns).
//!
//! `docs/ux/information-architecture.md` §1 asks for a `TrackView` list *with quality,
//! classification and age*, and selection; `docs/ux/ux-to-code-map.md` §1 names the
//! three new columns as score, age and asset. Two of the three are computable from what
//! the desktop already holds and one is not, and the difference is in the types rather
//! than in a comment:
//!
//! - **Age** is `now - track.mission_time`, both of which the caller has.
//! - **Asset** is the resource the plan in force assigns to the track, which is exactly
//!   what an `InterceptSolutionView` records. A track with no assignment shows a dash,
//!   which is a true statement about the plan, not a missing value.
//! - **Score** comes from `gungnir-assessment`, which neither binary constructs
//!   (GAP-028). It is a [`Section`]-style `Result`, so the column header says the score
//!   is unavailable instead of every row showing a plausible `0.00`.
//!
//! Selection is returned rather than written: the table borrows its data, and the state
//! it would have to mutate is `AppState`. `main.rs` applies what this returns, which
//! keeps the one-way flow `rust-ui-architecture-coding-standards.md` §2 requires.

use crate::panels::unavailable::{draw_unavailable, Unavailable};
use crate::theme;
use egui::RichText;
use gungnir_model::{InterceptSolutionView, MissionTime, TrackId, TrackView, Vocabulary};

/// Everything PN-03 draws.
#[derive(Debug, Clone, Copy)]
pub struct TrackTableView<'a> {
    pub tracks: &'a [TrackView],
    /// Mission time now, for the age column.
    pub now: MissionTime,
    /// The plan in force, read for the asset column. Empty means nothing is assigned.
    pub assignments: &'a [InterceptSolutionView],
    /// Threat score per track, or why there is none.
    pub scores: Result<&'a [(TrackId, f32)], Unavailable<'a>>,
    /// The track PN-04 is showing, highlighted here.
    pub selected: Option<TrackId>,
    /// The words this deployment uses (D-12). Not optional: every domain value on this
    /// table goes through it, so there is no path that shows a variant name.
    pub vocabulary: &'a Vocabulary,
}

impl TrackTableView<'_> {
    /// The resource assigned to this track by the plan in force, if any.
    fn asset_for(&self, track: TrackId) -> Option<u32> {
        self.assignments
            .iter()
            .find(|s| s.track == track)
            .map(|s| s.resource.0)
    }

    fn score_for(&self, track: TrackId) -> Option<f32> {
        self.scores
            .ok()?
            .iter()
            .find(|(id, _)| *id == track)
            .map(|(_, s)| *s)
    }
}

/// Age of an estimate in seconds. Negative when the estimate is stamped ahead of the
/// clock, which is a real condition (a predicted state, or two clocks disagreeing) and
/// is shown rather than clamped to zero.
#[must_use]
pub fn age_s(now: MissionTime, stamped: MissionTime) -> f64 {
    now.0 - stamped.0
}

/// Table-of-contents style, per rust-ui-architecture-coding-standards.md §6: this
/// function stays a short list of calls to sub-sections.
///
/// Returns the track the operator clicked this frame, if any.
pub fn render_track_table(ui: &mut egui::Ui, view: &TrackTableView<'_>) -> Option<TrackId> {
    ui.heading("Tracks");
    ui.label(RichText::new(format!("{} tracks", view.tracks.len())).color(theme::MUTED_TEXT_COLOR));
    if let Err(u) = view.scores {
        draw_unavailable(ui, u);
    }
    let mut clicked = None;
    egui::ScrollArea::vertical()
        .max_height(theme::TRACK_TABLE_MAX_HEIGHT)
        .show(ui, |ui| {
            egui::Grid::new("track_table").striped(true).show(ui, |ui| {
                render_header(ui, view.scores.is_ok());
                clicked = render_rows(ui, view);
            });
        });
    clicked
}

fn render_header(ui: &mut egui::Ui, scores_available: bool) {
    for heading in [
        "ID",
        "Status",
        "E (m)",
        "N (m)",
        "U (m)",
        "Speed (m/s)",
        "Class",
        "Age (s)",
        "Asset",
        "Marking",
    ] {
        ui.strong(heading);
    }
    // The score column carries its own unavailability in the header, so an operator
    // reading down the column is not left to guess what the dashes mean.
    if scores_available {
        ui.strong("Score");
    } else {
        ui.label(RichText::new("Score (n/a)").color(theme::MUTED_TEXT_COLOR));
    }
    ui.end_row();
}

fn render_rows(ui: &mut egui::Ui, view: &TrackTableView<'_>) -> Option<TrackId> {
    let mut clicked = None;
    for track in view.tracks {
        let [e, n, u] = track.position_enu();
        let selected = view.selected == Some(track.id);
        if ui
            .selectable_label(selected, track.id.0.to_string())
            .clicked()
        {
            clicked = Some(track.id);
        }
        ui.label(
            RichText::new(view.vocabulary.track_status(track.status))
                .color(theme::track_color(track.status, track.quality.is_stale)),
        );
        ui.label(theme::numeral(format!("{e:.1}")));
        ui.label(theme::numeral(format!("{n:.1}")));
        ui.label(theme::numeral(format!("{u:.1}")));
        ui.label(theme::numeral(format!("{:.1}", track.speed_mps())));
        ui.label(
            RichText::new(view.vocabulary.classification(track.classification))
                .color(theme::classification_color(track.classification)),
        );
        render_age(
            ui,
            age_s(view.now, track.mission_time),
            track.quality.is_stale,
        );
        match view.asset_for(track.id) {
            Some(resource) => ui.label(format!("R{resource}")),
            None => ui.label(RichText::new("--").color(theme::MUTED_TEXT_COLOR)),
        };
        // DN-17 §7: the marking on every row (GAP-062). An unmarked track reads
        // "internal", which is what it is.
        ui.label(
            RichText::new(match &track.releasability {
                gungnir_model::Releasability::Internal => "internal".to_string(),
                gungnir_model::Releasability::AllPeers => "all peers".to_string(),
                gungnir_model::Releasability::Parties { parties } => {
                    parties.iter().cloned().collect::<Vec<_>>().join(", ")
                }
            })
            .size(theme::SMALL_FONT_SIZE),
        );
        match view.score_for(track.id) {
            Some(s) => ui.label(theme::numeral(format!("{s:.2}"))),
            None => ui.label(RichText::new("--").color(theme::MUTED_TEXT_COLOR)),
        };
        ui.end_row();
    }
    clicked
}

/// Age, coloured by the staleness the tracker already decided.
///
/// The colour follows `quality.is_stale` rather than a threshold invented here: the
/// staleness rule is `gungnir-tracking-service`'s, and a second rule in the table would
/// let the list disagree with the viewport about the same track.
fn render_age(ui: &mut egui::Ui, age: f64, stale: bool) {
    let text = theme::numeral(format!("{age:.1}"));
    if stale {
        ui.label(text.color(theme::TRACK_STALE_COLOR).strong());
    } else {
        ui.label(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::ResourceId;

    fn solution(track: u64, resource: u32) -> InterceptSolutionView {
        InterceptSolutionView {
            resource: ResourceId(resource),
            track: TrackId(track),
            intercept_point: None,
            time_to_intercept_s: None,
        }
    }

    fn view<'a>(
        assignments: &'a [InterceptSolutionView],
        vocabulary: &'a Vocabulary,
    ) -> TrackTableView<'a> {
        TrackTableView {
            tracks: &[],
            now: MissionTime(100.0),
            assignments,
            scores: Err(Unavailable {
                owner: "gungnir-assessment",
                gap: "GAP-028",
            }),
            selected: None,
            vocabulary,
        }
    }

    /// The asset column reports the plan, so an unassigned track has no asset rather
    /// than someone else's.
    #[test]
    fn the_asset_column_reads_the_plan_in_force() {
        let assignments = [solution(7, 3), solution(9, 4)];
        let vocab = Vocabulary::default();
        let v = view(&assignments, &vocab);
        assert_eq!(v.asset_for(TrackId(7)), Some(3));
        assert_eq!(v.asset_for(TrackId(9)), Some(4));
        assert_eq!(v.asset_for(TrackId(8)), None);
    }

    /// With `gungnir-assessment` unwired every score is absent -- not zero, which would
    /// read as "assessed, and harmless".
    #[test]
    fn an_unavailable_score_is_absent_for_every_track() {
        let vocab = Vocabulary::default();
        let v = view(&[], &vocab);
        assert!(v.scores.is_err());
        for id in [TrackId(1), TrackId(2), TrackId(1_000)] {
            assert_eq!(v.score_for(id), None);
        }
    }

    /// Age is a difference of the two times the caller holds, and a state stamped ahead
    /// of the clock reports a negative age rather than being clamped to fresh.
    #[test]
    fn age_is_now_minus_the_stamp_and_may_be_negative() {
        assert!((age_s(MissionTime(100.0), MissionTime(97.5)) - 2.5).abs() < 1e-12);
        assert!((age_s(MissionTime(100.0), MissionTime(100.0))).abs() < 1e-12);
        assert!(age_s(MissionTime(100.0), MissionTime(101.0)) < 0.0);
    }
}
