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
