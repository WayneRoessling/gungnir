# GPU verification row gated on the recorded dispatch

The `gungnir-data-fusion` row "GPU path against CPU reference" in
`verification-capability-table.md` §2 is a gate as of 2026-09-16, gated by the owner, and
GAP-024 closes with it. Its criterion is unchanged: transform within 1e-3 (m, rad) of the
CPU result and inlier ratio within 0.01, run outside plain `cargo test`.

**The evidence is a recorded run, not a sandbox run.** `gpu-fusion.yml` run 35113741715 was
dispatched on `main` at `6374649` and ran on `gungnir-rtx-5060ti`, the registered runner
(NVIDIA RTX 5060 Ti, `rustc 1.98.1` on `x86_64-pc-windows-msvc`). It passed, and all four
`#[ignore]`d GPU tests in `gungnir-data-fusion/tests/gpu_vs_cpu.rs` executed: its own guard
step logged `GPU tests executed: 4`. Two of them assert this row's criterion in its own
terms against `CpuIcp`: `gpu_matches_cpu_reference_transform_and_inlier_ratio` and
`gpu_matches_cpu_reference_with_normals_present`, each checking translation and rotation
difference below 1e-3 and inlier-ratio difference below 0.01. The other two check
self-registration convergence and voxel-fusion self-consistency, which have no CPU oracle.

**Why the row waited for this and not for the earlier pass.** The tests passed on
2026-09-08 against a real adapter the implementing agent's own sandbox happened to have.
That was real hardware execution, but the row's method names the registered runner and
`release-governance.md` defines evidence as the log of a gate that ran, so GAP-024 kept
item (3) open for the dispatch. Getting there took four fixes to the runner itself
between 2026-09-15 and 2026-09-16: the runner was installed and never started; `bash`
resolved to the WSL shim; the full path to Git's bash was split at its space; and a cache
action deleted the owner's `rustup.exe`. Those are recorded as items 133 to 136.

**The evidence still holds on `main`.** Nothing under `gungnir-data-fusion/` or in
`gpu-fusion.yml` changed between `6374649` and `8e03a78`, the `main` this gate was
recorded against.

**What gating this does not claim.** The GPU path is point-to-point Kabsch. The
point-to-plane solve exists only on the CPU (`point_to_plane.rs`), and a GPU version stays
the named follow-on GAP-024 has always described it as, not something this gate covers.
Voxel fusion is checked for self-consistency only. The workflow stays on manual dispatch
(D-10 as amended 2026-09-08), so this gate is re-evidenced by firing
`gh workflow run gpu-fusion.yml`, not by every pull request.

**A correction to the walk sheet.** Group D of the GAP-067 walk sheet said "None has ever
executed on a GPU". That was wrong when it was written: the same table's test column
already recorded the 2026-09-08 sandbox pass. What the row lacked was a recorded dispatch,
not an execution, and the sheet now says so.
