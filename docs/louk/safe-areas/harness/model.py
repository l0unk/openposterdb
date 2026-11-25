# Pure-arithmetic model of the shade layers on the AIOStreams hero / title page / corner backdrops.
# Used (a) to validate the Chromium measurements and (b) to compute the numbers when the dev box was
# too busy to run Chromium. Every constant is a rule quoted in docs/louk/safe-areas.md.
# Usage: python3 -I model.py            prints the validation table and the footprint table (JSON on stdout
#        with --json). Shade = 1 - opacity of the image over the #070707 background (0 = untouched).
import json, sys, math

REM = 16.0
# Badge footprints at 1920x1080 (render code constants, see safe-areas.md "Badge footprints").
VW, VH, VSP, HSP, M = 88, 102, 7, 10, 20


def lerp_stops(t, stops):
    """stops = [(pos, value)...] sorted by pos; piecewise linear, clamped."""
    if t <= stops[0][0]:
        return stops[0][1]
    for (p0, v0), (p1, v1) in zip(stops, stops[1:]):
        if t <= p1:
            return v0 + (v1 - v0) * (t - p0) / (p1 - p0) if p1 > p0 else v1
    return stops[-1][1]


def layout(kind, W, H, wash=False):
    """Returns a dict with frame/image geometry and a shade(x, y) function in viewport px."""
    lg = W >= 1024
    landscape = W > H
    sidebar = 80 if lg else 0
    if kind == "hero":
        hh = max(26 * REM, 0.75 * H) if W < 640 else (30 * REM if W < 1024 else max(36 * REM, 0.53 * H))
        hx, hw = sidebar, W - sidebar
        maxw = 26 * REM * 2.6 if W < 640 else (30 * REM * 2.6 if W < 1024 else hh * 2.6)
        box = (hx, 0, hw, hh)
        fw = min(hw, maxw)
        fx = hx + hw - fw
        frame_fade = landscape or lg
    elif kind == "item":
        ch = 0.85 * H if (landscape or lg) else 0.55 * H
        cx, cw = sidebar, W - sidebar
        maxw = (2.21 if (landscape or lg) else 1.43) * H
        box = (cx, 0, cw, ch)
        fw = min(cw, maxw)
        fx = cx + cw - fw
        hh = ch
        hx, hw = cx, cw
        frame_fade = landscape or lg
    else:  # corner
        bw = 0.7 * W
        bh = min(bw * 9 / 16, 0.45 * H * 1.4)
        box = (W - bw, 0, bw, bh)
        fx, fw, hh = W - bw, bw, bh
        hx, hw = W - bw, bw
        frame_fade = False
    sc = max(fw / 1920, hh / 1080)
    dw, dh = 1920 * sc, 1080 * sc
    ox, oy = fx + (fw - dw) / 2, 0.0
    vis = dict(
        x0=max(0, (max(fx, 0) - ox) / dw), x1=min(1, (min(fx + fw, W) - ox) / dw),
        y0=0.0, y1=min(1, min(hh, H) / dh))

    def opacity(x, y):
        """Opacity of the white image at viewport point (x, y) over #070707 (1 = untouched)."""
        a = 1.0
        if kind == "corner":
            u = (x - box[0]) / box[2]
            v = y / box[3]
            a *= lerp_stops(u, [(0, 0), (0.2, 0.4), (0.45, 1), (1, 1)])
            a *= lerp_stops(v, [(0, 1), (0.45, 1), (0.75, 0.4), (1, 0)])
            return a
        if frame_fade:
            a *= min(1.0, max(0.0, (x - fx) / 192.0))
        if kind == "hero":
            if not wash:
                if W >= 640:
                    u = (x - hx) / hw
                    a *= 1 - lerp_stops(u, [(0, 1), (0.35, 0.7), (1, 0)])
                sh = hh * (1 / 2 if W < 640 else 2 / 3)
                t = (hh - y) / sh  # 0 at the hero bottom, 1 at the top of the shade
                s = 0.0 if t >= 1 else lerp_stops(t, [(0, 1), (0.5, 0.6), (1, 0)])
                a *= 1 - s
            else:
                a *= fade(x, y, hx, hw, hh, frame_fade)
        else:
            if not wash:
                if landscape or lg:
                    a *= 1 - (1 - (x - hx) / hw)  # side shade: linear 1 -> 0
                sh = hh * (1 / 2 if W < 640 else 3 / 4)
                t = (hh - y) / sh
                s = 0.0 if t >= 1 else lerp_stops(t, [(0, 1), (0.5, 0.7), (1, 0)])
                a *= 1 - s
            else:
                a *= fade(x, y, hx, hw, hh, landscape or lg)
        return a

    def shade_img(px, py):
        """Shade at image fraction (px, py); None when cropped."""
        if px < vis["x0"] or px > vis["x1"] or py < vis["y0"] or py > vis["y1"]:
            return None
        return 1 - opacity(ox + px * dw, oy + py * dh)

    return dict(box=box, frame=(fx, fw), drawn=(dw, dh), vis=vis, shade=shade_img)


def fade(x, y, hx, hw, hh, side):
    """data-art-wash: --fade-down (to top, transparent -> opaque at 50 %) x --fade-side (to right, same)."""
    a = min(1.0, max(0.0, (hh - y) / (0.5 * hh)))
    if side:
        a *= min(1.0, max(0.0, (x - hx) / (0.5 * hw)))
    return a


def footprints():
    fps = {}
    for n in range(1, 6):
        fps[f"stack{n}_tr"] = (1920 - VW - M, M, VW, VH * n + VSP * (n - 1))
    for k in range(5):
        fps[f"stack5_tr_b{k+1}"] = (1920 - VW - M, M + k * (VH + VSP), VW, VH)
    for n in (3, 5):
        w = VW * n + HSP * (n - 1)
        fps[f"row{n}_tr"] = (1920 - w - M, M, w, VH)
        fps[f"row{n}_tc"] = ((1920 - w) / 2, M, w, VH)
    fps["pillrow3_tr"] = (1920 - 520 - M, M, 520, 58)
    fps["pillrow3_tc"] = ((1920 - 520) / 2, M, 520, 58)
    return fps


def stat(L, f):
    x, y, w, h = f
    nx, ny = max(3, math.ceil(w / 16) + 1), max(3, math.ceil(h / 16) + 1)
    vals, miss = [], 0
    for u in range(nx):
        for v in range(ny):
            s = L["shade"]((x + w * u / (nx - 1)) / 1920, (y + h * v / (ny - 1)) / 1080)
            if s is None:
                miss += 1
            else:
                vals.append(s)
    return dict(x0=round(x / 19.2, 1), x1=round((x + w) / 19.2, 1), y0=round(y / 10.8, 1), y1=round((y + h) / 10.8, 1),
                mean=round(sum(vals) / len(vals), 2) if vals else None, max=round(max(vals), 2) if vals else None,
                cropped=miss, of=nx * ny)


SCEN = [  # (name, kind, W, H, wash)
    ("hero-tv-1280x720", "hero", 1280, 720, False),
    ("hero-desktop-1920x1080", "hero", 1920, 1080, False),
    ("hero-desktop-1440x900", "hero", 1440, 900, False),
    ("hero-phone-412x915", "hero", 412, 915, False),
    ("item-tv-1280x720", "item", 1280, 720, True),
    ("item-desktop-1920x1080", "item", 1920, 1080, True),
    ("item-desktop-1440x900", "item", 1440, 900, True),
    ("item-phone-412x915", "item", 412, 915, True),
    ("item-washoff-tv-1280x720", "item", 1280, 720, False),
    ("item-washoff-desktop-1920x1080", "item", 1920, 1080, False),
    ("item-washoff-desktop-1440x900", "item", 1440, 900, False),
    ("item-washoff-phone-412x915", "item", 412, 915, False),
    ("herowash-tv-1280x720", "hero", 1280, 720, True),
    ("herowash-desktop-1440x900", "hero", 1440, 900, True),
    ("corner-tv-1280x720", "corner", 1280, 720, False),
    ("corner-desktop-1920x1080", "corner", 1920, 1080, False),
    ("corner-desktop-1440x900", "corner", 1440, 900, False),
]


def validate():
    """Compare with the first-round Chromium measurement (measure.json of commit 0256404: alpha_rows at y = 3 %)."""
    path = sys.argv[sys.argv.index("--validate") + 1]
    old = json.load(open(path))
    worst = 0.0
    n = 0
    for r in old:
        kind = r["kind"]
        W, H = r["vp"]
        if kind == "corner":
            continue  # the first round used a 55dvh --hero-h; the app uses 45dvh (see doc)
        L = layout(kind, W, H, wash=False)
        for q, row in r["alpha_rows"].items():
            for p, a in row:
                s = L["shade"](p / 100, float(q))
                if a is None or s is None:
                    continue
                worst = max(worst, abs(s - a))
                n += 1
    print(f"validated {n} points against Chromium: max |model - measured| = {worst:.3f}")


if __name__ == "__main__":
    if "--validate" in sys.argv:
        validate()
        sys.exit()
    out = {}
    for name, kind, W, H, wash in SCEN:
        L = layout(kind, W, H, wash)
        out[name] = dict(
            vp=[W, H], wash=wash, box=[round(v, 1) for v in L["box"]], drawn=[round(v, 1) for v in L["drawn"]],
            visible_pct={k: round(v * 100, 1) for k, v in L["vis"].items()},
            top_row={str(p): (None if L["shade"](p / 100, 0.03) is None else round(L["shade"](p / 100, 0.03), 2)) for p in range(10, 100, 10)},
            footprints={k: stat(L, f) for k, f in footprints().items()})
    if "--json" in sys.argv:
        json.dump(out, sys.stdout, indent=1)
    else:
        for name, v in out.items():
            print(name, v["box"], v["drawn"], v["visible_pct"])
            print("  top row y=3%:", v["top_row"])
            for k, s in v["footprints"].items():
                print("  ", k, s)
