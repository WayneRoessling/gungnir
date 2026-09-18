# Release workflow first runs

`release.yml` had never run: the one workflow in the repository of which that was still
true, and GAP-061's last open item. On 2026-09-17 the owner had it rehearsed in its
assurance-only mode, which runs `cargo deny`, `cargo audit`, the CycloneDX SBOM and the
third-party notices, and builds, signs and publishes nothing. It took three runs, and the
first two each failed on something real. The third, run 35301215267 on `main` at
`a2e0e86`, passed every step, and GAP-061 closes on it.

**Run 1 (35294786920, `75cc451`): a TLS advisory nobody had been shown.** `cargo deny`
failed its advisories check on RUSTSEC-2026-0285: `rustls` 0.23.43 accepted TLS 1.3
handshake messages across an encryption-level boundary. The transcript stays
authenticated, so it is a conformance defect rather than a way to alter a handshake, but
RFC 8446 section 5.1 says such a connection must be terminated, and `rustls` carries
`gungnir-remote`'s human-owned TLS identity path. The owner approved a patch bump to
0.23.45, `Cargo.lock` only and inside the declared range (#141).

**Run 2 (35296523137, `1af6543`): two scanners, two views of what was accepted.**
`cargo deny` passed. `cargo audit` failed on RUSTSEC-2026-0194 and -0195, the `quick-xml`
0.30 advisories under `accesskit_unix` that the owner accepted in `deny.toml` on
2026-09-07, because `cargo audit` cannot read `deny.toml`. The owner chose one list over a
second copy in `.cargo/audit.toml`: `.github/scripts/deny_advisory_ignores.py` hands
`deny.toml`'s ignores to `cargo audit` and refuses any ignore without a reason (#142). It
was proved on a branch dispatch of the workflow (35300094067) before it merged.

**Run 3 (35301215267, `a2e0e86`): green.** `cargo deny`; `cargo audit` with the six
accepted advisories taken from `deny.toml` and no others; the SBOM for both binaries; the
notices; both artifacts uploaded.

**Why the TLS advisory reached main unseen.** Advisories are checked in exactly one place:
this workflow. The per-PR `cargo-deny` job in `ci.yml` checks licenses, bans and sources,
and nothing on a pull request or on a schedule consults the advisory database. A
workflow that runs only at release time is the only thing that would have found
RUSTSEC-2026-0285, and it had never run. That is a named follow-on, not a change made
here: whether advisories should also be checked per pull request or nightly is a separate
decision, and a new advisory would then fail an unrelated pull request, which is a cost
worth deciding rather than defaulting into.

**What closing GAP-061 does and does not claim.** The owner decided (2026-09-17) that the
rehearsal closes it: the gap was "release workflow unexercised", and the workflow is now
exercised and green. The first signed and published release is the release event itself:
a tag push runs `build`, `sign` and `container`, and `sign` uses keyless cosign, which
writes to Sigstore's public, append-only transparency log. None of those three jobs has
run yet, and nothing here says they would pass. The gap's other items were already
settled: the runner stays interactive by D-63, the `solve_assignment` total by D-43, the
`loom` checks by their signature, and the central differential harness retired by D-49.
