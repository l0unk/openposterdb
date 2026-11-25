#!/usr/bin/env bash
# One cold measurement of one variant, from a fresh instance (fork tooling):
#   stop -> wipe cache and db -> start with the given extra env -> bootstrap -> warm-up -> burst -> stop.
#
#   scripts/louk/run-variant.sh [--binary BIN] [--static-dir DIR] [--no-warmup] <label> [burst.py flags ...] -- [VAR=VALUE ...]
#
#   --binary      the server binary (default: $OPDB_BINARY); an extracted release image has it at
#                 <rootfs>/usr/local/bin/openposterdb, and its web files at <rootfs>/app/dist (--static-dir,
#                 default $OPDB_STATIC_DIR, becomes STATIC_DIR)
#   --no-warmup   skip the warm-up. Only to reproduce the failure it prevents: a fresh process sends every id
#                 lookup of the burst to the TMDB API at once and TMDB rate-limits it (500s, INVALID run)
#   <label>       result name (letters, digits . _ + -); results and the run log go to <results>/<label>.*
#   burst flags   passed to BOTH the warm-up and the burst, e.g. --idle 30 --titles-limit 20
#   VAR=VALUE     extra server environment on top of the nine production settings (SPEC section 2), e.g.
#                 BACKDROP_MAX_WIDTH=1920 MALLOC_ARENA_MAX=2 MALLOC_MMAP_THRESHOLD_=131072; they win over the
#                 defaults. For an allocator test add LD_PRELOAD=<lib>.
#
# Example (the three production settings, label C3):
#   scripts/louk/run-variant.sh --binary "$BINARY" --static-dir "$STATIC_DIR" C3 -- \
#       BACKDROP_MAX_WIDTH=1920 MALLOC_ARENA_MAX=2 MALLOC_MMAP_THRESHOLD_=131072 2>&1 | tee ~/opdb-dev/results/C3.run.log
#
# Needs the heavy slot (a real server plus 490 renders): do not run it without one. The instance always runs
# with RATINGS_DISABLED=true and no OMDb/MDBList/Trakt key (dev-instance.sh and burst.py both enforce it), on
# 127.0.0.1 only, and is stopped on every exit path. State: ~/opdb-dev (OPDB_DEV_HOME, OPDB_DEV_PORT as in
# dev-instance.sh); results: $OPDB_RESULTS, default $OPDB_DEV_HOME/results. Exit code: the burst's (0 valid,
# 2 server died, 3 INVALID), 4 warm-up failed, 9 not started (memory pressure), 1 other failure.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HOME_DIR="${OPDB_DEV_HOME:-$HOME/opdb-dev}"
PORT="${OPDB_DEV_PORT:-3900}"
RES="${OPDB_RESULTS:-$HOME_DIR/results}"
BIN="${OPDB_BINARY:-}"
STATIC="${OPDB_STATIC_DIR:-}"
SETTLE_S="${OPDB_SETTLE_S:-5}"
WARMUP=1

die() { echo "run-variant: $*" >&2; exit 1; }

while [[ "${1:-}" == --* && "${1:-}" != -- ]]; do
  case "$1" in
    --binary) [[ $# -ge 2 ]] || die "--binary needs a path"; BIN="$2"; shift 2 ;;
    --static-dir) [[ $# -ge 2 ]] || die "--static-dir needs a path"; STATIC="$2"; shift 2 ;;
    --no-warmup) WARMUP=0; shift ;;
    -h|--help) sed -n '2,/^set -uo/p' "${BASH_SOURCE[0]}" | sed '$d;s/^# \{0,1\}//' >&2; exit 0 ;;
    *) break ;;   # a burst.py flag: handled below, after the label
  esac
done
[[ $# -ge 1 ]] || die "usage: run-variant.sh [--binary BIN] [--static-dir DIR] [--no-warmup] <label> [burst flags] -- [VAR=VALUE ...]"
LABEL="$1"; shift
[[ "$LABEL" =~ ^[A-Za-z0-9._+-]+$ ]] || die "label may only contain letters, digits and . _ + -"
BURST_EXTRA=()
while [[ $# -gt 0 && "$1" != "--" ]]; do BURST_EXTRA+=("$1"); shift; done
[[ "${1:-}" == "--" ]] && shift
[[ -n "$BIN" ]] || die "no binary: pass --binary or set OPDB_BINARY"
[[ -x "$BIN" ]] || die "binary not found or not executable: $BIN"

# SPEC section 2: the production settings every variant runs on top of.
PROD=(IMAGE_MEM_CACHE_MB=128 RENDER_CONCURRENCY=4 RATINGS_STALE_SECS=86400 RATINGS_MAX_AGE_SECS=31536000
      IMAGE_STALE_SECS=0 DISABLE_PUBLIC_PAGES=true FREE_KEY_ENABLED=false ENABLE_CDN_REDIRECTS=false IMAGE_QUALITY=85)
[[ -n "$STATIC" ]] && PROD+=("STATIC_DIR=$STATIC")

DI="$HERE/dev-instance.sh"
BASE="http://127.0.0.1:$PORT"
KEYF="$HOME_DIR/api-key"
mkdir -p "$RES"
trap '"$DI" stop >/dev/null 2>&1' EXIT

SL=/sys/fs/cgroup/user.slice/user-$(id -u).slice
if [[ -r "$SL/memory.pressure" ]]; then
  avg60="$(grep '^full' "$SL/memory.pressure" | sed 's/.*avg60=\([0-9.]*\).*/\1/')"
  echo "slice: $(grep '^full' "$SL/memory.pressure"); memory.current $(( $(cat "$SL/memory.current") / 1048576 )) MiB"
  if awk "BEGIN{exit !(${avg60:-0} >= 20)}"; then echo "run-variant: memory pressure avg60 >= 20, not starting" >&2; exit 9; fi
fi
echo "$(date -u +%T) start $LABEL burst flags: ${BURST_EXTRA[*]:-none} extra env names: $(printf '%s ' "$@" | sed -E 's/=[^ ]*//g')"

"$DI" stop || exit 1
"$DI" wipe-cache || exit 1
"$DI" wipe-db || exit 1
"$DI" start "$BIN" "${PROD[@]}" "$@" || { echo "run-variant: START FAILED" >&2; exit 1; }
"$DI" bootstrap || { echo "run-variant: BOOTSTRAP FAILED" >&2; exit 1; }
PID="$(cat "$HOME_DIR/pid")"

# What reached the process (names only; values of the allocator/tuning variables are not secrets, nothing else is shown).
echo "server pid $PID; env names: $(tr '\0' '\n' < "/proc/$PID/environ" | cut -d= -f1 | sort | tr '\n' ' ')"
for n in BACKDROP_MAX_WIDTH MALLOC_ARENA_MAX MALLOC_MMAP_THRESHOLD_ MALLOC_CONF LD_PRELOAD RENDER_CONCURRENCY; do
  tr '\0' '\n' < "/proc/$PID/environ" | grep -q "^$n=" && echo "env $n present"
done
echo "mapped allocator libs: $(grep -oE '[^ ]*(jemalloc|mimalloc)[^ ]*' "/proc/$PID/maps" | sort -u | tr '\n' ' ')"

COMMON=(--base "$BASE" --key-file "$KEYF" --pid "$PID" --out "$RES")
if (( WARMUP )); then
  echo "warm-up (low concurrency, other render cache keys than the burst)"
  python3 -I "$HERE/burst.py" "${COMMON[@]}" --warmup --label "$LABEL" "${BURST_EXTRA[@]}"
  rc=$?
  if (( rc != 0 )); then echo "run-variant: warm-up failed (exit $rc); no burst" >&2; exit 4; fi
  echo "after warm-up: $(grep -E 'VmRSS|VmHWM|Threads' "/proc/$PID/status" | tr -s ' \t' ' ' | tr '\n' ' '); cache files $(find "$HOME_DIR/cache" -type f | wc -l)"
  sleep "$SETTLE_S"
else
  echo "warm-up skipped (--no-warmup)"
fi

# The burst: its own default --idle 90, overridable in the burst flags (the last --idle wins).
python3 -I "$HERE/burst.py" "${COMMON[@]}" --label "$LABEL" --idle 90 "${BURST_EXTRA[@]}"
rc=$?
echo "burst exit=$rc"
"$DI" stop
echo "$(date -u +%T) done $LABEL rc=$rc"
exit "$rc"
