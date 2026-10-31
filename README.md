# Masar (مسار)

A local-first Windows application for Swiss job search, manual review, application tracking and follow-up reminders. Built with Tauri v2, Rust, SQLite and plain HTML/CSS/JavaScript. No AI service, subscription or account is required; applications and emails are never sent automatically.

Source: https://github.com/Nobodyssssy/Masar-

**Development build 0.1.0. Version 1.0.0 acceptance is incomplete.** See DEV_STATE.md and docs/VERIFICATION-REPORT.md for limits and checks actually run.

## Development

Windows 11 with WebView2, Rust/MSVC build tools and Node.js/npm is the tested development environment. Tested tool versions: Node 24.22.1, Rust/Cargo 1.99.0. Preserve the lockfiles.

```powershell
npm ci
npx playwright install firefox
npm run dev
```

The preparation script generates `dist/` and `src-tauri/runtime/`. `node_modules/`, browser downloads and Rust build output are restored/generated locally; none belong in Git.

```powershell
npm run test:frontend
cargo test --manifest-path src-tauri/Cargo.toml --locked
npm run build
```

The NSIS installer is generated under `src-tauri/target/release/bundle/nsis/`. The installed app bundles Node/Playwright and does not need global Node or development folders. SwissDevJobs/eFinancialCareers use optional Firefox, set up from Settings → Data. A fresh tested browser cache used about 350 MiB. Existing stored jobs/tracking work offline; collection and browser setup require internet access.

## Workflow

Search → collect/validate → review → manually apply → track/follow up. Personal eligibility rules are optional. Advertisement language and explicitly required working languages are separate. Search profiles, Swiss geography, workload/date controls and conservative metadata filters are available. Unknown facts remain unknown.

Indeed, Emploi en Suisse, 100000jobs and Job-Room remain official-link/manual-import only. OK Job has regional coverage. Browser-source enrichment/date coverage and the full source/filter acceptance matrix remain incomplete. Sources may change or block public access; Masar does not bypass access controls.

## Storage and recovery

For compatibility, the stable identifier and database filename remain `ch.local.jobup.desktop` and `jobup.sqlite3`. On Windows the database is `%APPDATA%\ch.local.jobup.desktop\jobup.sqlite3`; WebView preferences can also exist under `%LOCALAPPDATA%\ch.local.jobup.desktop`.

Never commit databases, backups, personal documents, exports, browser profiles or credentials. See docs/DEVELOPMENT-AND-RECOVERY.md and docs/MASAR-USER-GUIDE.md. Private local history and application-data diagnostics are intentionally absent from this public publication branch; existing private Git history was preserved without rewriting its commits.

## Repository layout

- `frontend/`: application UI and Firefox helper source
- `src-tauri/src/`: backend, collectors and SQLite migrations
- `src-tauri/resources/`: required deterministic legacy-rule resources and test cases
- `src-tauri/tests/fixtures/` and `tests/`: non-personal regression fixtures and frontend checks
- `src-tauri/icons/`, `masar-icon.svg`: required assets
- `prepare-runtime.ps1`, manifests/lockfiles: reproducible dependency/runtime preparation
- `licenses/`, `THIRD_PARTY_NOTICES.md`: third-party notices and provenance

Original-source rights are reserved; a general open-source reuse licence has not been selected. Third-party materials keep their licences. See LICENSE.md and THIRD_PARTY_NOTICES.md.
