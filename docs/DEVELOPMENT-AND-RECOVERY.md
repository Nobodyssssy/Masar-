# Development and recovery

## Recreate the development workspace

Clone the repository or extract the verified Masar-Source.zip recovery archive. Install Windows MSVC build tools, Rust via rustup, Node.js/npm and WebView2. Tested tool versions are in README.md and .node-version.

```powershell
npm ci
npx playwright install firefox
npm run prepare:runtime
cargo test --manifest-path src-tauri/Cargo.toml --locked
npm run test:frontend
npm run dev
```

`npm ci` restores deleted development dependencies from package-lock.json. Cargo restores dependencies from Cargo.lock. Do not delete lockfiles. Preparation generates dist/ and src-tauri/runtime/. The installed app's runtime is separate and must not be deleted as a development cache.

## Recreate the installer

```powershell
npm run build
```

The NSIS output is src-tauri/target/release/bundle/nsis/Masar_0.1.0_x64-setup.exe. Firefox is a separate optional browser download. Refresh Node licence notices if changing the copied Node distribution; preserve all dependency notices.

## Restore private data

Close every Masar/older Jobup instance. Make a verified backup of the current data before replacing it. Prefer the app's native backup/restore controls; otherwise use SQLite's backup API to copy a verified snapshot into `%APPDATA%\ch.local.jobup.desktop\jobup.sqlite3`. Do not copy a live database file without its transactional context.

Choose the intended snapshot carefully: a current empty database and an older pre-reset database are different recovery points. Consult the private backup inventory rather than automatically replacing one with the other. Existing private backups and documents must not be published.

If needed, restore the separately backed-up WebView Local Storage/IndexedDB/Preferences under `%LOCALAPPDATA%\ch.local.jobup.desktop\EBWebView` only while the app is closed. Do not restore browser caches or cookies unnecessarily. External CV/document paths do not cause the documents themselves to be embedded in SQLite or source.

## Failed update

Keep the current data folder, reinstall the known verified setup from the recovery backup, and restore a chosen compatible SQLite snapshot if required. Check SQLite integrity and application history after launch. Restore the source from Masar-Source.zip; private old Git branches can be recovered from the private Git bundle. No history rewriting or force-push is required.

The timestamped recovery folder contains RESTORE-MASAR.md and a SHA-256 manifest with exact paths/commit identifiers. Read its data snapshot notes before restoring. A separate clean Windows machine has not been tested.
