# Plan 02: Mission analysis

## Purpose

Characterize the missions Gungnir supports so that every capability, view, screen,
test track, and gap in the other plans traces back to something an operator has to
do. Integrated air defense and counter-UAS is the lead mission; maritime and land are
supporting domains; intelligence and planning are cross-cutting functions.

## Scope

In scope: mission characterization per domain, the operational environment and the
threat systems in it, blue-force roles and organizations, end-to-end mission threads,
concrete vignettes, measures of effectiveness and performance, and the stakeholder and
role model. Written at the tactical and low operational level (a site, a sector, a
task group), which is where a C2 desktop and a service node sit.

Out of scope: strategic-level analysis, targeting doctrine beyond the recommend and
decide boundary, and anything classified.

## Inputs

- `docs/scenario-crate-narrative.md`: the five engineering scenarios, which become
  the seeds of mission vignettes.
- `docs/gungnir-capabilities.md`: what the product does today, so the analysis is
  anchored to it without being limited by it.
- Open doctrine and concepts for countering air and missile threats, counter-UAS,
  maritime domain awareness, and land ISR (national and NATO publications that are
  publicly released); public reporting on the Russia-Ukraine war for the threat
  picture, including one-way attack drones, loitering munitions, cruise and ballistic
  missiles, glide bombs, uncrewed surface vessels, and electronic warfare.
- Subject-matter reviewers for validation.

## Deliverables and target location

All under `docs/mission/`:

| File | Content |
|---|---|
| `README.md` | Index and how the mission set is organized |
| `mission-analysis.md` | The master document: method, mission set, summary of each domain, cross-cutting functions, traceability pointers |
| `operational-environment.md` | Physical, electromagnetic, and information environment; sensor and effector inventories typical of a site or sector; adversary systems and tactics from open sources |
| `air-defense-and-counter-uas.md` | Lead domain: threat classes, engagement sequence, timelines, sensor mix, effector mix, rules of engagement structure, human-decision points |
| `maritime.md` | Supporting domain: port and coastal defense against uncrewed surface vessels and fast craft, surface picture compilation, coordination with air defense |
| `land.md` | Supporting domain: ground picture from ISR feeds, convoy and battery tracking, cueing fires, force protection |
| `intelligence.md` | Cross-cutting: collection management, identification and classification evidence, pattern-of-life, order-of-battle maintenance, dissemination |
| `planning-and-battle-management.md` | Cross-cutting: defended-asset lists, sensor and effector allocation, mission planning and rehearsal, replay and after-action review |
| `mission-threads.md` | End-to-end threads (find, fix, track, target, engage, assess) per domain with steps, actors, information exchanged, decisions, timing |
| `vignettes.md` | Concrete scenarios with geography, forces, timeline, and success criteria; each maps to engineering scenarios and test tracks |
| `roles-and-stakeholders.md` | Roles (existing five plus proposed intelligence analyst, planner, commander), responsibilities, decisions each may take, information needs |
| `measures.md` | Measures of effectiveness and performance per thread, with target values where doctrine or the performance budgets give one |
| `glossary.md` | Mission terms, mapped to the engineering glossary in `docs/README.md` |

## Deliverable outline: `mission-analysis.md`

1. Purpose and method (mission engineering: characterize, define measures, analyze
   threads, identify gaps, feed capabilities).
2. Mission set and priorities (lead and supporting domains, cross-cutting functions).
3. Operational environment summary.
4. Domain summaries with links to the domain files.
5. Cross-cutting functions.
6. Mission threads summary and the thread catalogue identifiers.
7. Vignette catalogue.
8. Roles.
9. Measures.
10. Traceability: threads to capabilities (plan 04), vignettes to test tracks (plan
    07) and engineering scenarios, roles to UX (plan 06), everything to UAF
    operational views (plan 03).
11. Open questions and validation record.

## Mission thread catalogue (initial)

| Id | Thread | Domain |
|---|---|---|
| MT-01 | One-way attack drone raid: detect, track, classify, prioritize, engage with human authorization, assess | Air |
| MT-02 | Mixed salvo of cruise missiles and drones against a defended asset list | Air |
| MT-03 | Small UAS over a protected site: detect, identify hostile or friendly, non-kinetic or kinetic response | Air |
| MT-04 | Uncrewed surface vessel attack on a port or anchored ship | Maritime |
| MT-05 | Surface picture compilation from coastal radar, AIS, and patrol reports | Maritime |
| MT-06 | Convoy and artillery battery tracking from ISR feeds, cueing fires with deconfliction | Land |
| MT-07 | Sensor management under electronic attack: degrade, re-task, maintain picture | Cross-cutting |
| MT-08 | Collection management and classification evidence fusion | Intelligence |
| MT-09 | Defended-asset planning, sensor and effector allocation, rehearsal by replay | Planning |
| MT-10 | Disconnected operation and reconnection: local picture, store-and-forward, reconciliation | Cross-cutting |

Each thread is documented with: trigger, actors and roles, steps, information
exchanged (typed against `gungnir-model` where the system carries it), decision
points and who holds them, timing and tempo, success and failure conditions, and
the engineering scenario or test-track set that exercises it.

## Method

1. **Frame.** Confirm the mission priorities and the role set with the owner.
2. **Characterize.** Draft the operational environment and each domain from open
   doctrine and public reporting; record sources per claim.
3. **Threads.** Write the ten threads; walk each against the current system to note
   what the system does, what a human does, and what nothing does yet.
4. **Vignettes.** Write one vignette per thread, anchored to a realistic but
   fictional geography, with forces and timelines; map to engineering scenarios.
5. **Roles and measures.** Derive roles from the threads; define measures per
   thread with target values where available.
6. **Validate.** Subject-matter review of threads, timelines, and threat
   characterization; record the review.
7. **Publish** under `docs/mission/` and update the traceability pointers in the
   downstream plans.

## Roles

- Owner: mission priorities, role set, review sign-off.
- Writing agent: drafting, sourcing, traceability tables.
- Subject-matter reviewers (human-owned): doctrine interpretation, threat
  characterization, timeline realism.

## Dependencies

None to start. Everything downstream (plans 03 through 09) depends on this plan.

## Effort and sequencing

10 to 15 agent-assisted days; 3 weeks elapsed. First in the sequence.

## Acceptance criteria

- Every thread has actors, steps, decision points, and measures, and names the
  engineering scenario or test-track set that exercises it.
- Every vignette can be turned into a test-track scenario by plan 07 without
  further mission input.
- The role set is validated and each role's decisions are listed.
- Threat characterization cites open sources only, and the sourcing policy from
  plan 07 is followed.
- A subject-matter reviewer has signed off on the lead domain.

## Risks

- Doctrine varies by nation; mitigate by writing to functions and decisions rather
  than a single nation's terminology, with a mapping table.
- Threat characterization drifts out of date; mitigate with a dated sources table
  and a review cadence.

## Open questions

- Which nation's doctrine and terminology to use as the default vocabulary.
- Whether the three proposed roles are adopted now or deferred.
- Whether space and cyber effects on the mission are in scope for this revision.
