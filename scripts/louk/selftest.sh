#!/usr/bin/env bash
# Offline self-test of dev-instance.sh + burst.py against a throwaway local stub server.
# No cargo, no real server, no network beyond 127.0.0.1. Usage: scripts/louk/selftest.sh
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
PORT="$(python3 -I -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')"
export OPDB_DEV_HOME="$TMP/home" OPDB_DEV_PORT="$PORT" OPDB_DEV_NICE=10
pass=0; fail=0
ok()  { echo "ok   $*"; pass=$((pass+1)); }
bad() { echo "FAIL $*"; fail=$((fail+1)); }
check() { local name="$1"; shift; if "$@"; then ok "$name"; else bad "$name"; fi; }
STUB_PIDS=()
cleanup() {
  "$HERE/dev-instance.sh" stop >/dev/null 2>&1 || true
  for p in "${STUB_PIDS[@]:-}"; do [[ -n "$p" ]] && kill "$p" 2>/dev/null || true; done
  rm -rf "$TMP"
}
trap cleanup EXIT

cat > "$TMP/stub" <<'PY'
#!/usr/bin/env python3
# Minimal stand-in for openposterdb-api: auth/setup/login/keys + image routes with fake latency and RSS growth.
import json, os, random, re, threading, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
host, port = os.environ["LISTEN_ADDR"].rsplit(":", 1)
home = os.path.dirname(os.environ["CACHE_DIR"]) if "CACHE_DIR" in os.environ else "."
with open(os.path.join(home, "stub-env.txt"), "w") as f:  # names + a few safe values only
    for k in sorted(os.environ):
        f.write(k + ("=" + os.environ[k] if k in ("RATINGS_DISABLED", "FOO", "MALLOC_ARENA_MAX", "STATIC_DIR") else "") + "\n")
state = {"admin": False}
hog = []
class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *a): pass
    def send(self, code, body=b"", ctype="application/json"):
        self.send_response(code); self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body))); self.end_headers(); self.wfile.write(body)
    def body(self):
        n = int(self.headers.get("Content-Length") or 0); return json.loads(self.rfile.read(n) or b"{}")
    def do_GET(self):
        if self.path == "/api/auth/status":
            return self.send(200, json.dumps({"setup_required": not state["admin"]}).encode())
        m = re.match(r"^/([0-9a-f]+)/(imdb|tmdb)/(poster|backdrop|logo|episode)-default/([^?]+?)(\.jpg|\.png)(\?imageSize=large)?$", self.path)
        if not m: return self.send(400, b"{}")
        kind, ident = m.group(3), m.group(4)
        if kind == "backdrop" and not m.group(6): return self.send(400, b"backdrop without imageSize=large")
        time.sleep(random.uniform(0.01, 0.15))
        if kind == "logo" and hash(ident) % 5 == 0: return self.send(404, b"{}")
        if kind == "backdrop": hog.append(bytearray(os.urandom(256 * 1024)))  # visible RSS growth
        self.send(200, b"x" * 40000, "image/jpeg")
    def do_POST(self):
        b = self.body()
        if self.path == "/api/auth/setup": state["admin"] = True; state["pw"] = b["password"]; return self.send(200, b'{"token":"t"}')
        if self.path == "/api/auth/login":
            return self.send(200, b'{"token":"t"}') if b.get("password") == state.get("pw") else self.send(401, b"{}")
        if self.path == "/api/keys": return self.send(200, json.dumps({"key": os.urandom(32).hex()}).encode())
        self.send(404, b"{}")
ThreadingHTTPServer((host, int(port)), H).serve_forever()
PY
chmod +x "$TMP/stub"

# 1. start through dev-instance.sh with a polluted caller environment
export OMDB_API_KEY=leak-check MALLOC_ARENA_MAX=9
"$HERE/dev-instance.sh" start "$TMP/stub" FOO=bar MALLOC_ARENA_MAX=2 >/dev/null
unset OMDB_API_KEY MALLOC_ARENA_MAX
PID="$(cat "$OPDB_DEV_HOME/pid")"
check "start: pidfile pid is the stub" grep -q "stub" "/proc/$PID/cmdline"
check "start: RATINGS_DISABLED=true"   grep -qx "RATINGS_DISABLED=true" "$OPDB_DEV_HOME/stub-env.txt"
check "start: extra var passed"        grep -qx "FOO=bar" "$OPDB_DEV_HOME/stub-env.txt"
check "start: extra overrides caller"  grep -qx "MALLOC_ARENA_MAX=2" "$OPDB_DEV_HOME/stub-env.txt"
check "start: OMDB_API_KEY not leaked" bash -c "! grep -q '^OMDB_API_KEY' '$OPDB_DEV_HOME/stub-env.txt'"
check "start: refuses OMDB_API_KEY arg" bash -c "! '$HERE/dev-instance.sh' start '$TMP/stub' OMDB_API_KEY=x 2>/dev/null"
"$HERE/dev-instance.sh" bootstrap >/dev/null
check "bootstrap: key file mode 600" test "$(stat -c %a "$OPDB_DEV_HOME/api-key")" = 600
"$HERE/dev-instance.sh" bootstrap >/dev/null
check "bootstrap: idempotent"        test -s "$OPDB_DEV_HOME/api-key"

# 2. the burst
python3 -I "$HERE/burst.py" --base "http://127.0.0.1:$PORT" --key-file "$OPDB_DEV_HOME/api-key" --pid "$PID" \
  --label selftest --idle 2 --out "$TMP/results"
R="$TMP/results/selftest.json"
jget() { python3 -I -c 'import json,sys; d=json.load(open(sys.argv[1])); print(eval(sys.argv[2]))' "$R" "$1"; }
check "burst: 490 requests"           test "$(jget "d['requests']")" = 490
check "burst: 0 errors (404 is a status)" test "$(jget "d['errors']")" = 0
check "burst: has 200 and 404"        test "$(jget "set(d['status_counts'])=={'200','404'}")" = True
check "burst: 4 kinds with p95"       test "$(jget "sorted(d['kinds'])")" = "['backdrop', 'episode', 'logo', 'poster']"
check "burst: 150+150+... counts"     test "$(jget "[d['kinds'][k]['n'] for k in ('poster','backdrop','logo','episode')]")" = "[150, 150, 150, 40]"
check "burst: RSS sampled and grew"   test "$(jget "d['peak_rss_mb'] > d['rss_start_mb'] and d['vmhwm_mb'] >= d['peak_rss_mb']-1 and d['idle_rss_mb'] > 0")" = True
check "burst: p50<=p95<=max"          test "$(jget "d['p50_ms'] <= d['p95_ms'] <= d['max_ms']")" = True
check "burst: env recorded, no secrets" test "$(jget "d['env'].get('FOO') is None and d['env']['RATINGS_DISABLED']=='true' and 'TMDB_API_KEY' not in d['env']")" = True
check "burst: key not in result files"  bash -c "! grep -rqF \"\$(cat '$OPDB_DEV_HOME/api-key')\" '$TMP/results'"
python3 -I "$HERE/results-table.py" "$TMP/results" | head -3

# 3. refusals
check "refuse: non-local base" bash -c "! python3 -I '$HERE/burst.py' --base http://example.com:3900 --key-file '$OPDB_DEV_HOME/api-key' --pid $PID --label x --out '$TMP/r2' 2>/dev/null"
start_raw() {  # start the stub by hand with a chosen environment, echo its pid
  local port; port="$(python3 -I -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')"
  mkdir -p "$TMP/raw$1"
  env -i PATH="$PATH" LISTEN_ADDR="127.0.0.1:$port" CACHE_DIR="$TMP/raw$1/cache" "${@:2}" "$TMP/stub" >/dev/null 2>&1 &
  echo $!
}
P1="$(start_raw 1 OMDB_API_KEY=x RATINGS_DISABLED=true)"; STUB_PIDS+=("$P1")
P2="$(start_raw 2 FOO=1)";                                STUB_PIDS+=("$P2")
P3="$(start_raw 3 MDBLIST_API_KEY=x RATINGS_DISABLED=true)"; STUB_PIDS+=("$P3")
sleep 1
for n in 1 2 3; do
  eval "p=\$P$n"
  check "refuse: server env #$n (OMDB / no RATINGS_DISABLED / MDBLIST)" \
    bash -c "! python3 -I '$HERE/burst.py' --key-file '$OPDB_DEV_HOME/api-key' --pid $p --label x --out '$TMP/r3' --titles-limit 1 2>/dev/null"
done
check "refuse: wrote nothing"  test ! -e "$TMP/r3" -a ! -e "$TMP/r2"

"$HERE/dev-instance.sh" stop >/dev/null
check "stop: process gone" bash -c "! kill -0 $PID 2>/dev/null"
echo "selftest: $pass passed, $fail failed"
[[ $fail -eq 0 ]]
