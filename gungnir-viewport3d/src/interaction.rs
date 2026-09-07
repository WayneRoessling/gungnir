//! Picking and camera controls. The 3D orbit/pan/zoom controls come from three-d's
//! built-ins (rust-ui-tech-stack-summary.md §2) once the scene is attached; the
//! [`TopDownView`] here drives the 2D fallback projection and is pure math, so it is
//! unit-tested without a GPU (rust-ui-architecture-coding-standards.md §8).

use gungnir_ui::theme;

/// A top-down (E,N) view of the local ENU frame mapped onto a screen rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TopDownView {
    /// ENU coordinates at the centre of the rectangle, meters.
    pub center_e: f64,
    pub center_n: f64,
    /// Scale: meters per screen pixel. Smaller is more zoomed in.
    pub meters_per_px: f64,
}

impl Default for TopDownView {
    fn default() -> Self {
        Self {
            center_e: 0.0,
            center_n: 0.0,
            meters_per_px: 10.0,
        }
    }
}

const MIN_METERS_PER_PX: f64 = 0.1;
const MAX_METERS_PER_PX: f64 = 10_000.0;
/// Zoom factor per scroll unit (egui reports points).
const ZOOM_PER_SCROLL_POINT: f64 = 1.0025;

// Screen coordinates are f32 by egui's design; narrowing from the f64 world frame
// is intentional and happens once per glyph.
#[allow(clippy::cast_possible_truncation)]
impl TopDownView {
    /// ENU position to screen position within `rect`; north is up.
    pub fn project(&self, enu: [f64; 3], rect: egui::Rect) -> egui::Pos2 {
        let dx = (enu[0] - self.center_e) / self.meters_per_px;
        let dy = (enu[1] - self.center_n) / self.meters_per_px;
        egui::pos2(rect.center().x + dx as f32, rect.center().y - dy as f32)
    }

    /// Inverse of [`project`](Self::project) for the ground plane (u = 0).
    pub fn unproject(&self, pos: egui::Pos2, rect: egui::Rect) -> [f64; 3] {
        let dx = f64::from(pos.x - rect.center().x) * self.meters_per_px;
        let dy = f64::from(rect.center().y - pos.y) * self.meters_per_px;
        [self.center_e + dx, self.center_n + dy, 0.0]
    }

    /// Meters to pixels at the current scale.
    pub fn px(&self, meters: f64) -> f32 {
        (meters / self.meters_per_px) as f32
    }

    /// Drag pans; scroll zooms about the centre.
    pub fn apply_input(&mut self, response: &egui::Response, scroll_delta_y: f32) {
        if response.dragged() {
            let d = response.drag_delta();
            self.center_e -= f64::from(d.x) * self.meters_per_px;
            self.center_n += f64::from(d.y) * self.meters_per_px;
        }
        if response.hovered() && scroll_delta_y != 0.0 {
            self.zoom_by(ZOOM_PER_SCROLL_POINT.powf(-f64::from(scroll_delta_y)));
        }
    }

    pub fn zoom_by(&mut self, factor: f64) {
        self.meters_per_px =
            (self.meters_per_px * factor).clamp(MIN_METERS_PER_PX, MAX_METERS_PER_PX);
    }
}

/// Grid spacing that keeps roughly 60-200 px between lines at the current scale.
pub fn grid_spacing_m(view: &TopDownView) -> f64 {
    let target_m = view.meters_per_px * 100.0;
    let exponent = target_m.log10().floor();
    let base = 10_f64.powf(exponent);
    let mantissa = target_m / base;
    let step = if mantissa < 2.0 {
        1.0
    } else if mantissa < 5.0 {
        2.0
    } else {
        5.0
    };
    step * base
}

/// Draw a metric grid over `rect` for orientation.
pub fn draw_grid(painter: &egui::Painter, rect: egui::Rect, view: &TopDownView) {
    let spacing = grid_spacing_m(view);
    let stroke = egui::Stroke::new(theme::STROKE_HAIRLINE, theme::VIEWPORT_GRID_COLOR);
    let [min_e, min_n, _] = view.unproject(rect.left_bottom(), rect);
    let [max_e, max_n, _] = view.unproject(rect.right_top(), rect);
    let mut e = (min_e / spacing).floor() * spacing;
    while e <= max_e {
        let x = view.project([e, 0.0, 0.0], rect).x;
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            stroke,
        );
        e += spacing;
    }
    let mut n = (min_n / spacing).floor() * spacing;
    while n <= max_n {
        let y = view.project([0.0, n, 0.0], rect).y;
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            stroke,
        );
        n += spacing;
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn rect() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 200.0))
    }

    #[test]
    fn project_and_unproject_are_inverse_on_ground_plane() {
        let view = TopDownView {
            center_e: 100.0,
            center_n: -50.0,
            meters_per_px: 2.5,
        };
        let enu = [130.0, -20.0, 0.0];
        let p = view.project(enu, rect());
        let back = view.unproject(p, rect());
        assert!((back[0] - enu[0]).abs() < 1e-3);
        assert!((back[1] - enu[1]).abs() < 1e-3);
    }

    #[test]
    fn north_is_up_and_east_is_right() {
        let view = TopDownView::default();
        let origin = view.project([0.0, 0.0, 0.0], rect());
        let north = view.project([0.0, 100.0, 0.0], rect());
        let east = view.project([100.0, 0.0, 0.0], rect());
        assert!(north.y < origin.y);
        assert!(east.x > origin.x);
    }

    #[test]
    fn zoom_is_clamped() {
        let mut view = TopDownView::default();
        view.zoom_by(1e-9);
        assert_eq!(view.meters_per_px, MIN_METERS_PER_PX);
        view.zoom_by(1e12);
        assert_eq!(view.meters_per_px, MAX_METERS_PER_PX);
    }

    #[test]
    fn grid_spacing_is_a_round_number() {
        let view = TopDownView {
            meters_per_px: 3.7,
            ..TopDownView::default()
        };
        let s = grid_spacing_m(&view);
        assert!(
            [1.0, 2.0, 5.0]
                .iter()
                .any(|k| ((s / k).log10().fract()).abs() < 1e-9),
            "spacing {s}"
        );
    }
}
