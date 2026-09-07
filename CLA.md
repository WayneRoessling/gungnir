# Contributor License Agreement

Gungnir is published under AGPL-3.0-or-later and is also offered by Roessling Digital
Solutions LLC under a separate commercial license
([`LICENSE-ADDITIONAL-TERMS.md`](./LICENSE-ADDITIONAL-TERMS.md) section 5). That second
half only works if one party holds the rights to relicense the whole codebase. A
contribution that arrives under AGPL alone cannot be included in a commercial release,
and one such contribution merged without agreement would freeze the dual license for
good.

So: **every contribution requires both a sign-off and, for anything beyond a trivial
change, agreement to the terms below.**

## Sign-off (all contributions)

Every commit must carry a `Signed-off-by` line matching the author:

```bash
git commit -s -m "your message"
```

That line certifies the Developer Certificate of Origin 1.1
(<https://developercertificate.org/>): that you wrote the change or have the right to
submit it, and that you are willing for it to be distributed under this project's
license.

## Agreement (non-trivial contributions)

By submitting a contribution you agree to the following. "You" is the copyright owner —
you personally, or your employer where the work is theirs. "Contribution" is any work of
authorship you submit for inclusion, in any form.

**1. Copyright license.** You grant Roessling Digital Solutions LLC a perpetual,
worldwide, non-exclusive, royalty-free, irrevocable copyright license to reproduce,
prepare derivative works of, publicly display, publicly perform, sublicense, and
distribute your Contribution and such derivative works, **under any license terms,
including terms that differ from AGPL-3.0 and including proprietary terms**.

This is the clause that keeps the commercial license possible. It is a license, not an
assignment: you keep your copyright and may use your own Contribution however you like,
including in other projects under other licenses.

**2. Patent license.** You grant Roessling Digital Solutions LLC and recipients of the
software a perpetual, worldwide, non-exclusive, royalty-free, irrevocable patent license
to make, have made, use, offer to sell, sell, import, and otherwise transfer your
Contribution, covering only those patent claims you own or control that are necessarily
infringed by your Contribution alone or by its combination with this project. If you
institute patent litigation alleging that this project or a Contribution in it
constitutes patent infringement, the patent licenses you were granted under this
agreement terminate as of the date the litigation is filed.

**3. You have the right to grant this.** You represent that each Contribution is your
original creation and that you are legally entitled to grant the licenses above. If your
employer has rights to work you create, you represent that you have permission to make
the Contribution on their behalf, that they have waived those rights, or that they have
executed this agreement with Roessling Digital Solutions LLC.

**4. Third-party material.** If a Contribution includes work that is not yours, you must
identify it, its source, and its license, and it must be compatible with both
AGPL-3.0-or-later and commercial redistribution. `deny.toml` holds the allow-list for
dependencies; `testdata/*/SOURCE.md` is the pattern for fixtures. A GPL-licensed
dependency cannot be accepted, because it cannot be commercially relicensed.

**5. No warranty and no obligation.** You provide your Contribution "as is", without
warranty of any kind. Roessling Digital Solutions LLC is under no obligation to accept,
merge, or use any Contribution.

**6. Notification.** You agree to tell Roessling Digital Solutions LLC if you become
aware that any representation above has become inaccurate.

## How to accept

For a first non-trivial pull request, include this line in the pull request description,
with your own details filled in:

```
I have read CLA.md and I agree to its terms.
Name: <your full legal name>
Email: <email>
GitHub: @<username>
Employer (if the work is theirs): <name, or "none">
```

Accepted agreements are recorded against the contributor, so this is a one-time step.

## Status of this document

This has not been reviewed by counsel.
`docs/plans/01-product-business-plan.md` section 10 records the legal questions owed a
lawyer's answer; this agreement belongs in that review, and this line should be removed
once it has had one.
