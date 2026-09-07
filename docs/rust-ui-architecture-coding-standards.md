# Architecture & Coding Standards — eframe / egui / three-d / wgpu Stack

**Purpose:** Reference standards for any engineer or coding agent contributing Rust code to this UI stack. Goals: logical structure, clean/readable code, consistent patterns, and performance suitable for real-time 60fps rendering.

**Scope:** These standards govern `gungnir-render`, `gungnir-viewport3d`, `gungnir-ui`, and `gungnir-app`, and the GPU-resource and off-render-thread rules (§5, §7, §8) also govern `gungnir-data` and `gungnir-data-fusion`. The tracking core, service layer, and productization crates follow `agentic-coding-standards.md`, whose §7 settles the points where the two documents differ. Section numbers here are cited from Rust doc comments and must not be renumbered.

**Rendering contexts:** the operator interface is drawn through one OpenGL context (eframe's `glow` backend, shared by egui and three-d), and `wgpu` is used only as a headless compute device for point-cloud fusion. See `ARCHITECTURE.md` §4 and §9 and `rust-ui-tech-stack-summary.md` §3 before touching anything GPU-related.

---

## 1. Project Structure

Organize by **layer**, not by feature, at the top level — this keeps rendering concerns separate from UI/state concerns, which matters a lot in an immediate-mode + GPU stack.

```
src/
  main.rs                # eframe app bootstrap only — no logic here
  app/
    mod.rs                # top-level App struct implementing eframe::App
    state.rs              # central application state (single source of truth)
    update.rs             # per-frame update/tick logic (non-render)
  ui/
    mod.rs
    panels/               # one file per dashboard panel/widget group
      panel_a.rs
      panel_b.rs
    theme.rs               # colors, spacing, fonts — centralized, no magic numbers in panels
  viewport3d/
    mod.rs                # three-d scene setup, camera, render loop glue
    scene.rs               # mesh/surface construction & updates
    materials.rs
    interaction.rs         # picking, camera controls
  render/
    mod.rs                 # shared wgpu device/queue/surface management
    egui_integration.rs     # egui <-> wgpu render pass wiring
  data/
    mod.rs                  # data ingestion, transforms — no UI/render types imported here
  util/
    mod.rs
```

**Rule:** `data/` must never import from `ui/`, `viewport3d/`, or `render/`. Dependency direction is one-way: `data → app::state → ui/viewport3d → render`. If a lower layer needs to know about upper-layer types, invert the dependency with a trait.

**How this maps onto the workspace.** The tree above describes the layers of a single binary; the Gungnir workspace keeps the same layers as separate crates so each can be compiled and, where GPU-free, tested on its own:

| Layer above | Crate | Notes |
|---|---|---|
| `main.rs`, `app/` | `gungnir-app` (`src/main.rs`, `src/state.rs`, `src/update.rs`) | The only crate that depends on everything. |
| `ui/` | `gungnir-ui` (`src/panels/*.rs`, `src/theme.rs`) | One file per panel. |
| `viewport3d/` | `gungnir-viewport3d` (`scene`, `materials`, `interaction`, `tracks`, `streaming/`, `scientific/`) | three-d over the eframe `glow` context. |
| `render/` | `gungnir-render` (`GpuContext`, `egui_integration`) | Owns the headless `wgpu` compute device. `egui_integration` is inactive while eframe runs on `glow`. |
| `data/` | `gungnir-data` and `gungnir-data-fusion` | Split so the GPU-free I/O crate stays `cargo test`-able; see `ARCHITECTURE.md` §3. |
| `util/` | none | Shared helpers live in the lowest crate that needs them. |

The `data → app::state → ui/viewport3d → render` rule therefore reads, in crate terms, `gungnir-data`/`gungnir-data-fusion` and the two service facades → `gungnir-app::AppState` → `gungnir-ui`/`gungnir-viewport3d`, with `gungnir-render` consumed by `gungnir-app` (which lends its device to `gungnir-data-fusion`). `gungnir-ui` and `gungnir-viewport3d` read `gungnir-model` views, the service facades' public types, which `ARCHITECTURE.md` §5 justifies. `gungnir-viewport3d` also depends on `gungnir-ui` for the theme palette only, so 2D and 3D colours agree; no other UI-to-UI edge exists.

`gungnir-node`, the headless service binary, follows the same "wiring, not logic" rule as `gungnir-app` but is governed by `agentic-coding-standards.md`, since it contains no UI code.

---

## 2. State Management

- **Single source of truth.** All application state lives in one `AppState` struct (or a small tree of structs owned by it). Panels and 3D views read from and write to this state — they do not hold their own duplicate copies of business data.
- **Immediate-mode implication:** because egui rebuilds UI every frame, widgets must be *cheap to construct*. Never do I/O, allocation-heavy work, or blocking calls inside a panel's `ui()` function. Precompute or cache upstream.
- **Interior mutability sparingly.** Prefer passing `&mut AppState` down through render functions over `Rc<RefCell<..>>` scattered through the UI tree. Reserve `RefCell`/`Arc<Mutex<..>>` for genuine cross-thread or cross-frame async boundaries (e.g., background data loading).
- **No global mutable statics.** All state is owned and threaded explicitly through `eframe::App::update()`.

```rust
// Good
struct AppState {
    dashboard: DashboardState,
    viewport: ViewportState,
    data: DataStore,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ui::panels::render_dashboard(ctx, &mut self.state.dashboard, &self.state.data);
        viewport3d::render(ctx, &mut self.state.viewport, &self.state.data);
    }
}
```

---

## 3. Naming Conventions

Follow standard Rust conventions (`rustfmt` + `clippy` defaults) plus these project-specific rules:

| Item | Convention | Example |
|---|---|---|
| Modules | `snake_case`, singular unless a collection | `panel_layout`, `panels` |
| Types/Traits | `UpperCamelCase` | `ViewportCamera`, `Renderable` |
| Functions/vars | `snake_case`, verb-first for functions | `update_camera()`, `render_panel()` |
| Constants | `SCREAMING_SNAKE_CASE`, grouped in a `consts.rs` per module if >3 | `DEFAULT_FOV_DEGREES` |
| Panel render fns | `render_<panel_name>` | `render_status_panel` |
| Setter-style UI fns | `show_<widget>` if it returns `egui::Response` | `show_zoom_slider` |

Avoid abbreviations except well-known domain/graphics terms (`fov`, `pos`, `mvp`, `ctx`).

---

## 4. Error Handling

- Use `Result<T, E>` at all fallible boundaries (file I/O, GPU resource creation, data parsing). No `unwrap()`/`expect()` outside of `main.rs` bootstrap and tests.
- Each crate defines its own error enum (`DataError`, `FusionError`, `RenderError`); a project-level `AppError` lives only in `gungnir-app` and wraps them. All derive `thiserror` (approved, `agentic-coding-standards.md` §2.9). Example shape:

```rust
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("failed to initialize GPU device: {0}")]
    GpuInit(#[from] wgpu::RequestDeviceError),
    #[error("mesh load failed: {0}")]
    MeshLoad(String),
}
```

- **Render loop must never panic.** A per-frame error (e.g., a transient resource failure) should be logged and degrade gracefully (skip the frame's draw, show a UI error banner) rather than crash the app. Reserve panics for genuinely unrecoverable startup failures.
- Use `tracing` (not `println!`) for logging, with levels (`error!`, `warn!`, `info!`, `debug!`) so verbosity is filterable at runtime.

---

## 5. Rendering & Performance Standards

These matter more here than in typical application code, since the app runs a real-time render loop.

- **No heap allocation in the hot path where avoidable.** Per-frame UI/render code should reuse buffers (`Vec::clear()` + refill, not fresh `Vec::new()` every frame) and avoid `format!`/`String` construction for anything drawn every frame — cache formatted strings and invalidate only when the underlying value changes.
- **GPU resource lifecycle:** create buffers/textures/pipelines once at setup or on explicit state change — never re-create a `wgpu::Buffer`, a three-d `Gm`/`Mesh`, or a pipeline inside the per-frame draw call. This applies to both GPU contexts: the OpenGL context three-d draws into and the `wgpu` compute device `gungnir-render` owns. Data crossing from the compute device to the viewport goes through a CPU `PointBuffer`; do not attempt buffer sharing between the two.
- **Batch draw calls.** For `three-d` scenes, prefer combining static geometry into fewer meshes/instances over many small draw calls when the data allows it.
- **Decouple simulation/data-update rate from render rate** if data changes faster or slower than 60fps — don't let a data feed's cadence stall the render loop; use a channel (`crossbeam-channel` or `tokio::sync::mpsc`) to hand off the latest snapshot.
- **Long-running or blocking work (file loads, network, heavy compute) must run off the render thread** — spawn via `std::thread` or an async runtime and communicate results back via channel, polled non-blockingly in `update()`.
- **Profile before optimizing.** Use `tracing` spans to identify actual bottlenecks rather than guessing. `puffin` (which has an `egui` integration, `puffin_egui`) is a reasonable addition but is not in the signed-off stack; adding it needs the sign-off `agentic-coding-standards.md` §6 rule 4 requires.

---

## 6. Code Style & Readability

- **`rustfmt` and `clippy` are non-negotiable.** CI should fail on `clippy::all` warnings (`-D warnings`) and unformatted code.
- **Functions stay short and single-purpose.** A panel's top-level `render_x_panel()` function should read as a table of contents (calls to sub-sections), not a 300-line wall of `ui.horizontal(...)` calls. Extract sub-sections into their own functions.
- **No "magic" layout numbers.** Spacing, colors, and sizing constants live in `ui/theme.rs`, not inlined as raw floats/hex codes in panel code.
- **Doc comments (`///`) required on all public items** (structs, enums, public fns) — explain *why*, not just restate the signature. Module-level `//!` docs required for every file in `app/`, `viewport3d/`, and `render/`.
- **Prefer composition over deep inheritance-style trait hierarchies.** Rust doesn't have inheritance; don't fight the language by building deep trait-object chains where a simple struct + free functions would do.

```rust
/// Renders the mission status panel showing current system health,
/// active alerts, and connection state. Read-only view over `DashboardState`.
pub fn render_status_panel(ui: &mut egui::Ui, state: &DashboardState) {
    render_header(ui, state);
    render_alert_list(ui, &state.alerts);
    render_connection_indicator(ui, state.connection);
}
```

---

## 7. Concurrency & Async

- Keep the render/UI thread free of blocking calls. Background work (data ingestion, file I/O, long compute) runs on dedicated threads or the `tokio` runtime, communicating via bounded channels. `gungnir-app` creates the one `tokio` runtime in the process and hands its `Handle` to the tracking service; no UI or data crate creates its own (`agentic-coding-standards.md` §2.2).
- Shared state crossing thread boundaries uses `Arc<Mutex<..>>` or channels — never raw pointers or `unsafe` for this purpose.
- Document channel contracts clearly: what message types flow, expected cadence, backpressure behavior if the UI can't keep up.

---

## 8. Testing

- **Pure logic (data transforms, state updates, math/camera calculations) must be unit-testable** independent of any GPU context — keep this logic out of files that directly touch `wgpu`/`three-d`/`egui` types where possible, so it can run in plain `cargo test` without a GPU.
- Rendering code itself is typically validated via manual/visual QA or snapshot testing tools, not unit tests — don't force GPU-dependent code into unit tests artificially.
- Add regression tests for any bug fix involving state transitions or data parsing.

---

## 9. Dependency & Version Discipline

- Pin `wgpu`, `egui`, `eframe`, and `three-d` to compatible versions explicitly in the workspace `Cargo.toml` (these crates move fast and can break API compatibility between minor versions). The tested version set and its caveats are documented in `ARCHITECTURE.md` §9: `wgpu` 22 matches the line eframe 0.29's optional wgpu feature uses, and eframe runs on `glow` with that feature off.
- New dependencies require justification in the PR description: what problem it solves, why the standard library or an existing dependency can't. For these crates that is the same sign-off `agentic-coding-standards.md` §6 rule 4 requires; the young 3D-data crates named in `rust-3d-data-ecosystem-build-vs-adopt.md` §1.2 (`copc-rs`, `pasture-*`, `oxigdal-3d`) are pinned in the PR that first uses them.

---

## 10. Standards Checklist (for agents/reviewers)

Before considering a change complete, verify:

- [ ] No `unwrap()`/`expect()` outside `main.rs`/tests
- [ ] No allocation or blocking I/O inside per-frame render/update paths
- [ ] All public items have doc comments
- [ ] `clippy` and `rustfmt` pass cleanly
- [ ] New GPU resources are created once, not per-frame
- [ ] State flows one direction: `data → state → ui/viewport3d → render`
- [ ] Magic numbers replaced with named constants in `theme.rs` or a local `consts` module
- [ ] Long-running work is off the render thread
- [ ] Errors are handled via `Result` and the crate's error enum, not panics, in reachable runtime code
- [ ] No new dependency edge that `ARCHITECTURE.md` does not already show
- [ ] Nothing in `gungnir-app` beyond wiring; no `wgpu` code in `gungnir-viewport3d` or `gungnir-ui`; no `three-d` or `egui` code in `gungnir-data` or `gungnir-data-fusion`
