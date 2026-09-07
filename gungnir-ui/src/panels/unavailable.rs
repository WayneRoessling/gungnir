//! One section of a panel that this build cannot fill, and what owns it (GAP-073).
//!
//! Three panels need the same idea and it is worth having exactly one type for it: an
//! empty section and an unconnected section are different claims, and the difference is
//! what AP-02 and `CONTRIBUTING.md`'s "No fake wiring" rule are about. A panel that
//! renders "0 pending approvals" or "no evidence" from a subsystem it never called has
//! told the operator something false in the most convincing form available -- a number.
//!
//! So a panel section is a `Result<T, Unavailable>` or a [`Section`], never a bare `T`
//! that happens to be empty, and the unavailable arm has to name a crate and a register
//! entry. Naming them is not decoration: it is what makes the claim checkable, and the
//! tests in each panel assert that both are named.

use crate::theme;
use egui::{RichText, Ui};

/// A section this build cannot fill: which crate supplies it, and the register entry
/// that will wire it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unavailable<'a> {
    /// The crate that owns the data, e.g. `gungnir-assessment`.
    pub owner: &'a str,
    /// The register entry that will connect it, e.g. `GAP-028`.
    pub gap: &'a str,
}

impl Unavailable<'_> {
    /// The sentence a panel shows in place of the section.
    #[must_use]
    pub fn sentence(&self) -> String {
        format!(
            "Not available: {} supplies this, tracked as {}.",
            self.owner, self.gap
        )
    }
}

/// Draw the unavailable sentence in the muted style every panel uses for it.
pub fn draw_unavailable(ui: &mut Ui, u: Unavailable<'_>) {
    ui.label(RichText::new(u.sentence()).color(theme::MUTED_TEXT_COLOR));
}

/// Either a list of items, or an honest account of why there is none.
///
/// `Present(&[])` and `Unavailable(..)` are deliberately distinct and compare unequal:
/// "nothing to report" and "nobody asked" are different facts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Section<'a, T> {
    Present(&'a [T]),
    Unavailable(Unavailable<'a>),
    /// Nothing to list because of the deployment's own state -- no origin declared, no
    /// assets listed -- rather than because a crate is unbuilt. The reason is drawn; it
    /// is not a gap and does not name one.
    Empty {
        reason: &'a str,
    },
}

impl<T> Section<'_, T> {
    /// Draw the section's heading and, when there is nothing to list, the reason.
    ///
    /// Returns whether the caller should go on to draw rows.
    pub fn draw_header(&self, ui: &mut Ui, title: &str) -> bool {
        ui.strong(title);
        match self {
            Section::Present([]) => {
                ui.label(RichText::new("None recorded.").color(theme::MUTED_TEXT_COLOR));
                false
            }
            Section::Present(_) => true,
            Section::Unavailable(u) => {
                draw_unavailable(ui, *u);
                false
            }
            Section::Empty { reason } => {
                ui.label(RichText::new(*reason).color(theme::MUTED_TEXT_COLOR));
                false
            }
        }
    }

    /// The items, or `None` when the section is unavailable.
    #[must_use]
    pub fn items(&self) -> Option<&[T]> {
        match self {
            Section::Present(items) => Some(items),
            Section::Unavailable(_) | Section::Empty { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The distinction the type exists to preserve.
    #[test]
    fn an_unavailable_section_is_not_an_empty_one() {
        let empty: Section<'_, u8> = Section::Present(&[]);
        let unavailable: Section<'_, u8> = Section::Unavailable(Unavailable {
            owner: "gungnir-assessment",
            gap: "GAP-028",
        });
        assert_ne!(empty, unavailable);
        assert_eq!(empty.items(), Some(&[][..]));
        assert_eq!(unavailable.items(), None);
    }

    /// A reason that named neither a crate nor a register entry would be an apology
    /// rather than a status, and could not be checked against the register.
    #[test]
    fn the_reason_names_a_crate_and_a_register_entry() {
        let u = Unavailable {
            owner: "gungnir-command",
            gap: "GAP-038",
        };
        assert!(u.owner.starts_with("gungnir-"));
        assert!(u.gap.starts_with("GAP-"));
        assert!(u.sentence().contains("gungnir-command"));
        assert!(u.sentence().contains("GAP-038"));
    }
}
