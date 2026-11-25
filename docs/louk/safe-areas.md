# AIOStreams web app: backdrop geometry and badge safe areas

Task P5.1. Result: **geometry found**. Evidence date 2026-11-25. Source: the public web app at
`https://web.aiostreams.org`, bundles `static/css/index.a0c1a0d8d1.css` (CSS) and
`static/js/index.010d890cb7.js` (JS), plus the selector list at
`https://aiostreams.org/reference/web-app-css`. The docs page `https://aiostreams.org/apps` says the
Android, TV and desktop apps are "the same app everywhere" that "draw their screens the way a browser
draws a page", so this web CSS is what the native apps use (app builds may lag the web version; see
Confidence).

## Summary (the finding)

- **TV / desktop (>= 1024 px wide, or landscape): the top-right corner is the only clean corner.**
  Content (logo, overview, buttons; title page poster and header) sits bottom-left. The left of the
  image is darkened by a side shade, the bottom by a bottom shade, and `object-position: top` means the
  top of the image is never cropped. A top-right row at x 72-99 %, y 2-7 % of the image sees a mean
  shade of 0.13-0.16 (max <= 0.30). `tl`, `bl`, `l`, `r` are dead (shade 0.7-1.0); `bc`/`br` are
  cropped on the hero, shaded, and collide with the hero dots.
- **Phone portrait (< 640 px wide): no corner survives.** `object-cover object-top` in a 412 x 686 hero
  box shows only x 33-67 % of the image (title page: 27-73 %), so every `tr`/`tl` position is cropped
  away. Only a centred band works: `tc`, or `tr` with `edge_inset_x` of about 33. Both land at roughly
  x 37-66 %, where TV and desktop shading is 45-55 %.
- **So there is no single preset.** Two presets for P5.2 and Louk chooses (see "Presets"):
  `aiostreams` = `position=tr`, insets 0 (this equals the upstream defaults, so it changes no cache
  key), and `aiostreams-phone` = `position=tc`.
- Home banner `heroMode` = `corner` (non-default) shows the whole 16:9 image with shade 0 at the top
  right: ideal for `tr`.

## How to read the citations

Every number traces two hops. Hop 1: which Tailwind classes sit on which `data-ui` element, quoted from
the JS bundle (`J:` = `index.010d890cb7.js`, with the minified function name). Hop 2: what each class
resolves to, quoted from the CSS bundle (`C:` = `index.a0c1a0d8d1.css`). Breakpoints: `sm` =
`@media (min-width:640px)`, `lg` = `@media (min-width:1024px)`, `landscape` =
`@media (orientation:landscape)`; `max-sm:` = `@media not all and (min-width:640px)`.

## Evidence: the rules

### Defaults that decide the layout

| Fact | Source |
| --- | --- |
| Backdrop colour `--background:#070707` (dark mode, `<html class="dark">`) | C: `.dark,[data-mode=dark]{...--background:#070707;...}` |
| `--hero-height` | C: `:root{--nav-bar:calc(5rem + env(safe-area-inset-bottom));--hero-height:max(36rem, 53vh)}` |
| Home banner mode default `rotate` (`follow`, `corner` are options) | J: `heroMode:tM("heroMode",["rotate","follow","corner"],"rotate")` |
| Navigation defaults: touch `rail`, TV `rail`, desktop `sidebar`, rail shape `docked`, rail names `auto` | J: `touchNavigation:tP("aiostreams-web-interface-touch-navigation","rail",...)`, `tvNavigation:tP(...,"rail",["rail","top"],...)`, `desktopNavigation:tP(...,"sidebar",["sidebar","top"])`, `railShape:tP(...,"docked",["docked","floating","corner"],...)`, `railNames:tP(...,"auto",...)` |
| With those defaults neither `--top-bar` nor `--side-bar` is set (they are only set for `top` nav, `floating` or `corner` rail shape) | J: `style:w?{["top"===y?"--top-bar-wide":"--top-bar"]:Rz[g]}:"floating"===j?{"--side-bar":R$[...]}:void 0` and `"corner"===j&&("rail"===y?"lg:[--top-bar:calc(1rem+3.875rem)]":"[--top-bar:calc(1.5rem+3.875rem)]")` |
| Docked slim sidebar/rail is 5 rem wide from `lg` up; below `lg` there is none | J: `sidebarSize:"tv-rail"===y&&k?"sm":"slim"` (k false for `auto` names on TV), `{withSidebar:!0,sidebarSize:"slim",className:"lg:[&>.appLayout]:ps-20"}`; C: `@media (min-width:1024px){.lg\:\[\&\>\.appLayout\]\:ps-20>.appLayout{padding-inline-start:5rem}}` |

So at the default settings the content area is `viewport width - 80 px` on >= 1024 px and the full
viewport width below that.

### Hero banner (rotating, default), elements `hero`, `hero-art`, `hero-backdrops`, `backdrop-frame`, `hero-backdrop`

J, section: `kU="relative -mt-[var(--top-bar,0px)] h-[max(26rem,75svh)] w-full overflow-hidden sm:h-[30rem] lg:-ms-[var(--side-bar,0px)] lg:h-[calc(var(--hero-height)+var(--top-bar,0px))] lg:w-[calc(100%+var(--side-bar,0px))]"`.
C: `.h-\[max\(26rem\,75svh\)\]{height:max(26rem,75svh)}`, `.sm\:h-\[30rem\]{height:30rem}`.
Height: < 640 px: `max(416 px, 75svh)`; 640-1023 px: 480 px; >= 1024 px: `max(576 px, 53vh)`.

J, frame (`k$`): `"absolute inset-y-0 end-0 isolate w-full landscape:[mask-image:linear-gradient(to_right,transparent,black_12rem)] lg:[mask-image:linear-gradient(to_right,transparent,black_12rem)]"`
plus the hero's `className:"max-w-[calc(26rem*2.6)] sm:max-w-[calc(30rem*2.6)] lg:max-w-[calc(var(--hero-height)*2.6)]"`.
C: `.max-w-\[calc\(26rem\*2\.6\)\]{max-width:67.6rem}`, `.sm\:max-w-\[calc\(30rem\*2\.6\)\]{max-width:78rem}`,
`.lg\:max-w-\[calc\(var\(--hero-height\)\*2\.6\)\]{max-width:calc(var(--hero-height) * 2.6)}`,
`.lg\:\[mask-image\:linear-gradient\(to_right\,transparent\,black_12rem\)\]{-webkit-mask-image:linear-gradient(90deg,#0000,#000 12rem);...}`,
`.end-0{inset-inline-end:0}`.
The frame is right-aligned, at most 2.6 x the hero height wide (1498 px on >= 1024 px with the 576 px
hero), and its left 12 rem (192 px) fade in (landscape or >= 1024 px only).

J, image (`kG`): `className:"absolute inset-0 h-full w-full object-cover object-top will-change-[opacity]"` with `"data-ui":"hero-backdrop"`.
C: `.object-cover{object-fit:cover}`, `.object-top{object-position:top}`.
So the image is cover-fitted into the frame, **top-aligned, horizontally centred**.

J, shades (`kq`):
- side: `data-name="side"`, `"absolute inset-0 bg-[--background] [mask-image:linear-gradient(to_right,black,rgb(0_0_0/0.7)_35%,transparent)] max-sm:hidden"`.
  C: `.\[mask-image\:linear-gradient\(to_right\,black\,rgb\(0_0_0\/0\.7\)_35\%\,transparent\)\]{-webkit-mask-image:linear-gradient(90deg,#000,#000000b3 35%,#0000);...}`,
  `.max-sm\:hidden{display:none}`. Shade opacity: 1.0 at the left edge of the hero, 0.7 at 35 % of the
  hero width, 0 at the right edge. Hidden below 640 px.
- bottom: `data-name="bottom"`, `"absolute inset-x-0 bottom-0 h-2/3 bg-[--background] [mask-image:linear-gradient(to_top,black,rgb(0_0_0/0.6),transparent)] max-sm:h-1/2"`.
  C: `.h-2\/3{height:66.6667%}`, `.max-sm\:h-1\/2{height:50%}`,
  `.\[mask-image\:linear-gradient\(to_top\,black\,rgb\(0_0_0\/0\.6\)\,transparent\)\]{-webkit-mask-image:linear-gradient(#0000,#0009,#000);...}`.
  Opacity 0 at the top of the bottom 2/3 (1/2 on phones), 0.6 at its middle, 1.0 at the hero's bottom.
- `hero-art` itself: `"absolute inset-0 [mask-image:linear-gradient(to_top,transparent,black_2px)]"`
  (C: `.\[mask-image\:linear-gradient\(to_top\,transparent\,black_2px\)\]{...linear-gradient(0deg,#0000,#000 2px)...}`), a 2 px fade on the bottom edge only.

J, overlays (`kB`, `kJ`): content `"absolute inset-x-0 bottom-0 space-y-4 px-4 pb-8 max-sm:pb-12 max-sm:text-center lg:start-[var(--side-bar,0px)] lg:max-w-3xl lg:ps-0 lg:pe-10 lg:pb-14"`
(C: `.lg\:max-w-3xl{max-width:48rem}`, `.lg\:pb-14{padding-bottom:3.5rem}`, `.max-sm\:pb-12{padding-bottom:3rem}`):
a bottom-left column at most 768 px wide on >= 1024 px, a full-width centred column on phones.
`hero-logo` `"max-h-20 max-w-[min(24rem,75%)] object-contain object-left max-sm:mx-auto max-sm:object-center lg:max-h-28"`,
`hero-overview` `"max-w-2xl text-sm text-gray-300 line-clamp-2 sm:line-clamp-3 sm:text-base"`,
`hero-actions` `"flex flex-wrap gap-2 max-sm:justify-center"`.
`hero-dots`: `"absolute bottom-6 end-4 flex gap-1.5 max-sm:inset-x-0 max-sm:justify-center lg:bottom-14"` (bottom right on >= 1024 px).
Nothing is placed over the top 40 % of the hero (harness: logo top at y = 260 px of 576 px on >= 1024 px;
at y = 410 of 686 px on phones).

### Hero `corner` mode (non-default; desktop and TV only)

J (`k1`): `"pointer-events-none fixed end-0 top-0 -z-10 aspect-video max-h-[calc(var(--hero-h)*1.4)] w-[70vw] overflow-hidden [mask-image:linear-gradient(to_right,transparent,rgb(0_0_0/0.4)_20%,black_45%)]"`
with an inner `"absolute inset-0 [mask-image:linear-gradient(to_bottom,black_45%,rgb(0_0_0/0.4)_75%,transparent)]"`.
C: `.aspect-video{aspect-ratio:16/9}`, `.w-\[70vw\]{width:70vw}`,
`.\[mask-image\:linear-gradient\(to_right\,transparent\,rgb\(0_0_0\/0\.4\)_20\%\,black_45\%\)\]{...linear-gradient(90deg,#0000,#0006 20%,#000 45%)...}`,
`.\[mask-image\:linear-gradient\(to_bottom\,black_45\%\,rgb\(0_0_0\/0\.4\)_75\%\,transparent\)\]{...linear-gradient(#000 45%,#0006 75%,#0000)...}`.
A 16:9 box in the top-right of the viewport, so the image is shown whole; opaque for x >= 45 % and
y <= 45 %.

### Title page, elements `item-backdrop`, `item-backdrop-image`, `item-backdrop-shade`

J (`In`), container: `"pointer-events-none absolute inset-x-0 top-[calc(var(--top-bar,0px)*-1)] h-[calc(55vh+var(--top-bar,0px))] overflow-hidden ... [mask-image:linear-gradient(to_top,transparent,black_2px)]"`
plus `"landscape:h-[calc(85vh+var(--top-bar,0px))] lg:h-[calc(85vh+var(--top-bar,0px))]"`.
C: `.h-\[calc\(55vh\+var\(--top-bar\,0px\)\)\]{height:calc(55vh + var(--top-bar,0px))}`,
`.landscape\:h-\[calc\(85vh\+var\(--top-bar\,0px\)\)\]{height:calc(85vh + var(--top-bar,0px))}`.
Height: 55 vh in portrait below 1024 px; 85 vh in landscape or >= 1024 px.

J, frame: the same `k$` class string with `"max-w-[calc(55vh*2.6)] landscape:max-w-[calc(85vh*2.6)] lg:max-w-[calc(85vh*2.6)]"`.
C: `.max-w-\[calc\(55vh\*2\.6\)\]{max-width:143vh}`, `.lg\:max-w-\[calc\(85vh\*2\.6\)\]{max-width:221vh}`.

J, image: `"absolute inset-0 h-full w-full object-cover"` + `"object-top"` (when it is a real backdrop; a blurred
poster stand-in uses `scale-125 blur-3xl` instead and is out of scope). Same cover/top-aligned/centred rule as the hero.

J, shades:
- side: `data-name="side"`, `"absolute inset-0 hidden bg-gradient-to-r from-[--background] to-transparent landscape:block lg:block"`.
  C: `.bg-gradient-to-r{background-image:linear-gradient(to right, var(--tw-gradient-stops))}`,
  `.from-\[--background\]{--tw-gradient-from:var(--background) var(--tw-gradient-from-position);--tw-gradient-to:#fff0 ...}`,
  `.landscape\:block{display:block}`. Opacity falls **linearly from 1.0 at the left to 0 at the right**; shown only in landscape or >= 1024 px.
- bottom: `"absolute inset-x-0 bottom-0 h-3/4 bg-[--background] [mask-image:linear-gradient(to_top,black,rgb(0_0_0/0.7),transparent)] max-sm:h-1/2"`.
  C: `.h-3\/4{height:75%}`, `...linear-gradient(#0000,#000000b3,#000)...`. Opacity 0 at the top of the bottom 3/4 (1/2 on phones), 0.7 at its middle, 1.0 at the bottom.

J, content (`item-body`): `"relative z-[1] space-y-12 px-4 pb-16 lg:ps-0 lg:pe-10"` + `"pt-[38vh] lg:pt-[26vh]"`
(C: `.pt-\[38vh\]{padding-top:38vh}`, `.lg\:pt-\[26vh\]{padding-top:26vh}`). The poster / logo / title /
overview / buttons start 38 vh down on phones and 26 vh down on >= 1024 px, at the left
(`item-header` `"flex flex-col gap-6 md:flex-row md:items-end md:gap-8"`, `item-logo` `"max-h-24 ... lg:max-h-32"`).

### Other hooks named in the task

- `hero-logo`, `hero-overview`, `hero-actions`, `hero-corner`: covered above.
- `window-controls` (desktop app only): J: `"data-ui":"window-controls" ... className:"pointer-events-auto fixed right-2 top-0 z-[9999] flex h-10 items-center gap-1 ..."`;
  C: `@media (min-width:1024px){html[data-window-buttons=bar]:not(.playing):not(...) [data-ui=window-controls]{display:none}}`.
  A 40 px tall strip at the top right, drawn over everything unless the "bar" window-buttons setting is on or the platform is macOS (controls on the left, `html[data-platform=macos] ...`).
- `data-art-wash` (a setting, not measured): C: `html[data-art-wash] [data-ui=item-backdrop],html[data-art-wash] [data-ui=hero-art]{--fade-down:linear-gradient(to top, transparent, #000 50%);...}` and, for landscape or >= 1024 px, `--fade-side:linear-gradient(to right, transparent, #000 50%)`:
  adds a 50 % fade on the bottom and left of the art and sets the shades' `--background` to transparent.

## Derived geometry (a 16:9 backdrop)

How the numbers were derived: cover-fit a 1920 x 1080 image into the frame, top-aligned and
horizontally centred, then multiply the shade layers. Verified in headless Chromium with the **real CSS
bundle** and the class strings above applied to a reconstruction of the components (see "Method and
reproduction"); the shade values are measured from a flat white image over the `#070707` background
(`shade = 1 - (L - 7) / 248`), so they include every mask and gradient. "Derived" throughout means
this reconstruction, not a live app. Shade 0 = untouched, 1 = black.

Image box and visible window (default settings, `--top-bar` = `--side-bar` = 0, scrollbars hidden).

| Scenario | Image box (px) | Image drawn (px) | Visible x (% of image width) | Visible y (% of image height) |
| --- | --- | --- | --- | --- |
| Hero, TV 1920x1080 | 1498x576 at x=422 (frame max-w 2.6 x 576 px; the left 342 px of the 1840 px hero are bare background) | 1498x842 | 0-100 | 0-68.4 |
| Hero, desktop 1440x900 | 1360x576 at x=80 | 1360x765 | 0-100 | 0-75.3 |
| Hero, phone 412x915 | 412x686 at x=0 | 1220x686 | **33.1-66.9** | 0-100 |
| Title page, TV 1920x1080 | 1840x918 at x=80 | 1840x1035 | 0-100 | 0-88.7 |
| Title page, desktop 1440x900 | 1360x765 at x=80 | 1360x765 (exactly 16:9) | 0-100 | 0-100 |
| Title page, phone 412x915 | 412x503 at x=0 | 895x503 | **27.0-73.0** | 0-100 |
| Hero `corner` mode, TV | 1344x756 at x=576 | 1344x756 | 0-100 | 0-100 |
| Hero `corner` mode, desktop | 1008x567 at x=432 | 1008x567 | 0-100 | 0-100 |
| Hero, 1280x720 (TV at 1.5x) | 1200x576 | 1200x675 | 0-100 | 0-85.3 |
| Hero, 960x540 (TV at 2x, below `lg`: no sidebar, 480 px hero) | 960x480 | 960x540 | 0-100 | 0-88.9 |
| Title page, 1280x720 / 960x540 | 1200x612 / 960x459 | 1200x675 / 960x540 | 0-100 | 0-90.7 / 0-85.0 |

Shade along the top of the image (y = 3 %), by x position in the image. "crop" = outside the visible window.

| Scenario | x=10 % | 30 % | 50 % | 70 % | 80 % | 90 % | 95 % |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Hero TV 1920x1080 | 0.82 | 0.62 | 0.44 | 0.26 | 0.18 | 0.08 | 0.04 |
| Hero desktop 1440x900 | 0.94 | 0.75 | 0.54 | 0.33 | 0.21 | 0.10 | 0.06 |
| Hero phone 412x915 | crop | crop | 0 | crop | crop | crop | crop |
| Title page TV 1920x1080 | 0.90 | 0.70 | 0.50 | 0.30 | 0.20 | 0.10 | 0.05 |
| Title page desktop 1440x900 | 0.93 | 0.70 | 0.50 | 0.30 | 0.20 | 0.10 | 0.05 |
| Title page phone 412x915 | crop | 0 | 0 | 0 | crop | crop | crop |
| Hero `corner`, TV and desktop | 0.80 | 0.36 | 0 | 0 | 0 | 0 | 0 |
| Hero 1280x720 | 0.95 | 0.75 | 0.54 | 0.33 | 0.21 | 0.10 | 0.06 |
| Hero 960x540 | 0.96 | 0.75 | 0.54 | 0.32 | 0.21 | 0.10 | 0.06 |

The 1920 / 1280 / 960 px rows are nearly identical because the shades are fractions of the hero width:
the unknown CSS pixel size of a TV (1920x1080 at 1x, or 960x540 at 2x) barely matters for the shade.
It matters mainly for the hero height (576 px from `lg` up, 480 px below it) and hence how much of the
image bottom is cropped (11-32 % of the image height on the hero).

Shade at mid-height (y = 50 % of the image, where `l` / `r` anchor): 0.69-0.73 at the right edge on the
TV hero (0.50-0.58 on the title page): too dark for badges. The bottom of the image is hidden or dark
everywhere: hero visible only to 68-89 % of the height, with a bottom shade ramp from 0 at 33 % of the
hero height up to 1.0.

### Where does a rating row sit?

A 3-badge row is about 24 % of the image width and 5.4 % of its height, estimated from the badge code at
`badge_scale` 1 (1920 px wide): per badge, rounded shape,
`value_x = 48 (icon) + 2 x 14 = 76`, plus the value text (about 55 px for "8.1" at 34 px) plus
`10 + 5 + 2` = about 148 px; spacing 10 px (`BADGE_SPACING`); so n badges is about
`148 n + 10 (n - 1)` px: 2 = 306 (16 %), 3 = 464 (24 %), 4 = 622 (32 %), 5 = 780 (41 %); pill shape adds
20 px per badge. The harness uses a 520 x 58 px row (about 27 %, 3 pill badges), placed by the same rule
as `overlay_horizontal_rows`: `position tr` -> `x = width - row - 20 px - extra_x`,
`y = 20 px + extra_y`. `badge_scale = target_width / 1920` (`api/src/services/db.rs:528`), so all of these
percentages hold for `imageSize` small / medium / large.

| Scenario | `tr` inset 0/0: mean / max shade | `tc` inset 0/0: mean / max shade | `tr` `edge_inset_x`=33, y=4: mean / max |
| --- | --- | --- | --- |
| Hero TV 1920x1080 | 0.13 / 0.25 | 0.44 / 0.56 | 0.42 / 0.54 |
| Hero desktop 1440x900 | 0.16 / 0.30 | 0.54 / 0.69 | 0.51 / 0.66 |
| Hero phone 412x915 | **fully cropped** | 0 / 0 (visible) | 0 / 0 (visible) |
| Title page TV 1920x1080 | 0.15 / 0.28 | 0.50 / 0.64 | 0.48 / 0.61 |
| Title page desktop 1440x900 | 0.15 / 0.28 | 0.50 / 0.64 | 0.48 / 0.61 |
| Title page phone 412x915 | **50 of 55 samples cropped** (only a 71.9-73 % sliver stays) | 0 / 0 (visible) | 0 / 0 (visible) |
| Hero `corner` TV / desktop | 0 / 0 | 0.04 / 0.21 | 0.02 / 0.15 |

`tr` at x 71.9-99 %, y 1.9-7.2 %; `tc` at x 36.5-63.5 %; `tr` + `edge_inset_x`=33 at x 38.9-66 %.
Mean shade 0.15 means the row keeps about 85 % of its brightness; its left end is the darkest (max 0.25-0.30).

The two requirements cannot be combined: the unshaded part of a wide screen is x >= about 80 %, while a
portrait phone shows only x 33-67 % (hero) or 27-73 % (title page). On a 16:9 image there is no overlap.

## Presets (for P5.2, expressed as OpenPosterDB's existing options)

The options exist already, backdrop only (`api/src/handlers/image.rs:111-131`, `docs/api.md:80-88`,
`api/src/services/db.rs:337-376,451-458,654-675`):

| Option | Query param | Per-key setting (API / DB) | Values |
| --- | --- | --- | --- |
| Anchor | `position` | `backdrop_position` | `bc`, `tc`, `l`, `r`, `tl`, `tr`, `bl`, `br`; default `tr` (`default_backdrop_position()`) |
| Horizontal inset | `edge_inset_x` | `backdrop_edge_inset_x` | integer 0-50 (% of image width), clamped by `clamp_edge_inset`; applies only to anchors on the left or right edge (`l`, `tl`, `bl`, `r`, `tr`, `br`), ignored for `tc` / `bc`; default 0 |
| Vertical inset | `edge_inset_y` | `backdrop_edge_inset_y` | integer 0-50 (% of image height); applies only to anchors on the top or bottom edge (`tc`, `tl`, `tr`, `bc`, `bl`, `br`), ignored for `l` / `r`; default 0 |

Insets are `round(canvas width or height x pct / 100)` pixels added to the 20 px margin
(`render_backdrop_sync`, `api/src/image/generate.rs:654-657`; `overlay_horizontal_rows`,
`generate.rs:211-257`). The cache key carries `.eh{n}` / `.ev{n}` only for the relevant axis and only
when non-zero (`edge_inset_cache_suffix`, `api/src/image/serve.rs:135-147`).

1. **`aiostreams`** (TV, desktop, web, Android TV; the wide layout): `position=tr`,
   `edge_inset_x=0`, `edge_inset_y=0`. **This equals the upstream defaults** (`tr`, 0, 0), so the preset
   changes no cache key and no bytes: it is a statement that today's defaults already match the wide
   layout, and a named place to document why. Result: shade mean 0.13-0.16 in every wide scenario, row
   fully visible, and 0 on the `corner` banner.
   - Desktop app only: add `edge_inset_y=4` (variant `aiostreams-desktop`). `window-controls` is a
     40 px strip at the top right (`fixed right-2 top-0 h-10 z-[9999]`); at the desktop title page
     (765 px tall) the default 20 px margin is only 14 px on screen, and 4 % adds 31 px, so the row top
     sits about 45 px down. The shade does not change with y here (the top gradient is horizontal only),
     so this costs nothing on the other surfaces; with default settings on a TV or the web it is not
     needed.
2. **`aiostreams-phone`** (portrait phones only): `position=tc`, `edge_inset_x=0` (ignored on a centred
   anchor), `edge_inset_y=0` (or 4 below a translucent status bar). Row at x 36.5-63.5 % sits inside
   both phone windows (33.1-66.9 % on the hero, 27-73 % on the title page), 3.4 % slack on each side of the
   hero window. Equivalent: `tr` with `edge_inset_x=33`.
   - Row width limit: the hero window is 33.8 % of the image width, so at most about **3-4 badges**
     (4 badges are 32 %, 0.7 % slack). Set `ratings_limit` <= 3 for this preset.
   - Cost on wide screens: mean shade 0.44-0.54 (about half brightness). That is why this is a separate
     preset and not the default.
3. Compromise if one config must serve both: `aiostreams-phone` (`tc`). Nothing is ever cropped, but TV
   and desktop rows are dimmed to about half. Louk chooses between a per-device setup (not possible with
   one AIOMetadata background URL pattern per config) and this compromise.

Positions to avoid in every preset: `tl`, `bl`, `l`, `r` (shade 0.7-1.0 on wide screens, cropped on
phones), `bl`/`bc`/`br` (cropped by `object-top` on the hero, under the bottom shade, `br` also under
`hero-dots` at `bottom-6 end-4` / `lg:bottom-14`).

## Confidence

- **High:** the CSS rules, class-to-element wiring and layout rules. They were read directly from the
  production bundle and reproduced with the real CSS in a real browser engine; cover-fit, top-alignment
  and the shade multiplication match the hand calculation to 0.1 %.
- **High (qualified):** the claim that no single position serves a wide screen and a portrait phone.
  That follows from the 33 %-wide phone window and does not depend on any estimate.
- **Medium:** the practical value of the specific numbers at the default settings. They assume
  `--top-bar` = 0 and a docked sidebar, and rem = 16 px. TV CSS size is unknown (1x, 1.5x or 2x), shown
  above to move the shade by about 0.01 at x >= 50 % (up to 0.14 at x = 10 %) but the hero height by 96 px.
- **Medium-low:** the badge row width (estimated from code, the text widths come from font metrics that
  were not measured). The safe numbers scale: each extra badge adds about 8 % of the width.
- **Not verified:** any real device or the live app: the harness reconstructs the components from the JS
  class strings with synthetic content, and the JS was never executed (no login or Jellyfin server to
  mock; it would have exceeded the 15-minute budget). Native app builds may lag `web.aiostreams.org`.
  The phone `75svh` hero height assumes `svh` = 915 px (a browser with an address bar is shorter, which
  narrows the visible window slightly: at a 600 px hero, 412 / (600 x 16 / 9) = 38.6 % instead of 33.8 %).

Not measured (same shade recipe, different geometry, all non-default): hero `follow` mode (`k1`
non-corner: fixed viewport frame `max-w-[calc(100vh*2.6)]` plus the same shades, so on a 16:10 desktop
5 % of the image is cropped on each side, `tr` needs about `edge_inset_x=5`); `data-art-wash`;
`--top-bar` != 0 (nav `top` or rail shape `corner`, where a nav bar overlaps the top of the art);
phone landscape; RTL (mirrors everything; `tr` becomes top-left).
Real-device items for FORK.md: TV overscan (not in the CSS: `tr` inset 0 puts the row 1-2 % from the
edge), the Android status bar over the phone hero, and the desktop app's window controls.

## Method and reproduction

1. `curl https://web.aiostreams.org` -> `static/css/index.a0c1a0d8d1.css` (450,207 bytes, sha256
   `8580284ad85a27e8...`), `static/js/index.010d890cb7.js` (1,119,217 bytes, sha256 `c01664ce2f676cd1...`).
   Bundles were kept outside the repo and only read with grep and `python3 -I`; the JS was never executed.
2. The selectors the site exposes (`hero-*`, `item-backdrop*`) are `data-ui` attributes; the styling is
   Tailwind utility classes on those elements in the JS, resolved to rules in the CSS.
3. `harness/build.py` generates pages that link the real CSS bundle and copy the class strings above (hero,
   title page, `corner`), with a synthetic 1920x1080 SVG (percent grid and a simulated badge row), and
   a flat white image for shade measurement. `harness/measure.js` (Playwright core, headless Chromium
   `--hide-scrollbars`, scrollbars hidden in CSS) screenshots each scenario and writes
   `harness/measure.json` (box rectangles, visible window, shade per position, row statistics). Paths in
   both scripts point to the scratch directory of the build machine.
4. Screenshots: `harness-*.jpg` (JPEG, quality 60, 19-48 KB each, to stay under the 300 KB limit), named
   `harness-` because they show the reconstruction (grid image, placeholder logo/overview boxes), not the live
   app. Pink bar = simulated `tr` row; grid labels are percent of image width and height.

| Scenario | Hero | Title page | Hero `corner` |
| --- | --- | --- | --- |
| TV 1920x1080 | `safe-areas/harness-hero-tv-1920x1080.jpg` | `safe-areas/harness-item-tv-1920x1080.jpg` | `safe-areas/harness-corner-tv-1920x1080.jpg` |
| Desktop 1440x900 | `safe-areas/harness-hero-desktop-1440x900.jpg` | `safe-areas/harness-item-desktop-1440x900.jpg` | `safe-areas/harness-corner-desktop-1440x900.jpg` |
| Phone 412x915 | `safe-areas/harness-hero-phone-412x915.jpg` | `safe-areas/harness-item-phone-412x915.jpg` | n/a (`corner` is desktop/TV only) |
