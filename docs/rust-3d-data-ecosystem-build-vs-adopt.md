# Rust 3D Data Ecosystem — Build vs. Adopt

**Companion to:** `rust-ui-tech-stack-summary.md`, `rust-ui-architecture-coding-standards.md`, and the workspace `ARCHITECTURE.md` §3–§4

**Purpose:** This document extends the established `eframe` / `egui` / `three-d` stack, with `wgpu` for compute, with a concrete plan for the data side of the application — point cloud, VTK-style scientific mesh, and geospatial-tile ingestion — and identifies exactly where to adopt existing Rust crates, where to write thin bridge crates, and where genuinely new engineering (GPU-accelerated point cloud registration/fusion) is warranted.

> **Scope note:** This document covers 3D-data ingestion and rendering integration only. Sensor protocols belong to `gungnir-ingest`, map and geofence layers to `gungnir-geo`, and mission logic to the productization layer; see `gungnir-capabilities.md` §5. Section numbers here are cited from Rust doc comments and must not be renumbered.

> **Rendering contexts:** `three-d` renders through OpenGL (`glow`), not `wgpu`. The `wgpu` device this document's §3 uses for compute is a separate, headless device owned by `gungnir-render`; nothing in §1–§2 touches it. Where the text below says "GPU upload" for viewport geometry it means upload to the three-d OpenGL context. See `ARCHITECTURE.md` §9.

---

## 0. Summary Decision Table

| Area | Decision | Where it lives in the workspace |
|---|---|---|
| Point cloud / LAS / LAZ / COPC I/O | **Adopt** (`las`, `laz`, `copc-rs`, `pasture-core`, `pasture-io`) | `gungnir-data/src/pointcloud/` |
| VTK file format I/O | **Adopt** (`vtkio`, optionally `vtk-pure-rs` for filter pipelines) | `gungnir-data/src/scientific/` |
| Terrain/DEM/classification | **Adopt** (`oxigdal-3d`) | `gungnir-data/src/geospatial/` |
| glTF asset loading | **Adopt** (`gltf`) | `gungnir-data/src/assets/` |
| Streaming 3D Tiles / COPC-EPT → `three-d` scene | **Build (bridge module)** — working name `tiles3d-threed` | `gungnir-viewport3d/src/streaming/` |
| VTK data model → `three-d` mesh | **Build (bridge module)** — working name `vtk-threed-bridge` | `gungnir-viewport3d/src/scientific/` |
| GPU-accelerated point cloud registration/fusion | **Build (new subsystem)** — working name `pc-fusion-wgpu` | `gungnir-data-fusion` (its own crate: compute-only, GPU-aware, see §3) |

The working names in the Decision column are how these pieces are referred to in the text below; in the workspace they are modules of `gungnir-viewport3d` and the crate `gungnir-data-fusion`, not separately published crates. Paths written as `data/...` or `viewport3d/...` in the sections that follow are relative to `gungnir-data/src/` and `gungnir-viewport3d/src/` respectively.

---

## 1. Adopting the I/O and Data-Model Crates

### 1.1 Principle

Per the architecture standards' one-way dependency rule (`data → state → ui/viewport3d → render`), every crate in this section is a **`data/` layer citizen only**. None of them may know about `egui`, `three-d`, or `wgpu` types. This is what keeps them swappable and unit-testable per Section 8 of the coding standards — file parsing and point-cloud math run in plain `cargo test`, no GPU context required.

### 1.2 Crate Set and `Cargo.toml`

```toml
[dependencies]
# Point cloud I/O
las = { version = "0.9", features = ["laz-parallel"] }
copc-rs = "0.5"
pasture-core = "0.5"
pasture-io = "0.5"

# Scientific mesh / VTK
vtkio = "0.6"
# vtk-pure-rs pulled in only if/when filter-pipeline needs (isosurfacing,
# scalar-field derivation) exceed what a hand-rolled data/scientific/filters.rs
# can reasonably cover — see 1.4.

# Terrain / geospatial
oxigdal-3d = "0.1"

# glTF assets
gltf = "1"
```

Per Section 9 of the coding standards (dependency discipline), pin these to exact tested minor versions in the PR that introduces them, and record the tested set in `ARCHITECTURE.md` §9. The workspace `Cargo.toml` currently pins `las`, `vtkio`, and `gltf` only; `copc-rs`, `pasture-core`, `pasture-io`, and `oxigdal-3d` are young/pre-1.0, were not re-verified on crates.io when the scaffold was generated, and must be verified and pinned before first use — treat any later upgrade as a deliberate, reviewed change, not an incidental `cargo update`.

### 1.3 Module Layout

```
data/
  mod.rs                    # DataStore aggregate — single source of truth for loaded data
  pointcloud/
    mod.rs
    las_source.rs            # wraps `las`/`laz` — file → PointBuffer
    copc_source.rs            # wraps `copc-rs` — bounded/LOD queries against COPC files
    buffer.rs                  # thin newtype over pasture's VectorBuffer/columnar layout,
                                # re-exported so viewport3d never depends on `pasture` directly
  scientific/
    mod.rs
    vtk_source.rs              # wraps `vtkio` — Vtk::import → internal MeshData
    filters.rs                 # project-owned filter functions (threshold, clip, derive-scalar)
                                # operating on the internal MeshData type
  geospatial/
    mod.rs
    terrain_source.rs           # wraps `oxigdal-3d` — DEM/TIN → internal TerrainMesh
    classification.rs
  assets/
    mod.rs
    gltf_source.rs              # wraps `gltf` — asset → internal StaticMesh
```

The scaffold currently collapses each subdirectory into a single `mod.rs` holding the internal type and its `load_*` functions (plus `scientific/filters.rs`); split into the files above as each loader is implemented.

**Key design rule:** every `*_source.rs` module converts the third-party crate's types into an **internal, dependency-free type** (`PointBuffer`, `MeshData`, `TerrainMesh`, `StaticMesh`) defined in this project. Nothing outside `data/pointcloud/`, `data/scientific/`, etc. ever imports `pasture`, `vtkio`, or `oxigdal_3d` types directly. This is the same inversion-of-dependency pattern the architecture doc already mandates for the `data → ui` boundary, applied one level deeper — it means swapping `copc-rs` for a future replacement touches one file, not the whole codebase.

```rust
// data/pointcloud/buffer.rs
/// Project-internal point buffer, decoupled from `pasture`'s type so that
/// viewport3d and app::state never take a direct dependency on pasture.
pub struct PointBuffer {
    pub positions: Vec<[f32; 3]>,
    pub intensity: Option<Vec<f32>>,
    pub classification: Option<Vec<u8>>,
}

impl From<pasture_core::containers::VectorBuffer> for PointBuffer {
    fn from(buf: pasture_core::containers::VectorBuffer) -> Self {
        // conversion logic — isolated here, tested here
        todo!()
    }
}
```

### 1.4 `vtk-pure-rs`: adopt now, or defer?

`vtkio` alone (parser/writer) is sufficient if the only need is reading/writing VTK files and doing simple scalar coloring — a handful of filter functions in `data/scientific/filters.rs` (threshold, clip-by-plane, scalar-to-color mapping) cover most dashboard/situational-awareness needs and are easy to unit test.

Pull in `vtk-pure-rs` only when a specific panel needs something from its filter pipeline that isn't worth reimplementing (isosurface extraction via marching cubes, volume rendering setup). Because `vtk-pure-rs` includes its own `wgpu` rendering path, **do not** use its renderer — extract only its data/filter layer and feed results through the `vtk-threed-bridge` (§2.2) so all GPU rendering stays owned by this project's `render/` module, per the architecture doc's rule that GPU resources are created once, in one place, not scattered across dependencies.

### 1.5 Background Loading

All of the above are file-I/O-bound. Per Section 5/7 of the coding standards, none of this runs on the render/UI thread. Standard pattern:

```rust
// data/mod.rs
pub enum LoadRequest {
    PointCloud(PathBuf),
    VtkMesh(PathBuf),
    Terrain(PathBuf),
}

pub enum LoadResult {
    PointCloud(Result<PointBuffer, AppError>),
    VtkMesh(Result<MeshData, AppError>),
    Terrain(Result<TerrainMesh, AppError>),
}

// Background thread pulls LoadRequest from a crossbeam-channel receiver,
// does the pasture/vtkio/oxigdal_3d work, sends LoadResult back.
// update() polls the result channel non-blockingly each frame.
```

---

## 2. Bridge Crates: Connecting Existing Ecosystems to `three-d`

Both bridges below follow the same shape: consume the project-internal data types from §1, produce `three-d` scene objects, and own the *streaming/LOD policy* — which is genuinely new integration work, not something that exists off-the-shelf for `three-d` specifically. Both live as modules inside `gungnir-viewport3d` (they are the only code that needs both `gungnir-data` types and `three-d` types), and both draw into the eframe OpenGL context, never the `wgpu` compute device.

### 2.1 `tiles3d-threed` — Streaming 3D Tiles / COPC-EPT into `three-d`

**Why this needs to be built:** `forge3d` and the Bevy 3D-Tiles plugin already solve tileset traversal, screen-space-error (SSE) LOD selection, and B3DM/PNTS decoding — but both own their render loop (`forge3d` is a headless Python-facing renderer; the Bevy plugin is wired to Bevy's ECS/render graph). `three-d` has no equivalent. The bridge crate's job is to reuse the *algorithms* (SSE-based refine/traverse, octree culling) while producing `three-d`-native `CpuMesh`/`Gm<Mesh, ...>` instances and driving them through `three-d`'s own render pass.

**Scope:**
- Tileset JSON parsing + bounding-volume hierarchy traversal (can vendor logic patterned on `forge3d`'s `TilesetTraverser`, or depend on it directly for the traversal/culling core if its license and API allow being used as a library rather than only via its Python bindings — verify this before committing to it as a dependency).
- glTF tile content → `three-d::CpuMesh` conversion (via the `gltf` crate already adopted in §1).
- PNTS (point cloud tile) content → `three-d` point/instanced-sprite representation.
- COPC/EPT octree queries (via `copc-rs`/`pasture-io`) → per-tile `PointBuffer` → GPU upload.
- SSE-based LOD selection driven by the **live `three-d` camera** each frame (this is the actual novel work — wiring tile refinement to `three-d`'s camera/frustum types, which no existing crate does).
- Tile cache with eviction (LRU by last-used frame), so GPU buffers are created once per tile and reused, never recreated per frame, per Section 5 of the coding standards.

**Module shape (lives under `viewport3d/streaming/` in the existing tree):**

```
viewport3d/
  streaming/
    mod.rs                 # StreamingLayer — owns tileset root, tile cache
    tileset.rs               # tileset.json model + traversal
    sse.rs                    # screen-space-error calc against three-d Camera
    tile_cache.rs              # LRU GPU-resource cache: TileId -> three-d::Gm<...>
    content/
      gltf_tile.rs            # glTF tile -> CpuMesh
      pnts_tile.rs             # point-cloud tile -> point representation
```

**Threading:** tile *fetch/decode* (file read + glTF/PNTS parse) happens on a background thread pool, per the concurrency standards; only the already-decoded `CpuMesh`/`PointBuffer` and the *GPU upload* (creation of three-d objects in the OpenGL context) happen on the render thread, since GL resource creation must stay there.

**API sketch:**

```rust
pub struct StreamingLayer {
    root: TilesetNode,
    cache: TileCache,
}

impl StreamingLayer {
    /// Called once per frame from viewport3d::render(). Cheap: only walks
    /// already-resident nodes and issues background fetch requests for
    /// newly-needed tiles; never blocks.
    pub fn update(&mut self, camera: &three_d::Camera, ctx: &three_d::Context) {
        // 1. traverse tileset, compute SSE per node against `camera`
        // 2. mark nodes to refine/coarsen
        // 3. for missing-but-needed nodes, enqueue background load
        // 4. for completed background loads, upload to GPU via `ctx`
    }

    pub fn visible_objects(&self) -> impl Iterator<Item = &dyn three_d::Object> {
        self.cache.resident_objects()
    }
}
```

**Testing:** SSE calculation and tileset traversal/culling are pure math against camera/bounding-volume types — unit-testable without a GPU per Section 8. GPU upload and rendering remain manual/visual QA.

### 2.2 `vtk-threed-bridge` — VTK Data Model into `three-d`

**Why this needs to be built:** `vtkio`/`vtk-pure-rs` give a rich scientific data model (scalar fields, unstructured grids, cell data) but no path into `three-d`'s mesh/material types. This bridge is much smaller in scope than §2.1 — it's a conversion + colormap layer, not a streaming system.

**Scope:**
- `MeshData` (the internal type from `data/scientific/`) → `three-d::CpuMesh`, including scalar-field → vertex-color mapping (for e.g. sensor-coverage volumes, EM propagation surfaces, terrain stress/heat overlays).
- A small set of standard colormaps (viridis, turbo, or a project-specific palette pulled from `ui/theme.rs` so 2D and 3D visualizations stay visually consistent).
- Support for both surface (`PolyData`) and volumetric (`UnstructuredGrid`, sliced/clipped) representations, since `three-d` only natively understands triangle meshes — volumetric data must be sliced or iso-surfaced (via `data/scientific/filters.rs`, or `vtk-pure-rs`'s marching-cubes filter per §1.4) before reaching this bridge.

**Module shape:**

```
viewport3d/
  scientific/
    mod.rs
    mesh_convert.rs      # MeshData -> three_d::CpuMesh
    colormap.rs           # scalar -> RGBA, shared palette definitions
```

This one has no meaningful streaming/LOD concern (scientific datasets in this application are expected to be viewport-scoped, not planet-scale), so unlike §2.1 it's a straightforward, small, high-value crate — a good first bridge to build to validate the pattern before tackling the streaming one.

---

## 3. GPU-Accelerated Point Cloud Registration/Fusion

This is the one area identified as a genuine ecosystem gap — no Rust equivalent to Open3D's registration module or PCL's registration/segmentation stack exists today. Because this is the piece you're planning to build, this section goes deeper than §1–2.

### 3.1 Problem Framing

Registration/fusion takes point clouds or tracks from multiple sensors (potentially multiple drones) and aligns/merges them into a common frame in real time. The core operations:

1. **Correspondence search** — for each source point, find its nearest neighbor(s) in the target cloud (or a running fused map).
2. **Rejection** — discard bad correspondences (distance threshold, normal-angle threshold, dynamic-object filtering).
3. **Transform estimation** — solve for the rigid transform (rotation + translation) minimizing residual error over the accepted correspondences.
4. **Iterate** (this is the "I" in ICP) until convergence or a frame-budget cutoff.
5. **Fusion** — merge the aligned cloud into a running map/voxel structure, with confidence weighting and outlier suppression over time.

### 3.2 Algorithm Choice

| Algorithm | Fit for this use case |
|---|---|
| **Point-to-point ICP** | Simplest, cheapest per iteration; weaker convergence on sparse/noisy LiDAR-class data. Good baseline/reference implementation. |
| **Point-to-plane ICP** | Needs normals per target point; converges faster and more robustly than point-to-point on structured scenes. Recommended primary algorithm. |
| **Generalized ICP (GICP)** | Models local surface covariance; more robust still, more expensive per iteration. Consider as a later upgrade once point-to-plane is validated in real-time budgets. |
| **NDT (Normal Distributions Transform)** | Voxelizes target into Gaussians instead of doing per-point nearest-neighbor search; can be more GPU-friendly since it avoids a nearest-neighbor structure entirely — worth prototyping as an alternative to ICP if correspondence search becomes the bottleneck. |

**Recommendation:** implement point-to-plane ICP first (best complexity/robustness tradeoff, well-documented), with the correspondence-search stage architected so NDT can be swapped in later without touching the transform-estimation/fusion stages.

### 3.3 Why This Belongs on the GPU

Nearest-neighbor correspondence search over tens-to-hundreds of thousands of points, every frame, at 60fps, is the actual bottleneck — this is precisely the kind of "fast to build with, but must not touch the render thread" workload the architecture standards already anticipate (Section 5: decouple simulation/data-update rate from render rate). Running it as `wgpu` compute shaders keeps the whole ICP loop on the GPU between the small per-iteration CPU solve. The `wgpu` device is created once by `gungnir-render::GpuContext` and lent to `gungnir-data-fusion`, so there is one `wgpu` device in the process; it is, however, a second GPU context alongside the OpenGL context three-d renders with, and fusion output that must be displayed is read back to a CPU `PointBuffer` for the viewport (`ARCHITECTURE.md` §3, §9).

### 3.4 Proposed Pipeline (WGSL compute stages)

```
Source cloud (GPU buffer)          Target cloud / running map (GPU buffer)
        │                                        │
        ▼                                        ▼
 [1] Spatial hash / uniform grid build for target cloud  (compute pass)
        │
        ▼
 [2] Correspondence search: for each source point,
     query grid cells in target, find nearest neighbor    (compute pass)
        │
        ▼
 [3] Rejection: distance + normal-angle thresholds,
     write valid-correspondence mask                       (compute pass)
        │
        ▼
 [4] Parallel reduction: accumulate cross-covariance /
     residual terms for transform estimation                (compute pass + reduction)
        │
        ▼
 [5] Transform solve (small 6x6 or SVD-class problem) —
     cheap enough to read back to CPU and solve there        (CPU, nalgebra)
        │
        ▼
 [6] Apply transform to source cloud (compute pass),
     repeat 2-5 until convergence/iteration budget
        │
        ▼
 [7] Fusion: merge aligned source into running voxelized
     map, confidence-weighted, with GPU-side voxel grid       (compute pass)
```

**Key GPU-architecture decisions:**

- **Spatial index:** a uniform/hashed grid (not a GPU BVH or kd-tree) for the nearest-neighbor structure — grids are far simpler to build and query in WGSL compute (bucket-sort by cell, then bounded neighbor-cell search) than tree structures, and LiDAR-class point density is fairly uniform, which favors grids.
- **Transform solve stays on the CPU** (step 5): the linear system is tiny (a handful of scalars from the reduction), so reading back a small buffer and solving with `nalgebra` each ICP iteration is far simpler than implementing SVD in WGSL, and the cost is negligible next to the correspondence-search stage.
- **Iteration loop:** each ICP iteration is a handful of GPU dispatches plus one small CPU readback+solve — keep the iteration count bounded (e.g., max 10–20 per frame, or spread across frames if a single fusion pass can't fit the frame budget) so a poorly-converging registration never stalls the render loop, consistent with Section 5's real-time discipline.

### 3.5 Fit Within the Layered Architecture

This is GPU-touching code, which appears to cut against the architecture doc's `data/` layer rule ("no UI/render types imported") — resolve this the same way the existing standards already do for `render/`: give it its own layer rather than shoehorning it into either `data/` or `render/`. In the workspace that layer is its own crate, `gungnir-data-fusion`, which depends on `gungnir-data` for the internal types and on `wgpu`, and nothing else in the data layer depends on it.

```
gungnir-data-fusion/src/
  lib.rs                  # PointCloudFusion trait, FusionStepResult, FusionError,
                          # GpuFusionEngine — public API, GPU-independent surface
  cpu_reference.rs        # pure-CPU point-to-plane ICP reference impl (see 3.6)
  gpu/
    mod.rs                # wgpu compute pipeline setup; borrows the wgpu::Device
                          # from gungnir-render::GpuContext, never a second device
    shaders/
      spatial_hash.wgsl
      correspondence.wgsl
      reduction.wgsl
      fuse_voxels.wgsl
    buffers.rs            # persistent GPU buffer management — created once,
                          # resized only on point-count change, never recreated
                          # per frame (Section 5)
  transform_solve.rs      # CPU-side small linear solve (nalgebra)
```

`FusionEngine` exposes a GPU-independent trait so `app::state`/`viewport3d` can depend on the *interface*, not the `wgpu` internals directly — matching the existing rule that lower layers invert dependencies via traits rather than upper layers reaching into implementation details.

```rust
pub trait PointCloudFusion {
    /// Non-blocking: kicks off/continues registration, returns current best
    /// transform estimate and convergence status. Never blocks the caller.
    fn step(&mut self, source: &PointBuffer) -> FusionStepResult;
}

pub struct FusionStepResult {
    pub transform: nalgebra::Isometry3<f32>,
    pub converged: bool,
    pub inlier_ratio: f32,
}
```

### 3.6 Testing Strategy

GPU compute correctness is notoriously hard to unit test directly (Section 8 explicitly calls out keeping GPU-dependent code out of files that need `cargo test`-only coverage). The recommended approach:

1. **Write a pure-CPU reference implementation first** (`cpu_reference.rs`) — plain point-to-plane ICP over `Vec<[f32;3]>`, no `wgpu` types at all. This is fully unit-testable: known synthetic transforms (rotate/translate a cloud, register it back, assert convergence to the known transform within tolerance) and known degenerate cases (empty overlap, symmetric point sets).
2. **Validate the GPU path against the CPU reference** on small fixed test clouds — not as a `cargo test` (since it needs a GPU context) but as an integration-test binary run in CI on GPU-enabled runners, or as a manual/snapshot QA step per Section 8's guidance on GPU-dependent code.
3. **Keep the CPU reference as a runtime fallback**, not just a test fixture — useful on non-RTX / headless build configurations and as a correctness baseline if GPU results ever look suspicious in the field.

### 3.7 Error Handling & Dependencies

- Crate-local `FusionError` variants: `GpuInit` and `Divergence` (iteration budget exhausted without convergence — this is a normal, expected outcome under bad sensor geometry, not a crash condition; per Section 4, degrade gracefully — fall back to the last good transform and flag low confidence in the UI rather than panicking). `gungnir-app`'s `AppError` wraps these rather than redefining them.
- `nalgebra` (already in the workspace's fixed stack) for the small CPU-side linear solve — no new heavy dependency required for that step.
- No new spatial-indexing crate needed on the CPU side (the reference implementation can use `rstar`/`kiddo` if a CPU-side nearest-neighbor structure is useful for the reference/fallback path); the GPU path uses its own compute-shader grid.

---

## 4. Suggested Build Order

The workspace `README.md` merges this order with the tracking-core and productization order; this section keeps the 3D-data rationale.

1. `vtk-threed-bridge` (§2.2) — smallest scope, validates the data→viewport3d conversion pattern.
2. Adopt the I/O crates (§1) into `data/` behind internal types — needed as inputs to everything else.
3. `cpu_reference.rs` for point-to-plane ICP (§3.6, step 1) — de-risks the fusion algorithm choice before any GPU work starts.
4. GPU fusion pipeline (§3.4–3.5) — highest-risk, highest-value piece; build once the reference implementation gives a correctness baseline to validate against.
5. `tiles3d-threed` (§2.1) — largest scope, defer until a specific panel actually requires planet/site-scale streamed terrain or point clouds; don't build ahead of a concrete requirement.
