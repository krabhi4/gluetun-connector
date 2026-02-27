# Code Review Findings

> Last reviewed: 2026-02-28 (Rust migration)
> Scope: security, correctness, reliability, code quality
> Status key: 🔴 High · 🟡 Medium · 🔵 Low · ✅ Fixed

---

## Open Findings

### Security

| # | Severity | File | Finding |
|---|---|---|---|
| S-05 | 🔵 Low | `src/middleware.rs` | **No `Strict-Transport-Security` (HSTS) header.** Intentionally omitted for plain-HTTP local use. Must be added if the app is ever placed behind an HTTPS reverse proxy. |
| S-06 | 🔵 Low | `src/routes/mod.rs` | **Rate limiter uses in-memory store.** Counters reset on every container restart. Acceptable for single-instance home use; note for any production or shared deployment. |

### Code Quality / Correctness

| # | Severity | File | Finding |
|---|---|---|---|
| C-02 | 🔵 Low | `public/app.js` | **Total server failure does not reset card fields.** When `fetchHealth()` throws (server unreachable), the catch block only calls `renderBanner`. The four data cards retain stale values from the last successful poll. Reset all card fields in the catch path. |
| C-04 | 🔵 Low | All | **No tests.** No unit or integration test suite exists. |
| C-05 | 🔵 Low | `public/app.js` | **Spinner set via direct DOM property.** `refreshBtn.innerHTML = '<span ...> Refresh'` is safe (hardcoded string) but inconsistent with the `textContent`-only approach used elsewhere. Use `createElement` for consistency. |
| N-03 | 🔵 Low | `public/index.html` | **`<button>` elements missing `type="button"`.** `#refresh-btn`, `#btn-start`, and `#btn-stop` default to `type="submit"` per the HTML spec, which is semantically incorrect. |

### Infrastructure / Docker

| # | Severity | File | Finding |
|---|---|---|---|
| D-01 | 🔵 Low | `docker-compose.example.yml` | **No resource limits.** No `mem_limit`, `cpus`, or `pids_limit` defined. Add `deploy.resources.limits` to prevent resource exhaustion. |

---

## Recommended Next Steps (priority order)

1. **C-02** — Reset all card fields in `poll()` catch block
2. **N-03** — Add `type="button"` to all three `<button>` elements in `index.html`
3. **C-05** — Replace spinner assignment with `createElement`
4. **D-01** — Add container resource limits to `docker-compose.yml`
5. **C-04** — Add tests
6. **S-05 / S-06** — Acceptable as-is for local use; revisit if exposed publicly

---

## Fixed Findings

<details>
<summary>Click to expand — all resolved issues</summary>

### Fixed by Rust Migration (2026-02-28)

| # | Severity | Finding |
|---|---|---|
| S-03 | 🔵 Low | **`GLUETUN_CONTROL_URL` not validated at startup.** Fixed: `url::Url::parse()` in `config.rs` exits the process on a malformed URL before the server binds. |
| S-08 | 🔵 Low | **No graceful shutdown handler.** Fixed: `axum::serve().with_graceful_shutdown()` handles `SIGTERM`/`SIGINT`; `CancellationToken` stops the monitor task cleanly. |
| C-01 | 🔵 Low | **Dead `running` variable.** No longer exists — Rust compiler enforces usage, eliminating this class of bug. |
| C-03 | 🔵 Low | **Express 4 used; Express 5 is stable.** Not applicable — Express replaced by Axum. |
| C-06 | 🔵 Low | **Global body parser on every request.** Not applicable — Axum uses per-handler extractors; only `vpn.rs` uses a JSON body extractor. |

### Fixed by Rust Migration Code Review (2026-02-28)

| # | Severity | Finding |
|---|---|---|
| R-01 | 🔴 Critical | **`docker-compose.example.yml` healthcheck used `wget`**, which does not exist in the `FROM scratch` image. Fixed: changed to `["CMD", "/usr/local/bin/gluetun-connector", "--health-check"]`. |
| R-02 | 🟡 Medium | **`wait_for_healthy()` locked up on containers with no `HEALTHCHECK`.** Fixed: falls back to checking `running` state when health status is `None`. Also replaced manual deadline tracking with `tokio::time::timeout()`. |
| R-03 | 🟡 Medium | **Silent auth failure.** `build_auth_headers()` silently discarded invalid header values. Fixed: logs `tracing::warn!` with the reason. |
| R-04 | 🟡 Medium | **`MissedTickBehavior::Burst` (default) in monitor loop.** A slow check cycle caused a burst of back-to-back checks on the next tick. Fixed: `interval.set_missed_tick_behavior(MissedTickBehavior::Skip)`. |
| R-05 | 🟡 Medium | **Non-`biased` `tokio::select!` in monitor loop.** Cancellation signal could be starved if the interval was always ready. Fixed: `biased;` with cancellation arm first. |
| R-06 | 🟡 Medium | **`discover_dependents()` called `list_containers` twice.** Fixed: Gluetun ID extracted from the already-fetched container list. |
| R-07 | 🔵 Low | **Duplicate `std::net` import in `routes/mod.rs`.** Fixed: merged into single `use std::net::{IpAddr, SocketAddr}`. |
| R-08 | 🔵 Low | **Dead fields `duration` and `http_code` in `SiteResult`.** Fixed: removed from struct and constructors. |
| R-09 | 🔵 Low | **Stale "npm modules" comment in `docker-compose.example.yml`.** Removed. |

### Fixed in Node.js version (pre-migration, 2026-02-24 — 2026-02-25)

| # | Severity | Finding |
|---|---|---|
| F-01 | 🔴 High | `favicon.svg` missing — every page load 404'd |
| F-02 | 🔴 High | No rate limiting on read endpoints |
| F-03 | 🔴 High | `npm install` instead of `npm ci` — non-deterministic builds |
| F-04 | 🔴 High | `--no-audit` suppressed npm vulnerability scanning |
| F-05 | 🔴 High | Port bound to `0.0.0.0` — exposed to entire local network |
| F-23 | 🔴 High | CVE-2026-26996 (minimatch 10.1.2) — CVSS 8.7 |
| F-24 | 🔴 High | CVE-2026-26960 (tar 7.5.7) — CVSS 7.1 |
| F-27 | 🔴 High | `uiLimiter` referenced before declaration — server crashed on startup |
| F-25 | 🟡 Medium | Alpine 20 EOL — upgraded to Alpine 25 |
| F-26 | 🟡 Medium | No rate limiting on static file routes |
| F-06 | 🟡 Medium | `NODE_ENV=production` not set in Dockerfile |
| F-07 | 🟡 Medium | `node-fetch` dependency unnecessary |
| F-08 | 🟡 Medium | Healthcheck missing `start_period` |
| F-09 | 🟡 Medium | `X-Powered-By: Express` header leaked server fingerprint |
| F-10 | 🟡 Medium | `redirect: 'error'` missing — SSRF redirect amplification risk |
| F-11 | 🟡 Medium | No `Permissions-Policy` header |
| F-12 | 🟡 Medium | Docker base image not pinned to digest |
| F-13 | 🟡 Medium | `sessionStorage` history not validated — CSS class injection risk |
| D-02 | 🟡 Medium | docker-compose.example.yml network key mismatch |
| D-03 | 🟡 Medium | `npm install` instead of `npm ci` (regression) |
| D-04 | 🟡 Medium | Docker image not pinned to digest (regression) |
| S-01 | 🟡 Medium | `express.json()` had no body size limit |
| S-07 | 🟡 Medium | Upstream error details leaked to browser |
| S-02 | 🟡 Medium | No UI-layer authentication documented |
| N-01 | 🟡 Medium | `uiLimiter` applied globally, rate-limiting API routes |
| F-14 | 🔵 Low | Duplicate `Content-Security-Policy` (meta tag + HTTP header) |
| F-15 | 🔵 Low | Unknown `/api/*` GET paths returned `index.html` instead of JSON 404 |
| F-16 | 🔵 Low | `readLimiter` applied to all HTTP methods |
| F-17 | 🔵 Low | `express.json()` body parser without size limit |
| F-18 | 🔵 Low | `badge.warn` state displayed text "Unknown" |
| F-19 | 🔵 Low | Stale IP fields displayed with error badge |
| F-20 | 🔵 Low | Toast element missing `role="status"` / `aria-live="polite"` |
| F-21 | 🔵 Low | `no-new-privileges`, `cap_drop`, `read_only` not set in compose |
| F-22 | 🔵 Low | Redundant `PORT=3000` env var in docker-compose |

</details>
