# Generates harness HTML pages that reuse the REAL compiled CSS bundle (read-only link) and
# mirror the class strings of the AIOStreams web app components (hero kZ/kq/k$/kG/kJ, item In()).
import sys
CSS='file:///home/dev/opdb-fork/scratch/aiostreams-web/static/css/index.a0c1a0d8d1.css'

def svg(white):
    if white:
        body='<rect width="1920" height="1080" fill="#fff"/>'
    else:
        lines=[]
        for i in range(0,101,10):
            x=1920*i/100; y=1080*i/100
            lines.append(f'<line x1="{x}" y1="0" x2="{x}" y2="1080" stroke="#fff" stroke-opacity=".45" stroke-width="3"/>')
            lines.append(f'<line x1="0" y1="{y}" x2="1920" y2="{y}" stroke="#fff" stroke-opacity=".45" stroke-width="3"/>')
            if i<100:
                lines.append(f'<text x="{x+8}" y="44" font-size="34" fill="#fff" font-family="sans-serif">{i}%</text>')
                if i>0: lines.append(f'<text x="8" y="{y-8}" font-size="34" fill="#fff" font-family="sans-serif">{i}%</text>')
        # simulated badge row: 3 badges ~ 27% x 5.4%, tr, 20px margin (OpenPosterDB default tr)
        bw=520; bh=58; x=1920-bw-20; y=20
        body=('<rect width="1920" height="1080" fill="#3a6ea5"/>'+''.join(lines)+
              f'<rect x="{x}" y="{y}" width="{bw}" height="{bh}" rx="10" fill="#e0245e" fill-opacity=".9"/>')
    s=f'<svg xmlns="http://www.w3.org/2000/svg" width="1920" height="1080" viewBox="0 0 1920 1080">{body}</svg>'
    import urllib.parse
    return 'data:image/svg+xml,'+urllib.parse.quote(s)

CONTENT_HERO='''<div data-ui="hero-content" class="absolute inset-x-0 bottom-0 space-y-4 px-4 pb-8 max-sm:pb-12 max-sm:text-center lg:start-[var(--side-bar,0px)] lg:max-w-3xl lg:ps-0 lg:pe-10 lg:pb-14">
<div data-ui="hero-logo" class="max-h-20 max-w-[min(24rem,75%)] max-sm:mx-auto lg:max-h-28" style="background:#fff3;outline:1px solid #fff;height:5rem;width:20rem"></div>
<div data-ui="hero-meta" class="flex flex-wrap items-center gap-x-2.5 gap-y-1 text-sm text-gray-200 max-sm:justify-center"><span>2024</span><span>•</span><span>8.1</span><span>•</span><span>Drama, Sci-Fi</span></div>
<p data-ui="hero-overview" class="max-w-2xl text-sm text-gray-300 line-clamp-2 sm:line-clamp-3 sm:text-base">A long overview text that needs several lines so the clamp applies to the paragraph and shows the same height the app shows. A long overview text that needs several lines so the clamp applies. A long overview text that needs several lines so the clamp applies to the paragraph.</p>
<div data-ui="hero-actions" class="flex flex-wrap gap-2 max-sm:justify-center"><div class="rounded-full" style="height:2.5rem;width:7rem;background:#fff"></div><div class="rounded-full" style="height:2.5rem;width:7rem;background:#fff4"></div></div>
</div>'''

def hero(img):
    return f'''<section data-ui="hero" data-nav-top class="relative -mt-[var(--top-bar,0px)] h-[max(26rem,75svh)] w-full overflow-hidden sm:h-[30rem] lg:-ms-[var(--side-bar,0px)] lg:h-[calc(var(--hero-height)+var(--top-bar,0px))] lg:w-[calc(100%+var(--side-bar,0px))] touch-pan-y touch-pinch-zoom">
<div data-ui="hero-art" class="absolute inset-0 [mask-image:linear-gradient(to_top,transparent,black_2px)]"><div data-ui="hero-backdrops" class="absolute inset-0 isolate transition-opacity duration-700"><div data-slide="0" class="absolute inset-0">
<div data-ui="backdrop-frame" class="absolute inset-y-0 end-0 isolate w-full landscape:[mask-image:linear-gradient(to_right,transparent,black_12rem)] lg:[mask-image:linear-gradient(to_right,transparent,black_12rem)] max-w-[calc(26rem*2.6)] sm:max-w-[calc(30rem*2.6)] lg:max-w-[calc(var(--hero-height)*2.6)]">
<img data-ui="hero-backdrop" src="{img}" class="absolute inset-0 h-full w-full object-cover object-top will-change-[opacity]"></div></div></div></div>
<div data-ui="hero-shade" data-name="side" class="absolute inset-0 bg-[--background] [mask-image:linear-gradient(to_right,black,rgb(0_0_0/0.7)_35%,transparent)] max-sm:hidden rtl:[mask-image:linear-gradient(to_left,black,rgb(0_0_0/0.7)_35%,transparent)]"></div>
<div data-ui="hero-shade" data-name="bottom" class="absolute inset-x-0 bottom-0 h-2/3 bg-[--background] [mask-image:linear-gradient(to_top,black,rgb(0_0_0/0.6),transparent)] max-sm:h-1/2"></div>
<div data-slide="0" data-details class="absolute inset-0">{'' if img.startswith('WHITE') else CONTENT_HERO}</div>
</section>'''

def item(img, content=True):
    body='''<div data-ui="item-body" class="relative z-[1] space-y-12 px-4 pb-16 lg:ps-0 lg:pe-10 pt-[38vh] lg:pt-[26vh]"><div data-ui="item-header" class="flex flex-col gap-6 md:flex-row md:items-end md:gap-8">
<div data-ui="item-poster" class="hidden flex-none rounded-xl object-cover shadow-2xl ring-1 ring-white/10 md:block aspect-[2/3] w-48 lg:w-56" style="background:#fff4"></div>
<div class="min-w-0 max-w-3xl flex-1 space-y-4"><div data-ui="item-heading" class="space-y-4"><div data-ui="item-logo" class="max-h-24 max-w-[min(26rem,85%)] max-sm:mx-auto lg:max-h-32" style="background:#fff3;outline:1px solid #fff;height:6rem;width:20rem"></div>
<div data-ui="item-genres" class="flex flex-wrap gap-1.5 max-sm:justify-center"><span>Drama</span><span>Sci-Fi</span></div><p class="text-sm leading-relaxed text-gray-300 sm:text-base" style="max-height:4lh;overflow:hidden">Overview text overview text overview text overview text overview text overview text overview text overview text overview text overview text overview text.</p></div>
<div data-ui="item-actions" class="flex flex-wrap items-center gap-2 max-sm:justify-center"><div class="rounded-full" style="height:2.5rem;width:9rem;background:#fff"></div></div></div></div></div>''' if content else ''
    return f'''<div data-ui="item-page" data-type="Movie" class="relative">
<div aria-hidden data-ui="item-backdrop" class="pointer-events-none absolute inset-x-0 top-[calc(var(--top-bar,0px)*-1)] h-[calc(55vh+var(--top-bar,0px))] overflow-hidden transition-opacity duration-500 [mask-image:linear-gradient(to_top,transparent,black_2px)] landscape:h-[calc(85vh+var(--top-bar,0px))] lg:h-[calc(85vh+var(--top-bar,0px))]">
<div data-ui="backdrop-frame" class="absolute inset-y-0 end-0 isolate w-full landscape:[mask-image:linear-gradient(to_right,transparent,black_12rem)] lg:[mask-image:linear-gradient(to_right,transparent,black_12rem)] max-w-[calc(55vh*2.6)] landscape:max-w-[calc(85vh*2.6)] lg:max-w-[calc(85vh*2.6)]">
<img data-ui="item-backdrop-image" src="{img}" alt="" class="absolute inset-0 h-full w-full object-cover object-top opacity-100"></div>
<div data-ui="item-backdrop-shade" data-name="side" class="absolute inset-0 hidden bg-gradient-to-r from-[--background] to-transparent landscape:block lg:block rtl:bg-gradient-to-l"></div>
<div data-ui="item-backdrop-shade" data-name="bottom" class="absolute inset-x-0 bottom-0 h-3/4 bg-[--background] [mask-image:linear-gradient(to_top,black,rgb(0_0_0/0.7),transparent)] max-sm:h-1/2"></div></div>{body}</div>'''

def corner(img):
    return f'''<div aria-hidden data-ui="hero-corner" class="pointer-events-none fixed end-0 top-0 -z-10 aspect-video max-h-[calc(var(--hero-h)*1.4)] w-[70vw] overflow-hidden [mask-image:linear-gradient(to_right,transparent,rgb(0_0_0/0.4)_20%,black_45%)] rtl:[mask-image:linear-gradient(to_left,transparent,rgb(0_0_0/0.4)_20%,black_45%)] transition-opacity duration-700"><div class="absolute inset-0 [mask-image:linear-gradient(to_bottom,black_45%,rgb(0_0_0/0.4)_75%,transparent)]"><img data-ui="hero-backdrop" src="{img}" class="absolute inset-0 h-full w-full object-cover object-top"></div></div>'''

def page(kind, white, sidebar):
    img=svg(white)
    if kind=='hero': inner=hero(img if not white else 'WHITE'+img)
    elif kind=='item': inner=item(img, content=not white)
    else: inner=corner(img)
    if kind=='hero' and white: inner=inner.replace('src="WHITE','src="')
    wrapper_open = '<div class="lg:[&>.appLayout]:ps-20"><div class="appLayout">' if sidebar else '<div><div class="appLayout">'
    style=':root{}' if kind!='corner' else ''
    hero_h = '' if kind!='corner' else 'style="--hero-h:calc(55dvh)"'
    return f'''<!doctype html><html lang="en" class="dark"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1.0"><link rel="stylesheet" href="{CSS}"><style>html::-webkit-scrollbar,body::-webkit-scrollbar{{display:none}}html,body{{margin:0;background:#070707;color:#e5e7eb;font-family:sans-serif}}</style></head><body {hero_h}>{wrapper_open}{inner}</div></div></body></html>'''

for kind in ['hero','item','corner']:
    for white in [False,True]:
        for sb in [True,False]:
            open(f'{kind}-{"white" if white else "grid"}-{"sb" if sb else "nosb"}.html','w').write(page(kind,white,sb))
print('ok')
