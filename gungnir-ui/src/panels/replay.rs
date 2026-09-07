// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! PN-12, the replay timeline (GAP-071).
//!
//! `docs/ux/information-architecture.md` §1: a `ReplaySession`'s position, length,
//! remaining and clock, with step, seek and play rate.
//!
//! # What scrubbing this does and does not do
//!
//! It moves a cursor through the recorded **event stream**. It does not reconstitute the
//! tactical picture at that moment: the viewport, the track table and the queue keep
//! showing live state while the cursor moves. Rebuilding the picture from the journal is
//! GAP-045, and until it exists this panel must not look like a time machine. An analyst
//! who scrubbed to 14:32, saw the live picture, and read it as the picture at 14:32 would
//! draw a conclusion about a moment they never actually looked at -- which is why the
//! limitation is a field on the view and drawn every frame rather than a note in a
//! wireframe.
//!
//! # Replaying the session you are still recording
//!
//! The disconnected desktop journals to the same session it is running. Opening that
//! session for replay is legitimate -- "what just happened" is the common question -- but
//! its length grows underneath the cursor, so [`OpenReplay::live`] says so. A fixed
//! length shown for a growing session would make "42 of 100" mean something different
//! every second.

use crate::panels::unavailable::{draw_unavailable, Unavailable};
use crate::theme;
use egui::{RichText, Ui};

/// One session in the journal, as the picker lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionSummary {
    pub id: u64,
    /// `None` when the session's length has not been read; the picker says so rather
    /// than showing zero, which would read as an empty recording.
    pub envelopes: Option<usize>,
    /// Whether this is the session the desktop is currently writing to.
    pub live: bool,
}

/// How fast the cursor advances on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayRate {
    #[default]
    Paused,
    X1,
    X4,
    X16,
}

impl PlayRate {
    /// Envelopes advanced per second of wall time. Paused advances none.
    #[must_use]
    pub fn envelopes_per_second(self) -> f32 {
        match self {
            PlayRate::Paused => 0.0,
            PlayRate::X1 => 1.0,
            PlayRate::X4 => 4.0,
            PlayRate::X16 => 16.0,
        }
    }

    fn label(self) -> &'static str {
        match self {
            PlayRate::Paused => "Paused",
            PlayRate::X1 => "1x",
            PlayRate::X4 => "4x",
            PlayRate::X16 => "16x",
        }
    }

    /// The rates the panel offers, in order.
    pub const ALL: [PlayRate; 4] = [PlayRate::Paused, PlayRate::X1, PlayRate::X4, PlayRate::X16];
}

/// The session currently loaded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpenReplay<'a> {
    pub session: u64,
    /// Cursor position: how many envelopes have been stepped.
    pub position: usize,
    pub length: usize,
    pub remaining: usize,
    /// The replay clock: the mission time of the most recently stepped envelope.
    pub clock_s: f64,
    /// A one-line description of the envelope at the cursor, if there is one.
    pub cursor_event: Option<&'a str>,
    /// This session is still being written to, so `length` grows under the cursor.
    pub live: bool,
}

/// Everything PN-12 draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReplayView<'a> {
    pub sessions: &'a [SessionSummary],
    pub open: Option<OpenReplay<'a>>,
    pub rate: PlayRate,
    /// Why the picture does not move when the cursor does.
    pub reconstruction: Unavailable<'a>,
}

/// What the analyst asked for this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReplayAction {
    Open(u64),
    Close,
    Step,
    /// Move the cursor to this fraction of the session, 0.0 to 1.0.
    SeekFraction(f32),
    SetRate(PlayRate),
}

/// Render the replay timeline.
pub fn render_replay(ui: &mut Ui, view: &ReplayView<'_>) -> Option<ReplayAction> {
    ui.heading("Replay");
    // Drawn before anything else: it governs how everything below should be read.
    ui.label(
        RichText::new(
            "Scrubbing moves a cursor through the recorded events. It does not rebuild \
             the picture: the viewport and the track table stay live.",
        )
        .color(theme::WARNING_COLOR),
    );
    draw_unavailable(ui, view.reconstruction);
    ui.separator();

    let mut action = None;
    match view.open {
        None => action = draw_picker(ui, view.sessions),
        Some(open) => {
            if let Some(a) = draw_open(ui, &open, view.rate) {
                action = Some(a);
            }
        }
    }
    action
}

fn draw_picker(ui: &mut Ui, sessions: &[SessionSummary]) -> Option<ReplayAction> {
    ui.strong("Sessions in this journal");
    if sessions.is_empty() {
        ui.label(RichText::new("No sessions recorded.").color(theme::MUTED_TEXT_COLOR));
        return None;
    }
    let mut action = None;
    for s in sessions {
        ui.horizontal(|ui| {
            if ui.button(format!("Session {}", s.id)).clicked() {
                action = Some(ReplayAction::Open(s.id));
            }
            match s.envelopes {
                Some(n) => {
                    ui.label(RichText::new(format!("{n} events")).color(theme::MUTED_TEXT_COLOR));
                }
                None => {
                    ui.label(RichText::new("length not read").color(theme::MUTED_TEXT_COLOR));
                }
            }
            if s.live {
                ui.label(RichText::new("recording now").color(theme::WARNING_COLOR));
            }
        });
    }
    action
}

fn draw_open(ui: &mut Ui, open: &OpenReplay<'_>, rate: PlayRate) -> Option<ReplayAction> {
    let mut action = None;

    ui.horizontal(|ui| {
        ui.strong(format!("Session {}", open.session));
        if ui.button("Close").clicked() {
            action = Some(ReplayAction::Close);
        }
    });
    if open.live {
        ui.label(
            RichText::new("This session is still recording, so its length grows while you scrub.")
                .color(theme::WARNING_COLOR),
        );
    }

    ui.label(format!(
        "{} of {} events, {} remaining",
        open.position, open.length, open.remaining
    ));
    ui.label(format!("Replay clock {:.1} s", open.clock_s));
    match open.cursor_event {
        Some(text) => {
            ui.label(RichText::new(text).size(theme::SMALL_FONT_SIZE));
        }
        None => {
            ui.label(RichText::new("At the end of the session.").color(theme::MUTED_TEXT_COLOR));
        }
    }

    // The scrubber. Guarded against a zero-length session, where a fraction has no
    // meaning and a slider would divide by zero.
    if open.length > 0 {
        #[allow(clippy::cast_precision_loss)]
        let mut fraction = open.position as f32 / open.length as f32;
        if ui
            .add(egui::Slider::new(&mut fraction, 0.0..=1.0).text("position"))
            .changed()
        {
            action = Some(ReplayAction::SeekFraction(fraction));
        }
    } else {
        ui.label(RichText::new("This session recorded nothing.").color(theme::MUTED_TEXT_COLOR));
    }

    ui.horizontal(|ui| {
        if ui.button("Step").clicked() {
            action = Some(ReplayAction::Step);
        }
        for r in PlayRate::ALL {
            if ui.selectable_label(rate == r, r.label()).clicked() {
                action = Some(ReplayAction::SetRate(r));
            }
        }
    });

    action
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paused means paused. A rate that advanced the cursor while showing "Paused"
    /// would move an analyst's position without them asking.
    #[test]
    fn paused_advances_nothing_and_the_rates_increase() {
        assert!((PlayRate::Paused.envelopes_per_second()).abs() < f32::EPSILON);
        let rates: Vec<f32> = PlayRate::ALL
            .iter()
            .map(|r| r.envelopes_per_second())
            .collect();
        for pair in rates.windows(2) {
            assert!(pair[1] > pair[0], "the rates must increase: {rates:?}");
        }
    }

    /// An unread length is not a length of zero: one means "we have not looked", the
    /// other means "this session recorded nothing", and an analyst choosing a session
    /// to review would act differently on each.
    #[test]
    fn an_unread_length_is_not_an_empty_session() {
        let unread = SessionSummary {
            id: 1,
            envelopes: None,
            live: false,
        };
        let empty = SessionSummary {
            id: 1,
            envelopes: Some(0),
            live: false,
        };
        assert_ne!(unread, empty);
    }

    /// The limitation that governs the whole panel is a required field, so a build
    /// cannot ship the scrubber without the sentence that says it does not rebuild
    /// the picture.
    #[test]
    fn the_reconstruction_limit_names_its_gap() {
        let view = ReplayView {
            sessions: &[],
            open: None,
            rate: PlayRate::default(),
            reconstruction: Unavailable {
                owner: "gungnir-replay",
                gap: "GAP-045",
            },
        };
        assert_eq!(
            view.rate,
            PlayRate::Paused,
            "a replay does not start moving"
        );
        assert!(view.reconstruction.gap.starts_with("GAP-"));
    }
}
