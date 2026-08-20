# Issue taxonomy — the closed severity set

Every issue in this repository carries exactly one `S-*` label. The set is
closed; a new class is a PR to this file.

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
this mechanically.
