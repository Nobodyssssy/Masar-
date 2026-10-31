# Masar development state

## Current build

- Existing Tauri v2/Rust/SQLite/plain-JavaScript application; version 0.1.0. No v1.0.0 tag or production-ready claim.
- Search publication-window/Review propagation and LinkedIn relevance correction are implemented. Personal eligibility remains optional. Full-history/global historical-classifier counts are explicitly distinguished from filtered results and recommendations.
- Last native regression: 189 passed, 0 failed, 4 opt-in tests skipped by default. Explicit public LinkedIn verification retained 20 Swiss warehouse-related vacancies from 30 inspected after enrichment. Firefox controller/helper tests passed.
- Prior copied-database migration/recovery and installed smoke checks passed. Source publication/backup cleanup does not add features or alter application identifiers/schema/runtime behaviour.

## Open acceptance items

- Full live FR/EN/DE query/filter matrix across all sources.
- Browser-source description/publication-date completeness; nationwide OK Job coverage beyond its regional adapter.
- Independent clean-Windows-machine installation and long-running timeout/cancellation/process-leak fault testing.
- Complete installed UI mutation/backup/restore matrix; full occupation/language/salary grammar and remaining manual interest/view-state requirements.

## Source publication and storage

- Public publication uses a sanitized new branch with no private application-data diagnostics or local user paths.
- Original local branches/commits were preserved, not rewritten. A private Git bundle and required data/settings snapshots are part of the separately verified recovery backup.
- No personal database, CV, browser profile, credentials, installer or generated dependencies are committed.
- See THIRD_PARTY_NOTICES.md for independent collector provenance and applicable third-party notices. Original source has no general open-source reuse licence selected.

- Publication audit removed two obsolete source backup copies; active application source is unchanged. Recovery verification must complete before generated workspace artifacts are deleted.
