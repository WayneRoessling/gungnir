// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! PN-03, the track list (GAP-073 added selection and three columns; GAP-226 the rows,
//! the sort and the filter).
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
//! **Only the rows in view are laid out** (GAP-226, WF-03). The table was an `egui::Grid`
//! that laid out every track every frame to size its columns. Now each column has a width
//! measured from its heading and from the widest thing it can hold, so the header and the
//! rows line up without looking at the rows. The rows are drawn through
//! `ScrollArea::show_rows`, which lays out only those in view. Sorting and filtering
//! still read every track, but those are comparisons, not text layout.
//!
//! **Sort by any column, filter by class, status and staleness** (WF-03). Both are
//! session state the caller holds and passes in, like the selection. A click on a
//! heading or a filter comes back as a [`TrackTableAction`], so the panel holds nothing
//! and the one-way flow `rust-ui-architecture-coding-standards.md` §2 requires is kept.
//! A filter that hides every track says so, rather than leaving an empty table that
//! reads as an empty picture.

use std::cmp::Ordering;

use crate::panels::unavailable::{draw_unavailable, Unavailable};
use crate::theme;
use egui::RichText;
use gungnir_model::vocabulary::Term;
use gungnir_model::{
    Classification, InterceptSolutionView, MissionTime, Releasability, TrackId, TrackStatus,
    TrackView, Vocabulary,
};

/// A PN-03 column, in the order drawn. Every column sorts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrackColumn {
    #[default]
    Id,
    Status,
    East,
    North,
    Up,
    Speed,
    Class,
    Age,
    Asset,
    Marking,
    Score,
}

impl TrackColumn {
    /// Every column, in the order drawn.
    pub const ALL: [TrackColumn; 11] = [
        TrackColumn::Id,
        TrackColumn::Status,
        TrackColumn::East,
        TrackColumn::North,
        TrackColumn::Up,
        TrackColumn::Speed,
        TrackColumn::Class,
        TrackColumn::Age,
        TrackColumn::Asset,
        TrackColumn::Marking,
        TrackColumn::Score,
    ];

    fn heading(self) -> &'static str {
        match self {
            TrackColumn::Id => "ID",
            TrackColumn::Status => "Status",
            TrackColumn::East => "E (m)",
            TrackColumn::North => "N (m)",
            TrackColumn::Up => "U (m)",
            TrackColumn::Speed => "Speed (m/s)",
            TrackColumn::Class => "Class",
            TrackColumn::Age => "Age (s)",
            TrackColumn::Asset => "Asset",
            TrackColumn::Marking => "Marking",
            TrackColumn::Score => "Score",
        }
    }
}

/// The order PN-03 shows its rows in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrackOrder {
    pub by: TrackColumn,
    pub descending: bool,
}

impl TrackOrder {
    /// What a click on `column`'s heading asks for: the other direction if the table is
    /// already sorted by it, ascending by it otherwise.
    #[must_use]
    pub fn clicked(self, column: TrackColumn) -> Self {
        if self.by == column {
            Self {
                by: column,
                descending: !self.descending,
            }
        } else {
            Self {
                by: column,
                descending: false,
            }
        }
    }
}

/// Which tracks PN-03 shows. The default shows every track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrackFilter {
    pub class: Option<Classification>,
    pub status: Option<TrackStatus>,
    pub stale_only: bool,
}

impl TrackFilter {
    /// Whether this filter shows `track`.
    #[must_use]
    pub fn admits(&self, track: &TrackView) -> bool {
        self.class.is_none_or(|c| track.classification == c)
            && self.status.is_none_or(|s| track.status == s)
            && (!self.stale_only || track.quality.is_stale)
    }

    /// True when the filter hides anything it could.
    #[must_use]
    pub fn is_active(&self) -> bool {
        *self != Self::default()
    }
}

/// What a click inside PN-03 asked for this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackTableAction {
    /// Show this track in PN-04, or close the card if it is already shown.
    Select(TrackId),
    /// Show the rows in this order.
    Order(TrackOrder),
    /// Show only the tracks this filter admits.
    Filter(TrackFilter),
}

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
    /// Tracks whose last ASTERIX Category 129 report stated its height above mean sea
    /// level and no geoid model corrected it (GAP-196, D-124): their U is marked "MSL"
    /// in the warning colour, so a height that is not on the ellipsoid is never read as
    /// one from this table.
    pub msl_heights: &'a [TrackId],
    /// Tracks whose last ASTERIX Category 129 report gave no height anybody could place
    /// (GAP-199, D-130): their U is marked "unknown" in the warning colour, so a height
    /// held at 0 is never read as a measured one from this table.
    pub unknown_heights: &'a [TrackId],
    /// The words this deployment uses (D-12). Not optional: every domain value on this
    /// table goes through it, so there is no path that shows a variant name.
    pub vocabulary: &'a Vocabulary,
    /// The order the operator chose.
    pub order: TrackOrder,
    /// The filter the operator chose.
    pub filter: TrackFilter,
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

/// What a row sorts by in one column.
#[derive(Debug, Clone, PartialEq)]
enum SortKey {
    Number(f64),
    Text(String),
    /// No value: an unassigned track's asset, an unscored track's score. Always last,
    /// in either direction, so reversing the order never puts the gaps on top.
    Absent,
}

impl SortKey {
    fn compare(&self, other: &Self, descending: bool) -> Ordering {
        let ordered = |o: Ordering| if descending { o.reverse() } else { o };
        match (self, other) {
            (SortKey::Number(a), SortKey::Number(b)) => ordered(a.total_cmp(b)),
            (SortKey::Text(a), SortKey::Text(b)) => ordered(a.cmp(b)),
            // Absent last whichever way the column runs. One column always yields one
            // kind of value, so between a number and a text the rank only keeps the
            // order total.
            _ => self.rank().cmp(&other.rank()),
        }
    }

    fn rank(&self) -> u8 {
        match self {
            SortKey::Number(_) => 0,
            SortKey::Text(_) => 1,
            SortKey::Absent => 2,
        }
    }
}

/// Lifecycle order, so a sort by status reads tentative to deleted.
fn status_rank(status: TrackStatus) -> u8 {
    match status {
        TrackStatus::Tentative => 0,
        TrackStatus::Confirmed => 1,
        TrackStatus::Coasting => 2,
        TrackStatus::Deleted => 3,
    }
}

/// Threat order, so a sort by class puts hostile first: the order an operator works in.
fn class_rank(class: Classification) -> u8 {
    match class {
        Classification::Hostile => 0,
        Classification::Unknown => 1,
        Classification::Neutral => 2,
        Classification::Friendly => 3,
    }
}

/// The marking a row shows (DN-17 §7, GAP-062). An unmarked track reads "internal",
/// which is what it is.
fn marking(track: &TrackView) -> String {
    match &track.releasability {
        Releasability::Internal => "internal".to_string(),
        Releasability::AllPeers => "all peers".to_string(),
        Releasability::Parties { parties } => {
            parties.iter().cloned().collect::<Vec<_>>().join(", ")
        }
    }
}

fn sort_key(view: &TrackTableView<'_>, track: &TrackView) -> SortKey {
    let [e, n, u] = track.position_enu();
    match view.order.by {
        // Ids are far below 2^53, where an f64 stops holding every integer.
        #[allow(clippy::cast_precision_loss)]
        TrackColumn::Id => SortKey::Number(track.id.0 as f64),
        TrackColumn::Status => SortKey::Number(f64::from(status_rank(track.status))),
        TrackColumn::East => SortKey::Number(e),
        TrackColumn::North => SortKey::Number(n),
        TrackColumn::Up => SortKey::Number(u),
        TrackColumn::Speed => SortKey::Number(track.speed_mps()),
        TrackColumn::Class => SortKey::Number(f64::from(class_rank(track.classification))),
        TrackColumn::Age => SortKey::Number(age_s(view.now, track.mission_time)),
        TrackColumn::Asset => view
            .asset_for(track.id)
            .map_or(SortKey::Absent, |r| SortKey::Number(f64::from(r))),
        TrackColumn::Marking => SortKey::Text(marking(track)),
        TrackColumn::Score => view
            .score_for(track.id)
            .map_or(SortKey::Absent, |s| SortKey::Number(f64::from(s))),
    }
}

/// The tracks the filter admits, as indices into `view.tracks`, in the chosen order.
///
/// Ties go to the lower track id whichever way the column is sorted, so two rows that
/// compare equal never swap places from one frame to the next.
#[must_use]
pub fn rows(view: &TrackTableView<'_>) -> Vec<usize> {
    let mut keyed: Vec<(usize, SortKey)> = view
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| view.filter.admits(t))
        .map(|(i, t)| (i, sort_key(view, t)))
        .collect();
    keyed.sort_by(|(i, a), (j, b)| {
        a.compare(b, view.order.descending)
            .then_with(|| view.tracks[*i].id.0.cmp(&view.tracks[*j].id.0))
    });
    keyed.into_iter().map(|(i, _)| i).collect()
}

/// Table-of-contents style, per rust-ui-architecture-coding-standards.md §6: this
/// function stays a short list of calls to sub-sections.
///
/// Returns what the operator clicked this frame, if anything.
pub fn render_track_table(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    view: &TrackTableView<'_>,
) -> Option<TrackTableAction> {
    ui.heading("Tracks");
    let rows = rows(view);
    let mut action = render_summary(ui, palette, view, rows.len());
    if let Err(u) = view.scores {
        draw_unavailable(ui, palette, u);
    }
    let widths = column_widths(ui, palette, view);
    egui::ScrollArea::horizontal()
        .id_salt("track_table_columns")
        .show(ui, |ui| {
            action = render_header(ui, palette, view, &widths).or(action);
            let height = row_height(ui);
            egui::ScrollArea::vertical()
                .id_salt("track_table_rows")
                .max_height(palette.track_table_max_height)
                .show_rows(ui, height, rows.len(), |ui, range| {
                    for at in range {
                        let track = &view.tracks[rows[at]];
                        let selected = render_row(ui, palette, view, track, &widths, height, at);
                        action = selected.map(TrackTableAction::Select).or(action);
                    }
                });
        });
    action
}

/// The count, the filter controls, and what the filter is hiding.
fn render_summary(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    view: &TrackTableView<'_>,
    shown: usize,
) -> Option<TrackTableAction> {
    let stale = view.tracks.iter().filter(|t| t.quality.is_stale).count();
    let total = view.tracks.len();
    let summary = if view.filter.is_active() {
        format!("{total} tracks, {stale} stale; showing {shown}")
    } else {
        format!("{total} tracks, {stale} stale")
    };
    ui.label(RichText::new(summary).color(palette.muted_text_color()));
    let mut filter = view.filter;
    ui.horizontal_wrapped(|ui| {
        choose(
            ui,
            "track_table_class",
            "all classes",
            &mut filter.class,
            classes(),
            |c| view.vocabulary.classification(c),
        );
        choose(
            ui,
            "track_table_status",
            "all statuses",
            &mut filter.status,
            statuses(),
            |s| view.vocabulary.track_status(s),
        );
        ui.checkbox(&mut filter.stale_only, "stale only");
    });
    if shown == 0 && !view.tracks.is_empty() {
        // The tracks exist; the filter is what hides them. An empty table must not
        // read as an empty picture.
        ui.label(
            RichText::new(format!(
                "No track matches the filter; {} are hidden.",
                view.tracks.len()
            ))
            .color(palette.warning_color),
        );
    }
    (filter != view.filter).then_some(TrackTableAction::Filter(filter))
}

/// The classifications, from the vocabulary's own list of every term it can show.
fn classes() -> impl Iterator<Item = Classification> {
    Term::ALL.iter().filter_map(|t| match t {
        Term::Classification(c) => Some(*c),
        _ => None,
    })
}

/// The lifecycle states, from the same list.
fn statuses() -> impl Iterator<Item = TrackStatus> {
    Term::ALL.iter().filter_map(|t| match t {
        Term::TrackStatus(s) => Some(*s),
        _ => None,
    })
}

/// One filter: everything, or one value of `T`, named in the deployment's words.
fn choose<'w, T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    everything: &'w str,
    chosen: &mut Option<T>,
    values: impl Iterator<Item = T>,
    word: impl Fn(T) -> &'w str,
) {
    let shown = chosen.map_or(everything, &word).to_owned();
    egui::ComboBox::from_id_salt(id)
        .selected_text(shown)
        .show_ui(ui, |ui| {
            ui.selectable_value(chosen, None, everything);
            for value in values {
                ui.selectable_value(chosen, Some(value), word(value));
            }
        });
}

/// The height every row is drawn at. `show_rows` needs one height for all rows to know
/// which are in view, so each cell is made exactly this tall: the taller of the two fonts
/// a row uses, padded as a selectable label is, and never below a clickable control.
fn row_height(ui: &egui::Ui) -> f32 {
    let text = ui
        .text_style_height(&egui::TextStyle::Body)
        .max(ui.text_style_height(&egui::TextStyle::Monospace));
    (text + 2.0 * ui.spacing().button_padding.y).max(ui.spacing().interact_size.y)
}

/// Width of `text` laid out on one line.
fn text_width(ui: &egui::Ui, text: impl Into<egui::WidgetText>) -> f32 {
    text.into()
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        )
        .size()
        .x
}

/// Each column's width: its heading, sort mark included, or the widest thing it can
/// hold, whichever is wider. Measured from samples rather than from the rows, so it costs
/// the same at two hundred tracks as at two, and the header lines up with rows it has
/// not laid out. Anything wider still -- a long list of parties in the marking -- is cut
/// short with the whole of it on hover.
fn column_widths(ui: &egui::Ui, palette: &theme::Palette, view: &TrackTableView<'_>) -> [f32; 11] {
    let widest_id = view.tracks.iter().map(|t| t.id.0).max().unwrap_or(0);
    let widest_word = |words: Vec<&str>| {
        words
            .into_iter()
            .map(|w| text_width(ui, w))
            .fold(0.0_f32, f32::max)
    };
    let pad = 2.0 * ui.spacing().button_padding.x;
    TrackColumn::ALL.map(|column| {
        let heading = text_width(ui, RichText::new(heading(column, view)).strong());
        let content = match column {
            TrackColumn::Id => text_width(ui, widest_id.to_string()),
            TrackColumn::Status => widest_word(
                statuses()
                    .map(|s| view.vocabulary.track_status(s))
                    .collect(),
            ),
            TrackColumn::East | TrackColumn::North => {
                text_width(ui, theme::numeral(palette, "-000000.0"))
            }
            TrackColumn::Up => text_width(ui, theme::numeral(palette, "-00000.0 unknown").strong()),
            TrackColumn::Speed => text_width(ui, theme::numeral(palette, "0000.0")),
            TrackColumn::Class => widest_word(
                classes()
                    .map(|c| view.vocabulary.classification(c))
                    .collect(),
            ),
            TrackColumn::Age => text_width(ui, theme::numeral(palette, "-0000.0")),
            TrackColumn::Asset => text_width(ui, "R0000"),
            TrackColumn::Marking => {
                text_width(ui, RichText::new("all peers").size(palette.small_font_size))
            }
            TrackColumn::Score => text_width(ui, theme::numeral(palette, "0.00")),
        };
        heading.max(content) + pad
    })
}

/// A column's heading, with the sort mark when the rows are sorted by it.
fn heading(column: TrackColumn, view: &TrackTableView<'_>) -> String {
    let name = if column == TrackColumn::Score && view.scores.is_err() {
        // The score column carries its own unavailability in the header, so an operator
        // reading down the column is not left to guess what the dashes mean.
        "Score (n/a)"
    } else {
        column.heading()
    };
    if view.order.by == column {
        format!("{name} {}", if view.order.descending { "⏷" } else { "⏶" })
    } else {
        name.to_owned()
    }
}

/// One cell, exactly `width` by `height` whatever it holds, so every row and the header
/// line up without a grid that would have to lay out every row to size its columns.
fn cell<R>(ui: &mut egui::Ui, width: f32, height: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.allocate_ui_with_layout(
        egui::vec2(width, height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, height));
            ui.set_max_width(width);
            add(ui)
        },
    )
    .inner
}

/// A cell's text, cut short to its column with the whole on hover.
fn text(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) {
    ui.add(egui::Label::new(text).truncate());
}

fn render_header(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    view: &TrackTableView<'_>,
    widths: &[f32; 11],
) -> Option<TrackTableAction> {
    let height = row_height(ui);
    let mut action = None;
    ui.horizontal(|ui| {
        for (column, width) in TrackColumn::ALL.into_iter().zip(widths) {
            let name = heading(column, view);
            cell(ui, *width, height, |ui| {
                if column == TrackColumn::Score && view.scores.is_err() {
                    // Nothing to sort by: every score is absent.
                    text(ui, RichText::new(name).color(palette.muted_text_color()));
                } else if ui
                    .selectable_label(view.order.by == column, RichText::new(name).strong())
                    .clicked()
                {
                    action = Some(TrackTableAction::Order(view.order.clicked(column)));
                }
            });
        }
    });
    action
}

/// One row. Returns the track if its id was clicked.
fn render_row(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    view: &TrackTableView<'_>,
    track: &TrackView,
    widths: &[f32; 11],
    height: f32,
    at: usize,
) -> Option<TrackId> {
    if at % 2 == 1 {
        // Striped, as the grid was, so a long row can be followed across.
        let spacing = ui.spacing().item_spacing.x;
        #[allow(clippy::cast_precision_loss)]
        let width = widths.iter().sum::<f32>() + spacing * (widths.len() - 1) as f32;
        let stripe = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(width, height));
        ui.painter()
            .rect_filled(stripe, 0.0, ui.visuals().faint_bg_color);
    }
    let mut clicked = None;
    let [e, n, u] = track.position_enu();
    let w = widths;
    ui.horizontal(|ui| {
        cell(ui, w[0], height, |ui| {
            if ui
                .selectable_label(view.selected == Some(track.id), track.id.0.to_string())
                .clicked()
            {
                clicked = Some(track.id);
            }
        });
        cell(ui, w[1], height, |ui| {
            text(
                ui,
                RichText::new(view.vocabulary.track_status(track.status)).color(
                    theme::track_color(palette, track.status, track.quality.is_stale),
                ),
            );
        });
        cell(ui, w[2], height, |ui| {
            text(ui, theme::numeral(palette, format!("{e:.1}")));
        });
        cell(ui, w[3], height, |ui| {
            text(ui, theme::numeral(palette, format!("{n:.1}")));
        });
        let mark = if view.unknown_heights.contains(&track.id) {
            UpMark::Unknown
        } else if view.msl_heights.contains(&track.id) {
            UpMark::AboveMsl
        } else {
            UpMark::Ellipsoidal
        };
        cell(ui, w[4], height, |ui| render_up(ui, palette, u, mark));
        cell(ui, w[5], height, |ui| {
            text(
                ui,
                theme::numeral(palette, format!("{:.1}", track.speed_mps())),
            );
        });
        cell(ui, w[6], height, |ui| {
            text(
                ui,
                RichText::new(view.vocabulary.classification(track.classification))
                    .color(theme::classification_color(palette, track.classification)),
            );
        });
        cell(ui, w[7], height, |ui| {
            render_age(
                ui,
                palette,
                age_s(view.now, track.mission_time),
                track.quality.is_stale,
            );
        });
        cell(ui, w[8], height, |ui| match view.asset_for(track.id) {
            Some(resource) => text(ui, format!("R{resource}")),
            None => text(ui, RichText::new("--").color(palette.muted_text_color())),
        });
        cell(ui, w[9], height, |ui| {
            text(
                ui,
                RichText::new(marking(track)).size(palette.small_font_size),
            );
        });
        cell(ui, w[10], height, |ui| match view.score_for(track.id) {
            Some(s) => text(ui, theme::numeral(palette, format!("{s:.2}"))),
            None => text(ui, RichText::new("--").color(palette.muted_text_color())),
        });
    });
    clicked
}

/// What a track's up is measured from, as far as this table marks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpMark {
    Ellipsoidal,
    /// GAP-196: above mean sea level, uncorrected.
    AboveMsl,
    /// GAP-199: no height anybody could place.
    Unknown,
}

/// The up coordinate, marked when a Category 129 report put it above mean sea level
/// rather than the WGS-84 ellipsoid (GAP-196), or gave no height at all (GAP-199): the
/// number is still shown, because it is what the picture holds, and the mark says what it
/// is measured from.
fn render_up(ui: &mut egui::Ui, palette: &theme::Palette, up: f64, mark: UpMark) {
    let (suffix, hover) = match mark {
        UpMark::Ellipsoidal => {
            text(ui, theme::numeral(palette, format!("{up:.1}")));
            return;
        }
        UpMark::AboveMsl => (
            "MSL",
            "the last UAS report (ASTERIX Category 129) stated this height above mean sea \
             level and no geoid model corrected it: it is off the WGS-84 ellipsoid by the \
             local geoid separation",
        ),
        UpMark::Unknown => (
            "unknown",
            "the last UAS report (ASTERIX Category 129) gave no height above mean sea \
             level, and no DEM placed its height above ground: the height is held at 0 \
             and is not a measurement",
        ),
    };
    ui.add(
        egui::Label::new(
            theme::numeral(palette, format!("{up:.1} {suffix}"))
                .color(palette.warning_color)
                .strong(),
        )
        .truncate(),
    )
    .on_hover_text(hover);
}

/// Age, coloured by the staleness the tracker already decided.
///
/// The colour follows `quality.is_stale` rather than a threshold invented here: the
/// staleness rule is `gungnir-tracking-service`'s, and a second rule in the table would
/// let the list disagree with the viewport about the same track.
fn render_age(ui: &mut egui::Ui, palette: &theme::Palette, age: f64, stale: bool) {
    let numeral = theme::numeral(palette, format!("{age:.1}"));
    if stale {
        text(ui, numeral.color(palette.track_stale_color).strong());
    } else {
        text(ui, numeral);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{Provenance, Quality, ResourceId};

    fn solution(track: u64, resource: u32) -> InterceptSolutionView {
        InterceptSolutionView {
            resource: ResourceId(resource),
            track: TrackId(track),
            intercept_point: None,
            time_to_intercept_s: None,
        }
    }

    fn view<'a>(
        tracks: &'a [TrackView],
        assignments: &'a [InterceptSolutionView],
        vocabulary: &'a Vocabulary,
    ) -> TrackTableView<'a> {
        TrackTableView {
            tracks,
            now: MissionTime(100.0),
            assignments,
            scores: Err(Unavailable {
                owner: "gungnir-assessment",
                gap: "GAP-028",
            }),
            selected: None,
            msl_heights: &[],
            unknown_heights: &[],
            vocabulary,
            order: TrackOrder::default(),
            filter: TrackFilter::default(),
        }
    }

    fn track(
        id: u64,
        east: f64,
        class: Classification,
        status: TrackStatus,
        stale: bool,
    ) -> TrackView {
        let mut kinematics = nalgebra::SVector::<f64, 6>::zeros();
        kinematics[0] = east;
        TrackView {
            id: TrackId(id),
            status,
            state: kinematics,
            covariance: nalgebra::SMatrix::<f64, 6, 6>::identity(),
            classification: class,
            provenance: Provenance::default(),
            quality: Quality {
                is_stale: stale,
                ..Quality::default()
            },
            mission_time: MissionTime(100.0),
            releasability: Releasability::default(),
        }
    }

    fn ids(view: &TrackTableView<'_>) -> Vec<u64> {
        rows(view)
            .into_iter()
            .map(|i| view.tracks[i].id.0)
            .collect()
    }

    /// The asset column reports the plan, so an unassigned track has no asset rather
    /// than someone else's.
    #[test]
    fn the_asset_column_reads_the_plan_in_force() {
        let assignments = [solution(7, 3), solution(9, 4)];
        let vocab = Vocabulary::default();
        let v = view(&[], &assignments, &vocab);
        assert_eq!(v.asset_for(TrackId(7)), Some(3));
        assert_eq!(v.asset_for(TrackId(9)), Some(4));
        assert_eq!(v.asset_for(TrackId(8)), None);
    }

    /// With `gungnir-assessment` unwired every score is absent -- not zero, which would
    /// read as "assessed, and harmless".
    #[test]
    fn an_unavailable_score_is_absent_for_every_track() {
        let vocab = Vocabulary::default();
        let v = view(&[], &[], &vocab);
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

    /// A column sorts both ways, a second click on its heading reverses it, and equal
    /// rows keep their id order in either direction, so a row never swaps with its equal
    /// between frames.
    #[test]
    fn a_column_sorts_both_ways_and_ties_keep_their_id_order() {
        use Classification::{Friendly, Hostile, Unknown};
        use TrackStatus::Confirmed;
        let tracks = [
            track(4, 300.0, Unknown, Confirmed, false),
            track(1, -50.0, Hostile, Confirmed, false),
            track(3, 300.0, Friendly, Confirmed, false),
            track(2, 10.0, Hostile, Confirmed, false),
        ];
        let vocab = Vocabulary::default();
        let mut v = view(&tracks, &[], &vocab);
        assert_eq!(ids(&v), [1, 2, 3, 4], "the default is by id");

        v.order = v.order.clicked(TrackColumn::East);
        assert_eq!(ids(&v), [1, 2, 3, 4]);
        v.order = v.order.clicked(TrackColumn::East);
        assert!(v.order.descending);
        assert_eq!(
            ids(&v),
            [3, 4, 2, 1],
            "3 and 4 tie on east and keep id order"
        );

        v.order = v.order.clicked(TrackColumn::Class);
        assert!(!v.order.descending, "a new column starts ascending");
        assert_eq!(
            ids(&v),
            [1, 2, 4, 3],
            "hostile first, then unknown, then friendly"
        );
    }

    /// A track with no asset sorts last whichever way the column runs: reversing the
    /// order must not bring the unassigned to the top as if they were the answer.
    #[test]
    fn an_absent_value_sorts_last_in_either_direction() {
        let tracks = [
            track(
                1,
                0.0,
                Classification::Unknown,
                TrackStatus::Confirmed,
                false,
            ),
            track(
                2,
                0.0,
                Classification::Unknown,
                TrackStatus::Confirmed,
                false,
            ),
            track(
                3,
                0.0,
                Classification::Unknown,
                TrackStatus::Confirmed,
                false,
            ),
        ];
        let assignments = [solution(2, 9), solution(3, 1)];
        let vocab = Vocabulary::default();
        let mut v = view(&tracks, &assignments, &vocab);
        v.order = TrackOrder {
            by: TrackColumn::Asset,
            descending: false,
        };
        assert_eq!(ids(&v), [3, 2, 1]);
        v.order.descending = true;
        assert_eq!(ids(&v), [2, 3, 1]);
    }

    /// The filter narrows by class, status and staleness together, and the default
    /// admits every track.
    #[test]
    fn the_filter_narrows_by_class_status_and_staleness() {
        use Classification::{Hostile, Unknown};
        use TrackStatus::{Coasting, Confirmed};
        let tracks = [
            track(1, 0.0, Hostile, Confirmed, false),
            track(2, 0.0, Hostile, Coasting, true),
            track(3, 0.0, Unknown, Coasting, true),
            track(4, 0.0, Unknown, Confirmed, false),
        ];
        let vocab = Vocabulary::default();
        let mut v = view(&tracks, &[], &vocab);
        assert!(!v.filter.is_active());
        assert_eq!(ids(&v), [1, 2, 3, 4]);

        v.filter.class = Some(Hostile);
        assert!(v.filter.is_active());
        assert_eq!(ids(&v), [1, 2]);
        v.filter.stale_only = true;
        assert_eq!(ids(&v), [2]);
        v.filter = TrackFilter {
            status: Some(Coasting),
            ..TrackFilter::default()
        };
        assert_eq!(ids(&v), [2, 3]);
    }

    /// **Only the rows in view are drawn** (GAP-226). A thousand tracks put a few dozen
    /// ids on screen, the first of them the first in the order, and the count still says
    /// a thousand, so a short table is never read as a short picture.
    #[test]
    fn a_thousand_tracks_draw_only_the_rows_in_view() {
        let tracks: Vec<TrackView> = (1..=1_000)
            .map(|id| {
                track(
                    id,
                    0.0,
                    Classification::Unknown,
                    TrackStatus::Confirmed,
                    false,
                )
            })
            .collect();
        let vocab = Vocabulary::default();
        let mut v = view(&tracks, &[], &vocab);
        v.order = TrackOrder {
            by: TrackColumn::Id,
            descending: true,
        };
        let probe = crate::harness::RenderProbe::new();
        let (_, frame) = probe.draw(|ui| render_track_table(ui, &theme::Palette::day(), &v));

        let drawn: Vec<u64> = frame
            .texts
            .iter()
            .filter_map(|t| t.parse::<u64>().ok())
            .filter(|id| (1..=1_000).contains(id))
            .collect();
        assert!(
            !drawn.is_empty() && drawn.len() < 50,
            "{} ids drawn for a thousand tracks: {}",
            drawn.len(),
            frame.joined()
        );
        assert_eq!(
            drawn.first(),
            Some(&1_000),
            "the first row is the first in the order"
        );
        assert!(frame.says("1000 tracks, 0 stale"), "{}", frame.joined());
        assert!(
            frame.says("ID ⏷"),
            "the sorted heading is marked: {}",
            frame.joined()
        );
    }

    /// A filter that hides every track says so, in the warning colour, rather than
    /// leaving an empty table that reads as an empty picture.
    #[test]
    fn a_filter_that_hides_everything_says_so() {
        let tracks = [track(
            1,
            0.0,
            Classification::Unknown,
            TrackStatus::Confirmed,
            false,
        )];
        let vocab = Vocabulary::default();
        let mut v = view(&tracks, &[], &vocab);
        v.filter.stale_only = true;
        let probe = crate::harness::RenderProbe::new();
        let (_, frame) = probe.draw(|ui| render_track_table(ui, &theme::Palette::day(), &v));
        assert!(frame.says("showing 0"), "{}", frame.joined());
        assert!(
            frame.says("No track matches the filter"),
            "{}",
            frame.joined()
        );
    }

    /// The filter offers every classification and every status the vocabulary can name.
    #[test]
    fn the_filter_offers_every_class_and_status() {
        assert_eq!(classes().count(), 4);
        assert_eq!(statuses().count(), 4);
    }

    /// The sort marks are glyphs the default fonts carry, so a sorted heading never
    /// shows a missing-glyph box.
    #[test]
    fn the_sort_marks_are_in_the_fonts() {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
        let font = egui::FontId::proportional(14.0);
        ctx.fonts_mut(|f| {
            for mark in ['⏶', '⏷'] {
                assert!(
                    f.has_glyph(&font, mark),
                    "{mark} is not in the default fonts"
                );
            }
            // The check can fail: a glyph no default font carries is reported missing.
            assert!(!f.has_glyph(&font, '𓀀'));
        });
    }
}
