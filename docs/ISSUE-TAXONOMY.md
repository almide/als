# Issue taxonomy — the closed severity set

Every **problem report** in this repository carries exactly one `S-*` label.
The set is closed; a new class is a PR to this file. Two other kinds of issue
exist and carry no `S-*` label, because they are not problem reports:
**tracking issues** (`epic` — one per gap of the aviation-grade claim ladder,
#9) and **decision issues** (an ADR adjudication, e.g. #28/#29). Two further
labels are orthogonal markers that may sit on any issue: `needs-external`
(cannot be closed without an external party — an independent verifier, a
DER, a customer, an authority) and `needs-resources` (needs money, a licence
or a legal act). They exist so the queue states honestly what the project
cannot close alone (the lesson of the implementation's
`flight-organization.md`, which had moved such items out of the tracker
because unlabeled they made it lie).

| Label | Meaning | Edition-blocking |
|---|---|---|
| `S-unsound` | The spec certifies behaviour that is wrong, self-contradictory, or unsafe; or a contract's evidence does not establish its statement | **YES** |
| `S-ambiguous` | Two readings of the normative text survive the fixtures — implementations could conformantly disagree | **YES** |
| `S-untestable` | A normative claim whose evidence cannot be executed (or is missing) | YES until reclassified or evidenced |
| `S-divergence` | Implementations disagree observably while both pass the corpus — a corpus hole; lands with the missing fixture | **YES** |
| `S-editorial` | Wording, links, formatting — no normative change | no |
| `C-finding` | A conformance-runner finding against a released binary (the judge describing a release; routed to the implementation when it is a defect there) | no |

**The blocking rule**: an edition (tag) may not be cut while an
edition-blocking issue is open — the same shape as the implementation's
release-blocker gate (#1482). The edition-readiness instrument re-checks
this mechanically. Open `epic`, decision, `needs-external` and
`needs-resources` issues never block an edition: they describe the distance
to a claim, not a defect in the text.
