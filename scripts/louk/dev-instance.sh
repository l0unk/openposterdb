#!/usr/bin/env bash
# Local OpenPosterDB test instance for the dev box (fork tooling).
#
#   scripts/louk/dev-instance.sh start <binary> [VAR=VALUE ...]   start it (waits for /api/auth/status)
#   scripts/louk/dev-instance.sh bootstrap                         create admin louk-dev + API key
#   scripts/louk/dev-instance.sh stop                              kill it (pidfile)
#   scripts/louk/dev-instance.sh wipe-cache                        rm -rf the render cache
#   scripts/louk/dev-instance.sh wipe-db                           rm -rf the SQLite db (then bootstrap again)
#   scripts/louk/dev-instance.sh status                            print "running <pid>" or "stopped"
#
# State lives in ~/opdb-dev (override with OPDB_DEV_HOME): cache/, db/, pid, server.log,
# api-key (mode 600, never printed). The instance listens on 127.0.0.1:3900 (OPDB_DEV_PORT).
#
# The server starts in a CLEAN environment (env -i): nothing leaks in from the caller (no
# MALLOC_ARENA_MAX, no OMDB_API_KEY). It always gets RATINGS_DISABLED=true and never gets OMDb /
# MDBList / Trakt keys. Extra VAR=VALUE arguments are added on top of the defaults (they win).
# Always run `stop` when you are done.
set -euo pipefail

HOME_DIR="${OPDB_DEV_HOME:-$HOME/opdb-dev}"
PORT="${OPDB_DEV_PORT:-3900}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ENV_FILE="$HOME/.config/opdb-dev.env"
PIDFILE="$HOME_DIR/pid"
LOG="$HOME_DIR/server.log"
KEYFILE="$HOME_DIR/api-key"
BASE="http://127.0.0.1:$PORT"
ADMIN_USER="louk-dev"
ADMIN_PASS_FILE="$HOME_DIR/admin-password"

die() { echo "dev-instance: $*" >&2; exit 1; }

is_running() {
  [[ -f "$PIDFILE" ]] || return 1
  local pid
  pid="$(cat "$PIDFILE")"
  [[ "$pid" =~ ^[0-9]+$ ]] && kill -0 "$pid" 2>/dev/null
}

cmd_start() {
  local bin="${1:-}"
  [[ -n "$bin" ]] || die "usage: start <binary> [VAR=VALUE ...]"
  shift
  [[ -x "$bin" ]] || die "binary not found or not executable: $bin"
  is_running && die "already running (pid $(cat "$PIDFILE")); run stop first"
  if (exec 3<>"/dev/tcp/127.0.0.1/$PORT") 2>/dev/null; then
    die "port $PORT is already in use by something else"
  fi
  [[ -r "$ENV_FILE" ]] || die "missing $ENV_FILE"
  mkdir -p "$HOME_DIR/cache" "$HOME_DIR/db"

  # Extra VAR=VALUE arguments: validated, rating-provider keys refused.
  local extra=() kv name
  for kv in "$@"; do
    [[ "$kv" == *=* ]] || die "extra arguments must be VAR=VALUE (got '${kv%%=*}')"
    name="${kv%%=*}"
    [[ "$name" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]] || die "bad variable name: $name"
    case "$name" in
      OMDB_API_KEY|MDBLIST_API_KEY|TRAKT_CLIENT_ID) die "$name is never passed to the dev instance" ;;
      TMDB_API_KEY|FANART_API_KEY|JWT_SECRET) die "$name is set by the script; do not pass it" ;;
    esac
    extra+=("$kv")
  done

  local tmdb fanart jwt
  # Secrets stay inside this subshell / variables; nothing is echoed or traced.
  tmdb="$( set +x; set -a; . "$ENV_FILE"; set +a; printf '%s' "${TMDB_API_KEY:-}" )"
  fanart="$( set +x; set -a; . "$ENV_FILE"; set +a; printf '%s' "${FANART_API_KEY:-}" )"
  [[ -n "$tmdb" ]] || die "TMDB_API_KEY missing from $ENV_FILE"
  jwt="$(openssl rand -hex 32)"

  local envargs=(
    "PATH=$PATH" "HOME=$HOME" "LANG=${LANG:-C.UTF-8}"
    "LISTEN_ADDR=127.0.0.1:$PORT"
    "CACHE_DIR=$HOME_DIR/cache" "DB_DIR=$HOME_DIR/db"
    "STATIC_DIR=web/dist" "COOKIE_SECURE=false" "RATINGS_DISABLED=true"
    "JWT_SECRET=$jwt" "TMDB_API_KEY=$tmdb"
  )
  [[ -n "$fanart" ]] && envargs+=("FANART_API_KEY=$fanart")
  # Later assignments win in env(1): extras override the defaults above.
  envargs+=("${extra[@]}")

  : > "$LOG"
  chmod 600 "$LOG"
  # exec chain (nice -> env -> binary) keeps $! == the server's pid.
  (
    cd "$ROOT"
    exec nohup nice -n "${OPDB_DEV_NICE:-19}" env -i "${envargs[@]}" "$bin" </dev/null >>"$LOG" 2>&1
  ) &
  local pid=$!
  echo "$pid" > "$PIDFILE"

  local i
  for i in $(seq 1 120); do
    if ! kill -0 "$pid" 2>/dev/null; then
      rm -f "$PIDFILE"
      echo "dev-instance: server exited during startup; last log lines:" >&2
      tail -n 20 "$LOG" >&2 || true
      exit 1
    fi
    if curl -fsS -m 2 "$BASE/api/auth/status" >/dev/null 2>&1; then
      echo "dev-instance: up on $BASE (pid $pid)"
      return 0
    fi
    sleep 0.5
  done
  kill "$pid" 2>/dev/null || true
  rm -f "$PIDFILE"
  die "server did not answer /api/auth/status within 60 s (see $LOG)"
}

# Pull a JSON string field out of stdin without printing anything else.
json_field() {
  python3 -I -c 'import json,sys; print(json.load(sys.stdin)[sys.argv[1]])' "$1"
}

# Login/setup request body for the admin account (the password is read from stdin, not argv).
creds_json() {
  printf '%s' "$1" | python3 -I -c 'import json,sys; print(json.dumps({"username":sys.argv[1],"password":sys.stdin.read()}))' "$ADMIN_USER"
}

cmd_bootstrap() {
  is_running || die "not running; run start first"
  local status pass token key
  status="$(curl -fsS -m 10 "$BASE/api/auth/status")"
  if [[ -f "$ADMIN_PASS_FILE" ]]; then
    pass="$(cat "$ADMIN_PASS_FILE")"
  else
    pass="$(openssl rand -hex 16)"
    ( umask 077; printf '%s' "$pass" > "$ADMIN_PASS_FILE" )
  fi
  if python3 -I -c 'import json,sys; sys.exit(0 if json.loads(sys.argv[1])["setup_required"] else 1)' "$status"; then
    creds_json "$pass" \
      | curl -fsS -m 30 -H 'Content-Type: application/json' --data-binary @- "$BASE/api/auth/setup" >/dev/null
  fi
  token="$(creds_json "$pass" \
      | curl -fsS -m 30 -H 'Content-Type: application/json' --data-binary @- "$BASE/api/auth/login" | json_field token)" \
    || die "login failed (admin $ADMIN_USER exists with another password? run wipe-db and start again)"
  # The bearer token goes through a 600 header file, never argv.
  local hdr="$HOME_DIR/.hdr.$$"
  ( umask 077; printf 'Authorization: Bearer %s\n' "$token" > "$hdr" )
  key="$(curl -fsS -m 30 -H "@$hdr" -H 'Content-Type: application/json' \
          --data '{"name":"burst"}' "$BASE/api/keys" | json_field key)" \
    || { rm -f "$hdr"; die "could not create an API key"; }
  rm -f "$hdr"
  ( umask 077; printf '%s\n' "$key" > "$KEYFILE" )
  chmod 600 "$KEYFILE"
  echo "dev-instance: admin $ADMIN_USER ready, API key written to $KEYFILE (mode 600)"
}

cmd_stop() {
  if is_running; then
    local pid
    pid="$(cat "$PIDFILE")"
    kill "$pid" 2>/dev/null || true
    for _ in $(seq 1 50); do kill -0 "$pid" 2>/dev/null || break; sleep 0.2; done
    kill -9 "$pid" 2>/dev/null || true
    echo "dev-instance: stopped pid $pid"
  else
    echo "dev-instance: not running"
  fi
  rm -f "$PIDFILE"
}

cmd_status() {
  if is_running; then echo "running $(cat "$PIDFILE")"; else echo "stopped"; fi
}

cmd_wipe_cache() { is_running && die "stop first"; rm -rf "$HOME_DIR/cache"; echo "dev-instance: cache wiped"; }
cmd_wipe_db() { is_running && die "stop first"; rm -rf "$HOME_DIR/db" "$KEYFILE" "$ADMIN_PASS_FILE"; echo "dev-instance: db and credentials wiped"; }

case "${1:-}" in
  start) shift; cmd_start "$@" ;;
  bootstrap) cmd_bootstrap ;;
  stop) cmd_stop ;;
  status) cmd_status ;;
  wipe-cache) cmd_wipe_cache ;;
  wipe-db) cmd_wipe_db ;;
  *) sed -n '2,13p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2; exit 2 ;;
esac
