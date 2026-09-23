# Gungnir-render counts its own device creation

GAP-109 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
filed by the GAP-067 walk of 2026-09-16
([`../2026-09-16/gap-067-walk.md`](../2026-09-16/gap-067-walk.md)).

## What was wrong

The `gungnir-render` verification row's method was already written as "a debug counter of
device and buffer creations per frame", but nothing implemented one. `GpuContext::new`
is the single place `wgpu::Instance::request_adapter` and `request_device` are called
(rust-ui-architecture-coding-standards.md §5), and nothing measured how many times a
caller actually asked for it -- so a per-frame resource leak, exactly the regression the
row exists to catch, would have passed every test in the workspace.

## What was built

A debug-build-only `AtomicUsize`, `DEVICE_CREATION_ATTEMPTS`, incremented at the top of
`GpuContext::new`, read through `gungnir_render::device_creation_attempts()`.

**It counts invocations, not successful devices.** A call that returns
`RenderError::NoAdapter` still counts. The property this row protects is "the device is
requested at most once"; a caller that repeatedly *tried* to stand one up on a host with
no adapter would hit `request_adapter` every call, which is the same per-frame cost this
row exists to rule out, and counting only successes would make the guard vacuous on any
host without a real adapter -- including most of plain `cargo test`, this machine
included.

The caller side was already correct: `gungnir-app::fusion::FusionBackend` memoizes its
resolution behind an `Uninitialized`/`Gpu`/`Cpu` enum, so `GpuContext::new()` is called at
most once across the process regardless of how many frames call `engine_for`. This gap
adds the measurement the row named; it does not change that logic.

## What the test holds

`device_creation_stays_at_one_across_many_frames`
(`gungnir-app/src/fusion.rs`, `#[cfg(test)]`) builds a `FusionBackend`, reads the counter,
calls `engine_for` across five simulated frames, and asserts the counter moved by exactly
one. `#[ignore]`d: it needs a real `wgpu` adapter, per this workspace's own rule that GPU
point-cloud registration is validated on a GPU-enabled runner, never inside plain
`cargo test` (`agentic-workflow.md`; the same gate `gungnir-data-fusion/tests/gpu_vs_cpu.rs`
already uses).

**Negatively checked.** Temporarily removing the early-return guard in
`FusionBackend::ensure_resolved` and re-running with `--ignored` on this machine's
adapter failed the test as expected (`left: 5, right: 1`); the guard was restored before
committing.

## What it left open

The counter is debug-build only, matching the row's own method text; a release build
carries no measurement, which is consistent with `gungnir-ui`'s frame-budget row's own
debug/release split elsewhere in the table.
