# UAF views

Deliverables of [plan 03](../../plans/03-uaf-views.md): a Unified Architecture
Framework 1.2 description of Gungnir and its mission context, as Markdown, PlantUML,
and Mermaid in the repository, built on one element registry. Start with
[`summary-and-overview.md`](summary-and-overview.md).

Status: first draft 2026-09-04. The code-derived views are generated and checked;
the operational and strategic content inherits the first-draft status of plans 02
and 04 and awaits the owner's review; the resource content awaits engineering
review.

## The grid

Rows are UAF domains, columns the view kinds this project uses. **Produced** views
link to their file; *generated* views are written by `tools/build_uaf.py` from code
or from the mission set; "not produced" views name the reason.

| Domain | Taxonomy (Tx) | Structure (Sr) | Connectivity (Cn) | Processes (Pr) | States (St) | Interaction (Is) | Information (If) | Roadmap (Rm) | Other |
|---|---|---|---|---|---|---|---|---|---|
| Summary (Sm) | | | | | | | | | [Sm-Ov](summary-and-overview.md) |
| Strategic (St) | [St-Tx](strategic/St-Tx.md) | [St-Sr](strategic/St-Sr.md) | [St-Cn](strategic/St-Cn.md) | not produced: no strategic phasing beyond St-Rm | not produced: capability states not needed at scaffold stage | | | [St-Rm](strategic/St-Rm.md) | |
| Operational (Op) | [Op-Tx](operational/Op-Tx.md) | [Op-Sr](operational/Op-Sr.md) | [Op-Cn](operational/Op-Cn.md) | *[Op-Pr-MT-01](operational/Op-Pr-MT-01.md)* to *[Op-Pr-MT-10](operational/Op-Pr-MT-10.md)* | [Op-St](operational/Op-St.md) | *[Op-Is-VG-01](operational/Op-Is-VG-01.md)* to *[Op-Is-VG-10](operational/Op-Is-VG-10.md)* | [Op-If](operational/Op-If.md) | not produced: the operational roadmap is St-Rm | not produced: Op-Ct constraints are the ROE structure in the mission set |
| Services (Sv) | [Sv-Tx](services/Sv-Tx.md) | [Sv-Sr](services/Sv-Sr.md) | [Sv-Cn](services/Sv-Cn.md) | [Sv-Pr](services/Sv-Pr.md) | not produced: service states are Rs-St | not produced: Op-Is covers the scenarios | *[Sv-If](services/Sv-If.md)* | not produced: services follow St-Rm | |
| Personnel (Pr) | [Pr-Tx](personnel/Pr-Tx.md) | [Pr-Sr](personnel/Pr-Sr.md) | [Pr-Cn](personnel/Pr-Cn.md) | not produced: role steps are in Op-Pr | | | | [Pr-Rm](personnel/Pr-Rm.md) | not produced: availability and evolution views (open question) |
| Resources (Rs) | [Rs-Tx](resources/Rs-Tx.md) | *[Rs-Sr](resources/Rs-Sr.md)* | *[Rs-Cn](resources/Rs-Cn.md)* | [Rs-Pr](resources/Rs-Pr.md) | [Rs-St](resources/Rs-St.md) | not produced: Sv-Pr covers the sequences | *[Rs-If](resources/Rs-If.md)* | not produced: Pj-Rm and Sd-Rm cover it | |
| Security (Sc) | [Sc-Tx](security/Sc-Tx.md) | [Sc-Sr](security/Sc-Sr.md) | [Sc-Cn](security/Sc-Cn.md) | [Sc-Pr](security/Sc-Pr.md) | | | | | |
| Information (If) | [If-Tx](information/If-Tx.md) | *[If-Sr](information/If-Sr.md)* | [If-Cn](information/If-Cn.md) | | | | | | |
| Standards (Sd) | [Sd-Tx](standards/Sd-Tx.md) | not produced: no standards profile hierarchy beyond Sd-Tx | | | | | | [Sd-Rm](standards/Sd-Rm.md) | |
| Projects (Pj) | | | | | | | | [Pj-Rm](projects/Pj-Rm.md) | not produced: Pj-Tx and Pj-Sr are the plans index |
| Actual resources (Ar) | | [Ar-Sr](actual-resources/Ar-Sr.md) | [Ar-Cn](actual-resources/Ar-Cn.md) | | | | | | |
| Parameters (Pm) | | | | | | | | | [Pm-Me](parameters/Pm-Me.md) |
| Traceability | | | | | | | | | *[capability to activity](traceability/capability-to-activity.md)*, *[activity to service](traceability/activity-to-service.md)*, *[service to resource](traceability/service-to-resource.md)*, *[resource to standard](traceability/resource-to-standard.md)*, *[role to activity](traceability/role-to-activity.md)*, *[requirement to capability](traceability/requirement-to-capability.md)*, *[requirement to resource](traceability/requirement-to-resource.md)* (both generated from the requirements specification, GAP-083) |

Not produced anywhere: simulation views, XMI or tool exchange, and the dictionary
view (the registry is the dictionary).

A reader who works in DoDAF terms should start from
[`../togaf/framework-cross-reference.md`](../togaf/framework-cross-reference.md), which
maps thirty-six DoDAF views onto the views above rather than drawing a second set.

## The model

- [`model/elements.yaml`](model/elements.yaml): the element registry. Ten kinds,
  stable identifiers (`CAP-x.y`, `OP-nn`, `OA-nn`, `SV-nn`, `RS-<crate>`, `PT-nn`,
  `SD-nn`, `PJ-*`, `IE-nn`, `AR-nn`). The `resources` section is generated from
  the crate manifests.
- [`model/relationships.yaml`](model/relationships.yaml): typed relationships
  (exhibits, achieves, performs, realizes, implements, conforms_to, uses); `uses` is
  generated from the manifests; `status: planned` marks design not yet in code.

Every element named in a view or a diagram exists in the registry; the check
fails otherwise.

## Generating, checking, rendering

From the workspace root:

```bash
python docs/architecture/uaf/tools/build_uaf.py
```

regenerates the registry's generated sections and the generated views (marked
*italic* in the grid), then runs the consistency check: every referenced id
exists; every leaf capability is exhibited by a performer and achieved by an
activity; every activity is realized by a service or performed by a human and
performed by someone; every service is implemented by a resource and its `code`
path exists in the source; every id in a diagram source exists. `--check` runs the
check without writing. CI runs the check (`.github/workflows/ci.yml`, job
`uaf-registry`), which runs from the moment GAP-061 hosts the repository on GitHub.

```bash
bash docs/architecture/uaf/render.sh
```

or `render.ps1` renders every `.puml` and `.mmd` to SVG under `rendered/`
(PlantUML via `plantuml` or the Docker image; Mermaid via `mmdc` or `npx`).
Exercised end-to-end on 2026-09-10 via the `plantuml/plantuml` Docker image (no
local Java/Graphviz install needed) and `npx @mermaid-js/mermaid-cli`; all 53
sources render cleanly. Mermaid sources also render inline on GitLab and GitHub
when pasted into a Markdown fence.

```bash
python docs/architecture/uaf/tools/export_xmi.py
```

writes `exports/gungnir-uaf.xmi`: the same registry as XMI 2.1/UML 2.1, for
import into Sparx Enterprise Architect's UAF MDG Technology. One-way (registry
to EA, never the reverse) and not part of the CI drift check -- run it after
`build_uaf.py` whenever you want EA caught up with the registry. In practice,
XMI import got the elements into EA but not the relationships (twice, on two
different relationship encodings) -- see `export_ea_script.py` below for the
path that actually carries relationships across.

```bash
python docs/architecture/uaf/tools/export_ea_script.py
```

writes `exports/gungnir-uaf-import.vbs`: the same registry built directly in EA
through its own Scripting/Automation interface (`Repository`,
`Package.Elements.AddNew`, `Element.Connectors.AddNew`) rather than through
XMI import, since that carries relationships (as EA Connectors) where the XMI
path did not. Open your EA project, `Tools > Scripting`, create a new VBScript,
paste the file in, run it (Ctrl+F9); Script Output logs progress. Not
idempotent -- delete the "Gungnir UAF Model" package before re-running after a
registry change. `test_ea_script_mock.vbs` executes a freshly generated script
against a hand-written mock of the EA object model via `cscript.exe`, outside
of EA (`cscript.exe //Nologo docs/architecture/uaf/tools/test_ea_script_mock.vbs`);
that confirms the generated VBScript is well-formed and its control flow
completes, not that EA's real object model does what the mock assumes -- see
both scripts' docstrings for exactly what is and is not verified.

## Conventions

- File names carry the view code (`Op-Pr-MT-01.md`, `Op-Is-VG-04.puml`).
- PlantUML stereotypes carry the element kind (`<<Capability>>`,
  `<<OperationalPerformer>>`, `<<Service>>`, `<<Resource>>`, `<<Binary>>`,
  `<<InformationElement>>`, `<<Post>>`); PlantUML aliases replace `-` and `.` in
  ids with `_` (`CAP_2_1`, `RS_core`).
- Each view file: definition, purpose, diagram or table, elements used, notes,
  traceability, in that order. Generated files say so in their status line.
- Status vocabulary: real (implemented and tested), scaffold (trait or facade with
  `NotImplemented` inside), planned (design only, with the gap id).

## Maintenance

- Adding a crate: run the generator; the resource and its `uses` edges appear.
  Then add its `implements` and `conforms_to` relationships by hand.
- Adding a capability, activity, service, or role: append to `elements.yaml` with
  the next identifier, add its relationships, run the generator; fix what the check
  reports.
- Changing a thread step or vignette: edit the mission documents; the Op-Pr and
  Op-Is views regenerate from them; update the step-to-activity map in
  `tools/build_uaf.py` if steps were added.
- Never renumber; never reuse an identifier; retire an element by adding
  `status: retired` and keeping it.
