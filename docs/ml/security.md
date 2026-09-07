# Security

Status: first draft, 2026-09-04. Threats specific to learned components, on top of the
system-wide posture in `../../ARCHITECTURE.md` §8.5 and
`../architecture/uaf/security/Sc-Tx.md`. **Human-owned review**: the security reviewer
signs this off before `gungnir-ml` lands (`../agentic-workflow.md`).

## 1. Model supply chain

The model file is executable content in everything but name: it determines what the
system says about a track.

| Control | Mechanism |
|---|---|
| Integrity | SHA-256 of every artefact in the manifest; verified before the file reaches the runtime; a mismatch fails the load with `ArtefactHashMismatch` and never falls back to an unverified copy |
| Authenticity | Artefacts signed with the same keyless signing as the binaries (`../release-governance.md`); default models are release artefacts in the bill of materials |
| Provenance | The card records the dataset hash, the training repository commit, the exporter, and the opset, so any output can be traced to the rows that produced it |
| Runtime | The ONNX Runtime native library is a new native dependency in a workspace that has none; it enters through the §2.9 sign-off with the security reviewer's agreement, and it is covered by the advisory scanning in `../release-governance.md` |
| Third-party weights | None. Every model is trained by the project. A downloaded model would be untrusted executable content with no provenance |

## 2. Adversarial input

An adversary who understands the classifier can fly to be misclassified. This is not
hypothetical for a system whose input is an adversary's own vehicles.

The design's answer is structural rather than algorithmic:

- **The model is one voice.** A classification never rests on model evidence alone;
  the per-class policy margin (GAP-018) requires corroboration for a hostile
  declaration, and cooperative identity is stronger evidence than any model.
- **Friendly and civil recall is the tightest threshold** (0.98), so the failure the
  adversary would most want to induce, a friendly declared hostile, is the one the model
  is tuned hardest against.
- **Out-of-domain inputs are reported, not extrapolated.** A track outside every class
  envelope yields low confidence and a drift count, not a confident guess.
- **The anomaly detector cannot silence a sensor.** It raises alerts; quarantine stays
  with the gateway's deterministic rules, so an adversary who can drive the anomaly
  score cannot use it to blind the system.
- **A model cannot act.** The worst outcome of a fully compromised model is bad evidence
  in front of a human who can see the evidence list, the model's contribution, and what
  the answer would be without it (`ui-integration.md` §2).

Not claimed: robustness to adversarial perturbation in any formal sense. The models are
gradient-boosted trees and small networks on kinematic features, and no adversarial
training is planned. The mitigation is the architecture, not the model.

## 3. Data handling

The highest-likelihood security failure in this plan is not an attack. It is training
data leaving a deployment.

- Datasets built from session journals are **operational data**: they carry track
  positions, sensor identities, decisions, and operator identities.
- They stay in the deployment's jurisdiction, under the retention policy, and are not
  copied into the training repository without the customer's written agreement
  (`data-pipeline.md` §6).
- The training repository is separate partly for this reason: the boundary is explicit
  and crossing it is an act, not a default.
- Under the US jurisdiction decision (D-B1), a dataset derived from a customer's
  operational data may be controlled technical data. The export question in the business
  plan (D-B8) covers models and datasets, not only the software.
- Model cards state the data's origin. A model trained on one customer's data is not
  shipped to another.

## 4. Inference-time exposure

- Inference is local, in the same process, in every profile. **No feature, track, or
  detection leaves the host for inference.** This is the sharpest difference between the
  learned components here and the assistant in plan 08, whose cloud provider does
  receive data under an egress policy.
- Model outputs are journalled like any other event and inherit the journal's protection.
- No telemetry about model performance leaves a deployment.

## 5. Availability

- A model that fails to load, errors, or exceeds its latency budget is disabled and
  reported; the system continues on its deterministic path (`use-cases.md` failure
  behaviours).
- A model cannot stall the tick: budget breach disables it for the session
  (`architecture.md` §6).
- Therefore denial of the model is a degradation, never an outage, and it is visible.

## 6. Review checklist for the security reviewer

1. The `ort` dependency and its native library, against the dependency policy.
2. Hash and signature verification on the load path, including the failure branches.
3. That no consumer can reach an action from a model output, by review and by the
   cross-layer verification row.
4. The data-handling boundary between this workspace and the training repository.
5. The drift and absence reporting, that neither can be silently suppressed.
6. Whether the export determination (D-B8) covers model artefacts and datasets.

## Traceability

`../release-governance.md`; `../architecture/uaf/security/Sc-Pr.md`;
`../agentic-workflow.md` for the human-owned rule; `../business/open-questions.md` D-B8;
capability CAP-6.5 and CAP-6.7.
