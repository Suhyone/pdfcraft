# Status

## Phase 1 (Rebrand/Clean) - done, commit 5f506f83f
- Removed docs/brand/, brand assets in ATTRIBUTION.toml, logos, community card, Discord button, ArtCraft help links.
- Checked: fmt, clippy -D warnings (engine, ui-egui, cli, automation), xtask assets, links + UI tests (53 pass).

## Phase 2 (Build WASM) - done, no source change
- cargo xtask wasm: 32 crates ok. trunk build --release: ok (dist/web/, gitignored).
- Needed binaryen 133 (wasm-opt) downloaded from GitHub releases, sha256 verified, used via PATH only for that run.
- Output: pdfcraft-web wasm 50.9 MB (unzipped), ocr worker wasm 3.4 MB. index.html paths are already ./ . No "artcraft" in dist/web.

## Phase 3 - waiting for OK
- apps/pdfcraft-web/bridge.js created (untracked). index.html not yet edited (needs rebuild), dist-pdf-tool/ not created.

## Phase 3 (bridge.js + pack) - done
- index.html: canvas data-host="page", bridge.js copied by trunk and loaded; rebuilt ok.
- dist-pdf-tool/ (53 MB, not committed): build output, bridge.js, .htaccess, get_pdf.php, save_pdf.php, LICENSE-MIT, LICENSE-APACHE, NOTICE.
- "ArtCraft" remains only in LICENSE-MIT / NOTICE (copyright line + trademark paragraph for removed logos). Kept as-is on purpose.
- Not verified: running in a browser / against real PHP.

## Hardening (bridge.js, .htaccess, save_pdf.php) - done
- bridge.js: same-origin URL lock, %PDF- check, Thai alerts. .htaccess: immutable cache for hashed .wasm/.js, no-cache for index.html and bridge.js (no COOP header, on purpose). save_pdf.php: UTF-8 file names, forced .pdf.
- Tested on PHP built-in server: load, save, Thai/hostile names, cross-origin URLs ignored. Not tested: real Apache (.htaccess), Linux file names.

## Follow-up checks - done
- Apache (XAMPP, isolated config on :8088, since stopped): .htaccess headers correct; wasm gzip 50.9 MB -> 19.3 MB; PHP load/save ok.
- Thai alerts verified for load (HTTP 404) and save failures; save alert now shows the status, not the whole error page.
- dist-pdf-tool/NOTICE: removed the logo/trademark paragraph; the copyright line still names "ArtCraft Team" (same in LICENSE-MIT).
- Committed: small hand-written package files only; build output is ignored via dist-pdf-tool/.gitignore.
- Not verified: Thai file names on a Linux/UTF-8 host (Windows PHP writes them in the ANSI codepage).

## Phase 2 spec (onboarding, toast, postMessage, company demo) - done
- bridge.js (src + dist copy): first-time guide for 5 tools (menu names match the app), "? คู่มือใช้งาน" button, toasts, postMessage ready/dirty/saved to same origin. Short HTTP error text kept.
- company_demo.php: modal iframe + new tab; buttons use data-* attributes (no JS string injection).
- save_pdf.php: ?id=<id> stores <id>.pdf so get_pdf.php finds the saved file.
- INTEGRATION_GUIDE.md: from the plan; removed the non-existent `webgl` param, added `control`.
- Checked: node --check, php -l, Apache (XAMPP) run of demo: modal, guide, toast, ready/saved messages, file written back. No .wasm rebuild.
- Not verified: other browsers, mobile layout, ?control=1 API, real auth.
