# AIOStreams web app: backdrop geometry and badge safe areas

Task P5.1, with its fix round. Result: **geometry found**. Evidence date 2026-11-25. Source: the public
web app at `https://web.aiostreams.org`, bundles `static/css/index.a0c1a0d8d1.css` (CSS) and
`static/js/index.010d890cb7.js` (JS), plus the selector list at
`https://aiostreams.org/reference/web-app-css`. The docs page `https://aiostreams.org/apps` says the
Android, TV and desktop apps are "the same app everywhere" that "draw their screens the way a browser
draws a page", so this web CSS is what the native apps use (app builds may lag the web version; see
Confidence). Plain-language advice for Louk is the last section, "What to do, in plain words".

## Summary (the finding)

- **Which image is shown.** Both the Home hero and the title-page backdrop show Jellyfin's `Backdrop`
  image, which AIOMetadata fills from `meta.background`, i.e. OpenPosterDB's `backdrop-default`
  (details under "Which image feeds each surface"). Episode stills are not on these surfaces.
- **What OpenPosterDB draws by default.** On a backdrop the default is **a vertical column of up to 5
  badges at the right edge** (position `tr`, badge style vertical, direction "default", which resolves
  to vertical for every position except `tc`/`bc`). At 1920 px wide that is a strip x 94.4-99.0 % wide and
  y 1.9 % down to 51.7 % (5 badges), 41.6 % (4) or 31.5 % (3). It is **not** a row (see "Badge
  footprints"). Per-key backdrop settings decide what Louk's key really draws; this repo cannot see them.
- **TV / desktop (>= 1024 px wide, or landscape): the top-right is the only clean corner.** Content sits
  bottom-left. The Home hero darkens its left (side shade) and its bottom (bottom shade); the image is
  `object-position: top`, so the top is never cropped. Hero: a default 3-badge column has mean shade
  0.04-0.07 (max 0.13-0.27); the default 5-badge column has mean 0.15-0.24 but its bottom two
  badges sit in the bottom shade (badge 5: 0.41-0.63 mean, max up to 0.72); a horizontal row of 5 at
  `tr` has mean 0.12-0.15 (max 0.23-0.28). On the hero `tl`, `bl`, `l`, `r` are dead (0.5-1.0).
- **Title page, default settings: even better.** "Background from artwork" (`data-art-wash`) is **on by
  default for title pages**; it removes the two shade layers and replaces them with a fade whose top-right
  quadrant is fully opaque. Any `tr` footprint (3 or 5 badges, column or row) has mean shade 0.00-0.02, max
  <= 0.17 on TV and desktop. With the setting switched off, the title page is like the hero (kept as a
  variant below).
- **Phone portrait (< 640 px wide): no corner survives.** `object-cover object-top` in a 412 x 686 hero
  box shows only x 33.1-66.9 % of the image (title page: 27-73 %), so the default right-edge column is
  cropped away entirely. Only `tc` survives: a **row** of up to 5 badges at x 37.5-62.5 %, which sits inside
  both phone windows with 4.4 % to spare per side (6 badges still fit the hero window with 1.8 %).
- **No single setting serves both.** `tc` on a wide screen dims the badges on the Home hero to mean
  0.44-0.54 (about half brightness); on the title page (default) it costs only 0.04-0.07. `tr` on a phone
  loses every badge. The tool is a separate key per device class (see the last section).
- Home banner `heroMode` = `corner` (non-default) shows the whole image width with shade 0 at the top right.
- **No code preset was built.** The existing `position`, `edge_inset_x/y`, `badge_direction` and
  `ratings_limit` settings already express every choice above.

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

| TV: the viewport is fixed to 1280 CSS px wide (720 tall at 16:9) | J (at byte 1111074): `ma().tv&&(document.documentElement.dataset.tv="",document.querySelector('meta[name="viewport"]')?.setAttribute("content","width=1280, viewport-fit=cover")` |
| "Background from artwork" (`artBackground`) is on for title pages by default | J: `artBackground:tI("artBackground",function(){let e=arguments.length>0&&void 0!==arguments[0]?arguments[0]:"item";...})`: nothing stored means `["item"]`; `RU` sets `r=a.includes(lQ(s))` (`lQ` maps `/item/...` to `"item"`); `lJ` is `toggleAttribute("data-art-wash", lH && !!lW)` and `lW` is set once the 300 px artwork has loaded. The apps page agrees: "title pages to begin with, and also home, search and catalogs, or every other page" |
| Trailers play by themselves on TVs and with a mouse, behind a title's page and in the featured title | `https://aiostreams.org/apps` ("They're on with a mouse and on TVs, and off on phones and tablets"); J: `ar=...tP("aiostreams-web-interface-trailers-"+e, ()=>as()\|\|matchMedia("(hover: hover) and (pointer: fine)").matches)` with `as=()=>document.documentElement.hasAttribute("data-tv")`; `item-backdrop-video` is drawn in the same frame as the image and the image fades out (`opacity-0`) when it plays |

So at the default settings the content area is `viewport width - 80 px` on >= 1024 px and the full
viewport width below that. On a TV the layout is therefore the 1280 x 720 one (it is 1280 px wide at any
physical resolution; "very likely" rather than certain because a WebView can ignore the viewport meta).

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
A 16:9 box in the top-right of the viewport, opaque for x >= 45 % and y <= 45 % of the box. The app sets
`--hero-h` on the Home wrapper: J (`k1` caller): `n?"[--hero-h:calc(45dvh+var(--top-bar,0px)*0.3)]":"[--hero-h:calc(55dvh+...)]"`
with `n` = corner, so in corner mode `--hero-h` = 45dvh and the box is at most 63dvh tall. On a 16:9 screen
(1280 x 720, 1920 x 1080) that max height binds (the box would need 70vw x 9/16 = 39.4vw), so the box is
cropped to 90 % of the image height (the first round of this evidence assumed 55dvh and showed the whole
image). Nothing changes for the top-right corner.

### Title page, elements `item-backdrop`, `item-backdrop-image`, `item-backdrop-shade`

The two shade layers below apply only when "Background from artwork" is **off**; it is on by default (see
the subsection after this one), where both are transparent. The container, frame and image rules apply
in both states.

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

### Which image feeds each surface

- Hero: J `kH(e,t)` = `lw(...)` (Jellyfin `Backdrop`, falling back to `ParentBackdrop`) `?? lb(...)` (Thumb/Primary).
- Title page: `images = lj(...)` = `[Backdrop, Thumb]`, Backdrop first (BoxSets use `lw` only); episode routes redirect to the series page.

Both surfaces therefore show the item's Jellyfin `Backdrop`, which is `meta.background` from AIOMetadata,
which is OpenPosterDB's `backdrop-default` image: **the image these badges are drawn on**. The episode still
(`video.thumbnail`, OpenPosterDB `episode-default`) feeds episode cards and lists and appears on neither
surface; episode stills are out of scope here.

### Other runtime behaviour not modelled

- Trailers: on TVs and with a mouse, a trailer plays behind the title page and in the featured title, in the
  same frame as the image (`item-backdrop-video`); the image (and its badges) fades out while it plays. On a
  TV the badged backdrop is therefore only seen until the trailer starts (see "Not verified").
- On non-TV devices `scroll-lag` parallax and `[data-peek]` move the backdrop on scroll (no effect at scroll
  position 0; TV is excluded by `:root:not([data-tv])`). BoxSets use a `band` title-page variant
  (height `min(85vh, var(--hero-height))`); not measured.

### Other hooks named in the task

- `hero-logo`, `hero-overview`, `hero-actions`, `hero-corner`: covered above.
- `window-controls` (desktop app only): J: `"data-ui":"window-controls" ... className:"pointer-events-auto fixed right-2 top-0 z-[9999] flex h-10 items-center gap-1 ..."`;
  C: `@media (min-width:1024px){html[data-window-buttons=bar]:not(.playing):not(...) [data-ui=window-controls]{display:none}}`.
  A 40 px tall strip at the top right, drawn over everything unless the "bar" window-buttons setting is on or the platform is macOS (controls on the left, `html[data-platform=macos] ...`).
- `data-art-wash` ("Background from artwork"): see the next subsection. It is **on for title pages by default**.

### Title page with "Background from artwork" (`data-art-wash`, the default state)

C: `html[data-art-wash] [data-ui=item-backdrop],html[data-art-wash] [data-ui=item-trailer],html[data-art-wash] [data-ui=hero],html[data-art-wash] [data-ui=hero-shade],html[data-art-wash] [data-ui=hero-trailer]{--background:transparent}`
and `html[data-art-wash] [data-ui=item-backdrop],html[data-art-wash] [data-ui=hero-art]{--fade-down:linear-gradient(to top, transparent, #000 50%);...mask-image:var(--fade-down)}`
plus, in the same rule family for landscape or >= 1024 px, `--fade-side:linear-gradient(to right, transparent, #000 50%)` composited with
`mask-composite:intersect`.

Effect on the title page: both shade layers (`bg-[--background]`) become transparent, so the shades
disappear. The whole `item-backdrop` container (the 85 vh x content-width box, not the image) is
masked instead: opacity 0 at its bottom edge rising to 1 at 50 % of its height (fade-down), times
0 at its left edge rising to 1 at 50 % of its width (fade-side, landscape or >= 1024 px only). So the
**top-right quadrant of the container is fully opaque**; on a phone in portrait only fade-down applies and
the crop is unchanged. Behind the faded part sits the blurred, darkened art wash (`apps` page: "the artwork
of the title you are on, blurred and darkened") instead of `#070707`. The harness paints `#070707`
there, so "shade" in the wash rows below is an upper bound on how much the badge loses against the wash
(it is exact only if the wash is black).

The Home hero is **not** affected by default: the default list is `["item"]`, so `data-art-wash` is off on Home
(the hero rows are the default state). If Louk adds Home to the list, `hero-art` gets the same
fade and the hero shades vanish: measured as a variant ("hero with wash") below.

## Derived geometry (a 16:9 backdrop)

How the numbers were derived: cover-fit a 1920 x 1080 image into the frame, top-aligned and
horizontally centred, then multiply the shade layers (hero: side x bottom; title page: side x bottom, or
the art-wash fade). Verified in headless Chromium with the **real CSS bundle** and the class strings above
applied to a reconstruction of the components (see "Method and reproduction"); the shade values are
measured from a flat white image over the `#070707` background (`shade = 1 - (L - 7) / 248`), so they
include every mask and gradient. In the fix round every scenario was measured in Chromium **and**
recomputed with `harness/model.py` (pure arithmetic from the rules above): they agree to 0.01, and the
model also reproduces the 704 first-round samples to 0.014. "Derived" throughout means this
reconstruction, not a live app. Shade 0 = untouched, 1 = black.

Scenarios: **TV = 1280 x 720**, the layout the app forces on TVs (the `width=1280` viewport line above);
**desktop 1920 x 1080 and 1440 x 900** are browsers or the desktop app; **phone = 412 x 915** portrait.
Default settings (`--top-bar` = `--side-bar` = 0, docked slim sidebar from 1024 px, scrollbars hidden).

Image box and visible window.

| Scenario | Image box (px) | Image drawn (px) | Visible x (% of image width) | Visible y (% of image height) |
| --- | --- | --- | --- | --- |
| Hero, TV 1280x720 | 1200x576 at x=80 | 1200x675 | 0-100 | 0-85.3 |
| Hero, desktop 1920x1080 | 1498x576 at x=422 (frame max-w 2.6 x 576 px; the left 342 px of the 1840 px hero are bare background) | 1498x842 | 0-100 | 0-68.4 |
| Hero, desktop 1440x900 | 1360x576 at x=80 | 1360x765 | 0-100 | 0-75.3 |
| Hero, phone 412x915 | 412x686 at x=0 | 1220x686 | **33.1-66.9** | 0-100 |
| Title page, TV 1280x720 | 1200x612 at x=80 | 1200x675 | 0-100 | 0-90.7 |
| Title page, desktop 1920x1080 | 1840x918 at x=80 | 1840x1035 | 0-100 | 0-88.7 |
| Title page, desktop 1440x900 | 1360x765 at x=80 | 1360x765 (exactly 16:9) | 0-100 | 0-100 |
| Title page, phone 412x915 | 412x503 at x=0 | 895x503 | **27.0-73.0** | 0-100 |
| Hero `corner` mode, TV 1280x720 | 896x454 at x=384 (max-h 63dvh binds) | 896x504 | 0-100 | 0-90 |
| Hero `corner` mode, desktop 1920x1080 | 1344x680 at x=576 (max-h binds) | 1344x756 | 0-100 | 0-90 |
| Hero `corner` mode, desktop 1440x900 | 1008x567 at x=432 | 1008x567 | 0-100 | 0-100 |

The hero and title page are identical in the box and window for the wash on and off states (the wash only
changes the shading). If a TV WebView ignored the viewport meta, the layout would be 1920 x 1080 (the
desktop 1920 rows) or, at 2x, 960 x 540 (first-round numbers: hero box 960x480, visible y 0-88.9 %,
title page 960x459, visible y 0-85.0 %, no sidebar below 1024 px); the shades are fractions of the hero
width, so they barely change.

Shade along the top of the image (y = 3 %), by x position in the image (Chromium). "crop" = outside the
visible window. "Title page (wash on)" is the **default** title-page state.

| Scenario | x=10 % | 30 % | 50 % | 70 % | 80 % | 90 % | 95 % |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Hero TV 1280x720 (default) | 0.95 | 0.75 | 0.54 | 0.33 | 0.21 | 0.10 | 0.06 |
| Hero desktop 1440x900 (default) | 0.94 | 0.75 | 0.54 | 0.33 | 0.21 | 0.10 | 0.06 |
| Hero desktop 1920x1080 (default) | 0.82 | 0.62 | 0.44 | 0.26 | 0.18 | 0.08 | 0.04 |
| Hero phone 412x915 (default) | crop | crop | 0 | crop | crop | crop | crop |
| Title page TV 1280x720, **wash on (default)** | 0.88 | 0.40 | 0 | 0 | 0 | 0 | 0 |
| Title page desktop 1440x900, wash on | 0.86 | 0.40 | 0 | 0 | 0 | 0 | 0 |
| Title page desktop 1920x1080, wash on | 0.81 | 0.40 | 0 | 0 | 0 | 0 | 0 |
| Title page phone 412x915, wash on | crop | 0 | 0 | 0 | crop | crop | crop |
| Title page TV 1280x720, wash off | 0.94 | 0.70 | 0.50 | 0.30 | 0.20 | 0.10 | 0.05 |
| Title page desktop 1440x900, wash off | 0.93 | 0.70 | 0.50 | 0.30 | 0.20 | 0.10 | 0.05 |
| Title page desktop 1920x1080, wash off | 0.90 | 0.70 | 0.50 | 0.30 | 0.20 | 0.10 | 0.05 |
| Hero with wash on (non-default), TV | 0.88 | 0.40 | 0 | 0 | 0 | 0 | 0 |
| Hero `corner`, TV and desktop | 0.80 | 0.36 | 0 | 0 | 0 | 0 | 0 |

Shade at mid-height (y = 50 % of the image, where `l` / `r` anchor), x = 90-95 %: hero 0.49-0.52 (TV),
0.62-0.64 (1440), 0.69-0.71 (1920); title page, wash on, 0.00-0.13; wash off 0.50-0.63. The bottom of the
image is hidden or dark on the hero: visible only to 68-85 % of the height, with a bottom shade ramp from
0 at a third of the hero height up to 1.0.

### Badge footprints: what OpenPosterDB draws on a backdrop

Checked in the render code of the v1.3.0 reference (`/home/dev/opdb-fork/ref`, paths relative to `api/src`):

- **Defaults** (`RenderSettings::default()`, `services/db.rs` ~2320-2348): `backdrop_position` = `tr`,
  `backdrop_badge_style` = **Vertical**, `backdrop_badge_direction` = **Default**,
  `backdrop_ratings_limit` = **5** (`default_logo_backdrop_ratings_limit`), shape Rounded, label style
  Official (icons), size Medium, insets 0 / 0.
- **Direction `Default` resolves by position** (`BadgeDirection::resolve`, `db.rs` 155-163, applied in
  `image/serve.rs` `LbRenderParams::from_settings`, line 67): Horizontal for `tc` and `bc`, **Vertical for
  every other position**. An explicit `h` or `v` overrides it. The style `Vertical` stays vertical
  whatever the direction: the badge shape is fixed, only the stacking changes.
- **Rendering** (`render_backdrop_sync`, `image/generate.rs` 611-668): vertical style makes
  `render_vertical_badge` badges; vertical direction goes to `overlay_vertical_stack` (a column), otherwise
  to `overlay_horizontal_rows` with all badges in a single row.
- **Sizes at scale 1 (1920 px wide):** a vertical badge is **88 px wide** (`BASE_VERT_BADGE_WIDTH`) and
  8 + 48 (icon) + 4 + 34 (value) + 8 = **102 px tall** (80 px with `label_style=t`); column spacing 7
  (`BADGE_VERT_SPACING`), row spacing 10 (`BADGE_SPACING`), top margin 20 (`BADGE_TOP_MARGIN`), side margin
  20 (`BACKDROP_SIDE_MARGIN`). `badge_scale` = target width / 1920 (`db.rs` 528: small 1280, medium 1920, large
  3840), so the percentages below hold for every `imageSize`. At `tr` the column's x range is
  1812-1900 px = **94.4-99.0 %** of the width.

Footprints at 1920 x 1080 (x / y as % of the image):

| Footprint | x range | y range | Bounding size |
| --- | --- | --- | --- |
| Default `tr`: column of 5 | 94.4-99.0 % | 1.9-51.7 % | 88 x 538 px (4.6 % x 49.8 %) |
| Column of 4 | 94.4-99.0 % | 1.9-41.6 % | 88 x 429 px |
| Column of 3 | 94.4-99.0 % | 1.9-31.5 % | 88 x 320 px |
| `tr` with `badge_direction=h`: row of 5 | 74.0-99.0 % | 1.9-11.3 % | 480 x 102 px (25.0 % x 9.4 %) |
| Row of 3 at `tr` | 84.2-99.0 % | 1.9-11.3 % | 284 x 102 px |
| `tc` (direction resolves to horizontal): row of 5 | 37.5-62.5 % | 1.9-11.3 % | 480 x 102 px |
| Row of 3 at `tc` | 42.6-57.4 % | 1.9-11.3 % | 284 x 102 px |

Pills (`badge_shape=p`) are horizontal lozenges whatever the style; with direction `h` the earlier estimate
of about 148 px per pill (value text 55 px + 76 + 17) gives the 520 x 58 px "pill row of 3" below
(estimated from the code, not rendered). With direction left on default a pill at `tr` is a column of
those lozenges instead (about 150 px wide, not measured).

Louk's per-key backdrop settings decide which footprint his key draws (the production database is not
visible from here, and the AIOMetadata URL pattern carries no overrides): **assume the default column of 5 unless
his key says otherwise.**

Shade seen by each footprint (mean / max, 0 = untouched, Chromium). "Cropped" = outside the visible window.

**Home hero (default state)**

| Footprint | TV 1280x720 | Desktop 1440x900 | Desktop 1920x1080 | Phone 412x915 |
| --- | --- | --- | --- | --- |
| Column of 3 (`ratings_limit` 3) | 0.04 / 0.13 | 0.05 / 0.20 | 0.07 / 0.27 | **cropped** |
| Column of 4 | 0.08 / 0.32 | 0.12 / 0.43 | 0.15 / 0.52 | **cropped** |
| Column of 5 (default) | 0.15 / 0.52 | 0.20 / 0.65 | 0.24 / 0.72 | **cropped** |
|   of which badge 4 alone | 0.21 / 0.32 | 0.31 / 0.43 | 0.39 / 0.52 | **cropped** |
|   of which badge 5 alone | 0.41 / 0.52 | 0.54 / 0.65 | 0.63 / 0.72 | **cropped** |
| Row of 3 at `tr` (`badge_direction=h`, 3 badges) | 0.09 / 0.17 | 0.09 / 0.17 | 0.07 / 0.14 | **cropped** |
| Row of 5 at `tr` (`badge_direction=h`) | 0.15 / 0.28 | 0.15 / 0.28 | 0.12 / 0.23 | **cropped** |
| Row of 3 at `tc` | 0.54 / 0.62 | 0.54 / 0.62 | 0.44 / 0.50 | 0.00 / 0.00 |
| Row of 5 at `tc` (default for `tc`) | 0.54 / 0.67 | 0.54 / 0.67 | 0.44 / 0.55 | 0.00 / 0.00 |
| Pill row of 3 at `tr` (`badge_shape=p` or `badge_style=h`, about 520 x 58 px) | 0.16 / 0.30 | 0.16 / 0.30 | 0.13 / 0.25 | **cropped** |
| Pill row of 3 at `tc` | 0.54 / 0.69 | 0.54 / 0.69 | 0.44 / 0.56 | 0.00 / 0.00 |

**Title page, default state: "Background from artwork" on**

| Footprint | TV 1280x720 | Desktop 1440x900 | Desktop 1920x1080 | Phone 412x915 |
| --- | --- | --- | --- | --- |
| Column of 3 (`ratings_limit` 3) | 0.00 / 0.00 | 0.00 / 0.00 | 0.00 / 0.00 | **cropped** |
| Column of 4 | 0.00 / 0.00 | 0.00 / 0.00 | 0.00 / 0.00 | **cropped** |
| Column of 5 (default) | 0.01 / 0.14 | 0.00 / 0.04 | 0.01 / 0.17 | **cropped** |
|   of which badge 4 alone | 0.00 / 0.00 | 0.00 / 0.00 | 0.00 / 0.00 | **cropped** |
|   of which badge 5 alone | 0.05 / 0.14 | 0.01 / 0.04 | 0.07 / 0.17 | **cropped** |
| Row of 3 at `tr` (`badge_direction=h`, 3 badges) | 0.00 / 0.00 | 0.00 / 0.00 | 0.00 / 0.00 | **cropped** |
| Row of 5 at `tr` (`badge_direction=h`) | 0.00 / 0.00 | 0.00 / 0.00 | 0.00 / 0.00 | **cropped** |
| Row of 3 at `tc` | 0.04 / 0.15 | 0.04 / 0.15 | 0.04 / 0.15 | 0.00 / 0.00 |
| Row of 5 at `tc` (default for `tc`) | 0.07 / 0.25 | 0.07 / 0.25 | 0.07 / 0.25 | 0.00 / 0.00 |
| Pill row of 3 at `tr` (`badge_shape=p` or `badge_style=h`, about 520 x 58 px) | 0.00 / 0.00 | 0.00 / 0.00 | 0.00 / 0.00 | **cropped** except a sliver (160 of 170 samples) |
| Pill row of 3 at `tc` | 0.07 / 0.27 | 0.07 / 0.27 | 0.07 / 0.27 | 0.00 / 0.00 |

**Title page with "Background from artwork" off (variant)**

| Footprint | TV 1280x720 | Desktop 1440x900 | Desktop 1920x1080 | Phone 412x915 |
| --- | --- | --- | --- | --- |
| Column of 3 (`ratings_limit` 3) | 0.06 / 0.23 | 0.05 / 0.17 | 0.07 / 0.25 | **cropped** |
| Column of 4 | 0.13 / 0.43 | 0.10 / 0.35 | 0.13 / 0.44 | **cropped** |
| Column of 5 (default) | 0.21 / 0.63 | 0.17 / 0.53 | 0.21 / 0.65 | **cropped** |
|   of which badge 4 alone | 0.32 / 0.43 | 0.25 / 0.35 | 0.33 / 0.44 | **cropped** |
|   of which badge 5 alone | 0.52 / 0.63 | 0.43 / 0.53 | 0.54 / 0.65 | **cropped** |
| Row of 3 at `tr` (`badge_direction=h`, 3 badges) | 0.08 / 0.16 | 0.08 / 0.16 | 0.08 / 0.16 | **cropped** |
| Row of 5 at `tr` (`badge_direction=h`) | 0.14 / 0.26 | 0.13 / 0.26 | 0.14 / 0.26 | **cropped** |
| Row of 3 at `tc` | 0.50 / 0.57 | 0.50 / 0.57 | 0.50 / 0.57 | 0.00 / 0.00 |
| Row of 5 at `tc` (default for `tc`) | 0.50 / 0.63 | 0.50 / 0.63 | 0.50 / 0.63 | 0.00 / 0.00 |
| Pill row of 3 at `tr` (`badge_shape=p` or `badge_style=h`, about 520 x 58 px) | 0.15 / 0.28 | 0.15 / 0.28 | 0.15 / 0.28 | **cropped** except a sliver (160 of 170 samples) |
| Pill row of 3 at `tc` | 0.50 / 0.64 | 0.50 / 0.64 | 0.50 / 0.64 | 0.00 / 0.00 |

**Other variants: hero with the wash switched on for Home, and hero `corner` mode**

| Footprint | Hero with wash, TV | Hero with wash, desktop 1440 | `corner`, TV 1280x720 | `corner`, desktop 1440 |
| --- | --- | --- | --- | --- |
| Column of 3 (`ratings_limit` 3) | 0.00 / 0.00 | 0.00 / 0.00 | 0.00 / 0.00 | 0.00 / 0.00 |
| Column of 4 | 0.00 / 0.00 | 0.01 / 0.11 | 0.00 / 0.03 | 0.00 / 0.00 |
| Column of 5 (default) | 0.02 / 0.21 | 0.06 / 0.38 | 0.03 / 0.25 | 0.01 / 0.14 |
|   of which badge 4 alone | 0.00 / 0.00 | 0.03 / 0.11 | 0.00 / 0.03 | 0.00 / 0.00 |
|   of which badge 5 alone | 0.10 / 0.21 | 0.25 / 0.38 | 0.15 / 0.25 | 0.05 / 0.14 |
| Row of 3 at `tr` (`badge_direction=h`, 3 badges) | 0.00 / 0.00 | 0.00 / 0.00 | 0.00 / 0.00 | 0.00 / 0.00 |
| Row of 5 at `tr` (`badge_direction=h`) | 0.00 / 0.00 | 0.00 / 0.00 | 0.00 / 0.00 | 0.00 / 0.00 |
| Row of 3 at `tc` | 0.04 / 0.15 | 0.04 / 0.15 | 0.01 / 0.06 | 0.01 / 0.06 |
| Row of 5 at `tc` (default for `tc`) | 0.07 / 0.25 | 0.07 / 0.25 | 0.03 / 0.18 | 0.03 / 0.18 |
| Pill row of 3 at `tr` (`badge_shape=p` or `badge_style=h`, about 520 x 58 px) | 0.00 / 0.00 | 0.00 / 0.00 | 0.00 / 0.00 | 0.00 / 0.00 |
| Pill row of 3 at `tc` | 0.07 / 0.27 | 0.07 / 0.27 | 0.04 / 0.21 | 0.04 / 0.21 |

Reading the tables:

- **Hero, default column of 5:** the first three badges sit in clean light (0.03-0.13); badge 4 is dimmed
  to 0.21-0.39 and badge 5 to 0.41-0.63 (max 0.52-0.72) because the bottom shade starts at a third of the
  hero height. A column of 3 or a horizontal row of 5 avoids this (max <= 0.28).
- **Title page, default:** every `tr` footprint is at 0.00-0.01 mean (max <= 0.17; only the last badge of a
  column of 5 dips below the opaque quadrant: on TV 51.7 % of the 675 px image is 349 px of a 612 px container,
  opacity 0.86).
- **`tc`** costs 0.44-0.54 (mean) on the hero, 0.07 on the default title page, 0.50 on a wash-off title page.
- **Phone:** the default column and every `tr` row are cropped on the hero and the title page; only `tc` is
  visible (shade 0). `tc` row of 5 fits the 33.8 % hero window with 4.4 % to spare per side; six badges
  (578 px = 30.1 %) still fit with 1.8 %; seven do not (35.2 %).
- Mean 0.15 shade means a badge keeps about 85 % of its brightness; a row's left end is its darkest part.

The two requirements cannot be combined: the unshaded part of a wide hero is x >= about 80 %, while a
portrait phone shows only x 33-67 % (hero) or 27-73 % (title page). On a 16:9 image there is no overlap.

## Settings that express the choices (no code preset was built)

The options exist already, backdrop only (`api/src/handlers/image.rs` 395-475, `docs/api.md` 78-88,
`services/db.rs` 337-376, 451-458, 654-675). Query parameter, per-key setting name (API / DB), and the
label in the admin UI (box **"Backdrop (series/movie)"** of the key's settings: Admin, API Keys, the gear
icon on the key; or the key holder's "Image Settings" page; global defaults under Admin, Settings):

| Option | Query param | Per-key setting | Admin UI label | Values |
| --- | --- | --- | --- | --- |
| Anchor | `position` | `backdrop_position` | Position | `bc`, `tc`, `l`, `r`, `tl`, `tr`, `bl`, `br` (UI: Top Right, Top Center, ...); default `tr` |
| Stacking | `badge_direction` | `backdrop_badge_direction` | Badge direction | `d` Default (column except at `tc`/`bc`), `h` Horizontal, `v` Vertical |
| How many | `ratings_limit` | `backdrop_ratings_limit` | Max ratings | 0-10, default 5 for backdrops (0 = none) |
| Horizontal inset | `edge_inset_x` | `backdrop_edge_inset_x` | Distance from edge: Space from left / right | 0-50 (% of image width), only for left/right anchors, ignored for `tc`/`bc`; default 0 |
| Vertical inset | `edge_inset_y` | `backdrop_edge_inset_y` | Distance from edge: Space from top / bottom | 0-50 (% of height), only for top/bottom anchors, ignored for `l`/`r`; default 0 |

Insets are `round(canvas width or height x pct / 100)` pixels added to the 20 px margin
(`render_backdrop_sync`, `generate.rs` 654-657; `overlay_vertical_stack`, 159-205;
`overlay_horizontal_rows`, 211-257). The cache key carries `.eh{n}` / `.ev{n}` only for the relevant axis and only
when non-zero (`edge_inset_cache_suffix`, `serve.rs` 135-147). A different `ratings_limit`, direction or
position gives a separate cache entry, not a different image for the same key.

Which settings to use per device class (with the numbers above):

1. **Wide screens (Android TV, Google TV, desktop, web):** keep `position=tr`, insets 0 / 0 (the upstream
   default, so no cache change), and stop the column from reaching the bottom shade on the Home hero by
   either `ratings_limit` <= 3 (column x 94.4-99.0 %, y <= 31.5 %: hero mean 0.04-0.07, max <= 0.27) or
   `badge_direction=h` (a row at x 74-99 %, y <= 11.3 %: hero mean 0.12-0.15, max <= 0.28, all 5 badges kept).
   Title page: nothing needed with the default wash. The desktop app adds a 40 px window-controls strip at
   the top right; there `edge_inset_y=4` (31 px at 765 px height) clears it, and it costs no shade (the top
   gradient is horizontal only). On TVs add a small inset only after the real-device overscan check.
2. **Portrait phones:** `position=tc` (direction resolves to a row; `edge_inset_x` is ignored on a centred
   axis, `edge_inset_y` still applies); keep `ratings_limit` <= 5 (6 at most); add `edge_inset_y=4` below a
   translucent status bar if the real-device check shows an overlap.
3. **One key for both:** `tc` is the only position that is never cropped. It costs the wide Home hero
   0.44-0.54 shade (about half brightness), the default title page only 0.07 and a phone nothing. A mixed
   household with phones that matter more than the TV hero can accept it; otherwise use one key per device
   class.

Positions to avoid in every case: `tl`, `bl`, `l` and `r` on the hero (shade 0.5-1.0 on wide screens, cropped
on phones), `bl`/`bc`/`br` (cropped by `object-top` on the hero, under the bottom shade; `br` also under
`hero-dots` at `bottom-6 end-4` / `lg:bottom-14`).

## Confidence

- **High:** the CSS rules, class-to-element wiring and layout rules. They were read directly from the
  production bundle and reproduced with the real CSS in a real browser engine; cover-fit, top-alignment
  and the shade multiplication match the arithmetic model (`harness/model.py`) to 0.01, including the
  art-wash fade and the corner box.
- **High:** the badge footprints. They come straight from the render code (`generate.rs`, `badge.rs`,
  `db.rs`); the only estimate is the horizontal pill width (about 148 px), which is not in the default
  column or row of vertical badges.
- **High (qualified):** the claim that no single position serves a wide screen and a portrait phone.
  That follows from the 33 %-wide phone window and does not depend on any estimate.
- **Medium-high:** the TV scenario. The app sets `width=1280` on TVs, so 1280 x 720 is "very likely", not
  certain, because a WebView can ignore the viewport meta; the 1920 and 960 px alternatives give
  nearly the same shades.
- **Medium:** the title-page values in the default (wash on) state. They are exact for the mask, but the
  colour behind the faded part is the blurred art wash, which the harness paints black.
- **Not verified:** any real device or the live app. The harness reconstructs the components from the JS
  class strings with synthetic content, and the JS was never executed (no login or Jellyfin server to mock).
  Native app builds may lag `web.aiostreams.org`. The phone `75svh` hero height assumes `svh` = 915 px;
  a browser with an address bar is shorter, which **widens** the visible window slightly (at a 600 px hero,
  412 / (600 x 16 / 9) = 38.6 % instead of 33.8 %), so `tc` stays safe.

Not measured (same shade recipe, different geometry, all non-default): hero `follow` mode (`k1`
non-corner: fixed viewport frame `max-w-[calc(100vh*2.6)]` plus the same shades, so on a 16:10 desktop
5 % of the image is cropped on each side, `tr` needs about `edge_inset_x=5`); `--top-bar` != 0 (nav `top`
or rail shape `corner`, where a nav bar overlaps the top of the art); phone landscape; RTL (mirrors
everything; `tr` becomes top-left); the BoxSet `band` title page.

### Needs a real-device check (open)

- **Trailer autoplay on TVs.** Trailers play by themselves on TVs, behind the title page and in the featured
  title; the backdrop image (with its badges) fades out while one plays. On Louk's TV the badged backdrop is
  only seen until the trailer starts. Check how long, and whether the hero or the title page is in a trailer
  state most of the time.
- **TV overscan.** Not in the CSS: some TVs hide 1-3 % of every edge. The default `tr` column sits 1 % from
  the right edge and 1.9 % from the top (20 px of 1920 / 1080).
- **Android status bar** over the phone hero and title page: top-centre badges start 1.9 % down the image
  (about 13 px on the 686 px phone hero) while an Android status bar is about 24 dp tall.
- **Desktop window controls**: a 40 px strip at the top right of the desktop app (`window-controls`),
  drawn over the `tr` badges unless the "bar" setting is on or the platform is macOS.
- The real colour of the art wash behind the faded title-page backdrop, and that the TV really lays out
  at 1280 px.

## Method and reproduction

1. `curl https://web.aiostreams.org` -> `static/css/index.a0c1a0d8d1.css` (450,207 bytes, sha256
   `8580284ad85a27e8...`), `static/js/index.010d890cb7.js` (1,119,217 bytes, sha256 `c01664ce2f676cd1...`).
   Bundles were kept outside the repo and only read with grep and `python3 -I`; the JS was never executed.
2. The selectors the site exposes (`hero-*`, `item-backdrop*`) are `data-ui` attributes; the styling is
   Tailwind utility classes on those elements in the JS, resolved to rules in the CSS.
3. `harness/build.py <dir>` generates pages that link the real CSS bundle and copy the class strings above
   (hero, title page, `corner`; with and without `data-art-wash` on `<html>`), with a synthetic 1920x1080
   SVG (percent grid and the simulated badge footprints), and a flat white image for shade measurement.
   `harness/measure.js '<scenarios json>' <out dir> <html dir>` (Playwright core, headless Chromium
   `--hide-scrollbars`, scrollbars hidden in CSS; `harness/scenarios.json` is the scenario list used) screenshots
   each scenario and writes `harness/measure.json` (box rectangles, visible window, shade per position, mean
   and max shade per footprint). `harness/model.py` recomputes the same numbers by arithmetic
   (`python3 -I model.py`) and checks the first-round samples (`--validate <old measure.json>`). Paths in the
   scripts point to the scratch directory of the build machine.
4. Screenshots: `harness-*.jpg` (JPEG, quality 60, 19-50 KB each, to stay under the 300 KB limit), named
   `harness-` because they show the reconstruction (grid image, placeholder logo/overview boxes), not the live
   app. Footprints drawn: **pink** = the default `tr` column of 5; **cyan** = `tc` (a row of 5); **orange dashed** =
   where the same 5 badges go with `badge_direction=h` at `tr`. Grid labels are percent of image width and
   height. Item screenshots are in the default wash-on state except `item-washoff`.

| Scenario | Hero | Title page (wash on, default) | Hero `corner` |
| --- | --- | --- | --- |
| TV 1280x720 | `safe-areas/harness-hero-tv-1280x720.jpg` | `safe-areas/harness-item-tv-1280x720.jpg` (wash off: `harness-item-washoff-tv-1280x720.jpg`) | `safe-areas/harness-corner-tv-1280x720.jpg` |
| Desktop 1440x900 | `safe-areas/harness-hero-desktop-1440x900.jpg` | `safe-areas/harness-item-desktop-1440x900.jpg` | `safe-areas/harness-corner-desktop-1440x900.jpg` |
| Phone 412x915 | `safe-areas/harness-hero-phone-412x915.jpg` | `safe-areas/harness-item-phone-412x915.jpg` | n/a (`corner` is desktop/TV only) |

## What to do, in plain words

This is for Louk. It assumes his key still uses OpenPosterDB's default backdrop look, which is a column of
up to five rating badges down the right-hand edge. If his key has other backdrop settings, the same ideas apply, but check them first
(Admin, API Keys, the gear icon on the key, the box "Backdrop (series/movie)").

**Android TV / Google TV app (and the desktop and web app).** Leave the position at Top Right and leave
"Distance from edge" at 0. The right-hand edge is the only corner of the picture that stays clear on a TV.
The one problem is the default column of five: on the home screen's big banner the bottom two badges slip
under the dark fade at the bottom of the picture and look dim. Fix it with either one of these, in the same
box:
- **Max ratings: 3**, so the column is short and stays in the bright part (query parameter `ratings_limit=3`);
  or
- **Badge direction: Horizontal**, so all five badges lie in a row along the top edge instead of a column
  (query parameter `badge_direction=h`; the setting is called `backdrop_badge_direction`).

Either choice is enough; with the row you keep all five ratings, with the short column you keep the
current look. On a title's own page nothing needs changing: the app's "Background from artwork" setting
is on there by default and leaves the top right fully clear. (If a trailer is playing, the picture, and the
badges with it, is hidden anyway.)

**A phone-first key (phones held upright).** The phone only shows the middle third of the picture, so the
right-hand column is cut off completely. Set **Position: Top Center** (query parameter `position=tc`;
setting `backdrop_position`). The badges then line up in a row in the middle at the top, which fits on the
phone with room to spare (up to five or six). Leave Max ratings at 5 or less. If the phone's status bar
(clock and battery) covers them, put a small number such as 4 under "Space from top" (`edge_inset_y=4`).

**Both at once.** There is no single setting that suits both: Top Right is right for a TV and gets cut off
on a phone; Top Center always shows, but looks about half as bright on the TV's big home banner. So the
tool is a **setting per key**: one key for the TV and desktop devices (Top Right, Max ratings 3 or Badge
direction Horizontal) and one key for phones (Top Center), if he ever wants both to look right. Nothing
needs building for this; the settings exist today. Two things are still unchecked on real devices: how the
TV's edges, trailers and the phone's status bar interact with the badges (see "Needs a real-device check").
