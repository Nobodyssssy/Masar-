# Third-party materials and provenance

## Original source status

The original application currently has no general open-source reuse licence. LICENSE.md records that status without changing third-party licences. This source publication is not a stable binary release.

## Swiss Job Hunter reference (AGPL-3.0)

Reference: https://github.com/Donvink/swiss-job-hunter, reviewed revision `27a3722ada2618b9e4042604f7b675ddaf2319f1`, by Donvink and contributors. Its licence text is preserved as `licenses/AGPL-3.0-REFERENCE.txt` for reference attribution.

The existing provenance notes and Rust module headers identify Masar's collectors as independently written implementations of public endpoint behaviour and parsing techniques. The reviewed Rust code and JavaScript helpers do not vendor the upstream Python modules. The review covered JobCloud JSON-LD/card parsing, JobScout24 query parameters/pagination, LinkedIn guest cards and browser link validation. Shared provider URLs, schema vocabulary and selectors describe the provider interfaces.

This attribution is not a claim that the upstream AGPL licence is permissive or irrelevant to copied/translated code. Incorporating or translating upstream implementation code would require a new licence review and compliance with its applicable AGPL obligations before publication. No AI agents or hosted workflows from reference projects are included.

## Runtime dependencies

- Node.js: the build copies the locally installed Node executable. Tested with Node 24.22.1; `licenses/NODE-LICENSE.txt` contains that distribution's notices. Use that tested version when reproducing the archived build. If changing Node, refresh its distribution licence notice.
- Playwright and playwright-core: Apache-2.0, resolved by package-lock.json. Their licence, NOTICE and third-party notice files are preserved under licenses/ and also remain inside their installed runtime packages.
- Tauri: MIT OR Apache-2.0, resolved by Cargo.lock. https://github.com/tauri-apps/tauri
- Other Rust/npm libraries: restored from their upstream package registries using the preserved lockfiles, rather than vendored into this repository. Their original licences continue to apply. The installer is not published as a release in this task.
- Optional Playwright Firefox: downloaded by Playwright from its browser distribution service. Firefox is not committed to this repository. The browser distribution retains Mozilla/third-party notices; Firefox use and redistribution are separate from the Masar source licence.

## Geographic data

Swiss postal-place data is derived from GeoNames' CH postal export (2026-10-31), under CC-BY-4.0. Attribution, source URL, transformation description and the upstream readme are preserved in licenses/GEONAMES.txt. Original advertised workplace text is retained; geographic matching is approximate.

## Assets

The Masar M icon is generated from masar-icon.svg. No reference-project UI assets or user screenshots/data are included.
