# GAP-166 is nalgebra's issue 1520

GAP-166's action was to report nalgebra's Stacked Borrows violation upstream, a public post
being the owner's to make. Before drafting the report on 2026-10-25, the reproduction was
re-run and nalgebra's issues were searched. The defect is already reported, so this changes
the gap to track that report rather than to file a duplicate.

## Upstream

- **[nalgebra#1520](https://github.com/dimforge/nalgebra/issues/1520)**, "Undefined Behavior
  in some decomposition methods on `SMatrix`", open since 2025-05-29. It is the same
  defect, with the same path. A contributor traced it in February 2026: `axcpy_uninit`
  turns one column view into a `&mut [T]` and the other into a `&[T]` over the same
  storage, and the first retag invalidates the second.
- **[nalgebra#1587](https://github.com/dimforge/nalgebra/pull/1587)**, that contributor's
  fix. It moves `axcpy_uninit`'s helpers to raw pointers. It has been open since
  2026-02-18 with no review and no comments, and 0.35.0 does not include it.
- **[nalgebra#1588](https://github.com/dimforge/nalgebra/issues/1588)**, a second
  Stacked Borrows site in sub-view creation (`&mut self.data` in `matrix_view_impl!`),
  which #1587 would expose next. Both have to be fixed before the gate can run under
  Stacked Borrows.

## The reproduction, re-run 2026-10-25

A standalone crate, with miri 2026-09-25 on `x86_64-pc-windows-msvc`:

```toml
[dependencies]
nalgebra = "=0.35.0"
```

```rust
fn main() {
    let m = nalgebra::Matrix3::new(4.0, 2.0, 0.0, 2.0, 5.0, 1.0, 0.0, 1.0, 3.0);
    let chol = nalgebra::Cholesky::new(m).expect("symmetric positive definite");
    println!("{}", chol.l());
}
```

| nalgebra | `cargo +nightly miri run` | `MIRIFLAGS=-Zmiri-tree-borrows` |
|---|---|---|
| 0.35.0, the latest release | fails: a retag for Unique permission from a tag no longer in the borrow stack, at `matrix_view.rs:277` | passes, with the correct factor |
| 0.33.3, the workspace's | fails the same way, at `matrix_view.rs:256` | passes |

The 0.35.0 trace is `Cholesky::new` → `Cholesky::new_internal` (`cholesky.rs:234`) →
`Matrix::axpy` → `Matrix::axcpy` → `axcpy_uninit` (`blas_uninit.rs:108`) →
`ViewStorageMut::as_mut_slice_unchecked`. This is #1520's path, under a different release.

#1587's branch was not run. It is an unreviewed fork, and building it means running code
nobody here has reviewed.

## What the owner may post

The owner may add one comment to #1520. It would give the 0.35.0 data point above, say
that a downstream crate cannot run miri under its default model because of this, and ask
for a maintainer to review #1587. No new issue should be filed. The draft handed to the
owner on 2026-10-25 says exactly that, and nothing in it is stated without the run above
behind it.

## When this closes

When a nalgebra release carries the fixes for both #1520 and #1588, the workspace moves to
it and the miri gate goes back to Stacked Borrows. That reverses the gate's Tree Borrows
setting, which D-90 put in place. If the language adopts Tree Borrows first, the gate keeps
it deliberately and the gap closes on that. Until then, GAP-166 stays open.
