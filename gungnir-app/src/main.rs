//! main.rs -- eframe app bootstrap only, no logic here, per
//! rust-ui-architecture-coding-standards.md §1. Runs eframe with `Renderer::Glow`
//! because the 3D viewport draws through three-d's OpenGL context
//! (ARCHITECTURE.md §4, §9).

use gungnir_app::dock;
use gungnir_app::hazards;
use gungnir_app::requirements;
use gungnir_app::state::AppState;
use gungnir_app::sustainment;
use gungnir_app::update;
use gungnir_app::workspace::PanelAction;
use gungnir_command::OperatorDecision;
use gungnir_ui::panels::decision_dialog::DecisionChoice;
use gungnir_ui::panels::requirements::RequirementAction;
use gungnir_workflow::PanelId;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Target redraw cadence when nothing else requests a repaint.
const REPAINT_INTERVAL: Duration = Duration::from_millis(33);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let mut state = AppState::new()?;
    // GAP-089: `--rehearsal <seed.json>` seeds the session and marks the record.
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--rehearsal") {
        let Some(path) = args.get(i + 1) else {
            return Err("--rehearsal needs a seed file".into());
        };
        let (seed, hash) = gungnir_app::rehearsal::load_seed(std::path::Path::new(path))?;
        gungnir_app::rehearsal::install(&mut state, seed, hash)?;
        tracing::warn!(seed = %path, "this session is a rehearsal; the record says so");
    }

    let native_options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Gungnir",
        native_options,
        Box::new(move |cc| {
            // DS-07: egui's own chrome takes the theme tokens before the first frame,
            // so the panels and the picture are one surface.
            gungnir_ui::theme::install_egui_theme(&cc.egui_ctx);
            // GAP-022: attach three-d to the GL context eframe owns. A failure is
            // logged and the desktop carries on with the 2D projection: a rendering
            // feature must not stop the picture from coming up.
            let scene = match gungnir_viewport3d::gl::SceneRenderer::attach(cc.gl.clone()) {
                Ok(renderer) => {
                    tracing::info!("three-d scene attached to the OpenGL context");
                    Some(Arc::new(Mutex::new(renderer)))
                }
                Err(err) => {
                    tracing::warn!(%err, "three-d scene not attached; using the 2D projection");
                    None
                }
            };
            // The session starts on whichever renderer the baseline asks for, and only
            // if one is actually attached: `use_3d` must never claim a renderer that is
            // not there.
            let mut state = state;
            state.viewport.gl_ready = scene.is_some();
            state.viewport.use_3d = scene.is_some() && state.config.ui.scene_3d;
            Ok(Box::new(App {
                state,
                sustainment: sustainment::SustainmentState::default(),
                workspace: Workspace::default(),
                scene,
            }))
        }),
    )?;
    Ok(())
}

/// Which half of PN-15's scratch a successful action should clear.
///
/// Stating clears the new-requirement form; the other three clear the composed action.
/// Clearing both would wipe a half-typed requirement every time somebody declined an
/// unrelated one.
enum Committed {
    Statement,
    Action,
}

struct App {
    state: AppState,
    /// Session state the three sustainment panels own (GAP-071). It lives here rather
    /// than in `AppState` because none of it is mission state: a half-scrubbed replay
    /// cursor, the last report generated, and an unapplied candidate baseline are
    /// things this window is doing, not things the mission is.
    sustainment: sustainment::SustainmentState,
    /// The dock tree and what is in a second window (GAP-075). Rebuilt when the role
    /// changes, because the arrangement is per role.
    workspace: Workspace,
    /// The three-d scene, once attached to eframe's OpenGL context (GAP-022).
    ///
    /// `None` when attachment failed or the deployment has not opted in, in which case
    /// the viewport keeps the 2D projection. Behind an `Arc<Mutex<..>>` because
    /// `egui_glow`'s paint callback runs later in the frame and owns what it captures.
    scene: Option<Arc<Mutex<gungnir_viewport3d::gl::SceneRenderer>>>,
}

/// The arrangement on screen: the main window's tree and the detached panels.
///
/// Session-local. Dragging a panel changes this and not the baseline: D-17 saves the
/// arrangement *per role in the configuration baseline*, which is a deployment decision
/// needing `config.apply`, so a shift's worth of rearranging must not quietly become
/// the deployment's configuration.
#[derive(Default)]
struct Workspace {
    tree: Option<egui_tiles::Tree<dock::Pane>>,
    detached: Vec<PanelId>,
    /// The role the tree was built for, so a role change rebuilds it.
    built_for: Option<gungnir_security::Role>,
}

impl eframe::App for App {
    /// D-04: fsync when the session closes. Without this a clean exit could lose up
    /// to the 5 s buffered interval from a session the operator had just finished,
    /// which is the moment losing it is least excusable.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // GAP-051: saving and closing are two things. The first makes the envelopes
        // durable; the second records that this session ended on purpose, so the next
        // launch does not report it as interrupted.
        match self.state.close_session() {
            Ok(()) => tracing::info!("session saved and closed"),
            Err(err) => tracing::error!(
                %err,
                "session close did not complete; the session stays recorded as unclosed"
            ),
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        update::tick(&mut self.state);

        // Table-of-contents style, per rust-ui-architecture-coding-standards.md §6:
        // one line per area of the window, in the order they are drawn. Panels return
        // what was clicked rather than writing to `AppState`, and the last `Some` wins
        // because at most one panel is clicked per frame.
        self.draw_status_strip(ctx);
        let mut action = self.draw_workspace(ctx);
        if !self.workspace.detached.contains(&PanelId::ApprovalQueue) {
            // With the queue docked, its dialog belongs to this window. When the queue
            // is detached, `draw_detached` draws the dialog there instead.
            action = self.draw_decision_dialog(ctx).or(action);
        }
        self.draw_evidence_card(ctx);
        self.draw_viewport(ctx);
        action = self.draw_detached(ctx).or(action);

        // Applied after every panel has been drawn: a click changes what the *next*
        // frame shows, never what the current one has already half-drawn.
        if let Some(action) = action {
            self.apply(action);
        }

        // The replay cursor advances on wall time, not mission time: it is a review
        // control, and an analyst watching at 4x expects four events a second whatever
        // the recording's own pacing was. Capped so a stalled frame does not jump the
        // cursor a long way.
        let dt = ctx.input(|i| i.stable_dt).min(0.25);
        self.sustainment.replay.advance(dt);

        ctx.request_repaint_after(REPAINT_INTERVAL);
    }
}

impl App {
    /// GAP-072: PN-01 is on every layout, so it is a top panel rather than a slot in
    /// any one workspace. It reads and writes nothing.
    fn draw_status_strip(&self, ctx: &egui::Context) {
        let strip_data = gungnir_app::status::StatusStripData::from_state(&self.state);
        egui::TopBottomPanel::top("status_strip").show(ctx, |ui| {
            gungnir_ui::panels::status_strip::render_status_strip(
                ui,
                &gungnir_app::status::status_strip_view(&self.state, &strip_data),
            );
        });
    }

    /// GAP-055 and GAP-075: the side panel is the current role's workspace, drawn as a
    /// dock tree the operator can rearrange. The panels and their order come from
    /// `WorkspaceLayout`, which is tested against the plan 06 layout table, or from the
    /// baseline's arrangement for this role when it has one.
    fn draw_workspace(&mut self, ctx: &egui::Context) -> Option<PanelAction> {
        self.ensure_tree();
        let mut action = None;
        egui::SidePanel::left("dashboard")
            .resizable(true)
            .default_width(gungnir_ui::theme::DASHBOARD_DEFAULT_WIDTH)
            .show(ctx, |ui| {
                render_session_header(ui, &self.state);
                ui.separator();
                let Some(tree) = self.workspace.tree.as_mut() else {
                    ui.label("This role has no docked panels.");
                    return;
                };
                let mut behavior = dock::PanelBehavior::new(&self.state, &mut self.sustainment);
                tree.ui(&mut behavior, ui);
                action = behavior.action;
            });
        action
    }

    /// Build the dock tree for the current role, if it is not already built for it.
    ///
    /// The baseline's arrangement wins when it has one for this role; otherwise the
    /// default is the role's docked panels in the order `WorkspaceLayout` gives. Either
    /// way the detached panels are pruned from the tree, so nothing is drawn twice.
    fn ensure_tree(&mut self) {
        let role = self.state.role();
        if self.workspace.built_for == Some(role) {
            return;
        }
        let role_name = format!("{role:?}");
        let configured = self.state.config.ui.for_role(&role_name);
        let arrangement = configured.map_or_else(
            || dock::default_arrangement(self.state.layout()),
            |l| l.main.clone(),
        );
        self.workspace.detached = configured
            .map(|l| {
                l.detached
                    .iter()
                    .filter_map(|pn| dock::panel_for_pn(pn))
                    .filter(|p| dock::may_detach(*p))
                    .collect()
            })
            .unwrap_or_default();

        let detached_pns: Vec<&str> = self.workspace.detached.iter().map(|p| p.pn()).collect();
        self.workspace.tree = arrangement
            .without(&detached_pns)
            .map(|a| dock::tree_from(&a));
        self.workspace.built_for = Some(role);
    }

    /// GAP-075: the panels D-17 allows in a second window, each as an egui native
    /// viewport. No crate is needed for this half; egui 0.29 provides it.
    ///
    /// The decision dialog follows the approval queue: if the queue is detached, PN-07
    /// is drawn in that window. D-17 says decision dialogs stay with the queue, and a
    /// decision separated from the queue it came from is a decision taken without its
    /// context.
    fn draw_detached(&mut self, ctx: &egui::Context) -> Option<PanelAction> {
        let mut action = None;
        for panel in self.workspace.detached.clone() {
            let id = egui::ViewportId::from_hash_of(panel.pn());
            let builder = egui::ViewportBuilder::default()
                .with_title(format!("Gungnir -- {}", panel.title()))
                .with_inner_size([900.0, 700.0]);
            let mut inner = None;
            ctx.show_viewport_immediate(id, builder, |ctx, _class| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    inner = self.draw_detached_panel(ui, panel);
                });
                if panel == PanelId::ApprovalQueue {
                    // `take` rather than `or`: `PanelAction` stopped being `Copy` when
                    // PN-15's actions began carrying the operator's own words, and this
                    // closure is `FnMut`. The dialog still wins over the panel beneath
                    // it, which is the behaviour that matters here.
                    inner = self.draw_decision_dialog(ctx).or(inner.take());
                }
            });
            action = inner.or(action);
        }
        action
    }

    fn draw_detached_panel(&mut self, ui: &mut egui::Ui, panel: PanelId) -> Option<PanelAction> {
        match panel {
            PanelId::Viewport3d => {
                // The detached window shows the same coverage as the docked one: two
                // viewports disagreeing about what is covered would be worse than
                // either being wrong.
                let circles = sustainment::coverage_circles(&self.state);
                let coverage = sustainment::coverage_layer(&self.state, &circles);
                let report = sustainment::coverage_report(&self.state);
                let names = sustainment::approach_names(&self.state);
                let gaps = report
                    .as_ref()
                    .map(|r| sustainment::gap_polylines(&names, r))
                    .unwrap_or_default();
                let placed = hazards::placed(&self.state);
                let hazards = hazards::outlines(&placed);
                let fences = gungnir_app::geofences::placed(&self.state);
                let geofences = gungnir_app::geofences::outlines(&fences);
                let predicted =
                    gungnir_app::prediction::paths(&gungnir_app::prediction::predict(&self.state));
                let predictions: Vec<gungnir_viewport3d::layers::PredictedPath> = predicted
                    .iter()
                    .map(|p| gungnir_viewport3d::layers::PredictedPath {
                        track: p.track,
                        points: &p.points,
                        uncertainty_drawable: p.uncertainty_drawable,
                    })
                    .collect();
                gungnir_viewport3d::render(
                    ui,
                    &mut self.state.viewport,
                    self.state.tracking.tracks(),
                    &self.state.last_plan,
                    gungnir_viewport3d::layers::LayerInputs {
                        coverage,
                        gaps: &gaps,
                        hazards: &hazards,
                        geofences: &geofences,
                        predictions: &predictions,
                        terrain: gungnir_app::terrain::layer(&self.state.terrain, &self.state.data),
                    },
                );
                None
            }
            other => self.draw_docked(ui, other),
        }
    }

    /// One docked slot. The three sustainment panels (GAP-071) are drawn here rather
    /// than through `render_panel` because they read state this window owns.
    fn draw_docked(&mut self, ui: &mut egui::Ui, panel: PanelId) -> Option<PanelAction> {
        match panel {
            PanelId::Replay => {
                if !self.sustainment.sessions_read {
                    // A disk read per session, so it is taken when the picker appears
                    // rather than every frame.
                    self.sustainment.sessions = sustainment::session_summaries(&self.state);
                    self.sustainment.sessions_read = true;
                }
                gungnir_app::workspace::render_replay(
                    ui,
                    &self.state,
                    &self.sustainment.replay,
                    &self.sustainment.sessions,
                )
            }
            PanelId::Reports => {
                gungnir_app::workspace::render_reports(ui, &self.state, &mut self.sustainment)
            }
            PanelId::Requirements => gungnir_app::workspace::render_requirements(
                ui,
                &self.state,
                &mut self.sustainment.requirements,
            ),
            PanelId::Audit => {
                gungnir_app::workspace::render_audit(ui, &self.state, &mut self.sustainment)
            }
            PanelId::ConfigEditor => gungnir_app::workspace::render_config_editor(
                ui,
                &self.state,
                &self.sustainment.config,
            ),
            // GAP-054: PN-17 carries the handover at shift change, and the outgoing
            // watch's notes are a scratch buffer this window owns.
            PanelId::CommanderSummary => gungnir_app::workspace::render_commander_summary(
                ui,
                &self.state,
                &mut self.sustainment.handover_notes,
            ),
            _ => gungnir_app::workspace::render_panel(ui, panel, &self.state),
        }
    }

    /// GAP-038: PN-07 is a modal on the item chosen in PN-06, drawn before PN-04 so
    /// that if both are open the decision surface is the one in front. An evidence
    /// card is something to read; a decision dialog is something to act on, and the
    /// act must not be the thing behind a window.
    fn draw_decision_dialog(&mut self, ctx: &egui::Context) -> Option<PanelAction> {
        self.state.selected_approval()?;
        let mut dialog = std::mem::take(&mut self.state.dialog);
        let mut action = None;
        let mut open = true;
        egui::Window::new(PanelId::DecisionDialog.title())
            .open(&mut open)
            .collapsible(false)
            .show(ctx, |ui| {
                action =
                    gungnir_app::workspace::render_decision_dialog(ui, &self.state, &mut dialog);
            });
        self.state.dialog = dialog;
        if !open {
            // Closing the dialog abandons the decision. Nothing is recorded, which is
            // the correct outcome: an abandoned decision is not a rejection, and the
            // item stays in the queue.
            self.state.clear_selected_approval();
        }
        action
    }

    /// GAP-073: PN-04 is an on-demand panel -- `information-architecture.md` §1 opens
    /// it *on selection* rather than docking it -- so it is a window over the viewport,
    /// shown only while a track is selected and only for the roles whose layout may
    /// open it. Closing the window clears the selection, so the two cannot disagree
    /// about whether a card is open.
    fn draw_evidence_card(&mut self, ctx: &egui::Context) {
        if !self.state.layout().may_open(PanelId::TrackDetail) {
            return;
        }
        let Some(id) = self.state.selected_track() else {
            return;
        };
        let mut open = true;
        egui::Window::new(PanelId::TrackDetail.title())
            .open(&mut open)
            .show(ctx, |ui| {
                gungnir_app::workspace::render_panel(ui, PanelId::TrackDetail, &self.state);
            });
        if !open {
            self.state.select_track(id); // re-selecting the shown track clears it
        }
    }

    /// The central area. When the viewport has been detached (GAP-075) it says so
    /// rather than leaving a blank centre, which would read as a viewport showing
    /// nothing.
    fn draw_viewport(&mut self, ctx: &egui::Context) {
        let detached = self.workspace.detached.contains(&PanelId::Viewport3d);
        egui::CentralPanel::default().show(ctx, |ui| {
            if detached {
                ui.label("The viewport is in its own window.");
                return;
            }
            self.draw_picture(ui);
        });
    }

    /// The picture itself: the three-d scene when it is attached and the deployment has
    /// asked for it, otherwise the 2D projection.
    ///
    /// The 2D path is the default (GAP-022). It is the one that has been verified, and
    /// the GL draw call has not been looked at on a screen; a deployment that can look
    /// at it sets `ui.scene_3d`.
    fn draw_picture(&mut self, ui: &mut egui::Ui) {
        // GAP-007. Built before either branch so both renderers show the same coverage,
        // and so an unplaceable layer says so on whichever one is running.
        let circles = sustainment::coverage_circles(&self.state);
        let coverage = sustainment::coverage_layer(&self.state, &circles);
        // GAP-006: gaps along the declared approaches, drawn on whichever renderer is
        // running so the two cannot disagree about where the holes are.
        let report = sustainment::coverage_report(&self.state);
        let names = sustainment::approach_names(&self.state);
        let gaps = report
            .as_ref()
            .map(|r| sustainment::gap_polylines(&names, r))
            .unwrap_or_default();
        // GAP-017: the hazard layer, on whichever renderer is running.
        let placed = hazards::placed(&self.state);
        let hazards = hazards::outlines(&placed);
        // GAP-088: the fences, a rule layer, on whichever renderer is running.
        let fences = gungnir_app::geofences::placed(&self.state);
        let geofences = gungnir_app::geofences::outlines(&fences);
        // GAP-020: predicted lines, on whichever renderer is running.
        let predicted =
            gungnir_app::prediction::paths(&gungnir_app::prediction::predict(&self.state));
        let predictions: Vec<gungnir_viewport3d::layers::PredictedPath> = predicted
            .iter()
            .map(|p| gungnir_viewport3d::layers::PredictedPath {
                track: p.track,
                points: &p.points,
                uncertainty_drawable: p.uncertainty_drawable,
            })
            .collect();

        let layers = gungnir_viewport3d::layers::LayerInputs {
            coverage,
            gaps: &gaps,
            hazards: &hazards,
            geofences: &geofences,
            predictions: &predictions,
            terrain: gungnir_app::terrain::layer(&self.state.terrain, &self.state.data),
        };
        let Some(scene) = self.scene.clone().filter(|_| self.state.viewport.use_3d) else {
            gungnir_viewport3d::render(
                ui,
                &mut self.state.viewport,
                self.state.tracking.tracks(),
                &self.state.last_plan,
                layers,
            );
            return;
        };

        let Some(rect) = gungnir_viewport3d::prepare_3d(
            ui,
            &mut self.state.viewport,
            self.state.tracking.tracks(),
            &self.state.last_plan,
            layers,
        ) else {
            return;
        };

        // The callback runs after this function returns, so it owns copies rather than
        // borrowing: the glyph list is rebuilt only when the track set changes, so this
        // clone is per frame and small.
        let glyphs = self.state.viewport.glyphs.clone();
        let view = self.state.viewport.view;
        let callback = eframe::egui_glow::CallbackFn::new(move |info, _painter| {
            let pixels = info.viewport_in_pixels();
            let viewport = gungnir_viewport3d::gl::viewport_from(
                pixels.left_px,
                pixels.from_bottom_px,
                pixels.width_px,
                pixels.height_px,
            );
            // A poisoned lock means a previous frame panicked inside the callback.
            // Skipping the draw leaves the status line and the panels intact rather
            // than taking the window down with it.
            if let Ok(mut renderer) = scene.lock() {
                renderer.paint(&glyphs, &view, viewport);
            }
        });
        ui.painter().add(egui::PaintCallback {
            rect,
            callback: Arc::new(callback),
        });
        gungnir_viewport3d::draw_renderer_toggle(ui, rect, &mut self.state.viewport);
    }

    /// Apply what was clicked. Exhaustive on purpose: a new panel action must be given
    /// an effect here rather than being silently dropped.
    /// PN-17's handover (GAP-054).
    ///
    /// Applied after the frame like every other action: an acknowledgement publishes an
    /// event, and publishing mid-draw would put a journal write inside the paint.
    fn apply_handover(&mut self, action: gungnir_ui::panels::commander_summary::HandoverAction) {
        use gungnir_ui::panels::commander_summary::HandoverAction;
        let result = match action {
            HandoverAction::Notes(notes) => {
                gungnir_app::rhythm::set_handover_notes(&mut self.state, notes)
            }
            HandoverAction::Acknowledge => {
                // The role is who is signed in; there is no operator identity to use
                // instead until GAP-057 puts one on the session, and a fabricated name in
                // a handover record would be worse than the role.
                let by = format!("{:?}", self.state.role());
                let taken = gungnir_app::rhythm::acknowledge_handover(&mut self.state, &by);
                if taken.is_ok() {
                    self.sustainment.handover_notes.clear();
                }
                taken
            }
        };
        if let Err(err) = result {
            self.state.alerts.push(format!("Handover: {err}"));
        }
    }

    fn apply(&mut self, action: PanelAction) {
        match action {
            PanelAction::Handover(a) => self.apply_handover(a),
            PanelAction::SelectTrack(id) => self.state.select_track(id),
            PanelAction::SelectApproval(id) => self.state.select_approval(id),
            PanelAction::Decide(id, choice) => self.apply_decision(id, choice),
            PanelAction::CommandSensorMode(sensor, mode) => {
                match sustainment::command_sensor_mode(&mut self.state, sensor, mode) {
                    Ok(()) => self.state.sensor_error = None,
                    Err(err) => {
                        tracing::warn!(%err, sensor, "sensor command refused");
                        self.state.sensor_error = Some(err.to_string());
                    }
                }
            }
            PanelAction::RecordSensorMode(sensor, mode) => {
                match sustainment::record_observed_mode(&mut self.state, sensor, mode) {
                    Ok(()) => self.state.sensor_error = None,
                    Err(err) => {
                        tracing::warn!(%err, sensor, "recording an observed mode refused");
                        self.state.sensor_error = Some(err.to_string());
                    }
                }
            }
            PanelAction::CoverageLayer(a) => {
                use gungnir_ui::panels::coverage_layers::LayerAction;
                match a {
                    LayerAction::ShowRings(on) => self.state.viewport.layers.rings = on,
                    LayerAction::ShowGaps(on) => self.state.viewport.layers.gaps = on,
                    LayerAction::ShowHazards(on) => self.state.viewport.layers.hazards = on,
                    LayerAction::ShowGeofences(on) => self.state.viewport.layers.geofences = on,
                }
            }
            PanelAction::Requirement(a) => self.apply_requirement(a),
            PanelAction::Replay(a) => self.apply_replay(a),
            PanelAction::Reports(a) => self.apply_reports(a),
            PanelAction::Config(a) => self.apply_config(a),
            PanelAction::Session(a) => {
                gungnir_app::session::apply(&mut self.state, &mut self.sustainment.sign_in, a);
            }
            PanelAction::SwitchBack => {
                if let Err(reason) = gungnir_app::failover::switch_back(&mut self.state) {
                    self.state
                        .alerts
                        .push(format!("not switched back: {reason}"));
                }
            }
            PanelAction::ResolveConflict(plan, keep_local) => {
                if let Err(reason) =
                    gungnir_app::failover::resolve_conflict(&mut self.state, plan, keep_local)
                {
                    self.state
                        .alerts
                        .push(format!("conflict not resolved: {reason}"));
                }
            }
        }
    }

    /// PN-15's four outcomes (GAP-005).
    ///
    /// Every one of them can be refused, and every refusal is shown rather than logged:
    /// a decline that silently did nothing would leave the analyst waiting on a
    /// requirement the sensor manager believed they had closed.
    fn apply_requirement(&mut self, action: RequirementAction) {
        use gungnir_model::RequirementId;
        let now = self.state.clock.now();
        let outcome = match action {
            RequirementAction::State {
                title,
                area,
                priority,
                within_minutes,
            } => requirements::state_requirement(
                &mut self.state,
                title,
                area,
                priority,
                within_minutes,
            )
            .map(|_| Committed::Statement),
            RequirementAction::Task {
                requirement,
                sensor,
            } => requirements::task(&mut self.state, RequirementId(requirement), sensor, now)
                .map(|()| Committed::Action),
            RequirementAction::Decline {
                requirement,
                reason,
            } => requirements::decline(&mut self.state, RequirementId(requirement), reason)
                .map(|()| Committed::Action),
            RequirementAction::Satisfy {
                requirement,
                evidence,
            } => requirements::satisfy(&mut self.state, RequirementId(requirement), evidence)
                .map(|()| Committed::Action),
        };
        match outcome {
            // The text is cleared only on success. A refused decline keeps its reason in
            // the box, because the operator will want to change it rather than retype it.
            Ok(Committed::Statement) => {
                self.state.requirement_error = None;
                self.sustainment.requirements.clear_statement();
            }
            Ok(Committed::Action) => {
                self.state.requirement_error = None;
                self.sustainment.requirements.clear_action();
            }
            Err(err) => {
                tracing::warn!(%err, "requirement action refused");
                self.state.requirement_error = Some(err.to_string());
            }
        }
    }

    /// PN-07's three outcomes. The reject reason is read before `decide` clears the
    /// dialog: it is part of the record (DN-10 §3), and PN-07 will not enable the
    /// reject control without one.
    fn apply_decision(
        &mut self,
        id: gungnir_ui::panels::approval_queue::PendingId,
        choice: DecisionChoice,
    ) {
        let reason = self.state.dialog.reject_reason.trim().to_owned();
        let decision = match choice {
            DecisionChoice::Accept => OperatorDecision::Accepted,
            DecisionChoice::Override => OperatorDecision::Overridden,
            DecisionChoice::Reject => OperatorDecision::Rejected { reason },
        };
        if let Err(err) = gungnir_app::decisions::decide(&mut self.state, id, decision) {
            // The item is gone and the decision was not recorded. Say so rather than
            // closing the dialog as though it had been.
            tracing::error!(%err, "decision could not be recorded");
            self.state
                .alerts
                .push(format!("Decision not recorded: {err}"));
        }
    }

    /// PN-12. A failed open is an alert rather than a silent no-op: the panel would
    /// otherwise keep showing whatever was open before under the new session's heading.
    fn apply_replay(&mut self, action: gungnir_ui::panels::replay::ReplayAction) {
        use gungnir_ui::panels::replay::ReplayAction;
        match action {
            ReplayAction::Open(id) => {
                if let Err(err) = self
                    .sustainment
                    .replay
                    .open(&self.state, gungnir_store::SessionId(id))
                {
                    tracing::error!(%err, session = id, "could not open the session for replay");
                    self.state
                        .alerts
                        .push(format!("Replay of session {id} failed: {err}"));
                }
            }
            ReplayAction::Close => {
                self.sustainment.replay.close(&self.state);
                // The picker is shown again, so the lengths are worth re-reading: a
                // live session has grown since they were last taken.
                self.sustainment.sessions_read = false;
            }
            ReplayAction::Step => self.sustainment.replay.step(),
            ReplayAction::SeekFraction(f) => self.sustainment.replay.seek_fraction(f),
            ReplayAction::SetRate(r) => self.sustainment.replay.set_rate(r),
        }
    }

    /// PN-13. Both a failed generate and a failed export raise an alert: a report that
    /// silently did not export would leave the analyst believing a file exists.
    fn apply_reports(&mut self, action: gungnir_ui::panels::reports::ReportsAction) {
        use gungnir_ui::panels::reports::ReportsAction;
        let result = match action {
            ReportsAction::Generate => {
                // GAP-025: the order of battle is assembled beside the report, over the
                // retained sessions, through the resolver, and the pattern of life is
                // folded out of the same evidence. Both are held whether they succeeded
                // or not, so a product that could not be built says why rather than
                // leaving the last one on screen.
                let product = gungnir_app::identity::order_of_battle(&mut self.state)
                    .map_err(|e| e.to_string());
                self.sustainment.reports.set_order_of_battle(product);
                let pattern =
                    gungnir_app::identity::pattern_of_life(&self.state).map_err(|e| e.to_string());
                self.sustainment.reports.set_pattern_of_life(pattern);
                self.sustainment.reports.generate(&self.state)
            }
            ReportsAction::Export => self.sustainment.reports.export(&self.state),
            ReportsAction::Review(a) => {
                gungnir_app::review::apply(&mut self.state, &mut self.sustainment, a);
                return;
            }
        };
        if let Err(err) = result {
            tracing::error!(%err, ?action, "report action failed");
            self.state
                .alerts
                .push(format!("Report action failed: {err}"));
        }
    }

    /// PN-14. Applying is a decision, so its failure is reported rather than swallowed.
    fn apply_config(&mut self, action: gungnir_ui::panels::config_editor::ConfigAction) {
        use gungnir_ui::panels::config_editor::ConfigAction;
        match action {
            ConfigAction::Reload => {
                self.sustainment.config.reload(&self.state);
                if let Some(err) = self.sustainment.config.candidate_error() {
                    self.state
                        .alerts
                        .push(format!("Could not load the baseline: {err}"));
                }
            }
            ConfigAction::Validate => self.sustainment.config.validate(&self.state),
            ConfigAction::Apply => {
                let mut editor = std::mem::take(&mut self.sustainment.config);
                match editor.apply(&mut self.state) {
                    Ok(()) => {
                        tracing::info!("configuration baseline applied");
                        self.state.alerts.push(
                            "Baseline written; it takes effect when the desktop restarts"
                                .to_owned(),
                        );
                    }
                    Err(err) => {
                        tracing::error!(%err, "configuration apply failed");
                        self.state.alerts.push(format!("Apply failed: {err}"));
                    }
                }
                self.sustainment.config = editor;
            }
            ConfigAction::DiscardCandidate => self.sustainment.config.discard(),
        }
    }
}

/// Which profile this desktop is running, which session it is journaling, and what
/// background data is loaded. Wiring only; the values come straight from `AppState`.
fn render_session_header(ui: &mut egui::Ui, state: &AppState) {
    // Backend, session and role now live in the status strip (PN-01); this header
    // keeps only what the strip does not carry.
    ui.heading("Gungnir");
    let backend = match &state.backend {
        gungnir_config::BackendConfig::Embedded => {
            "embedded services (disconnected profile)".to_string()
        }
        gungnir_config::BackendConfig::Remote { endpoint } => format!("remote node {endpoint}"),
    };
    ui.label(backend);
    if let Some(session) = state.session() {
        ui.label(format!(
            "Session {} in {}",
            session.0, state.config.data_dir
        ));
    }
    ui.label(format!(
        "{} sensors, {} resources; {} point clouds, {} meshes, {} terrains, {} assets loaded",
        state.config.sensors.len(),
        state.resources.len(),
        state.data.point_clouds.len(),
        state.data.meshes.len(),
        state.data.terrains.len(),
        state.data.assets.len()
    ));
}
