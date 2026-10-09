# M2.3 printable impact report (1008.md B2)

The analysis export writes `impacts.html` beside `impacts.csv` and `impacts.md`: the readable
report as a standalone page (no scripts, no external resources) that prints with one motion per
page after the first. It is rendered from `impacts.md` itself
(`app/impact_report.rs::impacts_html`), so the two cannot disagree. Print it, or "Save as PDF"
from a browser.

[impacts-sample.html](impacts-sample.html) is the page for the unit test's fixture: one manual
motion and one SCIT storm reaching a saved place named `Smith & Sons <farm>` (to show escaping),
and no warnings. Reviewed in the desktop browser pane on 2026-10-09.

| File | SHA-256 |
| --- | --- |
| impacts-sample.html | `ffac21da2aa3bd251a77b93c0453b95d5a53cb6d417f32546dedcad886472352` |

Not established: printed on paper or saved as PDF on Android; place areas for true entry times.
