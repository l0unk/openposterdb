#!/usr/bin/env python3
"""Badge-look gallery for the fork (task P4.2): fetch, shrink, and lay out previews.

For a fixed set of real titles x every look x every image kind, this fetches the ADMIN preview
endpoints of a running instance (they draw SAMPLE ratings on the title's REAL TMDB artwork through
`?art=`, so nothing here calls OMDb/MDBList/Trakt), shrinks the results with ffmpeg and writes

    docs/louk/badges/*.jpg|png        the images (about 5 MB in all)
    docs/louk/badges/manifest.json    what was rendered, with the URLs and byte sizes
    docs/louk/badges.html             the self-contained gallery page (inline CSS, no JS, no network)

Standard library only (plus the `ffmpeg` binary for shrinking; without it the raw bytes are stored).

Typical run against a local dev instance (start it with scripts/louk/dev-instance.sh first):

    OPDB_DEV_HOME=~/opdb-dev/gallery scripts/louk/gallery.py \
        --base http://127.0.0.1:3900 --password-file ~/opdb-dev/gallery/admin-password

Credentials: `--token` / $OPDB_ADMIN_TOKEN (a bearer token), or a login with `--user`
($OPDB_ADMIN_USER, default louk-dev) and the password from `--password-file` or
$OPDB_ADMIN_PASSWORD. Nothing is hardcoded and nothing secret is printed.

After a new binary: restart it with a wiped cache (`dev-instance.sh wipe-cache`; previews are cached
per URL), then re-run this script. `--html-only` rebuilds the page from manifest.json.
"""
import argparse
import hashlib
import html
import json
import os
import shutil
import subprocess
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))

# (option value, name in the form, short note). `d` is upstream's default, for comparison.
LOOKS = [
    ("d", "Default", "upstream dark plate"),
    ("s", "Score colour", "white figure on a dark plate, colour meter by score"),
    ("m", "Mono", "grey glyph icons, no colour at all"),
    ("a", "One score (average)", "one disc with the mean of the shown sources"),
]

# Six real titles (ids from scripts/louk/titles.json) chosen to stress the looks.
# art = the `?art=` value; hero = shown as a full 1280x720 hero in the page.
TITLES = [
    dict(slug="godfather-2", art="movie-240", tmdb=240, name="The Godfather Part II",
         why="very dark poster", logo=True, episode=False, hero=False),
    dict(slug="kalki", art="movie-801688", tmdb=801688, name="Kalki 2898-AD",
         why="busy, high-detail poster; pale sky at the top right of the backdrop",
         logo=True, episode=False, hero=True),
    dict(slug="chainsaw-man", art="movie-1218925", tmdb=1218925, name="Chainsaw Man: Reze Arc",
         why="animated; white top-right area on the backdrop", logo=True, episode=False, hero=False),
    dict(slug="modern-family", art="series-1421", tmdb=1421, name="Modern Family",
         why="very bright pastel poster; white top-right on the backdrop",
         logo=True, episode=True, hero=True),
    dict(slug="arcane", art="series-94605", tmdb=94605, name="Arcane",
         why="animated, busy, saturated", logo=True, episode=True, hero=False),
    dict(slug="breaking-bad", art="series-1396", tmdb=1396, name="Breaking Bad",
         why="mid-tone, saturated green", logo=True, episode=True, hero=False),
]

# Output budgets. Posters keep the server's 580 px; backdrops are stored at 1280 px (the TV app's real
# viewport width, see safe-areas.md) and still fit the 6 MB directory cap; logos/stills shrink.
POSTER_MAX_BYTES = 75_000
BACKDROP_MAX_BYTES = 55_000
BACKDROP_WIDTH = 1280
EPISODE_MAX_BYTES = 38_000
EPISODE_WIDTH = 640
LOGO_WIDTH = 360

BACKDROP_VARIANTS = [
    ("col", "default right-edge column", "&position=tr"),
    ("row", "horizontal top-right row", "&position=tr&badge_direction=h"),
]


def die(msg):
    sys.exit("gallery: " + msg)


def get_token(args):
    token = args.token or os.environ.get("OPDB_ADMIN_TOKEN")
    if token:
        return token
    pw = os.environ.get("OPDB_ADMIN_PASSWORD")
    if args.password_file:
        with open(os.path.expanduser(args.password_file)) as f:
            pw = f.read().strip()
    if not pw:
        die("no credentials: pass --token, or --password-file, or set OPDB_ADMIN_TOKEN / OPDB_ADMIN_PASSWORD")
    body = json.dumps({"username": args.user, "password": pw}).encode()
    req = urllib.request.Request(args.base + "/api/auth/login", data=body,
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return json.load(r)["token"]
    except (urllib.error.URLError, KeyError) as e:
        die("login failed: %s" % type(e).__name__)


def fetch(base, token, path, retries=2):
    req = urllib.request.Request(base + path, headers={"Authorization": "Bearer " + token})
    last = None
    for _ in range(retries + 1):
        try:
            with urllib.request.urlopen(req, timeout=90) as r:
                return r.status, r.read(), r.headers.get("content-type", "")
        except urllib.error.HTTPError as e:
            return e.code, e.read(), e.headers.get("content-type", "")
        except urllib.error.URLError as e:
            last = e
    die("request failed: %s" % last)


def have_ffmpeg():
    return shutil.which("ffmpeg") is not None


def shrink(raw, kind, ext, max_bytes, width):
    """Return (bytes, ext). JPEG: lowest-quality-loss q that fits max_bytes. PNG: scale only."""
    if not have_ffmpeg():
        return raw, ext
    with tempfile.TemporaryDirectory() as td:
        src = os.path.join(td, "in.bin")
        with open(src, "wb") as f:
            f.write(raw)
        scale = ["-vf", "scale='min(%d,iw)':-2:flags=lanczos" % width] if width else []
        if ext == "png":
            out = os.path.join(td, "out.png")
            cmd = ["nice", "-n", "19", "ffmpeg", "-v", "error", "-y", "-i", src] + scale + \
                  ["-compression_level", "9", out]
            if subprocess.run(cmd).returncode != 0:
                return raw, ext
            with open(out, "rb") as f:
                return f.read(), "png"
        best = None
        for q in (2, 3, 4, 5, 6, 8, 10, 13):
            out = os.path.join(td, "out.jpg")
            cmd = ["nice", "-n", "19", "ffmpeg", "-v", "error", "-y", "-i", src] + scale + \
                  ["-q:v", str(q), "-pix_fmt", "yuvj420p", out]
            if subprocess.run(cmd).returncode != 0:
                return raw, ext
            with open(out, "rb") as f:
                best = f.read()
            if len(best) <= max_bytes:
                break
        return best, "jpg"


def render_all(args):
    token = get_token(args)
    out = os.path.abspath(args.out)
    os.makedirs(out, exist_ok=True)
    if args.raw_dir:
        os.makedirs(args.raw_dir, exist_ok=True)
    titles_json = json.load(open(os.path.join(HERE, "titles.json")))
    known = {t["tmdb"] for t in titles_json["titles"]}
    for t in TITLES:
        if t["tmdb"] not in known:
            die("title %s (%s) is not in titles.json" % (t["name"], t["tmdb"]))
    manifest = {"base": "preview endpoints, sample ratings on real TMDB art", "looks": [
        dict(value=v, name=n, note=d) for v, n, d in LOOKS], "titles": [], "images": {}}
    seen = {}  # sha256 of the raw render -> stored file name (identical renders share one file)

    def one(t, kind, variant, look, path, ext, budget, width):
        key = "%s/%s/%s/%s" % (t["slug"], kind, variant, look)
        status, body, ctype = fetch(args.base, token, path)
        if status != 200 or not ctype.startswith("image/"):
            print("  skip %-44s HTTP %s (%s)" % (key, status, body[:80].decode("utf8", "replace")), file=sys.stderr)
            return
        digest = hashlib.sha256(body).hexdigest()
        if digest in seen:
            manifest["images"][key] = dict(file=seen[digest], url=path, shared=True)
            return
        data, real_ext = shrink(body, kind, ext, budget, width)
        name = "%s-%s%s-%s.%s" % (t["slug"], kind, "" if variant == "-" else "-" + variant, look, real_ext)
        with open(os.path.join(out, name), "wb") as f:
            f.write(data)
        if args.raw_dir:
            with open(os.path.join(args.raw_dir, name.rsplit(".", 1)[0] + "." + ("png" if ctype.endswith("png") else "jpg")), "wb") as f:
                f.write(body)
        seen[digest] = name
        manifest["images"][key] = dict(file=name, url=path, bytes=len(data), raw_bytes=len(body))
        print("  %-44s %7d B  %s" % (key, len(data), name))

    for t in TITLES:
        print(t["name"])
        manifest["titles"].append({k: t[k] for k in ("slug", "name", "why", "tmdb", "art", "logo", "episode", "hero")})
        art = urllib.parse.quote(t["art"])
        for look, _, _ in LOOKS:
            bg = "&badge_background=" + look
            one(t, "poster", "-", look, "/api/admin/preview/poster?art=%s%s" % (art, bg), "jpg", POSTER_MAX_BYTES, 0)
            for variant, _, extra in BACKDROP_VARIANTS:
                one(t, "backdrop", variant, look,
                    "/api/admin/preview/backdrop?art=%s%s&imageSize=medium%s" % (art, bg, extra),
                    "jpg", BACKDROP_MAX_BYTES, BACKDROP_WIDTH)
            if t["episode"]:
                one(t, "episode", "-", look,
                    "/api/admin/preview/episode?art=%s&season=1&episode=1%s" % (art, bg),
                    "jpg", EPISODE_MAX_BYTES, EPISODE_WIDTH)
            if t["logo"]:
                one(t, "logo", "-", look, "/api/admin/preview/logo?art=%s%s" % (art, bg), "png", 0, LOGO_WIDTH)
    with open(os.path.join(out, "manifest.json"), "w") as f:
        json.dump(manifest, f, indent=1)
    total = sum(os.path.getsize(os.path.join(out, n)) for n in os.listdir(out))
    print("directory total: %.2f MB (%d files)" % (total / 1e6, len(os.listdir(out))))
    return manifest


# ---------------------------------------------------------------- the page

CSS = """
*{box-sizing:border-box}
html{background:#0b0b0f}
body{margin:0;min-width:1920px;background:#0b0b0f;color:#e8e8ee;font:14px/1.4 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif}
h1{font-size:28px;margin:0 0 6px}h2{font-size:22px;margin:0 0 4px}h3{font-size:16px;margin:0}
p.note{color:#9a9aa8;margin:0 0 16px;max-width:1100px}
code{color:#c9c9d6;background:#17171d;padding:1px 5px;border-radius:4px}
section{padding:36px 60px 28px;border-top:1px solid #1d1d25}
.lab{display:flex;align-items:baseline;gap:12px;margin:0 0 10px}
.lab small{color:#8a8a98}
.legend{display:grid;grid-template-columns:repeat(4,1fr);gap:16px;max-width:1500px}
.legend div{background:#13131a;border-radius:8px;padding:12px 14px}
/* TV row: 6 poster cards of 240x360 CSS px on a 1920 px layout */
.tvrow{width:1800px;display:flex;justify-content:space-between;margin-bottom:30px}
.card{width:240px}
.card .img{width:240px;height:360px;border-radius:8px;overflow:hidden;background:#15151b}
.card img{display:block;width:100%;height:100%;object-fit:cover}
.card figcaption{font-size:12px;color:#8a8a98;margin-top:6px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
figure{margin:0}
/* Phone grid: 3 columns in a 412 px wide screen */
.phones{display:flex;gap:40px;align-items:flex-start}
.phone{width:412px;height:915px;background:#070707;border:1px solid #1d1d25;padding:56px 12px 12px}
.phone h3{padding:0 4px 10px;font-size:13px;color:#b8b8c6}
.pgrid{display:grid;grid-template-columns:repeat(3,124px);gap:8px 8px}
.pgrid .c{width:124px;height:186px;border-radius:6px;overflow:hidden;background:#15151b}
.pgrid img{display:block;width:100%;height:100%;object-fit:cover}
/* Home hero as the app draws it on a 1280x720 TV: 1280 x 576 (--hero-height = max(36rem, 53vh)), with the app's shades (safe-areas.md: side shade 1 -> .7 at 35 % -> 0; bottom shade
   2/3 high, .6 in the middle, 1 at the bottom; image object-fit:cover, object-position:top). */
.hero{position:relative;width:1280px;height:576px;overflow:hidden;background:#070707;margin:0 0 6px}
.hero img.bd{position:absolute;inset:0;width:100%;height:100%;object-fit:cover;object-position:top}
.hero .side{position:absolute;inset:0;background:linear-gradient(to right,rgba(7,7,7,1),rgba(7,7,7,.7) 35%,rgba(7,7,7,0))}
.hero .bot{position:absolute;left:0;right:0;bottom:0;height:66.667%;background:linear-gradient(to top,rgba(7,7,7,1),rgba(7,7,7,.6),rgba(7,7,7,0))}
.hero .copy{position:absolute;left:48px;bottom:56px;width:620px}
.hero .copy b{display:block;font-size:44px;line-height:1.1;margin-bottom:14px}
.hero .copy i{display:block;height:9px;border-radius:5px;background:#3a3a46;margin-bottom:9px}
.hero .copy u{display:inline-block;margin-top:12px;padding:10px 28px;border-radius:8px;background:#e8e8ee;color:#0b0b0f;font-weight:600;text-decoration:none}
.heros{display:flex;flex-wrap:wrap;gap:36px 30px}
.heros>div{width:1280px}
.mini{display:flex;flex-wrap:wrap;gap:18px}
.mini figure{width:600px}
.mini .h{position:relative;width:600px;height:270px;overflow:hidden;background:#070707}
.mini .h img{position:absolute;inset:0;width:100%;height:100%;object-fit:cover;object-position:top}
.mini .h .side{position:absolute;inset:0;background:linear-gradient(to right,rgba(7,7,7,1),rgba(7,7,7,.7) 35%,rgba(7,7,7,0))}
.mini .h .bot{position:absolute;left:0;right:0;bottom:0;height:66.667%;background:linear-gradient(to top,rgba(7,7,7,1),rgba(7,7,7,.6),rgba(7,7,7,0))}
.mini figcaption,.hero-cap{font-size:12px;color:#8a8a98;margin-top:5px}
.stills{display:flex;flex-wrap:wrap;gap:24px}
.still{width:400px}
.still img{display:block;width:400px;border-radius:6px;background:#15151b}
.logos{display:flex;flex-wrap:wrap;gap:24px}
.logo{width:230px;background:#101016;border-radius:8px;padding:10px}
.logo img{display:block;width:210px;margin:0 auto}
.lookblock{margin-bottom:34px}
"""


def img(manifest, slug, kind, variant, look):
    key = "%s/%s/%s/%s" % (slug, kind, variant, look)
    e = manifest["images"].get(key)
    return e["file"] if e else None


def build_html(manifest, out_html):
    looks = [(l["value"], l["name"], l["note"]) for l in manifest["looks"]]
    titles = manifest["titles"]
    esc = html.escape

    def label(v, n):
        return '<div class="lab"><h3>%s</h3><small><code>badge_background=%s</code></small></div>' % (esc(n), v)

    h = ['<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width">',
         "<title>OpenPosterDB badge looks: gallery</title><style>%s</style></head><body>" % CSS]
    h.append('<section id="intro"><h1>Badge looks gallery</h1><p class="note">Generated by '
             "<code>scripts/louk/gallery.py</code> from the admin preview endpoints: sample ratings (MAL 8.12, IMDb 6.4, "
             "Letterboxd 2.1; the average disc shows 6.2) drawn on the real TMDB artwork of each title, so no rating "
             "service is called. Images are re-compressed for the repo (posters at the server's 580 px, backdrops at "
             "1280 px) so expect slightly softer text than a live render. Dark layout colour as in the AIOStreams app.</p>"
             '<div class="legend">')
    for v, n, d in looks:
        h.append("<div><h3>%s</h3><code>badge_background=%s</code><p class=\"note\" style=\"margin:6px 0 0\">%s</p></div>" % (esc(n), v, esc(d)))
    h.append("</div><p class=\"note\" style=\"margin-top:14px\">Titles: " +
             "; ".join("<b>%s</b> (%s)" % (esc(t["name"]), esc(t["why"])) for t in titles) + ".</p></section>")

    # (a) TV row
    h.append('<section id="tv-row"><h2>TV row</h2><p class="note">Six poster cards per row, 240 x 360 CSS px, on a '
             "1920 px wide layout, one row per look. Images are 580 px wide, so the browser scales them by 0.41.</p>")
    for v, n, _ in looks:
        h.append('<div class="lookblock" id="tv-%s">%s<div class="tvrow">' % (v, label(v, n)))
        for t in titles:
            f = img(manifest, t["slug"], "poster", "-", v)
            if f:
                h.append('<figure class="card"><div class="img"><img src="badges/%s" alt="%s"></div><figcaption>%s</figcaption></figure>' % (f, esc(t["name"]), esc(t["name"])))
        h.append("</div></div>")
    h.append("</section>")

    # (b) phone grid
    h.append('<section id="phone-grid"><h2>Phone grid</h2><p class="note">Three columns in a 412 px wide screen: cards '
             "124 x 186 px (12 px padding, 8 px gap). Badges are about 9 px tall here.</p><div class=\"phones\">")
    for v, n, _ in looks:
        h.append('<div class="phone" id="phone-%s">%s<div class="pgrid">' % (v, label(v, n)))
        for t in titles:
            f = img(manifest, t["slug"], "poster", "-", v)
            if f:
                h.append('<div class="c"><img src="badges/%s" alt="%s"></div>' % (f, esc(t["name"])))
        h.append("</div></div>")
    h.append("</div></section>")

    # (c) heroes
    def hero(t, v, variant, vlabel, mini=False):
        f = img(manifest, t["slug"], "backdrop", variant, v)
        if not f:
            return ""
        if mini:
            return ('<figure><div class="h"><img src="badges/%s" alt=""><div class="side"></div><div class="bot"></div></div>'
                    "<figcaption>%s: %s</figcaption></figure>" % (f, esc(t["name"]), vlabel))
        return ('<div id="hero-%s-%s-%s" data-look="%s"><div class="hero"><img class="bd" src="badges/%s" alt="">'
                '<div class="side"></div><div class="bot"></div><div class="copy"><b>%s</b><i style="width:90%%"></i>'
                '<i style="width:70%%"></i><u>Play</u></div></div><div class="hero-cap">%s &middot; %s</div></div>'
                % (t["slug"], variant, v, v, f, esc(t["name"]), esc(t["name"]), vlabel))

    h.append('<section id="backdrops"><h2>Backdrops as the app\'s Home hero (1280 x 576)</h2><p class="note">With the app\'s shades '
             "(left-to-right dark side shade, dark bottom shade; <code>safe-areas.md</code>). The hero box is 1280 x 576 "
             "(36 rem, the height on a 1280 x 720 TV), image cover-fitted and top-aligned, so the bottom 144 px of the picture "
             "are cropped; the app dims the badges together with the picture, so a grey figure low in the column is the "
             "shade, not the render. On a title's own page the app removes both shades. Rendered at 1920 px, shown at 1280 px. Per look: the default right-edge column, and the horizontal top-right row "
             "(<code>badge_direction=h</code>) that the safe-areas doc recommends on TV. The disc look has no row: the "
             "two images are identical.</p>")
    for v, n, _ in looks:
        h.append('<div class="lookblock" id="bd-%s">%s<div class="heros">' % (v, label(v, n)))
        for t in titles:
            if not t["hero"]:
                continue
            for variant, vl in (("col", "column (default)"), ("row", "row (badge_direction=h)")):
                h.append(hero(t, v, variant, "%s, %s" % (n, vl)))
        h.append("</div></div>")
    h.append('<h2 style="margin-top:30px">All six titles, both variants (600 px wide, same shades)</h2><div id="bd-all">')
    for v, n, _ in looks:
        h.append('<div class="lookblock">%s<div class="mini">' % label(v, n))
        for t in titles:
            for variant, vl in (("col", "column"), ("row", "row")):
                h.append(hero(t, v, variant, vl, mini=True))
        h.append("</div></div>")
    h.append("</div></section>")

    # (d) stills and logos
    h.append('<section id="stills"><h2>Episode stills</h2><p class="note">Season 1, episode 1 of the three series, 640 px '
             "wide in the repo, shown at 400 px.</p>")
    for v, n, _ in looks:
        h.append('<div class="lookblock">%s<div class="stills">' % label(v, n))
        for t in titles:
            f = img(manifest, t["slug"], "episode", "-", v)
            if f:
                h.append('<figure class="still"><img src="badges/%s" alt=""><figcaption>%s S01E01</figcaption></figure>' % (f, esc(t["name"])))
        h.append("</div></div>")
    h.append('</section><section id="logos"><h2>Logos</h2><p class="note">The logo preview (PNG with transparent '
             "background), 360 px wide.</p>")
    for v, n, _ in looks:
        h.append('<div class="lookblock">%s<div class="logos">' % label(v, n))
        for t in titles:
            f = img(manifest, t["slug"], "logo", "-", v)
            if f:
                h.append('<figure class="logo"><img src="badges/%s" alt=""><figcaption>%s</figcaption></figure>' % (f, esc(t["name"])))
        h.append("</div></div>")
    h.append("</section></body></html>")
    with open(out_html, "w") as f:
        f.write("\n".join(h))
    print("wrote", out_html)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--base", default=os.environ.get("OPDB_BASE", "http://127.0.0.1:3900"))
    ap.add_argument("--token")
    ap.add_argument("--user", default=os.environ.get("OPDB_ADMIN_USER", "louk-dev"))
    ap.add_argument("--password-file")
    ap.add_argument("--out", default=os.path.join(ROOT, "docs", "louk", "badges"))
    ap.add_argument("--html", default=os.path.join(ROOT, "docs", "louk", "badges.html"))
    ap.add_argument("--raw-dir", help="also keep the unshrunk server output here (not for the repo)")
    ap.add_argument("--html-only", action="store_true", help="rebuild the page from manifest.json")
    args = ap.parse_args()
    args.base = args.base.rstrip("/")
    if args.html_only:
        manifest = json.load(open(os.path.join(args.out, "manifest.json")))
    else:
        manifest = render_all(args)
    build_html(manifest, args.html)


if __name__ == "__main__":
    main()
