# Software assurance and release governance

The verification stack in `agentic-workflow.md` proves the algorithms are correct.
This document covers the parallel assurance track a release needs before an
organization can stand behind it externally (`gungnir-capabilities.md` §5.6):
dependency and license policy, vulnerability management, a software bill of
materials, reproducible builds, signing, and artifact promotion. It is enforced by
`.github/workflows/release.yml` and configured by `deny.toml`.

## Policy

| Control | Mechanism | Where |
|---|---|---|
| Allowed licenses | `cargo deny check licenses` against the allow-list, evaluated for the two release targets only; one scoped exception admits the OFL-1.1 and Ubuntu Font Licence font data in `epaint_default_fonts`, which `gungnir-app` embeds. Runs on every pull request (`ci.yml`) and every release | `deny.toml` `[graph]`, `[licenses]`, `[[licenses.exceptions]]` |
| Third-party notices | `cargo about generate` collects the license text and copyright notice of every crate linked into a release binary into `THIRD-PARTY-NOTICES.md`, shipped beside the binaries with `LICENSE`, `LICENSE-ADDITIONAL-TERMS.md` and `NOTICE`; the permissive licenses and both font licenses condition redistribution on exactly that, and an SBOM's identifiers do not satisfy it | `about.toml`, `deploy/third-party-notices.hbs`, `release.yml` `assurance` job |
| Outbound license | `AGPL-3.0-or-later` plus §7 additional terms; every allow-list entry must be one-way compatible *into* it, so a denied license can make the workspace undistributable rather than merely unvetted | `LICENSE`, `LICENSE-ADDITIONAL-TERMS.md`, `Cargo.toml` `[workspace.package]` |
| Relicensing rights | Contributors sign off (DCO) and grant Roessling Digital Solutions LLC the right to relicense, without which the commercial edition cannot ship | `CLA.md`, `CONTRIBUTING.md` |
| Third-party material in-tree | Fixtures under `testdata/` are redistributed under their own licenses, never linked or shipped; each directory's `SOURCE.md` records origin and license, and the required license texts sit beside them | `NOTICE`, `testdata/*/SOURCE.md` |
| Known vulnerabilities | `cargo deny check advisories` and `cargo audit` on every release; `fuzz-nightly.yml` for our own parsers | `release.yml`, `fuzz-nightly.yml` |
| Dependency provenance | Only crates.io; git and unknown registries denied; wildcard versions denied | `deny.toml` `[sources]`, `[bans]` |
| Duplicate dependency versions | Warned, so the single resolved set in the workspace `Cargo.toml` stays single | `deny.toml` `[bans]` |
| SBOM | CycloneDX JSON for both binaries, attached to every release | `release.yml` `assurance` job |
| Auditable binaries | `cargo auditable` embeds the dependency list in each binary for later scanning | `release.yml` `build` job |
| Signing | Keyless Sigstore `cosign` signatures for every artifact | `release.yml` `sign` job |
| Container image | Built from `deploy/node/Dockerfile`, unprivileged runtime user, pinned base images; carries `LICENSE`, `LICENSE-ADDITIONAL-TERMS.md` and `NOTICE` under `/usr/share/doc/gungnir/` because an image conveys object code (AGPL sections 4 and 6) | `release.yml` `container` job |
| Toolchain pinning | `rust-toolchain.toml` pins the channel every job and developer uses | Repository root |

## Artifact promotion

1. A tag `vX.Y.Z` on `main` triggers `release.yml`.
2. The `assurance` job must pass before any build starts; a policy failure blocks
   the release, it does not warn.
3. Builds produce `gungnir-app` for Windows x86_64 and `gungnir-node` for Linux
   x86_64 (the container image), each with embedded audit data.
4. Signatures and the SBOM are uploaded next to the artifacts.
5. Promotion to a deployment environment (on-prem or cloud) is a separate, manual
   step that verifies the signature and records which SBOM was deployed. The
   mechanics of that step are environment-specific and are written per deployment.

## Compliance evidence

Each release leaves, as CI artifacts: the `cargo deny` and `cargo audit` reports,
the SBOM, the `licenses` bundle (`LICENSE`, `LICENSE-ADDITIONAL-TERMS.md`, `NOTICE`,
`THIRD-PARTY-NOTICES.md`), the signatures, and the CI logs of every gate that ran. That set is the
evidence package a reviewer or accreditor asks for; nothing needs to be assembled
by hand after the fact.

## What is not yet decided

- Registry decided on 2026-09-04 and amended on 2026-09-07 (D-10): the GitHub
  Container Registry; push stays disabled until GAP-061 runs the workflows against a
  hosted repository.
- Whether releases require a signed configuration baseline as well as signed
  binaries (`gungnir-security` names it; nothing enforces it yet).
- Vulnerability-response time objective, set on 2026-09-04 (D-10 follow-up): critical
  advisories in dependencies or own code fixed or mitigated within 7 days of
  publication, high within 30 days, lower severities at the next release.

## Hosting

Decided on 2026-09-04 and amended on 2026-09-07 (D-10 in
`mission/gap-analysis/decisions-needed.md`): the repository is hosted on GitHub at
`https://github.com/WayneRoessling/gungnir`, CI is GitHub Actions, and images go to the
GitHub Container Registry. The eight workflows under `.github/workflows/` are both the
specification of the gates and the thing that runs them, so GAP-061 no longer ports
anything; the gate names and pass criteria do not change.

`gpu-fusion.yml` needs a self-hosted runner labelled `gpu` registered to the repository.
No hosted runner has a GPU, and that requirement belonged to the workflow rather than to
the forge it ran on, so the amendment does not remove it. Since 2026-09-07 the workflow
runs on manual dispatch only: a pull-request trigger queued a job that waited a day for
the absent runner and then failed, and the job it would have run is empty, because the
GPU path and its tests (GAP-024) do not exist yet. It fails a run that executed zero
tests, so registering a runner alone cannot make it green.

**No forge credential appears in this repository, and none can.** GitHub does not accept
a password for git operations; access is a personal access token or an SSH key, held by
the owner outside the workspace. No configuration baseline names one -- a baseline may
not carry key material or a path to it -- and the release workflow's publish step reads
its token from the CI environment.

The repository was created and pushed on 2026-09-07, which is the first line of
GAP-061 and not the rest of it: no gate result is recorded anywhere yet, because none
has been read. And the evidence
package above is published with the code if the repository is public, which is the
owner's call and is not fixed here.
