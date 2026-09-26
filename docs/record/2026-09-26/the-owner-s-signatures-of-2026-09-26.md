# The owner's signatures of 2026-09-26

Six entries in [`../../signatures.md`](../../signatures.md), each at the main commit that
holds what it covers. They cover the human-owned work of the autonomous batch of
2026-09-25, whose decisions were taken under the owner's delegation and whose code
waited on this review.

- **D-88**, walked with the owner: the thirteen disagreements GAP-111's matrix test
  found between `role_permits` and roles-and-stakeholders §4, and which side was
  corrected. The owner considered the administrator's `Role::rank`, still the highest
  though it now takes no engagement decision, and left it: nothing it decides is weighed
  by rank. Two points were put to the owner and remain their own items: GAP-162, a
  baseline applied through `config.apply` can still change weapons control status, and a
  deployment whose only decider was an administrator now escalates to nobody.
- **GAP-111's code** in `gungnir-security` and `gungnir-api` (PR #172).
- **GAP-105's gateway refusal** in `gungnir-ingest` (PR #171).
- **GAP-138's removal** of the unreachable decision type from `gungnir-api` (PR #165).
- **GAP-119's restructured solve** in `gungnir-allocation` (PR #169).
- **GAP-124's risk score** in `gungnir-assessment`, for its numerical guarantees (PR #168).
