# Verification scope

## Checks recorded

- Native regression: 189 passed, 0 failed; 4 explicit opt-in tests are skipped in the ordinary run.
- Firefox frontend/controller checks with a simulated native bridge passed. Browser helper self-tests and saved parser fixtures passed.
- Explicit live LinkedIn warehouse/Switzerland verification retained 20 relevant Swiss vacancies from 30 inspected after description enrichment. Matching uses explicit warehouse/Lager/entrepôt/magasinier text. This proves the tested query, not every LinkedIn query.
- Publication-window tests reject old publications despite fresh collection dates. Relative/custom dates are mutually exclusive; Search publication scope is carried into Review; stale personal Review scope is cleared.
- Previous copied-database migration/recovery and installed Windows smoke checks passed. A fresh optional Firefox cache was installed and launched using bundled Node/Playwright on the development PC.

## Source limits

Jobup, Jobs.ch, JobScout24, Michael Page, Züri Jobs and the regional OK Job adapter have returned parsed vacancies in bounded probes. SwissDevJobs/eFinancialCareers individual-vacancy validation was tightened after navigation links were incorrectly accepted; enrichment and publication-date coverage remain partial. Indeed, Emploi en Suisse, 100000jobs and Job-Room stay manual-only.

## Not fully verified / not complete

Full source/filter/language acceptance matrix; complete browser-source enrichment; independent clean Windows installation/WebView2 bootstrap; long-run cancellation/process-leak measurements and timeout fault injection; full installed mutation/undo/bulk/backup/restore matrix; exhaustive occupation/language/salary extraction; remaining manual interest and viewed-state requirements.

No invented vacancies or statistics were added to improve counts. Unknown metadata remains unknown. No stable release or v1.0.0 tag is justified by these checks. Binary/source publication is separate from release acceptance.
