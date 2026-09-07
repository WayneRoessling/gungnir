# Plan 01: Product business plan

## Purpose

Produce the business plan that lets Roessling Digital decide how to fund, position,
price, and sell Gungnir: a defense-first command-and-control product with a later
commercial counter-UAS line. The plan must be specific enough to support a funding
conversation and an export-control review, and honest about what is scaffold versus
working software today (`README.md`, "Status").

## Scope

In scope: the customer problem, the product and its differentiation, market sizing,
competitive landscape, business model and pricing, go-to-market, roadmap aligned with
the engineering increments, team, financial projections, risks, and milestones.

Out of scope: legal formation documents, contracts, and detailed sales collateral. The
commercial counter-UAS line is planned as a second phase and sized, not fully
developed.

## Inputs

- `docs/mission/` (plan 02) for the customer problem and operational context.
- `docs/mission/gap-analysis/` (plan 05) for the roadmap's engineering content.
- `docs/gungnir-capabilities.md` for what the product does in business terms.
- `ARCHITECTURE.md` §8 for the deployment profiles that become product SKUs.
- `docs/performance-budgets.md` and `docs/release-governance.md` for the claims the
  product can make about performance and assurance.
- Open-source market data: defense budgets for air defense and counter-UAS, published
  program values, competitor public material, analyst reports where licensed.

## Deliverables and target location

All under `docs/business/`:

| File | Content |
|---|---|
| `business-plan.md` | The plan itself, self-contained, with the sections below |
| `market-analysis.md` | Segments, sizing method and numbers, buyer personas, procurement paths |
| `competitive-landscape.md` | Competitor and adjacent-product profiles, positioning map, differentiation evidence |
| `pricing-and-licensing.md` | License models, price points, packaging by deployment profile, services rates |
| `go-to-market.md` | Channels, partners, pilot strategy, program entry points, sales process |
| `financial-model.md` | Assumptions and a three-year projection, with the spreadsheet `financial-model.xlsx` beside it |
| `roadmap.md` | Product roadmap tied to the engineering increments and gap register |
| `risk-register.md` | Business risks with likelihood, impact, owner, mitigation |
| `open-questions.md` | Decisions the plan needs from the owner, with the date each was answered |

## Deliverable outline: `business-plan.md`

1. **Executive summary.** One page: problem, product, market, ask.
2. **The problem.** Fragmented, expensive command-and-control for air defense and
   counter-UAS; the cost asymmetry of one-way attack drones versus interceptors; the
   need for systems that work disconnected and integrate with what a customer already
   has. Evidence from the mission analysis.
3. **The product.** Gungnir as a desktop plus service-node C2 with a verified tracking
   and intercept-planning engine, an open API, recommendation-only decision support
   with a recorded human decision, and three deployment profiles. What exists today
   and what is on the roadmap, stated plainly.
4. **Differentiation.** Memory-safe Rust; numerics verified against named oracles;
   the same crates in every profile; disconnected operation with reconciliation; an
   agent-assisted development process that lowers cost per capability; open interop
   (ASTERIX, STANAG 4676, Arrow, JSON). Each claim points at the document or code
   that backs it.
5. **Market.** Defense and government first: national air-defense modernization,
   counter-UAS programs, base and site protection, partner-nation programs; primes as
   channel and as customers for the engine. Commercial second: airports, energy and
   utilities, ports, large events. Sizing bottom-up from program counts and
   installation counts, top-down from published budgets; state the method and the
   uncertainty.
6. **Customers and use cases.** Three to five concrete buyer scenarios drawn from the
   mission vignettes, each with the buying organization, the decision maker, the
   procurement path, and the deployment profile they would buy.
7. **Competitive landscape.** Summary of `competitive-landscape.md`: integrated C2
   platforms from primes and new entrants, counter-UAS command-and-control vendors,
   open-source tracking toolkits, and tactical awareness ecosystems; where Gungnir
   competes, complements, or integrates.
8. **Business model.** Perpetual license plus support, or subscription per node and
   per desktop; integration and adaptation services; training; sustainment. Packaging
   by profile (disconnected desktop, on-prem node, cloud node). Services as the early
   revenue while the product matures.
9. **Go-to-market.** Pilots with a lead customer; innovation and accelerator programs
   for defense technology; prime partnerships; conferences and demonstrations built on
   the test-track suite; the commercial line's channel later.
10. **Regulatory and export control.** The product plans intercepts, so it is likely
    controlled under defense export regimes in whichever jurisdiction the company
    sells from. State the jurisdiction, the classification to be obtained, and the
    consequences for cloud hosting, foreign customers, and open-source components.
    This section is written with counsel; the plan records the questions.
11. **Roadmap.** From `roadmap.md`: increments 1 through 4 from
    `docs/gungnir-capabilities.md` §7, the gap register's priorities, and the product
    milestones (first demonstration, first pilot, first production deployment).
12. **Team and hiring.** Current capacity, the roles needed for the first pilot
    (tracking engineer, integration engineer, UX, security and accreditation), and
    the agent-assisted development model as a staffing lever.
13. **Financials.** Summary of `financial-model.md`: revenue by line, cost by
    category, headcount, cash, funding required and use of funds.
14. **Risks.** Summary of `risk-register.md`: export control, accreditation and
    certification, competition from primes, key-person, technical (the tracking math
    is unimplemented), and adoption.
15. **Milestones and asks.** What is being asked of investors, partners, or the
    company, tied to milestones.

## Method

1. **Frame.** Confirm the answers in `open-questions.md` (jurisdiction, funding stage,
   existing conversations, naming). One session with the owner.
2. **Research.** Agent-led desk research with sources recorded per claim: budgets,
   programs, competitors, pricing benchmarks. Human review of every market number.
3. **Customer scenarios.** Derive from `docs/mission/vignettes.md`; write the buyer
   view of each.
4. **Model.** Build the financial spreadsheet with named assumptions; the Markdown
   file explains them. Sensitivity on the three assumptions that move the outcome
   most.
5. **Draft.** Sections in the order above; the executive summary last.
6. **Review.** Owner review of pricing, financials, and regulatory sections; a second
   reader for the narrative.
7. **Publish.** Files under `docs/business/`, indexed in `docs/README.md`.

## Roles

- Owner (human): decisions, financial assumptions, regulatory position, pricing.
- Writing agent: research, drafting, model construction, consistency with the
  engineering documents.
- Reviewer: one reader who has bought or sold defense software, if available.

## Dependencies

Plans 02 and 05 for content; plan 06 for the product story's screens; counsel for
section 10. Can start in parallel with 02 on sections 3, 4, and 8.

## Effort and sequencing

8 to 12 agent-assisted days after plans 02 and 05 exist; 3 weeks elapsed.

## Acceptance criteria

- Every market number has a source and a stated method; every product claim points
  at the document or code that backs it.
- The financial model reproduces the plan's numbers from its stated assumptions.
- The regulatory section names the jurisdiction and the classification path, even if
  the answer is "to be determined by counsel by a date".
- The roadmap matches the engineering increments and the gap register; no product
  promise lacks an engineering item.
- The owner signs off on pricing, financials, and the asks.

## Risks

- Market sizing for defense software is noisy; mitigate with two methods and stated
  ranges.
- Over-claiming maturity; mitigate by quoting the status section of `README.md`.
- Export-control findings could change the cloud profile; mitigate by getting the
  question to counsel first.

## Open questions

- Jurisdiction and legal entity selling the product.
- Funding stage and the size of the ask.
- Any customer or partner conversations already under way.
- Whether "Gungnir" is the product name.
- Whether the commercial line should be sized now or deferred to a later revision.
