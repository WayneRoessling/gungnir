//! PN-04, the evidence card (GAP-073).
//!
//! `docs/ux/ux-to-code-map.md` §1: reads the selected `TrackView`, identity evidence
//! from `gungnir-identification`, lineage from `gungnir-identity`, and threat factors
//! from `gungnir-assessment`; writes a designation back as evidence.
//!
//! # Why the sections are optional
//!
//! The card exists so that an identity decision is made with the evidence on screen
//! rather than from a label. Three of its four sections come from crates the desktop
//! does not yet wire -- cooperative identity evidence (GAP-010), cross-session lineage
//! (GAP-019), and threat factors (GAP-028) -- so each is a
//! [`Section`](crate::panels::unavailable::Section): present means the data is real,
//! unavailable means the section names the crate that owns it and is not drawn as
//! empty. An evidence card that rendered "no evidence" when it simply had not been
//! connected would be the most dangerous kind of wrong on this panel -- it invites an
//! operator to conclude there is nothing to weigh.
//!
//! The designation control is likewise absent rather than inert: a button that looks
//! like it records a declaration and does not would be worse than no button.

use crate::panels::unavailable::Section;
use crate::theme;
use egui::{RichText, Ui};
use gungnir_model::{Classification, Releasability, TrackView, Vocabulary};

/// One piece of identity evidence: what was observed, by what, and how strongly it
/// argues for the classification.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvidenceLine<'a> {
    /// What the evidence is, e.g. "IFF Mode 5 reply" or "ADS-B ident".
    pub kind: &'a str,
    /// The source string `gungnir-identification` records.
    pub source: &'a str,
    /// Contribution to the declared classification, 0.0 to 1.0.
    pub weight: f32,
    /// What it argues for.
    pub supports: Classification,
}

/// A prior session this entity was seen in (`gungnir-identity` lineage).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineageLine<'a> {
    pub session: u64,
    pub local_track: u64,
    /// How the correlation was made, e.g. "session track id".
    pub basis: &'a str,
}

/// One factor in the threat assessment (`gungnir-assessment`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FactorLine<'a> {
    pub name: &'a str,
    pub contribution: f32,
}

/// Everything the card draws for the selected track.
/// One approach the track makes to an asset (DN-02 §7, GAP-020).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApproachLine<'a> {
    pub asset: &'a str,
    pub time_ahead_s: f64,
    pub distance_m: f64,
    /// Reaches the asset's boundary, as opposed to passing it.
    pub arrives: bool,
    /// Which predictor produced it, named on every line (DN-02 §5).
    pub predictor: &'a str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EvidenceCardView<'a> {
    pub track: &'a TrackView,
    pub evidence: Section<'a, EvidenceLine<'a>>,
    pub lineage: Section<'a, LineageLine<'a>>,
    pub factors: Section<'a, FactorLine<'a>>,
    /// What the track approaches, or why that is not known (GAP-020).
    pub approach: Section<'a, ApproachLine<'a>>,
    /// Warnings owed because of this track (DN-03 §7, GAP-042).
    pub warnings: &'a [crate::panels::alerts::WarningLine],
    /// Whether this build can record a designation. `false` draws no control at all.
    pub designation_available: bool,
    /// The words this deployment uses (D-12).
    pub vocabulary: &'a Vocabulary,
}

/// Render the evidence card for the selected track.
pub fn render_evidence_card(ui: &mut Ui, view: &EvidenceCardView<'_>) {
    let t = view.track;
    ui.heading(format!("Track {}", t.id.0));

    draw_identity(ui, t, view.vocabulary);
    ui.separator();
    draw_kinematics(ui, t);
    ui.separator();

    if view.evidence.draw_header(ui, "Identity evidence") {
        if let Section::Present(lines) = view.evidence {
            egui::Grid::new("evidence_grid")
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Evidence");
                    ui.strong("Source");
                    ui.strong("Supports");
                    ui.strong("Weight");
                    ui.end_row();
                    for e in lines {
                        ui.label(e.kind);
                        ui.label(RichText::new(e.source).size(theme::SMALL_FONT_SIZE));
                        ui.label(
                            RichText::new(view.vocabulary.classification(e.supports))
                                .color(theme::classification_color(e.supports)),
                        );
                        ui.label(theme::numeral(format!("{:.2}", e.weight)));
                        ui.end_row();
                    }
                });
        }
    }
    ui.separator();

    if view.lineage.draw_header(ui, "Identity lineage") {
        if let Section::Present(lines) = view.lineage {
            for l in lines {
                ui.label(format!(
                    "Session {} track {} ({})",
                    l.session, l.local_track, l.basis
                ));
            }
        }
    }
    ui.separator();

    if view.factors.draw_header(ui, "Threat factors") {
        if let Section::Present(lines) = view.factors {
            for f in lines {
                ui.label(format!("{}: {:+.2}", f.name, f.contribution));
            }
        }
    }
    ui.separator();

    if view.approach.draw_header(ui, "Approach") {
        if let Section::Present(lines) = view.approach {
            if lines.is_empty() {
                ui.label(
                    RichText::new("Approaches no declared asset within the horizon.")
                        .color(theme::MUTED_TEXT_COLOR),
                );
            }
            for a in lines {
                let what = if a.arrives {
                    format!("arrives at {} in {:.0} s", a.asset, a.time_ahead_s)
                } else {
                    format!(
                        "passes {} at {:.0} m in {:.0} s",
                        a.asset, a.distance_m, a.time_ahead_s
                    )
                };
                ui.label(format!("{what} ({})", a.predictor));
            }
        }
    }
    ui.separator();

    if !view.warnings.is_empty() {
        ui.strong("Warnings owed because of this track");
        for w in view.warnings {
            ui.label(
                RichText::new(format!(
                    "{}: {} via {} ({:+.0} s)",
                    w.asset, w.state, w.channel, w.remaining_s
                ))
                .color(if w.loud {
                    theme::ALERT_COLOR
                } else {
                    theme::WARNING_COLOR
                }),
            );
        }
        ui.separator();
    }

    if view.designation_available {
        ui.label("Designation controls are available in this build.");
    } else {
        ui.label(
            RichText::new(
                "No designation control: recording a declaration needs \
                 gungnir-identification wired to the desktop.",
            )
            .color(theme::MUTED_TEXT_COLOR),
        );
    }
}

/// Affiliation, confidence and releasability: what the card is chiefly about.
fn draw_identity(ui: &mut Ui, t: &TrackView, vocabulary: &Vocabulary) {
    ui.horizontal(|ui| {
        ui.strong("Classification");
        ui.label(
            RichText::new(vocabulary.classification(t.classification))
                .color(theme::classification_color(t.classification))
                .strong(),
        );
        ui.label(
            RichText::new(format!(
                "{} frame",
                theme::frame_label(theme::classification_frame(t.classification))
            ))
            .color(theme::MUTED_TEXT_COLOR)
            .size(theme::SMALL_FONT_SIZE),
        );
    });
    ui.label(format!(
        "Association confidence {:.2}",
        t.quality.association_confidence
    ));
    // Releasability is named rather than summarised: DN-17 puts the marking on the
    // data precisely so it is read from the data, and "Parties" without the parties
    // would leave an operator guessing who this may go to.
    let releasability = match &t.releasability {
        Releasability::Internal => "Internal only".to_owned(),
        Releasability::AllPeers => "Releasable to any authenticated peer".to_owned(),
        Releasability::Parties { parties } => {
            let names: Vec<&str> = parties.iter().map(String::as_str).collect();
            format!("Releasable to {}", names.join(", "))
        }
    };
    ui.label(RichText::new(releasability).color(theme::WARNING_COLOR));
    // Where it came from (DN-17 §7's PN-04 row, GAP-062): the marking is fixed by the
    // data's origin, so the origin is named beside it.
    ui.label(
        RichText::new(origin_line(t))
            .color(theme::MUTED_TEXT_COLOR)
            .size(theme::SMALL_FONT_SIZE),
    );
}

/// The sources behind a track and how strongly they were authenticated, in one line.
#[must_use]
pub fn origin_line(t: &TrackView) -> String {
    use gungnir_model::SourceAuthentication;
    let authentication = match t.provenance.authentication {
        SourceAuthentication::Unauthenticated => "unauthenticated",
        SourceAuthentication::AllowList => "sensor on the allow-list",
        SourceAuthentication::MachineIdentity => "machine identity verified",
    };
    match &t.provenance.peer {
        Some(peer) => format!(
            "From peer {} (their track {}), quality assigned here; {authentication}",
            peer.peer, peer.remote_track
        ),
        None if t.provenance.source_sensor_ids.is_empty() => {
            format!("From no named sensor; {authentication}")
        }
        None => {
            let ids: Vec<String> = t
                .provenance
                .source_sensor_ids
                .iter()
                .map(ToString::to_string)
                .collect();
            format!("From sensor(s) {}; {authentication}", ids.join(", "))
        }
    }
}

/// Position, speed, uncertainty and freshness.
fn draw_kinematics(ui: &mut Ui, t: &TrackView) {
    let [e, n, u] = t.position_enu();
    ui.label(format!("Position {e:.0}, {n:.0}, {u:.0} m ENU"));
    ui.label(format!("Speed {:.1} m/s", t.speed_mps()));
    let [se, sn, su] = t.position_sigma();
    ui.label(format!("One sigma {se:.0}, {sn:.0}, {su:.0} m"));
    if t.quality.is_stale {
        ui.label(
            RichText::new("STALE")
                .color(theme::TRACK_STALE_COLOR)
                .strong(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::panels::unavailable::Unavailable;

    /// The card's own three sections each name the crate that owns them. If one of
    /// these ever pointed at a crate that does supply the data, the section would be
    /// claiming unavailability it does not have.
    #[test]
    fn every_section_of_the_card_names_its_owner_and_gap() {
        for (owner, gap) in [
            ("gungnir-identification", "GAP-010"),
            ("gungnir-identity", "GAP-019"),
            ("gungnir-assessment", "GAP-028"),
        ] {
            let s: Section<'_, EvidenceLine<'_>> = Section::Unavailable(Unavailable { owner, gap });
            assert_ne!(s, Section::Present(&[]));
            assert_eq!(s.items(), None);
        }
    }

    /// Every affiliation has a frame the card can name, so the identity line never
    /// falls back to a colour alone.
    #[test]
    fn every_classification_has_a_frame_and_colour() {
        for c in [
            Classification::Hostile,
            Classification::Friendly,
            Classification::Neutral,
            Classification::Unknown,
        ] {
            let _ = theme::classification_frame(c);
            let _ = theme::classification_color(c);
        }
    }
}
