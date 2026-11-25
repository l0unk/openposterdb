#!/usr/bin/env python3
"""Latency probe (P6): sequential requests per image kind against the local dev instance.

usage: speed-probe.py <label> <titles.json> <out.jsonl> [--fast]
  --fast  append ratings_limit=0 (same cache key with ratings disabled; takes the
          production fast path that skips the id resolve on a cache hit)
The API key is read from $OPDB_DEV_HOME/api-key and never printed.
"""
import http.client, json, os, sys, time, urllib.parse

label, titles_path, out_path = sys.argv[1:4]
fast = "--fast" in sys.argv
home = os.environ.get("OPDB_DEV_HOME", os.path.expanduser("~/opdb-dev"))
key = open(os.path.join(home, "api-key")).read().strip()
d = json.load(open(titles_path))
movies = [t for t in d["titles"] if t["type"] == "movie"][:12]
tv = [t for t in d["titles"] if t["type"] == "tv"][:8]
titles = movies + tv
eps, seen = [], {}
for e in d["episodes"]:
    if seen.get(e["imdb"], 0) < 2:
        seen[e["imdb"]] = seen.get(e["imdb"], 0) + 1
        eps.append(e)
eps = eps[:20]

reqs = []
for t in titles:
    typ = "movie" if t["type"] == "movie" else "series"
    reqs.append(("poster", t["name"], f"/imdb/poster-default/{t['imdb']}.jpg", {}))
    reqs.append(("backdrop", t["name"], f"/tmdb/backdrop-default/{typ}-{t['tmdb']}.jpg", {"imageSize": "large"}))
    reqs.append(("logo", t["name"], f"/tmdb/logo-default/{typ}-{t['tmdb']}.png", {}))
for e in eps:
    reqs.append(("episode", f"{e['imdb']} S{e['season']}E{e['episode']}",
                 f"/imdb/episode-default/episode-{e['imdb']}-S{e['season']}E{e['episode']}.jpg", {}))

KEEP = {"content-type", "content-length", "cache-control", "etag", "last-modified", "vary",
        "accept-ranges", "content-range", "date", "age", "expires"}
with open(out_path, "a") as out:
    for kind, name, path, q in reqs:
        q = dict(q)
        if fast:
            q["ratings_limit"] = "0"
        url = "/" + key + path + ("?" + urllib.parse.urlencode(q) if q else "")
        c = http.client.HTTPConnection("127.0.0.1", int(os.environ.get("OPDB_DEV_PORT", "3900")), timeout=120)
        t0 = time.perf_counter()
        try:
            c.request("GET", url)
            r = c.getresponse()
            t1 = time.perf_counter()
            body = r.read()
            t2 = time.perf_counter()
            rec = {"label": label, "kind": kind, "name": name, "status": r.status,
                   "ttfb_ms": round((t1 - t0) * 1000, 2), "total_ms": round((t2 - t0) * 1000, 2),
                   "bytes": len(body), "headers": {k.lower(): v for k, v in r.getheaders() if k.lower() in KEEP}}
        except Exception as ex:  # noqa: BLE001
            rec = {"label": label, "kind": kind, "name": name, "status": -1, "error": type(ex).__name__,
                   "ttfb_ms": None, "total_ms": round((time.perf_counter() - t0) * 1000, 2), "bytes": 0}
        finally:
            c.close()
        out.write(json.dumps(rec) + "\n")
        print(f"{label:8} {kind:8} {rec['status']:4} {rec['total_ms']:9.1f} ms {rec['bytes']:8} B  {name[:40]}", flush=True)
