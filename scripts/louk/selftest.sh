#!/usr/bin/env bash
# Offline self-test of dev-instance.sh + burst.py + results-table.py against a throwaway local stub server.
# The stub validates id forms (movie-|series-|tt..|episode-tt..-SxEy) and mimics the upstream 240-per-bucket
# rate limiter, so a wrong id prefix or a missing X-Forwarded-For spread shows up as errors/429s.
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
  for p in "${STUB_PIDS[@]:-}"; do
    [[ -n "$p" ]] || continue
    pkill -P "$p" 2>/dev/null || true
    kill "$p" 2>/dev/null || true
  done
  rm -rf "$TMP"
}
trap cleanup EXIT

cat > "$TMP/stub" <<'PY'
#!/usr/bin/env python3
# Stand-in for openposterdb-api: auth/setup/login/keys + image routes with fake latency, RSS growth,
# strict id forms (400 otherwise) and a per-(rightmost X-Forwarded-For | "unknown") limiter: more than 240
# requests within 1 s -> 429, like upstream's burst of 240.
import json, os, random, re, threading, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
host, port = os.environ["LISTEN_ADDR"].rsplit(":", 1)
home = os.path.dirname(os.environ["CACHE_DIR"]) if "CACHE_DIR" in os.environ else "."
os.makedirs(home, exist_ok=True)
with open(os.path.join(home, "stub-env.txt"), "w") as f:  # names + a few safe values only
    for k in sorted(os.environ):
        f.write(k + ("=" + os.environ[k] if k in ("RATINGS_DISABLED", "FOO", "MALLOC_ARENA_MAX", "STATIC_DIR", "OPDB_DEV_INSTANCE") else "") + "\n")
state = {"admin": False}
hog = []
hits, lock = {}, threading.Lock()
ID_OK = {  # (route id type, kind) -> id form upstream accepts
    ("imdb", "poster"): r"tt\d+", ("imdb", "episode"): r"episode-tt\d+-S\d+E\d+",
    ("tmdb", "backdrop"): r"(movie|series)-\d+", ("tmdb", "logo"): r"(movie|series)-\d+",
}
def limited(bucket):
    now = time.monotonic()
    with lock:
        q = [t for t in hits.get(bucket, []) if now - t < 1.0]
        q.append(now); hits[bucket] = q
        return len(q) > 240
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
        if not re.fullmatch(ID_OK.get((m.group(2), kind), r"(?!)"), ident): return self.send(400, b"invalid id type")
        if kind == "backdrop" and not m.group(6): return self.send(400, b"backdrop without imageSize=large")
        xff = (self.headers.get("X-Forwarded-For") or "").rsplit(",", 1)[-1].strip() or "unknown"
        if limited(m.group(1)[:16] + ":" + xff): return self.send(429, b"{}")
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

PROD='IMAGE_MEM_CACHE_MB=128 RENDER_CONCURRENCY=4 RATINGS_STALE_SECS=86400 RATINGS_MAX_AGE_SECS=31536000 IMAGE_STALE_SECS=0 DISABLE_PUBLIC_PAGES=true FREE_KEY_ENABLED=false ENABLE_CDN_REDIRECTS=false IMAGE_QUALITY=85'
BASE="http://127.0.0.1:$PORT"
KEYF="$OPDB_DEV_HOME/api-key"
burst() { python3 -I "$HERE/burst.py" --base "$BASE" --key-file "$KEYF" --pid "$PID" --out "$TMP/results" "$@"; }
jget() { python3 -I -c 'import json,sys; d=json.load(open(sys.argv[1])); print(eval(sys.argv[2]))' "$R" "$1"; }
# refuses PATTERN cmd...: the command must fail and its stderr must contain PATTERN
refuses() { local pat="$1" err; shift; if err="$("$@" 2>&1 >/dev/null)"; then return 1; fi; [[ "$err" == *"$pat"* ]]; }

# 1. start through dev-instance.sh with a polluted caller environment
export OMDB_API_KEY=leak-check MALLOC_ARENA_MAX=9 SELFTEST_CALLER_LEAK=1
# shellcheck disable=SC2086
"$HERE/dev-instance.sh" start "$TMP/stub" $PROD FOO=bar MALLOC_ARENA_MAX=2 >/dev/null
unset OMDB_API_KEY MALLOC_ARENA_MAX SELFTEST_CALLER_LEAK
PID="$(cat "$OPDB_DEV_HOME/pid")"
ENVTXT="$OPDB_DEV_HOME/stub-env.txt"
check "start: pidfile pid is the stub" grep -q "stub" "/proc/$PID/cmdline"
check "start: RATINGS_DISABLED=true"   grep -qx "RATINGS_DISABLED=true" "$ENVTXT"
check "start: extra var passed"        grep -qx "FOO=bar" "$ENVTXT"
check "start: extra overrides caller"  grep -qx "MALLOC_ARENA_MAX=2" "$ENVTXT"
check "start: OMDB_API_KEY not leaked" bash -c "! grep -q '^OMDB_API_KEY' '$ENVTXT'"
check "start: caller env not leaked (clean env)" bash -c "! grep -q '^SELFTEST_CALLER_LEAK' '$ENVTXT'"
check "start: secrets reach the server" bash -c "grep -qx 'JWT_SECRET' '$ENVTXT' && grep -qx 'TMDB_API_KEY' '$ENVTXT'"
check "start: instance marker set"     grep -qx "OPDB_DEV_INSTANCE=$OPDB_DEV_HOME" "$ENVTXT"
check "start: server cmdline has no VAR=VALUE (no secrets on argv)" bash -c "! tr '\\0' ' ' < /proc/$PID/cmdline | grep -q '='"
check "start: no env -i / envargs on a command line in the script" bash -c "! grep -v '^ *#' '$HERE/dev-instance.sh' | grep -E 'env -i|exec .*envargs'"
check "start: refuses OMDB_API_KEY arg" bash -c "! '$HERE/dev-instance.sh' start '$TMP/stub' OMDB_API_KEY=x 2>/dev/null"
check "start: refuses RATINGS_DISABLED arg" bash -c "! '$HERE/dev-instance.sh' start '$TMP/stub' RATINGS_DISABLED=false 2>/dev/null"
"$HERE/dev-instance.sh" bootstrap >/dev/null
check "bootstrap: key file mode 600" test "$(stat -c %a "$KEYF")" = 600
"$HERE/dev-instance.sh" bootstrap >/dev/null
check "bootstrap: idempotent"        test -s "$KEYF"

# 1b. the stub itself must reject what upstream rejects (so the burst can catch it)
http_code() { python3 -I -c 'import sys,urllib.request,urllib.error
try: print(urllib.request.urlopen(sys.argv[1]).status)
except urllib.error.HTTPError as e: print(e.code)' "$1"; }
K="$(cat "$KEYF")"
check "stub: tmdb tv- prefix is 400"      test "$(http_code "$BASE/$K/tmdb/logo-default/tv-84773.png")" = 400
check "stub: tmdb series- prefix is 200"  test "$(http_code "$BASE/$K/tmdb/backdrop-default/series-84773.jpg?imageSize=large")" = 200
check "stub: imdb poster without tt is 400" test "$(http_code "$BASE/$K/imdb/poster-default/1234.jpg")" = 400

# 2. the default burst: spread over X-Forwarded-For groups, cold, valid
rc=0; burst --label selftest --idle 2 | tee "$TMP/line1" || rc=$?
R="$TMP/results/selftest.json"
check "burst: exit code 0"            test "$rc" = 0
check "burst: 490 requests"           test "$(jget "d['requests']")" = 490
check "burst: 0 errors (404 is a status)" test "$(jget "d['errors']")" = 0
check "burst: has 200 and 404, no 400/429" test "$(jget "set(d['status_counts'])=={'200','404'}")" = True
check "burst: valid, no reasons"      test "$(jget "d['valid'] is True and d['invalid_reasons']==[]")" = True
check "burst: 3 rate-limit buckets"   test "$(jget "d['rate_limit_buckets']==3 and d['single_bucket'] is False")" = True
check "burst: backdrop/logo use series- (no 400s per kind)" test "$(jget "all('400' not in v['status_counts'] for v in d['kinds'].values())")" = True
check "burst: 4 kinds with p95"       test "$(jget "sorted(d['kinds'])")" = "['backdrop', 'episode', 'logo', 'poster']"
check "burst: 150+150+... counts"     test "$(jget "[d['kinds'][k]['n'] for k in ('poster','backdrop','logo','episode')]")" = "[150, 150, 150, 40]"
check "burst: latency over 200s only" test "$(jget "d['kinds']['logo']['n_ok'] < d['kinds']['logo']['n'] and d['kinds']['poster']['n_ok']==150")" = True
check "burst: RSS sampled and grew"   test "$(jget "d['peak_rss_mb'] > d['rss_start_mb'] and d['vmhwm_mb'] >= d['peak_rss_mb']-1 and d['idle_rss_mb'] > 0")" = True
check "burst: VmHWM reset recorded"   test "$(jget "isinstance(d['vmhwm_reset'], bool) and d['vmhwm_start_mb'] > 0")" = True
check "burst: p50<=p95<=max"          test "$(jget "d['p50_ms'] <= d['p95_ms'] <= d['max_ms']")" = True
check "burst: default timeout 600"    test "$(jget "d['timeout_s']==600")" = True
check "burst: cold run recorded"      test "$(jget "d['cache_files_before']")" = 0
check "burst: prod env complete"      test "$(jget "d['prod_env_complete'] is True and d['prod_env_missing']==[]")" = True
check "burst: env recorded, no secrets" test "$(jget "d['env'].get('FOO') is None and d['env']['RATINGS_DISABLED']=='true' and 'TMDB_API_KEY' not in d['env']")" = True
check "burst: summary says cold, no INVALID" bash -c "grep -q 'cold' '$TMP/line1' && ! grep -q INVALID '$TMP/line1'"

# 3. single bucket + warm cache: the stub 429s ~250 requests -> INVALID, exit 3
mkdir -p "$OPDB_DEV_HOME/cache" && : > "$OPDB_DEV_HOME/cache/x.jpg"
rc=0; burst --label selftest-single --single-bucket --idle 0 | tee "$TMP/line2" || rc=$?
R="$TMP/results/selftest-single.json"
check "single-bucket: exit code 3"          test "$rc" = 3
check "single-bucket: INVALID, 429s listed" test "$(jget "d['valid'] is False and any('429' in r for r in d['invalid_reasons']) and d['status_counts'].get('429',0) > 100")" = True
check "single-bucket: 1 bucket recorded"    test "$(jget "d['rate_limit_buckets']==1 and d['single_bucket'] is True")" = True
check "single-bucket: warm cache recorded"  test "$(jget "d['cache_files_before']")" = 1
check "single-bucket: INVALID in summary line" grep -q "^INVALID" "$TMP/line2"
check "results-table: INVALID, cold/warm"   bash -c "python3 -I '$HERE/results-table.py' '$TMP/results' | grep -q 'selftest-single (INVALID) | warm | 1 |' && python3 -I '$HERE/results-table.py' '$TMP/results' | grep -q '| selftest | cold | 3 |'"
check "burst: key not in result files"  bash -c "! grep -rqF \"\$(cat '$KEYF')\" '$TMP/results'"
python3 -I "$HERE/results-table.py" "$TMP/results" | head -4

# 4. refusals (against throwaway stubs on their own ports; nothing real is ever contacted)
check "refuse: non-local base" refuses "not local" python3 -I "$HERE/burst.py" --base http://example.com:3900 --key-file "$KEYF" --pid "$PID" --label x --out "$TMP/r2"
start_raw() {  # start the stub by hand with a chosen environment; sets RAW_PID / RAW_PORT
  local port; port="$(python3 -I -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')"
  mkdir -p "$TMP/raw$1"
  env -i PATH="$PATH" LISTEN_ADDR="127.0.0.1:$port" CACHE_DIR="$TMP/raw$1/cache" "${@:2}" "$TMP/stub" >/dev/null 2>&1 &
  RAW_PID=$!; RAW_PORT=$port; STUB_PIDS+=("$RAW_PID")
}
wait_port() { for _ in $(seq 1 50); do (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null && return 0; sleep 0.1; done; return 1; }
start_raw 1 OMDB_API_KEY=x RATINGS_DISABLED=true;    P1=$RAW_PID; Q1=$RAW_PORT
start_raw 2 FOO=1;                                    P2=$RAW_PID; Q2=$RAW_PORT
start_raw 3 MDBLIST_API_KEY=x RATINGS_DISABLED=true; P3=$RAW_PID; Q3=$RAW_PORT
for q in $Q1 $Q2 $Q3; do wait_port "$q" || bad "raw stub on $q did not start"; done
for n in 1 2 3; do
  eval "p=\$P$n; q=\$Q$n"
  check "refuse: server env #$n (OMDB / no RATINGS_DISABLED / MDBLIST)" \
    refuses "refusing to run" python3 -I "$HERE/burst.py" --base "http://127.0.0.1:$q" --key-file "$KEYF" --pid "$p" --label x --out "$TMP/r3" --titles-limit 1
done
# a wrapper shell that owns no socket: the env check passes (it inherits RATINGS_DISABLED) but --pid must own the listener
WPORT="$(python3 -I -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')"
env -i PATH="$PATH" RATINGS_DISABLED=true LISTEN_ADDR="127.0.0.1:$WPORT" CACHE_DIR="$TMP/rawW/cache" bash -c '"$0" & wait' "$TMP/stub" >/dev/null 2>&1 &
WRAP=$!; STUB_PIDS+=("$WRAP"); wait_port "$WPORT" || bad "wrapped stub did not start"
check "refuse: --pid is a wrapper, not the listener" \
  refuses "does not listen on --base" python3 -I "$HERE/burst.py" --base "http://127.0.0.1:$WPORT" --key-file "$KEYF" --pid "$WRAP" --label x --out "$TMP/r3" --titles-limit 1
check "refuse: wrote nothing"  test ! -e "$TMP/r3" -a ! -e "$TMP/r2"

# 5. RSS abort: threshold 1 MB kills the (throwaway) server and the run is INVALID, exit 2
start_raw 5 RATINGS_DISABLED=true; P5=$RAW_PID; Q5=$RAW_PORT; wait_port "$Q5" || bad "abort stub did not start"
rc=0; python3 -I "$HERE/burst.py" --base "http://127.0.0.1:$Q5" --key-file "$KEYF" --pid "$P5" --label selftest-abort \
  --idle 0 --rss-abort-mb 1 --titles-limit 3 --episodes-limit 1 --out "$TMP/results" >"$TMP/line3" 2>"$TMP/err3" || rc=$?
R="$TMP/results/selftest-abort.json"
check "abort: exit code 2"                  test "$rc" = 2
check "abort: INVALID, aborted recorded"    test "$(jget "d['valid'] is False and d['aborted'] is True and any('aborted' in r for r in d['invalid_reasons'])")" = True
check "abort: server was stopped"           bash -c "sleep 0.5; ! kill -0 $P5 2>/dev/null"
check "abort: not-prod env warned and recorded" test "$(jget "d['prod_env_complete'] is False and 'IMAGE_QUALITY' in d['prod_env_missing']")" = True
check "abort: warning printed"              grep -q "SPEC section 2" "$TMP/err3"

# 6. stop, pidfile reuse and orphans
"$HERE/dev-instance.sh" stop >/dev/null
check "stop: process gone" bash -c "! kill -0 $PID 2>/dev/null"
echo $$ > "$OPDB_DEV_HOME/pid"   # a live pid without our marker (a reused pid)
check "status: foreign pid in pidfile is not running" test "$("$HERE/dev-instance.sh" status)" = stopped
rm -f "$OPDB_DEV_HOME/pid"
"$HERE/dev-instance.sh" start "$TMP/stub" >/dev/null
PID2="$(cat "$OPDB_DEV_HOME/pid")"
rm -f "$OPDB_DEV_HOME/pid"       # lost pidfile: the server is an orphan
check "start: refuses while an orphan runs" bash -c "! '$HERE/dev-instance.sh' start '$TMP/stub' 2>/dev/null"
"$HERE/dev-instance.sh" stop >/dev/null
check "stop: kills an orphan without pidfile" bash -c "! kill -0 $PID2 2>/dev/null"
check "stop: did not kill this shell"         kill -0 $$
echo "selftest: $pass passed, $fail failed"
[[ $fail -eq 0 ]]
