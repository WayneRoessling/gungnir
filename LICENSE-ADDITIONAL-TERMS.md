# Additional terms under GNU AGPL v3 section 7

Gungnir is licensed under **AGPL-3.0-or-later**; the license text is
[`LICENSE`](./LICENSE). Section 7 of that license lets the copyright holder
supplement it with terms drawn from a closed list. Roessling Digital Solutions LLC,
as sole copyright holder of the Gungnir source (see [`CLA.md`](./CLA.md)), exercises
clauses **(b)**, **(c)** and **(e)**. They are reproduced here because section 7
requires additional terms to be stated in the license notice of the material they
govern; every source file's header points at this document.

These are terms **added by the copyright holder**, not "further restrictions"
imposed by a downstream redistributor. Section 7 lets a recipient strip the latter;
it does not let a recipient strip these, and they travel with the work.

Nothing below narrows any freedom AGPL-3.0 grants. You may still run, study, modify,
redistribute, and self-host Gungnir at no charge, including commercially. These terms
govern **credit and identity only**.

## 1. Attribution — section 7(b)

You must preserve, in unmodified form, the attribution in [`NOTICE`](./NOTICE) and the
copyright line in the header of each source file you convey or modify.

Where a work based on Gungnir has an interactive user interface, that attribution must
also appear in the interface's Appropriate Legal Notices, as section 0 defines them: a
convenient and prominently visible feature displaying the copyright notice, the absence
of warranty, the fact that the work may be conveyed under this License, and how to read
the License. An "About" dialog, a credits panel, or a startup banner all satisfy this.
Placement is yours to choose; removal is not.

This term binds a derivative only to the extent that section 5(d) makes it bind — see
section 4 below.

## 2. Origin and marking — section 7(c)

You may not misrepresent the origin of this material. If you convey a modified version,
you must mark it in a reasonable way as different from the original: a changed product
name, a version suffix, a stated fork identity, or an equivalent that a user encountering
your build would notice.

This exists so that a fork's behaviour is not attributed to Roessling Digital Solutions
LLC. Gungnir is an architectural scaffold in which several subsystems deliberately report
`NotImplemented` rather than pretending to work; a fork that fills those in — or fails to
— must be identifiable as the author of its own results.

## 3. Trademarks — section 7(e)

No rights are granted under trademark law to "Gungnir", "Roessling Digital Solutions",
"Roessling Digital Solutions LLC", or to any logo or service mark of the copyright
holder, except as required by section 1 above for attribution and by fair use.

Preserving the attribution notice is required. Using these names to brand, market, or
endorse a derivative work is not permitted by this License and needs separate written
permission.

## 4. How section 1 reaches a derivative's user interface

AGPL section 5(d) makes the interface requirement conditional in a way worth stating
plainly:

> If the work has interactive user interfaces, each must display Appropriate Legal
> Notices; however, if the Program has interactive interfaces that do not display
> Appropriate Legal Notices, your work need not make them do so.

So a derivative inherits the obligation **only if Gungnir's own interface carries the
notices first**.

It does. `gungnir-app` displays them in **PN-21, the About panel**
(`gungnir-ui/src/panels/about.rs`), opened from the status strip — the one surface
present in every workspace, for every role, and for a session with nobody signed in.
The panel draws all four elements section 0 lists, and this document's section 1 is what
requires a derivative work to keep drawing them.

**Removing or emptying that panel would silently release every future fork of Gungnir
from the requirement**, and nothing else in the repository would object. So it is a
licensing surface, not decoration, and it is defended as one: the render test in
`gungnir-ui/src/panels/rendered.rs` asserts each of the four elements actually reaches
the screen rather than merely being held in a constant, and
`gungnir-app/tests/appropriate_legal_notices.rs` asserts the panel's text still matches
`NOTICE` and that every role can open it.

## 5. Commercial licensing

AGPL-3.0-or-later is not the only way to use Gungnir. Section 13 requires anyone who
modifies Gungnir and offers it to users over a network to offer those users the
corresponding source of their modified version, and section 5 requires derivative works
to be conveyed under this same License. Where that is incompatible with a program's
requirements, Roessling Digital Solutions LLC offers Gungnir under a separate commercial
license on negotiated terms.

Contact: wayne.roessling@roesslingdigital.com

## 6. Status of this document

This states the copyright holder's intent in the form AGPL section 7 provides for. It has
not been reviewed by counsel. `docs/plans/01-product-business-plan.md` section 10 records
the legal and export-control questions that are owed a lawyer's answer; the license terms
here should be reviewed in the same pass and this line removed when they have been.
