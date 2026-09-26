# A baseline applied section by section

GAP-162 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
D-91, DN-08 §10, roles-and-stakeholders §4 "Applying a baseline, section by section".

## What was wrong

GAP-111's matrix test found it. `config.apply` was one action for a whole baseline, and
the sensor manager held it because §4 gives that role calibration and sensor baselines.
A baseline is one file, so a sensor manager applying a calibration change could set
weapons control status to free and rewrite the authority rules in the same file; after
D-88 the administrator, too, could still reach the engagement chain it had just been
withheld at the console, by applying a baseline. Separately, `ConfigEditorState::apply`
checked no permission: PN-14 hid the control from a role without `config.apply`, and
that was the whole of the rule.

## What was decided

The owner took four questions, each as recommended (D-91):

1. **A second action.** `config.apply_sensing` covers a baseline that changes only its
   sensing sections, and is the sensor manager's; `config.apply` stays the whole-baseline
   action of the supervisor, the commander and the administrator, who hold both. §4's two
   apply rows now map one to one onto two actions, and the sensor manager no longer holds
   a coarse grant wider than its cell.
2. **The engagement chain needs weapons control status.** Every policy section, the
   resources, assets, geofences, hazards and approaches, the allocation horizon and solve
   budget, and the assessment settings: the supervisor and the commander only.
3. **The security section needs account administration.** Accounts and authentication,
   keys, TLS, escrow, machine identities and retention: the administrator only.
4. **Refused whole.** A candidate that changes one section its applier may not change is
   refused and nothing is written. Applying the permitted part was rejected: it writes a
   version of the baseline nobody validated as a whole.

## How it is built

`gungnir_config::changed_sections` compares a candidate with the baseline in force and
names each changed section with its kind. It destructures every field of
`ConfigBaseline` and of `PolicySettings` with no `..`, so a field added later without a
kind is a compile error rather than a section nobody's authority covers. `gungnir-config`
may not reach `gungnir-security`, so which action a kind needs is mapped in
`gungnir-app`'s `sustainment::actions_for_section`, the same inversion DN-08 amendment 1
made for the authority vocabulary.

`ConfigEditorState::apply` asks, before anything is read or written, whether the role may
apply a baseline at all and then whether every changed section is the role's. The
baseline in force is the one most recently applied in this process, since that is what
the candidate replaces on disk, or the running one when nothing has been applied, which is
GAP-128's distinction for the revision. PN-14 draws the refusal before apply is pressed
(`ApplyState::Refused`), naming each section and the action it needs, and a successful
apply's audit entry now says which sections it changed.

## What the tests found on the way

Three existing tests had applied a baseline with nobody signed in, as the desktop's
default role, Operator, which may not apply one. They passed only because nothing
checked. Each now selects the role its own comments name. `config_apply_authority.rs` covers:

- the sensor manager's control-status change refused, with its sensing change, and
  nothing applied or audited;
- PN-14's `Refused` state;
- the sensor manager's sensing-only apply, with the audit entry naming the section;
- the administrator refused on the engagement chain;
- the supervisor applying control status and refused on security, and the administrator
  applying security;
- the operator refused inside `apply` by name.
