# Sd-Tx Standards taxonomy

**UAF definition.** The standards taxonomy view presents the technical, operational,
and business standards that apply to the architecture.

**Purpose here.** Every standard Gungnir implements or is committed to, grouped by
where it applies, with its status and the registry element that carries it. Read by
integrators, accreditors, and plan 10 (standards information base).

Status: first draft, 2026-09-04. The status column follows the `standards` entries of
`../model/elements.yaml`, which `../tools/build_uaf.py` checks against every
`conforms_to` edge in `../model/relationships.yaml`; a row here that disagrees with the
registry is the stale one.

## The taxonomy

| Group | Standard | Applies at | Status |
|---|---|---|---|
| Data encoding | SD-01 Gungnir canonical schema v1 | journal, API, configuration | real |
| Data encoding | SD-05 JSON (RFC 8259) | journal lines, configuration, API payloads | real |
| Data encoding | SD-02 Apache Arrow (arrow 53) | detections for analytics and bulk exchange | real |
| Sensor and track interoperability | SD-03 ASTERIX Category 048 | radar feeds through the gateway | real (`gungnir_interop::AsterixCat048Codec`; GAP-064) |
| Sensor and track interoperability | SD-04 STANAG 4676 | coalition track exchange | planned: blocked on AEDP-12, which is not public (`../../../design/external-standards.md` §2; GAP-064 closed on that finding; out of the release under D-44) |
| Sensor and track interoperability | SD-16 Cursor-on-Target (CoT) | exchange with a participant that holds no machine identity here: reported positions inbound, picture, warnings and handoffs outbound | planned (GAP-090, GAP-091; DN-25). **In scope for the release under D-33**; schema version 2.0, the type tree and the protocol version 1 framing all pinned as of 2026-09-08 |
| Cooperative identity | SD-09 AIS (ITU-R M.1371) | maritime cooperative identity | real (`gungnir_ingest::adapters::ais`; D-09, GAP-010) |
| Cooperative identity | SD-10 ADS-B 1090 ES (RTCA DO-260B) | air cooperative identity | real (`gungnir_ingest::adapters::adsb`; D-09, GAP-010) |
| Transport | SD-06 HTTP/1.1 (RFC 9112); SD-07 WebSocket (RFC 6455) | the API and its event stream | real (`gungnir_api::transport`; GAP-041) |
| Security | SD-08 TLS 1.3 (RFC 8446) | every network crossing; mutual for machines (D-02) | real (`gungnir_api::tls`; GAP-060) |
| Identity | SD-11 UUID v7 (RFC 9562) | `GlobalEntityId` textual form | real: minted since GAP-069 (D-11), and written on every wire and in every journal in this form since GAP-175 (D-101) |
| Packaging and release | SD-12 OCI container image | the service node | real |
| Packaging and release | SD-13 SBOM and signed releases | every release | planned: the SBOM and the audit run in `release.yml` (GAP-061); signing runs at the first tagged release |
| Architecture | SD-14 UAF 1.2 (OMG) | this description | real |
| Architecture | SD-15 TOGAF 10 ADM | plan 10 | real (tailored; `../../togaf/preliminary/tailored-adm.md`) |

Version negotiation for the interop schemas is exact-match in
`gungnir_interop::SchemaCatalog::is_compatible`; minor-version tolerance is a policy
decision recorded in `../../../gungnir-api-v1.md` when a second version exists.

## Elements used

- SD-01 to SD-16.

## Notes

- The registry carries no standard entry for ASTERIX Categories 034, 205 and 129,
  SAPIENT or MISB ST 0601, whose codecs and adapters are `gungnir-interop`'s and
  `gungnir-ingest`'s (`../../../design/external-standards.md` pins each edition, and
  `../../../unbuilt.md` lists the paths that refuse). The `standards` entries and the
  `conforms_to` edges are hand-authored, so adding them is a registry edit, not a
  generator change.
- Platform interfaces (OpenGL through `glow` for the viewport, DirectX 12 or Vulkan
  through `wgpu` for compute) are not standards the product conforms to on behalf of
  a customer and are recorded in `../../../../ARCHITECTURE.md` §9 instead.
- Specifications are obtained from their public sources only (D-09); no controlled
  specification is used.
- SD-16 is **in scope for the release** under D-33 (2026-09-06), which is an addition to
  D-01's scope lock rather than an exception to it. Its status stays `planned` because no
  code exists: the vocabulary describes the implementation, not the commitment.
- SD-16's reference implementations are GPLv3 (verified 2026-09-06 at the upstream
  client and server repositories). The format is implemented from its published schema:
  a wire format is not a derivative work, their source is, and DN-25 §9 keeps the two
  apart. `../../../design/external-standards.md` §5 pins the schema version (2.0, MITRE
  case #11-3895, approved for public release) **before** any codec is written, on
  GAP-064's rule; §5.7 pins the type tree (MITRE's August 2005 developer guide, case
  #06-0249, and the `friend` predicate `^a-f-`) after the owner asked for it on 2026-09-07;
  and §5.4 leaves the protobuf framing unpinned on purpose. Three artifacts, two pinned.

## Traceability

- Derives from: `gungnir-interop/src/lib.rs` (the catalogue); `../../../gungnir-api-v1.md`;
  `../../../release-governance.md`; decisions D-02, D-09, D-11.
- Feeds: Sd-Rm, the resource-to-standard matrix, the interop conformance suite
  (GAP-063).
