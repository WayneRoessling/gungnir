# p256 moves to 0.14 for a constant-time advisory

D-133, taken by the owner on 2026-10-24. RUSTSEC-2026-0387, published 2026-10-23, finds
operations in `crypto-bigint` 0.5.5 that are meant to run in constant time but can take
branches that depend on secret values, depending on the compiler and the target CPU.
`cargo-deny` failed on main and on every open pull request from that day, PR #204 among
them. This is D-34's advisories gate failing a third time, after the two failures of
2026-10-23 that lockfile bumps and the zbus ignores cleared.

## Why the fix is an upgrade

- **Where it comes from.** `crypto-bigint` 0.5 reaches every binary through `p256` 0.13,
  `ecdsa` 0.16 and `elliptic-curve` 0.13. `p256` is the one direct dependency, admitted
  under D-22.
- **No 0.5 release fixes it.** `elliptic-curve` 0.13.8, the last 0.13, requires
  `crypto-bigint` `^0.5`, and the fix is 0.7.6. A lockfile bump cannot reach it.
- **An ignore was rejected.** Unlike the zbus ignores, this is a vulnerability notice in a
  shipped cryptographic path: baseline signing, the transport identity and escrow. A fix
  exists, so there is no reason to carry the notice.

## What changed

- **The workspace entry.** `p256` moves from 0.13 to 0.14 and gains the `getrandom`
  feature. The lockfile now holds `elliptic-curve` 0.14.1, `crypto-bigint` 0.7.6,
  `ecdsa` 0.17, `signature` 3.0, `der` 0.8, `spki` 0.8, `pkcs8` 0.11 and `sec1` 0.8.
- **Key generation, in `gungnir-security/src/asymmetric.rs`.** 0.14 no longer re-exports
  `rand_core`'s `OsRng`, and `SigningKey::random`, `SecretKey::random` and
  `EphemeralSecret::random` are deprecated. Each now calls `Generate::generate()`, which
  draws from the operating system's RNG through `getrandom` and panics if that RNG fails,
  exactly as `OsRng` did. The `getrandom` feature is what provides it. So the source of
  every key is unchanged.
- **Points.** `ToEncodedPoint::to_encoded_point` becomes `ToSec1Point::to_sec1_point`, with
  the same uncompressed SEC1 encoding.
- **The escrow nonce.** `Aes256Gcm::generate_nonce` keeps `aes-gcm` 0.10's own `OsRng`,
  imported as `aes_gcm::aead::OsRng`, because that is the `rand_core` version `aes-gcm`
  takes. It used `p256`'s re-export before; both are the same operating-system RNG.
- **Tests in `gungnir-security/src/managed_service.rs`** generate their keys the same way.
  `gungnir-remote/src/identity.rs`'s tests, the only other user of `p256`, compiled
  unchanged.

## What did not change

No signature, key or wire format moved. Signatures are still DER-encoded ECDSA over
SHA-256, points are still uncompressed SEC1, and keys are still PKCS#8 PEM. The baseline
signatures, escrowed keys and transport identities written under 0.13 read under 0.14.
The tests of `gungnir-security`, `gungnir-remote`, `gungnir-api` and `gungnir-node`,
including the escrow round trips and the managed-service signature tests, pass: 306 tests,
none failing.

## Duplicate linkage

§2.9 asks a version change to state `cargo tree -d` on the release targets. After the
change, `x86_64-unknown-linux-gnu` shows 41 duplicated crates and `x86_64-pc-windows-msvc`
34. In the RustCrypto stack only `getrandom` is duplicated, and it was before: 0.2, 0.3 and
0.4 were all in the lockfile. Across the whole lockfile the duplicated crates fall from 83
to 81. `const-oid` 0.9 and `hkdf` 0.12 leave, so each collapses to one version, and
`itertools` goes from three versions to two. Nothing new is duplicated.

## Ownership

`gungnir-security` is human-owned (`docs/agentic-workflow.md`), so the two changed files
there, `src/asymmetric.rs` and `src/managed_service.rs`, take the owner's signature once
they are on main. `docs/signatures.md` records it.
