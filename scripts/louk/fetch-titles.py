#!/usr/bin/env python3
"""Build scripts/louk/titles.json: real TMDB titles + imdb ids for the burst harness.

TMDB only (never OMDb/MDBList). Reads TMDB_API_KEY from the environment; the caller
sources the env file in a subshell. The key is never printed or written anywhere.

  ( set -a; . ~/.config/opdb-dev.env; set +a; unset OMDB_API_KEY MDBLIST_API_KEY;
    python3 -I scripts/louk/fetch-titles.py [--movies 100] [--shows 50] [--episodes 40] )

Output: {"titles":[{"type":"movie|tv","tmdb":int,"imdb":"tt..","name":str}],
         "episodes":[{"imdb":"tt..(series)","season":1,"episode":N}]}
"""
import argparse, json, os, sys, time, urllib.error, urllib.parse, urllib.request

API = "https://api.themoviedb.org/3"
KEY = os.environ.get("TMDB_API_KEY", "")


def get(path, **params):
    """GET a TMDB path. Errors never include the URL (it carries the key)."""
    q = urllib.parse.urlencode({"api_key": KEY, **params})
    for attempt in range(4):
        try:
            with urllib.request.urlopen(f"{API}{path}?{q}", timeout=20) as r:
                return json.load(r)
        except urllib.error.HTTPError as e:
            if e.code == 429 or e.code >= 500:
                time.sleep(1.5 * (attempt + 1))
                continue
            if e.code == 404:
                return None
            raise SystemExit(f"TMDB {path}: HTTP {e.code}")
        except (urllib.error.URLError, TimeoutError):
            time.sleep(1.5 * (attempt + 1))
    raise SystemExit(f"TMDB {path}: gave up")


def candidates():
    """Ordered (type, tmdb_id, name) candidates: trending first, then popular, then top rated."""
    seen = set()
    sources = [("/trending/all/week", None)]
    for page in (1, 2, 3, 4, 5):
        sources += [("/movie/popular", "movie"), ("/tv/popular", "tv"),
                    ("/movie/top_rated", "movie"), ("/tv/top_rated", "tv")]
        yield from _pages(sources, seen, page)
        sources = []


def _pages(sources, seen, page):
    for path, forced in sources:
        data = get(path, page=page, language="en-US") or {}
        for r in data.get("results", []):
            typ = forced or r.get("media_type")
            if typ not in ("movie", "tv"):
                continue
            key = (typ, r["id"])
            if key in seen:
                continue
            seen.add(key)
            yield typ, r["id"], r.get("title") or r.get("name") or ""


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--movies", type=int, default=100)
    ap.add_argument("--shows", type=int, default=50)
    ap.add_argument("--episodes", type=int, default=40)
    ap.add_argument("--out", default=os.path.join(os.path.dirname(os.path.abspath(__file__)), "titles.json"))
    a = ap.parse_args()
    if not KEY:
        raise SystemExit("TMDB_API_KEY not set (source ~/.config/opdb-dev.env in a subshell)")

    want = {"movie": a.movies, "tv": a.shows}
    titles = []
    for typ, tid, name in candidates():
        if sum(1 for t in titles if t["type"] == typ) >= want[typ]:
            if all(sum(1 for t in titles if t["type"] == k) >= n for k, n in want.items()):
                break
            continue
        ext = get(f"/{typ}/{tid}/external_ids") or {}
        imdb = ext.get("imdb_id") or ""
        if not imdb.startswith("tt"):
            continue  # skip titles without an imdb id
        titles.append({"type": typ, "tmdb": tid, "imdb": imdb, "name": name})
        time.sleep(0.03)
    for k, n in want.items():
        have = sum(1 for t in titles if t["type"] == k)
        if have < n:
            raise SystemExit(f"only found {have}/{n} {k} titles")

    # Episodes: S1E1..E4 of the first 10 shows that really have them.
    shows = [t for t in titles if t["type"] == "tv"]
    per_show = 4
    episodes = []
    for s in shows:
        if len(episodes) >= a.episodes:
            break
        season = get(f"/tv/{s['tmdb']}/season/1", language="en-US")
        have = {e["episode_number"] for e in (season or {}).get("episodes", [])}
        if not all(n in have for n in range(1, per_show + 1)):
            continue
        for n in range(1, per_show + 1):
            episodes.append({"imdb": s["imdb"], "season": 1, "episode": n})
        time.sleep(0.03)
    episodes = episodes[: a.episodes]
    if len(episodes) < a.episodes:
        raise SystemExit(f"only {len(episodes)}/{a.episodes} episodes")

    with open(a.out, "w") as f:
        json.dump({"titles": titles, "episodes": episodes}, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print(f"wrote {a.out}: {len(titles)} titles "
          f"({sum(t['type']=='movie' for t in titles)} movies, {sum(t['type']=='tv' for t in titles)} shows), "
          f"{len(episodes)} episodes")


if __name__ == "__main__":
    main()
