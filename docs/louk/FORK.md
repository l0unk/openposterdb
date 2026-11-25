# What this fork adds

Fork of [PNRxA/openposterdb](https://github.com/PNRxA/openposterdb) at `v1.3.0`, built for loukdekker.nl.
Fork code lives in new modules (`api/src/louk/`, `web/src/louk/`, `scripts/louk/`, `docs/louk/`) and
hooks into upstream files are marked `// louk-fork:`. Baseline numbers: [baseline.md](baseline.md).

## Divergence points in upstream files

| File | Hook | Why |
| --- | --- | --- |
| `.github/workflows/test.yml` | `on:` runs on pushes to every branch, no `paths:` filter | Every commit on every branch gets a CI run |
| `.github/workflows/release.yml` | single amd64 job, ghcr only, no Docker Hub | The fork ships `ghcr.io/l0unk/openposterdb` only |

## Settings added

_None yet._

## Syncing upstream

Placeholder: `scripts/louk/sync-upstream.sh` (added in P7b).

## Backups

Overrides and uploads live in `./db`, which Backrest backs up.

## Going back to upstream

Placeholder: filled in as fork features land (all schema changes are additive, so the upstream
binary can open a fork-migrated database).

## Backdrop safe areas

Evidence and numbers: [safe-areas.md](safe-areas.md), measured from the AIOStreams web app's own CSS (the Android TV, desktop and web apps are one app). Both its Home banner and title page show OpenPosterDB's backdrop.
- Wide screens (TV, desktop, web) crop the bottom of the backdrop and darken its left, so ratings are safest top-right, which is the default (`position=tr`, insets 0). The default is a column of up to 5 badges: on the Home banner its bottom two dim, so use `ratings_limit=3` or `badge_direction=h` (admin UI: "Max ratings" / "Badge direction" in the key's "Backdrop (series/movie)" box).
- On a title page "Background from artwork" is on by default and leaves the top right clear; no change needed.
- A phone held upright shows only the middle third, so top-right badges vanish; only `position=tc` survives, at the cost of dimmer badges on the TV banner. No single setting serves both: use one key per device class.
- No code preset was built: the existing `position`, `edge_inset_x/y`, `badge_direction` and `ratings_limit` settings already express this.
- Open, needs a real-device check: TV overscan, trailers that replace the backdrop on TVs, the Android status bar over the phone banner, and the desktop app's window controls.
