#!/usr/bin/env python3
"""Catalog-burst load harness for the OpenPosterDB fork (memory measurements).

Fires, all at once, what a client does when it opens a catalog: per title a poster, a
backdrop (`?imageSize=large`) and a logo, plus episode stills, using Louk's exact URL patterns:

  /{key}/imdb/poster-default/{imdb}.jpg
  /{key}/tmdb/backdrop-default/{type}-{tmdb}.jpg?imageSize=large
  /{key}/tmdb/logo-default/{type}-{tmdb}.png
  /{key}/imdb/episode-default/episode-{imdb}-S{season}E{episode}.jpg

While the burst runs it samples the server's VmRSS/VmHWM from /proc/<pid>/status every 200 ms,
then waits --idle seconds and samples again. One JSON (and a small .md) file per run goes to
--out so variants can be compared (see results-table.py).

Safety (the harness refuses to run otherwise):
  * the server's environment (/proc/<pid>/environ) must have RATINGS_DISABLED=true and must NOT
    have OMDB_API_KEY, MDBLIST_API_KEY or TRAKT_CLIENT_ID (no quota is ever spent);
  * --base must point at the local machine (127.0.0.1 / localhost / ::1);
  * waits while the dev slice memory pressure (PSI full avg10) is above 20.

stdlib only. Run as:  python3 -I scripts/louk/burst.py --label A-baseline --pid $(cat ~/opdb-dev/pid)
The API key is read from --key-file and never printed or stored in the result.
"""
import argparse
import concurrent.futures
import json
import os
import random
import re
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
)
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

def build_requests(base, key, titles_doc, n_titles=None, n_episodes=None, seed=42):
    """Return a list of (kind, url), shuffled with a fixed seed. The URL carries the key: never log it."""
    base = base.rstrip("/")
    titles = titles_doc["titles"][:n_titles]
    episodes = titles_doc["episodes"][:n_episodes]
    reqs = []
    for t in titles:
        reqs.append(("poster", f"{base}/{key}/imdb/poster-default/{t['imdb']}.jpg"))
        reqs.append(("backdrop", f"{base}/{key}/tmdb/backdrop-default/{t['type']}-{t['tmdb']}.jpg?imageSize=large"))
        reqs.append(("logo", f"{base}/{key}/tmdb/logo-default/{t['type']}-{t['tmdb']}.png"))
    for e in episodes:
        reqs.append(("episode", f"{base}/{key}/imdb/episode-default/episode-{e['imdb']}-S{e['season']}E{e['episode']}.jpg"))
    random.Random(seed).shuffle(reqs)
    return reqs


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *a, **k):
        return None


def one_request(kind, url, timeout):
    """Fetch one URL. Returns dict(kind, status|None, ms, bytes, err). Never includes the URL."""
    opener = urllib.request.build_opener(NoRedirect)
    t0 = time.perf_counter()
    status, nbytes, err = None, 0, None
    try:
        with opener.open(url, timeout=timeout) as r:
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
    ms = sorted(r["ms"] for r in results)
    if not ms:
        return {"n": 0, "p50_ms": None, "p95_ms": None, "max_ms": None}
    return {"n": len(ms), "p50_ms": round(percentile(ms, 50), 1),
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
            per_kind[k] = {**latency_stats(sub), "status_counts": status_counts(sub),
                           "errors": sum(is_error(r) for r in sub),
                           "mean_bytes": round(statistics.fmean(r["bytes"] for r in sub))}
    return {"requests": len(results), "errors": sum(is_error(r) for r in results),
            "status_counts": status_counts(results), **{k: v for k, v in latency_stats(results).items() if k != "n"},
            "kinds": per_kind, "total_bytes": sum(r["bytes"] for r in results)}


# ---------------------------------------------------------------- RSS sampler

class RssSampler(threading.Thread):
    def __init__(self, pid, interval=0.2):
        super().__init__(daemon=True)
        self.pid, self.interval = pid, interval
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
    ap.add_argument("--timeout", type=float, default=60)
    ap.add_argument("--out", default=os.path.expanduser("~/opdb-dev/results"))
    ap.add_argument("--titles", default=os.path.join(HERE, "titles.json"))
    ap.add_argument("--titles-limit", type=int, default=None, help="use only the first N titles (smoke tests)")
    ap.add_argument("--episodes-limit", type=int, default=None)
    ap.add_argument("--seed", type=int, default=42)
    ap.add_argument("--nice", type=int, default=19, help="nice the client (default 19)")
    args = ap.parse_args(argv)

    host = urllib.parse.urlsplit(args.base).hostname or ""
    if host not in [h.strip("[]") for h in LOCAL_HOSTS]:
        raise SystemExit(f"refusing to run: --base host {host!r} is not local")
    if args.concurrency < 1 or args.idle < 0:
        raise SystemExit("bad --concurrency/--idle")
    if not re.fullmatch(r"[A-Za-z0-9._+-]+", args.label):
        raise SystemExit("--label may only contain letters, digits and . _ + -")

    env_subset = check_server_env(read_environ(args.pid))
    st0 = read_status(args.pid)
    if st0 is None:
        raise SystemExit(f"process {args.pid} is not running")
    try:
        with open(args.key_file) as f:
            key = f.read().strip()
    except OSError as e:
        raise SystemExit(f"cannot read --key-file ({e.strerror})")
    if not re.fullmatch(r"[A-Za-z0-9_-]+", key or ""):
        raise SystemExit("--key-file does not contain a plausible API key")
    with open(args.titles) as f:
        doc = json.load(f)
    reqs = build_requests(args.base, key, doc, args.titles_limit, args.episodes_limit, args.seed)

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
    sampler = RssSampler(args.pid)
    sampler.start()
    t_start = time.perf_counter()
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.concurrency) as ex:
        futs = [ex.submit(one_request, kind, url, args.timeout) for kind, url in reqs]
        results = [fu.result() for fu in futs]
    wall_s = time.perf_counter() - t_start
    st_end = read_status(args.pid)
    burst_peak_kb = sampler.peak_kb()
    server_died = sampler.died or st_end is None

    idle_rss = idle_hwm = None
    if not server_died and args.idle > 0:
        # keep sampling so a late spike (or a death) during the idle window is still seen
        deadline = time.monotonic() + args.idle
        while time.monotonic() < deadline and not sampler.died:
            time.sleep(min(1.0, max(0.0, deadline - time.monotonic())))
    sampler.stop()
    st_idle = read_status(args.pid)
    if st_idle is None:
        server_died = True
    else:
        idle_rss, idle_hwm = st_idle["rss_kb"], st_idle.get("hwm_kb")

    summ = summarize(results)
    out = {
        "label": args.label,
        "when": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
        **{k: summ[k] for k in ("requests", "errors", "status_counts", "p50_ms", "p95_ms", "max_ms")},
        "wall_s": round(wall_s, 2),
        "rss_start_mb": mb(st0["rss_kb"]),
        "peak_rss_mb": mb(burst_peak_kb or None),
        "vmhwm_mb": mb(st_end.get("hwm_kb") if st_end else sampler.hwm_kb or None),
        "idle_rss_mb": mb(idle_rss),
        "idle_vmhwm_mb": mb(idle_hwm),
        "idle_s": args.idle,
        "server_died": server_died,
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
    return 2 if server_died else 0


def summary_line(o):
    return (f"{o['label']}: {o['requests']} req, {o['errors']} err, p50 {o['p50_ms']} ms, p95 {o['p95_ms']} ms, "
            f"max {o['max_ms']} ms, wall {o['wall_s']} s, peak RSS {o['peak_rss_mb']} MB, "
            f"VmHWM {o['vmhwm_mb']} MB, idle RSS {o['idle_rss_mb']} MB"
            + (", SERVER DIED" if o["server_died"] else ""))


def render_md(o):
    lines = [f"# {o['label']}", "",
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
