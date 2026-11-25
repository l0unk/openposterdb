#!/usr/bin/env python3
"""Catalog-burst load harness for the OpenPosterDB fork (memory measurements).

Fires, all at once, what a client does when it opens a catalog: per title a poster, a
backdrop (`?imageSize=large`) and a logo, plus episode stills, using Louk's exact URL patterns:

  /{key}/imdb/poster-default/{imdb}.jpg
  /{key}/tmdb/backdrop-default/{movie|series}-{tmdb}.jpg?imageSize=large
  /{key}/tmdb/logo-default/{movie|series}-{tmdb}.png
  /{key}/imdb/episode-default/episode-{imdb}-S{season}E{episode}.jpg

While the burst runs it samples the server's VmRSS/VmHWM from /proc/<pid>/status every 200 ms,
then waits --idle seconds and samples again. One JSON (and a small .md) file per run goes to
--out so variants can be compared (see results-table.py).

Upstream rate-limits each (API key prefix, client IP) pair to a burst of 240 plus 1 request per 200 ms
(the IP comes from X-Forwarded-For / CF-Connecting-IP / X-Real-IP, honoured from any peer, no setting
needed). One caller would get ~250 instant 429s out of 490 requests, so by default the requests are spread
over fake X-Forwarded-For groups of --bucket-size (200) so that all 490 really render. --single-bucket sends no
header (what one prod key sees); expect 429s and an INVALID run.

--warmup runs the warm-up pass instead of the burst (run it once, after bootstrap, before the burst). A fresh
server process has empty in-memory id / TMDB-image-list caches, so a cold burst would send all 490 lookups to
the TMDB API at once and TMDB answers 429 (seen as 500 here). The warm-up requests the same titles once at low
concurrency (--warmup-concurrency, default 4) under DIFFERENT render cache keys than the burst, so the lookups
are cached but none of the burst's renders is: backdrops and episodes with ?imageSize=small, posters with
?badge_style=v, logos with ?badge_style=h (the non-default style of each; with ratings disabled no badge is
drawn, so it is the same work under another key). It uses its own X-Forwarded-For groups (10.253.x.1), so the
burst's rate-limit buckets stay full. Same safety checks and RSS abort as the burst; it writes no result file.
Exit codes: 0 ok, 2 server died, 3 failed (429, transport errors, or more than 5% errors: the lookups are
not warm and the burst would be invalid). About 8 logos fail to decode upstream (500) in every run: that is
under the 5%.

A run is INVALID (JSON "valid": false, "invalid_reasons", INVALID in the summary line, exit code 3) when the
server died or was aborted, more than 2% of requests errored, any 429 or transport error/timeout happened, or
poster/backdrop/episode got fewer than 90% 200s: the server did less work, so its memory looks better than it is.
Exit codes: 0 valid, 2 server died (or was aborted at --rss-abort-mb), 3 invalid.

Safety (the harness refuses to run otherwise):
  * the server's environment (/proc/<pid>/environ) must have RATINGS_DISABLED=true and must NOT
    have OMDB_API_KEY, MDBLIST_API_KEY or TRAKT_CLIENT_ID (no quota is ever spent);
  * --base must point at the local machine (127.0.0.1 / localhost / ::1);
  * --pid must own the listening socket of --base (the server, not a wrapper shell);
  * waits while the dev slice memory pressure (PSI full avg10) is above 20;
  * kills the server (SIGTERM) and marks the run INVALID if its RSS exceeds --rss-abort-mb (default 3000),
    to protect the shared dev box.

stdlib only. Run as:  python3 -I scripts/louk/burst.py --label A-baseline --pid $(cat ~/opdb-dev/pid)
The API key is read from --key-file and never printed or stored in the result.
"""
import argparse
import concurrent.futures
import json
import os
import random
import re
import signal
import statistics
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
KINDS = ("poster", "backdrop", "logo", "episode")
FORBIDDEN_ENV = ("OMDB_API_KEY", "MDBLIST_API_KEY", "TRAKT_CLIENT_ID")
# Only these (non-secret) variables are copied into the result's "env" field.
ENV_ALLOWLIST = (
    "RATINGS_DISABLED", "BACKDROP_MAX_WIDTH", "IMAGE_MEM_CACHE_MB", "RENDER_CONCURRENCY",
    "RATINGS_STALE_SECS", "RATINGS_MAX_AGE_SECS", "IMAGE_STALE_SECS", "IMAGE_QUALITY",
    "DISABLE_PUBLIC_PAGES", "FREE_KEY_ENABLED", "ENABLE_CDN_REDIRECTS", "EXTERNAL_CACHE_ONLY",
    "MALLOC_ARENA_MAX", "MALLOC_CONF", "MALLOC_TRIM_THRESHOLD_", "MALLOC_MMAP_THRESHOLD_",
    "MIMALLOC_PURGE_DELAY", "MIMALLOC_ALLOW_LARGE_OS_PAGES", "RUST_LOG",
    "CACHE_DIR", "DB_DIR",  # paths, not secrets (used to tell a cold run from a warm one)
)
# SPEC section 2: the production settings a baseline must run with.
PROD_ENV = {
    "IMAGE_MEM_CACHE_MB": "128", "RENDER_CONCURRENCY": "4", "RATINGS_STALE_SECS": "86400",
    "RATINGS_MAX_AGE_SECS": "31536000", "IMAGE_STALE_SECS": "0", "DISABLE_PUBLIC_PAGES": "true",
    "FREE_KEY_ENABLED": "false", "ENABLE_CDN_REDIRECTS": "false", "IMAGE_QUALITY": "85",
}
BUCKET_BURST = 240  # upstream image limiter burst per (key prefix, client ip)
ENV_PREFIX_ALLOW = ("LOUK_",)
LOCAL_HOSTS = ("127.0.0.1", "localhost", "::1", "[::1]")


# ---------------------------------------------------------------- /proc helpers

def read_status(pid):
    """Return {'rss_kb', 'hwm_kb'} from /proc/<pid>/status, or None if the process is gone."""
    try:
        with open(f"/proc/{pid}/status") as f:
            txt = f.read()
    except OSError:
        return None
    out = {}
    for name, key in (("VmRSS", "rss_kb"), ("VmHWM", "hwm_kb")):
        m = re.search(rf"^{name}:\s+(\d+)\s+kB", txt, re.M)
        if m:
            out[key] = int(m.group(1))
    return out if "rss_kb" in out else None


def read_environ(pid):
    try:
        with open(f"/proc/{pid}/environ", "rb") as f:
            raw = f.read()
    except OSError as e:
        raise SystemExit(f"cannot read /proc/{pid}/environ ({e.strerror}); is --pid the server and is it yours?")
    env = {}
    for item in raw.split(b"\0"):
        if b"=" in item:
            k, _, v = item.partition(b"=")
            env[k.decode("utf-8", "replace")] = v.decode("utf-8", "replace")
    return env


def prod_env_missing(env):
    """Names of SPEC section 2 variables that are absent from (or differ in) the server environment."""
    out = []
    for k, v in PROD_ENV.items():
        if k not in env:
            out.append(k)
        elif env[k] != v:
            out.append(f"{k}={env[k]} (prod: {v})")
    return out


def listening_inodes(port):
    inodes = set()
    for f in ("/proc/net/tcp", "/proc/net/tcp6"):
        try:
            with open(f) as fh:
                lines = fh.read().splitlines()[1:]
        except OSError:
            continue
        for line in lines:
            parts = line.split()
            if len(parts) > 9 and parts[3] == "0A" and int(parts[1].rsplit(":", 1)[1], 16) == port:
                inodes.add(parts[9])
    return inodes


def check_pid_listens(pid, base):
    """The pid must own the listening socket of --base, or we would measure a wrapper shell."""
    u = urllib.parse.urlsplit(base)
    port = u.port or (443 if u.scheme == "https" else 80)
    want = {f"socket:[{i}]" for i in listening_inodes(port)}
    owned = set()
    try:
        for fd in os.listdir(f"/proc/{pid}/fd"):
            try:
                owned.add(os.readlink(f"/proc/{pid}/fd/{fd}"))
            except OSError:
                pass
    except OSError as e:
        raise SystemExit(f"cannot list /proc/{pid}/fd ({e.strerror})")
    if not (want & owned):
        raise SystemExit("--pid does not listen on --base; pass the server pid, not a wrapper")


def cache_state(env, pid):
    """(cache file count, db bytes) of the server's CACHE_DIR / DB_DIR right now (for cold/warm)."""
    try:
        cwd = os.readlink(f"/proc/{pid}/cwd")
    except OSError:
        cwd = "/"
    def resolve(name):
        v = env.get(name)
        return os.path.join(cwd, v) if v else None
    files = 0
    cdir = resolve("CACHE_DIR")
    if cdir:
        for _, _, fs in os.walk(cdir):
            files += len(fs)
    size = 0
    ddir = resolve("DB_DIR")
    if ddir:
        for root, _, fs in os.walk(ddir):
            for fn in fs:
                try:
                    size += os.path.getsize(os.path.join(root, fn))
                except OSError:
                    pass
    return files, size


def check_server_env(env):
    """Refuse to run unless ratings are disabled and no rating-provider key exists. Returns the safe env subset."""
    bad = [k for k in FORBIDDEN_ENV if env.get(k, "").strip()]
    if bad:
        raise SystemExit(f"refusing to run: server environment has {', '.join(bad)} set "
                         f"(the harness must never spend rating quota)")
    if env.get("RATINGS_DISABLED", "").strip().lower() != "true":
        raise SystemExit("refusing to run: server environment does not have RATINGS_DISABLED=true")
    return {k: v for k, v in env.items()
            if k in ENV_ALLOWLIST or k.startswith(ENV_PREFIX_ALLOW)}


def pressure_avg10():
    for p in ("/sys/fs/cgroup/user.slice/user-1100.slice/memory.pressure",
              f"/sys/fs/cgroup/user.slice/user-{os.getuid()}.slice/memory.pressure"):
        try:
            with open(p) as f:
                for line in f:
                    if line.startswith("full"):
                        return float(re.search(r"avg10=([\d.]+)", line).group(1))
        except (OSError, AttributeError):
            continue
    return None


def wait_for_pressure(limit=20.0, max_wait=600):
    t0 = time.monotonic()
    while True:
        p = pressure_avg10()
        if p is None or p <= limit:
            return p
        if time.monotonic() - t0 > max_wait:
            raise SystemExit(f"memory pressure avg10={p} stayed above {limit} for {max_wait}s; not running")
        print(f"memory pressure avg10={p} > {limit}; waiting", file=sys.stderr)
        time.sleep(10)


# ---------------------------------------------------------------- request generation

def build_requests(base, key, titles_doc, n_titles=None, n_episodes=None, seed=42, bucket_size=200, single_bucket=False):
    """Return ([(kind, url, xff)], n_buckets), shuffled with a fixed seed. The URL carries the key: never log it.

    xff is the fake client IP for the X-Forwarded-For header (request i gets group i // bucket_size, so no
    upstream rate-limit bucket sees more than bucket_size requests), or None with single_bucket.
    """
    base = base.rstrip("/")
    titles = titles_doc["titles"][:n_titles]
    episodes = titles_doc["episodes"][:n_episodes]
    reqs = []
    for t in titles:
        tp = "series" if t["type"] == "tv" else "movie"  # upstream/AIOMetadata prefix: movie-|series- (tv- is a 400)
        reqs.append(("poster", f"{base}/{key}/imdb/poster-default/{t['imdb']}.jpg"))
        reqs.append(("backdrop", f"{base}/{key}/tmdb/backdrop-default/{tp}-{t['tmdb']}.jpg?imageSize=large"))
        reqs.append(("logo", f"{base}/{key}/tmdb/logo-default/{tp}-{t['tmdb']}.png"))
    for e in episodes:
        reqs.append(("episode", f"{base}/{key}/imdb/episode-default/episode-{e['imdb']}-S{e['season']}E{e['episode']}.jpg"))
    random.Random(seed).shuffle(reqs)
    if single_bucket:
        return [(k, u, None) for k, u in reqs], 1
    n_buckets = max(1, -(-len(reqs) // bucket_size))
    return [(k, u, f"10.254.{i // bucket_size}.1") for i, (k, u) in enumerate(reqs)], n_buckets


WARMUP_BUCKET_SIZE = 200
WARMUP_MAX_ERROR_FRACTION = 0.05


def build_warmup_requests(base, key, titles_doc, n_titles=None, n_episodes=None, bucket_size=WARMUP_BUCKET_SIZE):
    """Same titles as build_requests, but every URL has a render cache key the burst does not use.

    Returns [(kind, url, xff)] in titles.json order (not shuffled: low concurrency, order does not matter).
    Posters/logos reject imageSize (400), so they vary badge_style instead; backdrops/episodes take imageSize.
    """
    base = base.rstrip("/")
    reqs = []
    for t in titles_doc["titles"][:n_titles]:
        tp = "series" if t["type"] == "tv" else "movie"
        reqs.append(("poster", f"{base}/{key}/imdb/poster-default/{t['imdb']}.jpg?badge_style=v"))
        reqs.append(("backdrop", f"{base}/{key}/tmdb/backdrop-default/{tp}-{t['tmdb']}.jpg?imageSize=small"))
        reqs.append(("logo", f"{base}/{key}/tmdb/logo-default/{tp}-{t['tmdb']}.png?badge_style=h"))
    for e in titles_doc["episodes"][:n_episodes]:
        reqs.append(("episode", f"{base}/{key}/imdb/episode-default/episode-{e['imdb']}-S{e['season']}E{e['episode']}.jpg?imageSize=small"))
    return [(k, u, f"10.253.{i // bucket_size}.1") for i, (k, u) in enumerate(reqs)]


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *a, **k):
        return None


def one_request(kind, url, xff, timeout):
    """Fetch one URL. Returns dict(kind, status|None, ms, bytes, err). Never includes the URL."""
    opener = urllib.request.build_opener(NoRedirect)
    t0 = time.perf_counter()
    status, nbytes, err = None, 0, None
    try:
        req = urllib.request.Request(url, headers={"X-Forwarded-For": xff} if xff else {})
        with opener.open(req, timeout=timeout) as r:
            status = r.status
            while True:
                chunk = r.read(65536)
                if not chunk:
                    break
                nbytes += len(chunk)
    except urllib.error.HTTPError as e:
        status = e.code
        try:
            while True:
                chunk = e.read(65536)
                if not chunk:
                    break
                nbytes += len(chunk)
        except Exception:
            pass
    except Exception as e:  # timeouts, resets: class name only, the message may hold the URL
        err = type(e).__name__
    return {"kind": kind, "status": status, "ms": (time.perf_counter() - t0) * 1000.0,
            "bytes": nbytes, "err": err}


def is_error(r):
    """Transport failure, any 5xx, or a 4xx other than 404 (404 = title without that art: a status, not an error)."""
    s = r["status"]
    return s is None or s >= 500 or (400 <= s < 500 and s != 404)


# ---------------------------------------------------------------- statistics

def percentile(sorted_vals, q):
    """Linear-interpolated percentile (q in 0..100) of an ascending list."""
    if not sorted_vals:
        return None
    if len(sorted_vals) == 1:
        return sorted_vals[0]
    pos = (len(sorted_vals) - 1) * q / 100.0
    lo = int(pos)
    hi = min(lo + 1, len(sorted_vals) - 1)
    return sorted_vals[lo] + (sorted_vals[hi] - sorted_vals[lo]) * (pos - lo)


def latency_stats(results):
    """Latency over the 200 responses only (instant 429/400 and timeouts would hide the render latency)."""
    ms = sorted(r["ms"] for r in results if r["status"] == 200)
    if not ms:
        return {"n_ok": 0, "p50_ms": None, "p95_ms": None, "max_ms": None}
    return {"n_ok": len(ms), "p50_ms": round(percentile(ms, 50), 1),
            "p95_ms": round(percentile(ms, 95), 1), "max_ms": round(ms[-1], 1)}


def status_counts(results):
    c = {}
    for r in results:
        k = str(r["status"]) if r["status"] is not None else (r["err"] or "error")
        c[k] = c.get(k, 0) + 1
    return dict(sorted(c.items()))


def summarize(results):
    per_kind = {}
    for k in KINDS:
        sub = [r for r in results if r["kind"] == k]
        if sub:
            per_kind[k] = {"n": len(sub), **latency_stats(sub), "status_counts": status_counts(sub),
                           "errors": sum(is_error(r) for r in sub),
                           "mean_bytes": round(statistics.fmean(r["bytes"] for r in sub))}
    return {"requests": len(results), "errors": sum(is_error(r) for r in results),
            "status_counts": status_counts(results), **latency_stats(results),
            "kinds": per_kind, "total_bytes": sum(r["bytes"] for r in results)}


# ---------------------------------------------------------------- RSS sampler

class RssSampler(threading.Thread):
    def __init__(self, pid, interval=0.2, abort_mb=None):
        super().__init__(daemon=True)
        self.pid, self.interval, self.abort_kb = pid, interval, (abort_mb * 1024 if abort_mb else None)
        self.aborted = False
        self.stop_ev = threading.Event()
        self.samples = []  # (t_since_start_s, rss_kb)
        self.hwm_kb = 0
        self.died = False
        self.t0 = time.monotonic()

    def run(self):
        while not self.stop_ev.is_set():
            st = read_status(self.pid)
            if st is None:
                self.died = True
                return
            self.samples.append((time.monotonic() - self.t0, st["rss_kb"]))
            self.hwm_kb = max(self.hwm_kb, st.get("hwm_kb", 0))
            if self.abort_kb and st["rss_kb"] > self.abort_kb:
                self.aborted = True  # protect the shared dev box: stop the server, the run is INVALID
                try:
                    os.kill(self.pid, signal.SIGTERM)
                except OSError:
                    pass
                return
            self.stop_ev.wait(self.interval)

    def stop(self):
        self.stop_ev.set()
        self.join(timeout=2)

    def peak_kb(self):
        return max((s[1] for s in self.samples), default=0)

    def series(self, max_points=300):
        s = self.samples
        step = max(1, len(s) // max_points)
        return [[round(t, 2), round(kb / 1024.0, 1)] for t, kb in s[::step]]


def mb(kb):
    return None if kb is None else round(kb / 1024.0, 1)


# ---------------------------------------------------------------- main

def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--base", default="http://127.0.0.1:3900")
    ap.add_argument("--key-file", default=os.path.expanduser("~/opdb-dev/api-key"))
    ap.add_argument("--pid", type=int, required=True, help="pid of the server under test")
    ap.add_argument("--label", required=True, help="run name, e.g. A-baseline (also the result file name)")
    ap.add_argument("--idle", type=float, default=90, help="seconds to wait before the idle RSS sample (default 90)")
    ap.add_argument("--concurrency", type=int, default=512, help="client threads (default 512 >= requests: all at once)")
    ap.add_argument("--timeout", type=float, default=600,
                    help="per socket read, seconds (default 600: queued renders must not time out)")
    ap.add_argument("--bucket-size", type=int, default=200,
                    help=f"requests per fake X-Forwarded-For group (default 200, max {BUCKET_BURST}: below the upstream limiter burst)")
    ap.add_argument("--single-bucket", action="store_true",
                    help="no X-Forwarded-For: one rate-limit bucket, like one prod key (expect 429s, INVALID run)")
    ap.add_argument("--rss-abort-mb", type=int, default=3000,
                    help="SIGTERM the server and mark the run INVALID above this RSS (default 3000; 0 = off)")
    ap.add_argument("--out", default=os.path.expanduser("~/opdb-dev/results"))
    ap.add_argument("--titles", default=os.path.join(HERE, "titles.json"))
    ap.add_argument("--titles-limit", type=int, default=None, help="use only the first N titles (smoke tests)")
    ap.add_argument("--episodes-limit", type=int, default=None)
    ap.add_argument("--seed", type=int, default=42)
    ap.add_argument("--nice", type=int, default=19, help="nice the client (default 19)")
    ap.add_argument("--warmup", action="store_true",
                    help="run the warm-up pass instead of the burst (see the module docstring); writes no result file")
    ap.add_argument("--warmup-concurrency", type=int, default=4,
                    help="client threads for --warmup (default 4: low enough not to trip TMDB's rate limit)")
    args = ap.parse_args(argv)

    host = urllib.parse.urlsplit(args.base).hostname or ""
    if host not in [h.strip("[]") for h in LOCAL_HOSTS]:
        raise SystemExit(f"refusing to run: --base host {host!r} is not local")
    if args.concurrency < 1 or args.idle < 0 or args.rss_abort_mb < 0 or args.warmup_concurrency < 1:
        raise SystemExit("bad --concurrency/--idle/--rss-abort-mb/--warmup-concurrency")
    if not 1 <= args.bucket_size <= BUCKET_BURST:
        raise SystemExit(f"--bucket-size must be 1..{BUCKET_BURST}")
    if not re.fullmatch(r"[A-Za-z0-9._+-]+", args.label):
        raise SystemExit("--label may only contain letters, digits and . _ + -")

    env_subset = check_server_env(read_environ(args.pid))
    st0 = read_status(args.pid)
    if st0 is None:
        raise SystemExit(f"process {args.pid} is not running")
    check_pid_listens(args.pid, args.base)
    missing = prod_env_missing(env_subset)
    if missing:
        print("warning: server does not run with the SPEC section 2 prod settings (baseline not comparable): "
              + ", ".join(missing), file=sys.stderr)
    try:
        with open(args.key_file) as f:
            key = f.read().strip()
    except OSError as e:
        raise SystemExit(f"cannot read --key-file ({e.strerror})")
    if not re.fullmatch(r"[A-Za-z0-9_-]+", key or ""):
        raise SystemExit("--key-file does not contain a plausible API key")
    with open(args.titles) as f:
        doc = json.load(f)
    if args.warmup:
        return run_warmup(args, key, doc)
    reqs, n_buckets = build_requests(args.base, key, doc, args.titles_limit, args.episodes_limit, args.seed,
                                     args.bucket_size, args.single_bucket)
    cache_files, db_bytes = cache_state(env_subset, args.pid)

    try:
        os.nice(args.nice)
    except OSError:
        pass
    threading.stack_size(512 * 1024)  # hundreds of client threads: keep the client small
    pressure_before = wait_for_pressure()

    try:
        exe = os.readlink(f"/proc/{args.pid}/exe")
    except OSError:
        exe = None
    try:  # reset VmHWM to the current RSS so a warm pass does not report the cold pass's peak
        with open(f"/proc/{args.pid}/clear_refs", "w") as f:
            f.write("5")
        hwm_reset = True
    except OSError:
        hwm_reset = False
    st_pre = read_status(args.pid) or st0
    sampler = RssSampler(args.pid, abort_mb=args.rss_abort_mb)
    sampler.start()
    t_start = time.perf_counter()
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.concurrency) as ex:
        futs = [ex.submit(one_request, kind, url, xff, args.timeout) for kind, url, xff in reqs]
        results = [fu.result() for fu in futs]
    wall_s = time.perf_counter() - t_start
    st_end = read_status(args.pid)
    burst_peak_kb = sampler.peak_kb()
    server_died = sampler.died or sampler.aborted or st_end is None

    idle_rss = idle_hwm = None
    if not server_died and args.idle > 0:
        # keep sampling so a late spike (or a death) during the idle window is still seen
        deadline = time.monotonic() + args.idle
        while time.monotonic() < deadline and not sampler.died and not sampler.aborted:
            time.sleep(min(1.0, max(0.0, deadline - time.monotonic())))
    sampler.stop()
    st_idle = read_status(args.pid)
    if st_idle is None or sampler.aborted:
        server_died = True
    else:
        idle_rss, idle_hwm = st_idle["rss_kb"], st_idle.get("hwm_kb")

    summ = summarize(results)
    reasons = []
    if sampler.aborted:
        reasons.append(f"aborted: RSS > {args.rss_abort_mb} MB")
    if server_died and not sampler.aborted:
        reasons.append("server died")
    if summ["errors"] > max(2, 0.02 * summ["requests"]):
        reasons.append(f"{summ['errors']} errors")
    if summ["status_counts"].get("429"):
        reasons.append(f"{summ['status_counts']['429']}x 429")
    if any(r["status"] is None for r in results):
        reasons.append("transport errors/timeouts")
    for k in ("poster", "backdrop", "episode"):  # these exist for every committed title
        v = summ["kinds"].get(k)
        if v and v["status_counts"].get("200", 0) < 0.9 * v["n"]:
            reasons.append(f"{k}: only {v['status_counts'].get('200', 0)}/{v['n']} 200")
    out = {
        "label": args.label,
        "when": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
        **{k: summ[k] for k in ("requests", "errors", "status_counts", "p50_ms", "p95_ms", "max_ms")},
        "wall_s": round(wall_s, 2),
        "rss_start_mb": mb(st0["rss_kb"]),
        "peak_rss_mb": mb(burst_peak_kb or None),
        "vmhwm_mb": mb(st_end.get("hwm_kb") if st_end else sampler.hwm_kb or None),
        "vmhwm_reset": hwm_reset,
        "vmhwm_start_mb": mb(st_pre.get("hwm_kb")),
        "idle_rss_mb": mb(idle_rss),
        "idle_vmhwm_mb": mb(idle_hwm),
        "idle_s": args.idle,
        "server_died": server_died,
        "aborted": sampler.aborted,
        "rss_abort_mb": args.rss_abort_mb,
        "valid": not reasons,
        "invalid_reasons": reasons,
        "cache_files_before": cache_files,
        "db_bytes_before": db_bytes,
        "rate_limit_buckets": n_buckets,
        "single_bucket": args.single_bucket,
        "bucket_size": None if args.single_bucket else args.bucket_size,
        "prod_env_complete": not missing,
        "prod_env_missing": missing,
        "timeout_s": args.timeout,
        "kinds": summ["kinds"],
        "total_bytes": summ["total_bytes"],
        "concurrency": args.concurrency,
        "titles": len(doc["titles"][:args.titles_limit]),
        "episodes": len(doc["episodes"][:args.episodes_limit]),
        "memory_pressure_avg10_before": pressure_before,
        "server_exe": exe,
        "env": env_subset,
        "rss_series_mb": sampler.series(),
    }
    os.makedirs(args.out, exist_ok=True)
    jpath = os.path.join(args.out, f"{args.label}.json")
    with open(jpath, "w") as f:
        json.dump(out, f, indent=1)
        f.write("\n")
    with open(os.path.join(args.out, f"{args.label}.md"), "w") as f:
        f.write(render_md(out))
    print(summary_line(out), f"-> {jpath}")
    return 2 if server_died else (3 if reasons else 0)


def run_warmup(args, key, doc):
    """Warm-up pass: low-concurrency requests under cache keys the burst does not use. Prints one line."""
    reqs = build_warmup_requests(args.base, key, doc, args.titles_limit, args.episodes_limit)
    try:
        os.nice(args.nice)
    except OSError:
        pass
    wait_for_pressure()
    sampler = RssSampler(args.pid, abort_mb=args.rss_abort_mb)
    sampler.start()
    t0 = time.perf_counter()
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.warmup_concurrency) as ex:
        results = list(ex.map(lambda a: one_request(a[0], a[1], a[2], args.timeout), reqs))
    wall_s = time.perf_counter() - t0
    sampler.stop()
    st_end = read_status(args.pid)
    died = sampler.died or sampler.aborted or st_end is None
    summ = summarize(results)
    reasons = []
    if sampler.aborted:
        reasons.append(f"aborted: RSS > {args.rss_abort_mb} MB")
    elif died:
        reasons.append("server died")
    if summ["status_counts"].get("429"):
        reasons.append(f"{summ['status_counts']['429']}x 429")
    if any(r["status"] is None for r in results):
        reasons.append("transport errors/timeouts")
    if summ["errors"] > WARMUP_MAX_ERROR_FRACTION * summ["requests"]:
        reasons.append(f"{summ['errors']} errors (more than {int(WARMUP_MAX_ERROR_FRACTION * 100)}%: "
                       f"the id/TMDB caches are not warm)")
    per_kind = {k: v["status_counts"] for k, v in summ["kinds"].items()}
    print(f"{'WARMUP FAILED (' + '; '.join(reasons) + ') ' if reasons else ''}warmup {args.label}: "
          f"{summ['requests']} req at concurrency {args.warmup_concurrency} in {wall_s:.1f} s, "
          f"{summ['errors']} err, statuses {json.dumps(per_kind, sort_keys=True)}, "
          f"VmRSS {mb(st_end['rss_kb']) if st_end else None} MB, VmHWM {mb(st_end.get('hwm_kb')) if st_end else None} MB")
    return 2 if died else (3 if reasons else 0)


def invalid_prefix(o):
    return "" if o.get("valid", True) else f"INVALID ({'; '.join(o.get('invalid_reasons', []))}) "


def summary_line(o):
    return (f"{invalid_prefix(o)}{o['label']} [{'warm' if o.get('cache_files_before') else 'cold'}, "
            f"{o.get('rate_limit_buckets', '?')} bucket(s)]: {o['requests']} req, {o['errors']} err, p50 {o['p50_ms']} ms, p95 {o['p95_ms']} ms, "
            f"max {o['max_ms']} ms, wall {o['wall_s']} s, peak RSS {o['peak_rss_mb']} MB, "
            f"VmHWM {o['vmhwm_mb']} MB, idle RSS {o['idle_rss_mb']} MB"
            + (", SERVER DIED" if o["server_died"] else ""))


def render_md(o):
    lines = [f"# {invalid_prefix(o)}{o['label']}", "",
             f"- cache files before: {o.get('cache_files_before')} ({'warm' if o.get('cache_files_before') else 'cold'}), "
             f"rate-limit buckets: {o.get('rate_limit_buckets')}, VmHWM reset: {o.get('vmhwm_reset')}"
             + ("" if o.get("prod_env_complete", True) else f", NOT prod env: {', '.join(o['prod_env_missing'])}"),
             f"- when: {o['when']}  wall: {o['wall_s']} s  requests: {o['requests']}  errors: {o['errors']}",
             f"- RSS start {o['rss_start_mb']} MB, peak {o['peak_rss_mb']} MB, VmHWM {o['vmhwm_mb']} MB, "
             f"idle ({o['idle_s']} s) {o['idle_rss_mb']} MB" + ("  **SERVER DIED**" if o["server_died"] else ""),
             f"- status counts: {json.dumps(o['status_counts'])}",
             f"- env: {json.dumps(o['env'], sort_keys=True)}", "",
             "| kind | n | p50 ms | p95 ms | max ms | errors | statuses |", "|---|---|---|---|---|---|---|"]
    for k, v in o["kinds"].items():
        lines.append(f"| {k} | {v['n']} | {v['p50_ms']} | {v['p95_ms']} | {v['max_ms']} | {v['errors']} | "
                     f"{json.dumps(v['status_counts'])} |")
    lines.append(f"| all | {o['requests']} | {o['p50_ms']} | {o['p95_ms']} | {o['max_ms']} | {o['errors']} | |")
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    sys.exit(main())
