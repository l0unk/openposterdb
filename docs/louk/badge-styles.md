# Badge looks: design notes (fork task P4.1)

Three new rating-badge looks for the AIOStreams app: a dark web UI shown on Android TV from across the
room, in phone grids and on desktop. They are new values of the existing **Badge background** setting
(`badge_background`), so every place that already takes that setting takes them too. Nothing changes
unless a key (or the global settings) picks one. Code: `api/src/louk/badge_looks.rs`.

| Look | Value | Cache token | Name in the dashboard | What it is for |
| --- | --- | --- | --- | --- |
| Score | `s` | `.bgs` | Score colour | Every source, with how good the score is visible before the number is read. The everyday look for posters. |
| Mono | `m` | `.bgm` | Mono | The quietest look: no colour at all, for art that is already loud, or for people who find colour badges noisy. |
| Average | `a` | `.bga` | One score (average) | One number instead of three to five. Made for phone grids and backdrops, where a row of small badges does not read. |

Upstream's `d`, `k`, `t`, `n` are untouched. The tokens never change: they are part of every cached file
name.

## The brief, and what it means in pixels

"Surprise me, it has to look good within the AIOStreams app." The surprise is meant to come from how
quickly a score reads, not from effects: these are broadcast overlays, and broadcast graphics earn their
place by reading in half a second over any picture.

The sizes that matter (default settings, `imageSize=medium`):

| Where | Image | Shown at | Badge (plate) height | Figure height |
| --- | --- | --- | --- | --- |
| TV poster row | 580 px poster | about 240 CSS px wide (360 physical px on a 1080p TV; the app's TV viewport is 1280 CSS px) | 24 CSS px | 8 CSS px |
| Phone grid, 3 columns | 580 px poster | about 125 px wide | 12.5 px | 4.3 px |
| Backdrop, TV hero | 1920 px backdrop | 1280 CSS px wide | vertical badges, 59 CSS px wide (88 px at 1920) | 13 CSS px |

The phone grid row is the hard case: three 12-px badges do not read on any look. That is why there is
an Average look whose single figure is as tall as a row badge's figure but stands alone.

## One family, one system

All three looks share:

- **Plate colour "off-air" `#0E1116`.** A cool near-black. The app's own background is `#070707`, a
  neutral black; the slight blue keeps a plate visible as an object on black or warm-black artwork,
  instead of reading as a hole in the poster.
- **The same geometry as upstream.** Height 58, value font 34, label font 26, icon 48, vertical width 88
  (all times the badge scale), the same paddings and the same uniform label/value widths per row. So
  every position, direction, size, split and inset setting places the new looks exactly where it places
  upstream badges, and the layout code needed no changes.
- **Softer, anti-aliased corners.** Radius 0.22 x the badge height (12.8 px at 58; upstream uses 10 px
  with a hard, stair-stepped edge). Drawn with a signed-distance mask, so the edge is smooth at any
  scale. The `Pill` shape still works and still means radius = half the short side.
- **A 1 px hairline** (white at 14 %, scaled up to 4 px on large renders) just inside the outline. On a
  dark poster a dark plate has no edge; the hairline gives it one without a visible border.
- **White figures, Inter Bold** (the one font upstream embeds; a second font would add binary size for
  little gain). Figures are always white, so contrast is a property of the plate, not of the score.
- **One colour scale**, used only by Score and Average (below).

## Colour and contrast

The score scale, on a 0..1 scale per source (IMDb and MAL out of 10, Letterboxd out of 5, Roger Ebert
out of 4, Metacritic and the MDBList score out of 100, anything with `%` out of 100). Thresholds follow
the spec: below 5/10 red, 5 to 6.9 amber, 7 and up good.

| Band | Hex | CIE L* | Contrast against the plate |
| --- | --- | --- | --- |
| Good (>= 0.70) | `#5EE6C8` teal | 84 | 12.3:1 |
| Middling (0.50 to 0.69) | `#F5A524` amber | 74 | 9.3:1 |
| Weak (< 0.50) | `#F2545B` red | 58 | 5.6:1 |
| Track (empty part) | white at 16 % | | |

Why teal and not green: under deuteranopia and protanopia, green and amber collapse into similar
yellows. Teal keeps a blue component, so it stays distinct. Simulated (Machado 2009, full severity), the
smallest colour difference between any two bands is Delta-E 38 (deutan red vs amber), and the lightness
order (L* 84 > 74 > 58) is monotonic, so even in grey-scale "brighter is better" holds. The score is
never carried by colour alone: Score encodes it as the **length** of the meter, Average as the length of
the ring, and both show the number.

Plate opacity, worst case (the plate over pure white art):

| Look | Plate alpha | Result over white | White text | Mono grey label `#AEB4C0` |
| --- | --- | --- | --- | --- |
| Score | 86 % | `#303237` | 12.8:1 | |
| Mono | 72 % | `#515457` | 7.6:1 | 3.2:1 (label is secondary; the figure carries the score) |
| Average | 88 % | | above 13:1 | |

Over black art every combination is above 19:1. For comparison, upstream's dark value section is black
at 78 % (alpha 200).

## The looks

### Score (`s`)

```
 ┌───────────────────────┐
 │ [IMDb]   8.1          │   off-air plate, official logo (or your label style), white figure
 │▆▆▆▆▆▆▆▆▆▆▆▆▆▆▆▆░░░░░░░│   meter: 0.085 x badge height (5 px at 58), teal/amber/red, length = score
 └───────────────────────┘
```

- One plate instead of upstream's two sections; the source is identified by its logo, the quality by
  the meter. The content is centred in the part above the meter.
- The meter runs the full width at the bottom edge and is clipped by the rounded corners, so it reads as
  a lit lip of the plate. Its right end is anti-aliased. A value that does not parse ("N/A") shows only
  the track.
- Vertical badges (the default on backdrops and episode stills): logo on top, figure below, meter along
  the bottom; the badge is 5 px taller than upstream's so the meter does not crowd the figure.
- Label style is honoured: official logos (the default), white glyphs, or text.

### Mono (`m`)

- The same plate at 72 %: lighter, so it sits back. No meter, no colour.
- Official logos (the default label style) are replaced by the white source glyphs upstream ships for
  the "Icon" label style (drawn at 80 % so the figure stays the brightest thing). A coloured IMDb logo
  in a "monochrome" look would defeat the point. Text labels are grey `#AEB4C0`.
- This is the look to pair with busy, colourful key art.

### Average (`a`)

(Tuned after the gallery: in rows it is now a pill, see "Tuning round" below; the disc is the column form.)

```
     ╭─────╮
   ╭─  7.8  ─╮      dark disc (88 %), ring track white 16 %,
   │         │      arc from 12 o'clock, clockwise, length = score, band colour
   ╰─       ─╯
     ╰─────╯
```

- One disc, 1.6 x the badge height across (93 px at the 580 px poster, 93 px at a 1920 px backdrop).
  The number is the mean of the **shown** sources (after your order, exclusions and "Max ratings"), each
  normalised to 0..1 first, written on a 0 to 10 scale with one decimal ("10" for a perfect score).
  Sources whose value does not parse are left out; if none parse, no badge is drawn.
- The figure is 0.36 x the disc (figures 20 px tall at 580 px, the same height as a Score badge's
  figures), centred on the disc's optical middle (Inter's figures sit 0.50 of the font size below the
  line top), and shrunk if a value would touch the ring.
- Badge direction, style and shape do not apply to a single disc. Label style is ignored (there is no
  single source to label).
- The cache key still names the sources (`@mil…`) and the "Max ratings" value, so changing either
  redraws.

## Tuning round after the real-art gallery (P4.2)

The gallery rendered every look on six real titles (the darkest, brightest and busiest found) and
screenshotted them at TV-row, phone-grid and hero size. What the pixels showed, and what changed:

| Seen | Cause | Change |
| --- | --- | --- |
| Score: the meter read as a 2 px "swoosh" whose ends were eaten by the corners | a 5 px strip at the very bottom edge, clipped by the rounded mask | The meter is now a **bar**: its own capsule (round ends, AA), 6.4 px thick at 58, inset 5.6 px from the bottom and at least 10 px (or whatever clears the corner arc) from the sides. The filled part is a capsule too. The row badge gets a 52 px content zone plus the bar zone: 64 px tall (upstream 58), the same width. Vertical badges keep upstream's content and add the same bar zone. |
| Score: Letterboxd's official logo showed a dark box on the plate | that logo sits on its own 48 px dark gradient tile | The tile is keyed out (dark, unsaturated pixels become transparent with a soft edge) and the logo is cropped to the three dots. Official marks now fit a 42 px box in the 52 px content zone (48 in columns). |
| Mono: IMDb was a heavy light-grey tile with unreadable letters | upstream's IMDb and MDBList glyphs are filled tiles with knocked-out text | In Mono, those two sources are written as text ("IMDb", "MDB") in the glyphs' tone `#D6DAE0`; the open glyphs (RT, Metacritic, Letterboxd, TMDB, Trakt, …) stay, at 82 %. The plate goes from 72 % to 80 %: white figures 10.3:1 and grey text labels 4.9:1 over pure white (was 7.6 and 3.2). |
| One score on posters sat higher than a badge row and covered title art (Godfather "PART II", Arcane's subtitle) | a 93 px disc anchored at the row's bottom margin rises 35 px above a row | In a row (posters, logos, horizontal backdrops) the look is now a **pill as tall as a Score badge** (64 px), so it occupies exactly the row's band: a 40 px score ring at the left and the figure at 0.70 of the pill (digits 27 px, row badges 20 px). The disc stays for columns (backdrop and episode defaults), where the gallery liked it. |
| Metacritic's figure looked mid-grey in the hero screenshots, in every look and in `d` | not the badge: the raw render has a pure white "47"; the 5th badge of the column sits where the app's bottom fade starts (the gallery page reproduces that shade), which is why "Backdrop safe areas" recommends 3 ratings or One score there | No code change. Every figure in the fork looks is white (`#FFFFFF`), and the tests check the bright figure pixels. |
| Phone grid (124 px cards): Score figures 5–6 px, the disc 17 px | the server renders a 580 px poster; the app shrinks it 4.7x. A render cannot know the card it ends up in | Within a render: below badge scale 0.45 (badge height under ~26 px, e.g. extra-small badges) Score and Mono drop the source mark and keep the figure at no less than 9 px (Average 12 px; pill at least 22 px, disc 32 px). For phone keys: One score's pill figure is 35 % taller than a row figure and alone on the card; add badge size Large or Extra large for more. |

Also for comparison: previews with `art=` now draw the spread sample scores for every look, `d` included,
so default and fork looks can be compared value for value (previews without `art` are unchanged).

## What was left out, and why

- **Frosted glass.** A real blur behind the badge would be cheap (a box blur over about 100 x 200 px per
  badge, against a 1.1 M px Lanczos resize per poster), but upstream draws each badge as a separate
  image before it knows where it goes. Blurring the art under it means passing the look through both
  layout functions and their eight call sites in `image/generate.rs`, which is more divergence than
  the look is worth. Glass without the blur is upstream's `Transparent`, which fails on bright, busy
  art. Not built.
- **A compact stacked column** is a layout, not a look: upstream's vertical direction already does it,
  and all three looks work in it.
- **Score-coloured fill** (the whole value section in teal/amber/red with dark figures) was considered.
  It reads well for normal vision, but red and teal fills invite the colour to carry the meaning, and
  the contrast of the figure would then depend on the score. The meter keeps the figure white on the
  same plate for every score.

## Cost

Per badge, the fork does one extra pass over the badge's own pixels (mask and hairline): about 9,000
pixels for a poster badge, 50,000 for a vertical badge on a 3840 px backdrop. The Average disc is one
pass over its square. All of it is small next to decoding and resizing the artwork, so render time stays
at upstream's.

## Preview

The admin preview endpoints draw upstream's sample badges, which are all 100 % (they would only ever
show teal). For the fork looks only, the preview uses spread samples instead, so all three bands show:
the default poster preview (MAL, IMDb, Letterboxd) is teal 8.12, amber 6.4, red 2.1; the Average
poster preview shows 6.2 (amber). Upstream looks keep upstream's samples.

## For the visual check (gallery, P4.2)

These are the things the numbers above cannot settle; look at them on real art:

1. The meter at poster-card size (2 px at 240 CSS px): visible enough, or should it be 0.11 x height?
2. Mono's grey text labels over white art (3.2:1): acceptable, or should Mono's plate be 80 %?
3. The Average figure's vertical centring (computed from the font metrics, tested to within 10 %).
4. Teal vs the RT "certified fresh" and Metacritic logos next to it: does the meter clash with the
   official logos' own colours on Score?
5. The Average disc at the default backdrop position (top right): alone in the corner, does it look
   intended, or does it want `edge_inset` of 1 to 2 %?
