# Architecture frameworks

Two framework views of the same architecture, both text-first in the repository:

- `uaf/` for [plan 03, UAF views](../plans/03-uaf-views.md): the Unified Architecture
  Framework 1.2 views and the element registry every view references (first draft
  2026-09-04; start at `uaf/README.md`).
- `togaf/` for [plan 10, TOGAF ADM documentation](../plans/10-togaf-adm-documentation.md):
  the governance and lifecycle documents, which reference the UAF views rather than
  redrawing them.

The technical reference for the code itself remains the workspace
[`ARCHITECTURE.md`](../../ARCHITECTURE.md); the crate manifests are the truth for the
dependency graph, and the views here are generated from or checked against them.
