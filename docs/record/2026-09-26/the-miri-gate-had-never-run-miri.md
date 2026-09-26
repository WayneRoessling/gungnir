# The miri gate had never run miri

GAP-164 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
Gate 3 of [`../../agentic-workflow.md`](../../agentic-workflow.md).

## What was wrong

`miri.yml` installs a nightly toolchain with the `miri` component and then runs
`cargo miri`. The workspace's `rust-toolchain.toml` pins `1.98`, and a toolchain file
outranks the default the setup action sets, so `cargo` resolved to the stable release,
which has no `miri` component: `cargo miri setup` failed before anything was
interpreted. It was found on 2026-09-26 when a doc comment in GAP-124's pull request used
the word the gate scans for, which triggered the job three times, and all three runs failed at setup
with "the 'miri' component ... is not available for the '1.98-x86_64-unknown-linux-gnu'
toolchain". No workspace source holds an `unsafe` block, function or impl today, so no
pull request had needed the gate yet; the next one that does would have met a gate that
could not pass, whatever its code did.

## What changed

`cargo +nightly miri setup` and `cargo +nightly miri test`: an explicit `+toolchain`
outranks the toolchain file. The workflow also takes `workflow_dispatch`, and a dispatched
run skips the `unsafe` scan and runs miri, so the gate can be seen to work on a diff that
has no `unsafe` in it. Nothing about when the gate runs on a pull request changed: the scan
still matches the word anywhere on an added line, comments included, which is broader
than the code it protects and is left as it is, because narrowing a gate is the owner's
call.

## Evidence

The first dispatched run on `main` after this merges is the evidence that the job
interprets the twelve crates; GAP-164 records its outcome.
