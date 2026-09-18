# egui 0.34 and three-d 0.19, to clear the quick-xml advisories

The desktop's UI stack moved from eframe/egui 0.29.1 and three-d 0.18.2 to eframe/egui
0.34.3 and three-d 0.19.0, with egui_tiles 0.10.1 to 0.15.0 alongside, on 2026-09-17
(GAP-144). `wgpu` stayed at 22. Nothing an operator uses was meant to change, and where
the new versions changed behaviour underneath the code, the code was adjusted to keep the
old behaviour; each of those calls is below.

## Why

The first runs of `release.yml` (`release-workflow-first-runs.md`, this date) found
`cargo audit` failing on RUSTSEC-2026-0194 and RUSTSEC-2026-0195 in quick-xml 0.30.0:
quadratic time checking duplicate attribute names, and unbounded namespace allocation.
Both are patched from 0.41. quick-xml 0.30 reached this workspace by one path only,
`eframe` -> `egui-winit` -> `accesskit_winit` -> `accesskit_unix` -> `atspi` ->
`zbus-lockstep` -> `zbus_xml` 4.0.0, which pins 0.30: eframe 0.29's Linux accessibility
bridge. No compatible update existed. `deny.toml` had accepted the two advisories on
2026-09-07, and #142 has since handed that acceptance to `cargo audit` as well, which is how
run 35301215267 went green. Offered keeping the acceptance, an upgrade, or turning off
eframe's `accesskit` on Linux, the owner chose the upgrade. This change removes the crate
the acceptance covers; the acceptance itself is untouched (below).

## The target, and why 0.34 is the ceiling

As resolved: eframe, egui, egui_glow, egui-winit, epaint and emath 0.34.3; egui_tiles
0.15.0; three-d 0.19.0 with three-d-asset 0.10.0; winit 0.30.13; **one `glow` for the
shared context, 0.17.0**, taken by eframe, egui_glow and three-d alike; accesskit 0.24.1
(accesskit_windows 0.32.1, accesskit_unix 0.21.1, accesskit_winit 0.32.2); zbus 5.19.0 and
zbus_xml 5.2.1; and **one quick-xml, 0.41.0**, under `wayland-scanner`, a proc-macro run at
build time on Linux.

The viewport draws into eframe's OpenGL context (`ARCHITECTURE.md` §4, §9), so three-d's
`glow` must be eframe's; `gungnir-app/tests/gl_attachment.rs` states it as a compile-time
fact and it still compiles. three-d 0.19 is built on egui 0.34 and glow 0.17. eframe 0.36
exists, but its `glow` is not the one three-d takes, so 0.34 is as far as the shared context
can go until three-d moves.

## What was measured about the advisories

`cargo deny` and `cargo audit` are not installed where this was done, so neither was run,
and nothing here claims either passed. Three things were run instead.

- `cargo tree -i quick-xml --target all -e normal,build`: before, 0.30.0 under `zbus_xml`
  and 0.41.0 under `wayland-scanner`; after, 0.41.0 alone. `Cargo.lock` holds one
  quick-xml, 0.41.0.
- `Cargo.lock` matched against the RustSec database as last fetched on this machine (a
  checkout at 2026-09-08, `bf25f65`), applying `cargo audit`'s rule: an advisory applies
  unless the version is in its `patched` or `unaffected` ranges. This is a stand-in for
  `cargo audit`, not the tool. Before the move it reported exactly what run 35296523137's
  `cargo audit` reported: vulnerabilities RUSTSEC-2026-0194 and -0195 in quick-xml 0.30.0,
  and warnings for ansi_term, cgmath (-0196 and the unsound -0197), instant, paste and
  ttf-parser. After: **no vulnerability**, and the same warnings less ttf-parser, which
  left the graph (below). By that reading `cargo audit` passes with no `--ignore` at all.
  Advisories published after 2026-09-08 were not seen.
- `deny.toml`'s license rules evaluated over `cargo metadata` for both release targets
  (645 and 695 packages): everything passes with the change to its font exception
  described below, and fails on `epaint_default_fonts` 0.34.3 without it.

**Three of `deny.toml`'s six accepted advisories now match nothing**: RUSTSEC-2026-0194 and
-0195 (quick-xml 0.30) and RUSTSEC-2026-0192 (ttf-parser). cargo-deny's documented default
for such an entry is a warning (`unused-ignored-advisory = "warn"`, which `deny.toml` does
not change), and `cargo audit --ignore` does not mind an id it never meets, so the release
gate is unaffected either way. They stay as they are:
removing an accepted advisory is the owner's decision, and it was not asked for here. Their
comments now describe a graph that no longer exists, and `paste`'s says it arrives through
`accesskit_windows` too, when only nalgebra's `simba` brings it now.

## The duplicate set, before and after

`cargo tree -d --depth 0 --target <triple>`, default edges (normal, build, dev), run on the
Windows development host. A crate at one version appears when the host and target builds of
it differ, which is why `bytes`, `log`, `regex` and the like are listed.

| Crate | Windows before | Windows after | Linux before | Linux after |
|---|---|---|---|---|
| `aes` | -- | -- | 0.8.4, 0.9.3 | 0.8.4, 0.9.3 |
| `approx` | 0.4.0, 0.5.1 | 0.4.0, 0.5.1 | 0.4.0, 0.5.1 | 0.4.0, 0.5.1 |
| `base64` | 0.13.1, 0.22.1, 0.23.1 | 0.13.1, 0.22.1, 0.23.1 | 0.13.1, 0.22.1, 0.23.1 | 0.13.1, 0.22.1, 0.23.1 |
| `bit-set` | 0.6.0, 0.8.0 | 0.6.0, 0.8.0 | 0.6.0, 0.8.0 | 0.6.0, 0.8.0 |
| `bit-vec` | 0.7.0, 0.8.0 | 0.7.0, 0.8.0 | 0.7.0, 0.8.0 | 0.7.0, 0.8.0 |
| `bitflags` | 1.3.2, 2.13.1 | 1.3.2, 2.13.1 | 1.3.2, 2.13.1 | 1.3.2, 2.13.1 |
| `block-buffer` | 0.10.4, 0.12.1 | 0.10.4, 0.12.1 | 0.10.4, 0.12.1 | 0.10.4, 0.12.1 |
| `bytes` | 1.12.1 | 1.12.1 | 1.12.1 | 1.12.1 |
| `calloop` | -- | -- | 0.13.0, 0.14.4 | 0.13.0, 0.14.4 |
| `calloop-wayland-source` | -- | -- | 0.3.0, 0.4.1 | 0.3.0, 0.4.1 |
| `cfg_aliases` | 0.1.1, 0.2.2 | 0.1.1, 0.2.2 | 0.1.1, 0.2.2 | 0.1.1, 0.2.2 |
| `cipher` | -- | -- | 0.4.4, 0.5.2 | 0.4.4, 0.5.2 |
| `const-oid` | 0.9.6, 0.10.2 | 0.9.6, 0.10.2 | 0.9.6, 0.10.2 | 0.9.6, 0.10.2 |
| `cpufeatures` | 0.2.17, 0.3.1 | 0.2.17, 0.3.1 | 0.2.17, 0.3.1 | 0.2.17, 0.3.1 |
| `crypto-common` | 0.1.7, 0.2.2 | 0.1.7, 0.2.2 | 0.1.7, 0.2.2 | 0.1.7, 0.2.2 |
| `darling` | 0.14.4, 0.21.3 | 0.14.4, 0.21.3 | 0.14.4, 0.21.3 | 0.14.4, 0.21.3 |
| `darling_core` | 0.14.4, 0.21.3 | 0.14.4, 0.21.3 | 0.14.4, 0.21.3 | 0.14.4, 0.21.3 |
| `darling_macro` | 0.14.4, 0.21.3 | 0.14.4, 0.21.3 | 0.14.4, 0.21.3 | 0.14.4, 0.21.3 |
| `deku` | 0.16.0, 0.20.3 | 0.16.0, 0.20.3 | 0.16.0, 0.20.3 | 0.16.0, 0.20.3 |
| `deku_derive` | 0.16.0, 0.20.3 | 0.16.0, 0.20.3 | 0.16.0, 0.20.3 | 0.16.0, 0.20.3 |
| `digest` | 0.10.7, 0.11.3 | 0.10.7, 0.11.3 | 0.10.7, 0.11.3 | 0.10.7, 0.11.3 |
| `foldhash` | -- | 0.1.5, 0.2.0 | -- | 0.1.5, 0.2.0 |
| `getrandom` | 0.2.17, 0.3.4, 0.4.3 | 0.2.17, 0.3.4, 0.4.3 | 0.2.17, 0.3.4, 0.4.3 | 0.2.17, 0.3.4, 0.4.3 |
| `glow` | 0.13.1, 0.14.2 | 0.13.1, 0.17.0 | 0.13.1, 0.14.2 | 0.13.1, 0.17.0 |
| `hashbrown` | 0.15.5, 0.17.1 | 0.15.5, 0.16.1, 0.17.1 | 0.15.5, 0.17.1 | 0.15.5, 0.16.1, 0.17.1 |
| `hkdf` | -- | -- | 0.12.4, 0.13.0 | 0.12.4, 0.13.0 |
| `hmac` | 0.12.1, 0.13.0 | 0.12.1, 0.13.0 | 0.12.1, 0.13.0 | 0.12.1, 0.13.0 |
| `http` | 0.2.12, 1.5.0 | 0.2.12, 1.5.0 | 0.2.12, 1.5.0 | 0.2.12, 1.5.0 |
| `http-body` | 0.4.6, 1.1.0 | 0.4.6, 1.1.0 | 0.4.6, 1.1.0 | 0.4.6, 1.1.0 |
| `inout` | -- | -- | 0.1.4, 0.2.2 | 0.1.4, 0.2.2 |
| `itertools` | 0.10.5, 0.13.0 | 0.10.5, 0.13.0, 0.14.0 | 0.10.5, 0.13.0 | 0.10.5, 0.13.0, 0.14.0 |
| `linux-raw-sys` | -- | -- | 0.4.15, 0.12.1 | 0.4.15, 0.12.1 |
| `log` | 0.4.34 | 0.4.34 | 0.4.34 | 0.4.34 |
| `memchr` | 1.0.2, 2.8.3 | 1.0.2, 2.8.3 | 1.0.2, 2.8.3 | 1.0.2, 2.8.3 |
| `miniz_oxide` | 0.8.9, 0.9.1 | 0.8.9, 0.9.1 | 0.8.9, 0.9.1 | 0.8.9, 0.9.1 |
| `phf_shared` | -- | -- | -- | 0.13.1 |
| `proc-macro-crate` | 1.3.1, 3.5.0 | 1.3.1, 3.5.0 | 1.3.1, 3.5.0 | 1.3.1, 3.5.0 |
| `quick-error` | 1.2.3, 2.0.1 | 1.2.3, 2.0.1 | 1.2.3, 2.0.1 | 1.2.3, 2.0.1 |
| `quick-xml` | -- | -- | 0.30.0, 0.41.0 | -- |
| `rand` | 0.8.8, 0.9.5, 0.10.2 | 0.8.8, 0.9.5, 0.10.2 | 0.8.8, 0.9.5, 0.10.2 | 0.8.8, 0.9.5, 0.10.2 |
| `rand_chacha` | 0.3.1, 0.9.0 | 0.3.1, 0.9.0 | 0.3.1, 0.9.0 | 0.3.1, 0.9.0 |
| `rand_core` | 0.6.4, 0.9.5, 0.10.1 | 0.6.4, 0.9.5, 0.10.1 | 0.6.4, 0.9.5, 0.10.1 | 0.6.4, 0.9.5, 0.10.1 |
| `regex` | 1.13.1 | 1.13.1 | 1.13.1 | 1.13.1 |
| `regex-automata` | 0.4.18 | 0.4.18 | 0.4.18 | 0.4.18 |
| `regex-syntax` | 0.8.11 | 0.8.11 | 0.8.11 | 0.8.11 |
| `rustix` | -- | -- | 0.38.44, 1.1.4 | 0.38.44, 1.1.4 |
| `serde` | -- | -- | 1.0.229 | 1.0.229 |
| `serde_core` | -- | -- | 1.0.229 | 1.0.229 |
| `sha2` | 0.10.9, 0.11.0 | 0.10.9, 0.11.0 | 0.10.9, 0.11.0 | 0.10.9, 0.11.0 |
| `smithay-client-toolkit` | -- | -- | 0.19.2, 0.20.0 | 0.19.2, 0.20.0 |
| `strsim` | 0.10.0, 0.11.1 | 0.10.0, 0.11.1 | 0.10.0, 0.11.1 | 0.10.0, 0.11.1 |
| `syn` | 1.0.109, 2.0.119, 3.0.4 | 1.0.109, 2.0.119, 3.0.4 | 1.0.109, 2.0.119, 3.0.4 | 1.0.109, 2.0.119, 3.0.4 |
| `thiserror` | 1.0.69, 2.0.20 | 1.0.69, 2.0.20 | 1.0.69, 2.0.20 | 1.0.69, 2.0.20 |
| `thiserror-impl` | 1.0.69, 2.0.20 | 1.0.69, 2.0.20 | 1.0.69, 2.0.20 | 1.0.69, 2.0.20 |
| `tokio-tungstenite` | 0.28.0, 0.29.0 | 0.28.0, 0.29.0 | 0.28.0, 0.29.0 | 0.28.0, 0.29.0 |
| `toml_datetime` | 0.6.11, 1.1.1+spec-1.1.0 | 0.6.11, 1.1.1+spec-1.1.0 | 0.6.11, 1.1.1+spec-1.1.0 | 0.6.11, 1.1.1+spec-1.1.0 |
| `toml_edit` | 0.19.15, 0.25.13+spec-1.1.0 | 0.19.15, 0.25.13+spec-1.1.0 | 0.19.15, 0.25.13+spec-1.1.0 | 0.19.15, 0.25.13+spec-1.1.0 |
| `tracing` | 0.1.44 | 0.1.44 | 0.1.44 | 0.1.44 |
| `tracing-core` | 0.1.36 | 0.1.36 | 0.1.36 | 0.1.36 |
| `tungstenite` | 0.28.0, 0.29.0 | 0.28.0, 0.29.0 | 0.28.0, 0.29.0 | 0.28.0, 0.29.0 |
| `windows` | 0.52.0, 0.58.0 | 0.52.0, 0.62.2 | -- | -- |
| `windows-core` | 0.52.0, 0.58.0 | 0.52.0, 0.62.2 | -- | -- |
| `windows-sys` | 0.52.0, 0.60.2, 0.61.2 | 0.52.0, 0.60.2, 0.61.2 | -- | -- |
| `windows-targets` | 0.52.6, 0.53.5 | 0.52.6, 0.53.5 | -- | -- |
| `windows_x86_64_msvc` | 0.52.6, 0.53.1 | 0.52.6, 0.53.1 | -- | -- |
| `winnow` | 0.5.40, 1.0.4 | 0.5.40, 1.0.4 | 0.5.40, 1.0.4 | 0.5.40, 1.0.4 |
| `zbus` | -- | -- | 4.4.0, 5.19.0 | -- |
| `zbus_macros` | -- | -- | 4.4.0, 5.19.0 | -- |
| `zbus_names` | -- | -- | 3.0.0, 4.3.4 | -- |
| `zvariant` | -- | -- | 4.2.0, 5.15.0 | -- |
| `zvariant_derive` | -- | -- | 4.2.0, 5.15.0 | -- |
| `zvariant_utils` | -- | -- | 2.1.0, 4.2.0 | -- |

Crates listed: Windows 52 to 53, Linux 65 to 60. On normal and build edges only
(`-e normal,build`, what reaches a binary): Windows 35 to 36, Linux 48 to 44.

**`egui`, `eframe`, `winit`, `egui-winit`, `egui_glow`, `epaint` and `emath` appear in
none of these lists**, before or after. **`glow` has two versions before and after, and
that is not the shared context**: 0.13.1 is `wgpu-hal` 22's, under the compute device, and
it sat beside eframe's 0.14.2 before exactly as it sits beside 0.17.0 now. The shared
context is 0.17.0 alone (`cargo tree -i glow@0.17.0`: eframe, egui_glow, three-d).

What is new, and why:

- **`foldhash` 0.2.0 and a third `hashbrown`, 0.16.1.** hashbrown 0.16 is the one accesskit
  0.24's consumer takes (`accesskit_consumer` 0.35 under `accesskit_windows` on Windows,
  0.36 under `accesskit_atspi_common` on Linux), and the one vello_common and vello_cpu
  take, which is epaint 0.34's new rasterizer. foldhash 0.2 is its default hasher.
  hashbrown 0.15 and 0.17 were already there for other crates.
- **A third `itertools`, 0.14.0**, which is egui_tiles 0.15's. On normal and build edges it
  is the only itertools; criterion's 0.10 and the rs1090 oracle's prost 0.13 are
  dev-dependencies, which is why the row appears only with dev edges.
- **`phf_shared` 0.13.1, and on normal and build edges `fastrand` 2.5.0, on Linux**: one
  version each. `accesskit_atspi_common` takes `phf`, whose `phf_macros` is a proc-macro, so
  cross-compiling the Linux target from a Windows host builds these twice, once per
  platform. Neither is a second version.
- **`windows` and `windows-core` 0.58 became 0.62.2** with accesskit_windows 0.32; 0.52 is
  gpu-allocator's, under `wgpu-hal` 22. Two versions before and after.
- **Gone**: quick-xml 0.30.0, and the zbus 4 family (`zbus`, `zbus_macros`, `zbus_names`,
  `zvariant`, `zvariant_derive`, `zvariant_utils`), because accesskit_unix 0.21 moved AT-SPI
  to zbus 5.

`Cargo.lock` also went from 846 to 858 packages, and it now lists `wgpu` 29 with its `naga`,
`wgpu-core`, `wgpu-hal` and `wgpu-types`. **None of them is built**: `cargo tree -i
wgpu@29.0.4 --target all` finds no path to them. eframe names `egui-wgpu` through weak
features (`egui-wgpu?/wayland`, `egui-wgpu?/x11`) and Cargo locks such a dependency without
activating it. eframe 0.29 did the same with egui-wgpu 0.29, whose wgpu was 22, so nothing
showed. They are in the lockfile `cargo audit` reads, so an advisory against them would
reach it.

## API changes that needed a judgement

**eframe's default features.** 0.34 swapped the `glow` renderer out of eframe's defaults
and `wgpu` in, so `eframe = "0.34"` would have dropped `Renderer::Glow` and linked wgpu 29
beside the compute device's 22. eframe is now taken without default features, with 0.29's
default set named, less `winit/default`, which a dependent cannot name. On Windows that
changes nothing: every feature `winit/default` adds is Linux-only code, and `rwh_06`, the
one that is not, eframe turns on itself. On Linux, libwayland is still loaded at run time,
because glutin turns on `wayland-sys/dlopen`, and `wayland-backend`'s own `dlopen` feature
only forwarded to that. **What is lost is winit's `wayland-csd-adwaita`**: under a Wayland
compositor that asks clients to draw their own title bar (GNOME's), the window gets SCTK's
plain fallback frame instead of the Adwaita one. Keeping it would need `winit` as a direct
dependency, which `agentic-coding-standards.md` §2.9 does not admit, so it goes to the owner
rather than into this change. Its absence also took sctk-adwaita, ab_glyph and ttf-parser
out of the graph.

**The frame split in two.** eframe 0.34 replaced `App::update` with `App::logic`, called on
every frame, and a required `App::ui`, called only while the window is visible: minimized,
or occluded where the platform reports it (winit 0.30 does not on Windows), it is skipped.
0.29 called `update` on every frame, minimized or not, and the desktop's tick is the
journal's fsync, the link to the node and the queue's clocks. So the tick is in `logic`.
egui also closes a second window that its parent's frame did not draw, and 0.29 drew the
detached panels every frame, so a detached approval queue stayed open and live with the main
window minimized. `logic` therefore draws the detached windows itself when `ui` will not
run, by the same test eframe applies. eframe's documentation asks `logic` not to draw; this
is the one exception, and only for windows other than its own. Both paths end the frame the
same way, so the order 0.29 had -- tick, draw, apply the click, advance the replay cursor,
request the next repaint -- holds whether the window is drawn or not.

**Panels inside the frame's `Ui`.** `SidePanel` and `TopBottomPanel` became `Panel::left`
and `Panel::top`, `default_width` became `default_size`, and `show(ctx)` gave way to
`show_inside(ui)` on the root `Ui` eframe now hands `ui`. Keeping the deprecated
`show(ctx)` was considered and refused. eframe 0.34 always runs the frame through `run_ui`,
and `Context::is_pointer_over_egui` now asks whether the pointer is outside the root `Ui`'s
remaining rectangle. With the panels placed on the context, the root `Ui` would stay whole,
and a pointer over any panel would read as not over egui, which in 0.29 it did. The panels'
defaults (resizable, separator, sizes) are the same in both versions, and windows are
constrained to the content rectangle as before. The render probe (`gungnir-ui`'s `harness`)
moved to `run_ui` and `CentralPanel::show_inside` for the same reason: the panel gets the
same rectangle and frame, and only its widgets' ids, which no test reads, now descend from
the root `Ui`.

**Where a rectangle's stroke goes.** egui 0.31 made it an argument. epaint 0.29 stroked
every rectangle outside its edge (`tessellate_rect`, `PathStroke::outside`), so the track
frames and the one-sigma outline pass `StrokeKind::Outside`. epaint 0.34 also snaps
rectangles to pixels and rounds a stroked corner 0.4 points wider; those are egui's own and
are not undone.

**A corner radius is now whole points, at most 255** (`CornerRadius`, a `u8`). The friendly
frame's 2.5 is drawn at 3: half a point on a ten-point glyph, accepted rather than
allocating a path per glyph per frame (`rust-ui-architecture-coding-standards.md` §5). The
one-sigma outline's radius is half its shorter side, and above 255 it would have become 255,
turning a capsule wider than about 510 points into a rounded square. Above that radius the
outline is now built as a path with the radius it asks for, and below it, which is every
outline at ordinary zoom, it is drawn as before.

**Mechanical renames**, each checked for the same meaning: `Rounding` to `CornerRadius`;
`window_rounding`, `menu_rounding` and a widget state's `rounding` to `*_corner_radius`;
`Context::style` and `set_style` to `global_style` and `set_global_style`, whose bodies are
the same; `Margin` holds `i8`, and `Margin::from(8.0)` is `Margin::same(8)`; and
`show_viewport_immediate`'s callback receives a `Ui`.

**three-d 0.19 needed no source change**, and what it does was compared rather than assumed.
An instanced mesh now composes model, instance and animation transforms in that order; this
workspace sets no model transform and no animation, so each glyph's transform is its own, as
before. A shader failure now returns an error that the mesh turns into a panic where 0.18
panicked inside; the same outcome. The perspective camera, the render-state defaults and
context attachment are unchanged; 0.19 adds a planar projection nobody here uses.

**egui_tiles 0.15** keeps every `Behavior` default this workspace does not override, and its
tree simplification defaults are identical. It now paints a tab's border inside the tab,
its own choice. It closes a tab on a middle click, but only a closable one, and none is.

**The bundled fonts' licence identifier.** epaint_default_fonts 0.34.3 declares the Ubuntu
Font Licence as `Ubuntu-font-1.0`, where 0.29.1 said `LicenseRef-UFL-1.0`; `deny.toml`'s
exception for the crate would have failed the licence check on it. Ubuntu-Light, Hack and
NotoEmoji are the same bytes, as are all four licence texts; emoji-icon-font.ttf changed,
under the same MIT text. The exception, `about.toml`'s `accepted` and the two hand-kept
sections of `deploy/third-party-notices.hbs` now name the identifier and 0.34.3. Whether
cargo-about 0.9.2 now prints the Ubuntu licence itself, since the LicenseRef defect
`about.toml` records no longer touches it, was not seen: cargo-about is not installed here
either, so both hand-kept sections stay.

## Tests

Every test passes with its assertions unchanged. Before merging main: 1,989 tests, the same
list with the same outcomes as `main` at `1af6543` (1,985 pass, four ignored). Main moved
four times while this was done and was merged three times, last at `8ecc672`, and the whole
suite ran again after each merge: at the last, 2,004 tests, 2,000 passing and the same four
ignored, fifteen of them new from main. No rendered-panel assertion needed changing: the
render probe reads what egui drew and in what order, and egui 0.34's new text path moved
nothing any assertion reads.

Two test functions in `gungnir-ui/src/theme.rs` had to change, and only to follow renames.
`the_installer_lands_on_the_context` read `ctx.style()` and asserted
`style.visuals.window_rounding == Rounding::ZERO`; it now reads `ctx.global_style()` and
asserts `style.visuals.window_corner_radius == CornerRadius::ZERO`, which is the same
square-cornered window. `the_installer_reflects_whichever_palette_it_is_given` read
`ctx.style()` and now reads `ctx.global_style()`; its assertions are untouched.
`gungnir-app/tests/gl_attachment.rs` changed a doc comment only.

## The desktop, run

`gungnir-app` with no configuration, from a scratch directory: the default baseline, a live
session opened, three-d attached to eframe's context on the machine's NVIDIA GPU (OpenGL 3.3,
driver 576.88), and the window made visible, which eframe 0.34 does only after painting a
first frame. Closed from its window, it saved and closed its session and exited with 0.

**Its pixels could not be looked at from where this ran**, and the same is true of 0.29. The
window's own capture showed its frame with an empty client area, a capture of the screen
behind it showed the desktop, and eframe's back-buffer screenshot (its `__screenshot`
feature, turned on for a diagnostic build and not committed) read back fully transparent
black -- byte for byte the same file for `main`'s 0.29 build as for this one. So the
session, not the move, is what hides them. What the panels look like on a screen after the
move has not been seen.
