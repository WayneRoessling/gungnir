# Rust UI technology stack: decision record

**Context:** Gungnir's operator interface is a Windows 11 desktop application targeting a
discrete NVIDIA RTX-class GPU. Its UI surfaces are dashboards, a 3D situational-awareness
viewport, and interactive panel-based controls. The same UI crates are never built into
the headless service node (`ARCHITECTURE.md` §8), so this decision affects only
`gungnir-render`, `gungnir-viewport3d`, `gungnir-ui`, and `gungnir-app`.

This document began as a survey of options with open questions. The decisions have
since been made and are recorded here; §1 keeps the survey for context, §5 records
which of the original open questions are now answered.

## 1. Rust GUI landscape (context)

Rust has no built-in GUI toolkit; the ecosystem provides several mature third-party
options.

| Category | Libraries |
|---|---|
| Immediate-mode GUI | egui |
| Declarative, Elm-style | iced, Xilem |
| Markup-based declarative | Slint |
| Native bindings | gtk-rs, relm4 (GTK4) |
| Web-view desktop | Tauri (Rust backend, HTML/CSS/JS frontend) |
| Web and WASM frontends | Yew, Leptos, Dioxus, Sycamore |

For a 3D viewport, the candidates were `three-d` (a lightweight renderer with meshes,
lighting, camera controls, and picking) and Bevy (a full ECS game engine, `wgpu`-based,
with `bevy_egui` for overlay UI).

## 2. Decision

| Layer | Choice | Rendering context |
|---|---|---|
| Windowing and app shell | `eframe` with `eframe::Renderer::Glow` | OpenGL, created by eframe |
| 2D dashboard panels and controls | `egui` | Drawn by eframe's glow painter |
| 3D viewport | `three-d` | Draws into eframe's OpenGL context via `glow` |
| Point-cloud registration compute | `wgpu` (DirectX 12 or Vulkan on Windows) | A separate, headless device owned by `gungnir-render::GpuContext` |

Rationale:

- **egui and eframe** were chosen over iced and Slint because the panels are
  live-updating telemetry and control widgets, which immediate mode handles with the
  least ceremony, and because `AppState` as a single source of truth
  (`rust-ui-architecture-coding-standards.md` §2) maps directly onto egui's per-frame
  rebuild.
- **three-d** was chosen over Bevy because the viewport needs meshes, terrain, point
  clouds, camera controls, and picking, not entity management, animation, or a plugin
  architecture. Bevy remains the fallback if scene complexity grows.
- **wgpu for compute only.** GPU point-cloud fusion needs compute shaders and a modern
  API; `wgpu` provides that portably and selects DirectX 12 or Vulkan on Windows.

## 3. Correction: three-d renders through OpenGL, not wgpu

Earlier drafts stated that `three-d` is built on `wgpu` and could share one `wgpu`
device with the rest of the application. That is not the case: `three-d` renders
through `glow`, the OpenGL and WebGL binding, and integrates with `eframe` only through
eframe's `glow` backend. The consequences, now reflected in `ARCHITECTURE.md` §4 and §9:

1. `gungnir-app` must run eframe with `Renderer::Glow`. The egui panels and the three-d
   viewport share that one OpenGL context.
2. The desktop therefore has two GPU contexts: OpenGL for everything the operator sees,
   and a headless `wgpu` compute device for `gungnir-data-fusion`. On Windows the RTX
   GPU accelerates both, but DirectX 12 is used only by the compute side.
3. There is no zero-copy path between the two contexts. Fused point clouds are read
   back to a CPU `PointBuffer` and uploaded to the viewport. This is acceptable because
   fusion output changes far less often than the frame rate.
4. `gungnir-render`'s `egui_integration` module is a placeholder for an egui-over-wgpu
   presentation path and is inactive. It becomes relevant only if the 3D layer is ever
   replaced by a `wgpu`-native renderer (Bevy, a `wgpu` scene crate, or hand-written
   `wgpu` code), which is also the only route to DirectX 12 *rendering*. That switch is
   not planned.
5. eframe runs with its default `glow` feature and without its optional `wgpu`
   feature. The workspace pins `wgpu` 22, the line that feature would use, so
   enabling it later can never produce two `wgpu` versions in one build.

## 4. Version set

The pinned versions and their caveats are in `ARCHITECTURE.md` §9. In short: `eframe`
and `egui` 0.29, `three-d` 0.18, `wgpu` 22 (compute only), `gltf` 1, `vtkio` 0.6,
`las` 0.9, on the Rust 1.98 toolchain pinned in `rust-toolchain.toml`. Pin `wgpu`,
`egui`, `eframe`, and `three-d` explicitly; these crates move fast and break API
compatibility between minor versions (`rust-ui-architecture-coding-standards.md` §9).

## 5. Status of the original open questions

| Question | Status |
|---|---|
| Data sources and update cadence for dashboard panels (polling versus push) | Decided by design: services expose a non-blocking snapshot for the per-frame tick today, and `gungnir-eventing` is the push path every subscriber moves to (`gungnir-capabilities.md` §5.1). |
| Number and layout of simultaneous views (single window with docking versus multi-window) | Partially decided: one window with a left side panel of dashboards and a central viewport is wired in `gungnir-app`; the panels each role sees are defined in `gungnir-workflow`. Docking and multi-window were decided on 2026-09-04 (D-17): adopted where appropriate, per `ux/information-architecture.md` §1; implemented under GAP-075 with egui native viewports for detached panels and a docking crate signed off in §2.9 when it lands. |
| Whether 3D views render point clouds, meshes, or both | Both: point clouds via `gungnir-data::pointcloud` and streamed tiles, meshes via the VTK bridge and glTF assets (`rust-3d-data-ecosystem-build-vs-adopt.md`). |
| Design system or branding constraints for a native-Windows look | Open. `gungnir-ui::theme` centralizes colors and spacing so a design system can be applied in one place. |
| Role-specific workspaces and alert workflow | Decided in outline: `gungnir-workflow` defines the panel set per role and the alert lifecycle (`gungnir-capabilities.md` §5.6); `gungnir-ui` does not yet switch layouts by role. |
