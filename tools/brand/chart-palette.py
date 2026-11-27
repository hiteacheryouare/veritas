"""Search for Veritas's categorical chart palette.

The palette starts from Ryan's primary and secondary and adds hues that stay
clear of the money states (good, warning, bad), reach 3:1 on every chart
surface, and stay apart under normal vision and the three common colour-vision
deficiencies (Machado, Oliveira & Fernandes 2009, full severity).

    python3 tools/brand/chart-palette.py
"""

import importlib.util
import itertools
import math
import pathlib
import random

spec = importlib.util.spec_from_file_location("palette", pathlib.Path(__file__).with_name("palette.py"))
palette = importlib.util.module_from_spec(spec)
spec.loader.exec_module(palette)

LIGHT_CHART = [0xF3F3F6, 0xFCFCFD, 0xFBFBFC]
DARK_CHART = [0x1E1E22, 0x27272C, 0x2E2E34]

ANCHORS_LIGHT = [0x23169C, 0x007EA7]

# OKLCH hues of the money states, kept free so a category never reads as one.
MONEY_HUES = {"bad": (25, 26), "warning": (85, 20), "good": (160, 22)}

MACHADO = {
    "protan": ((0.152286, 1.052583, -0.204868), (0.114503, 0.786281, 0.099216), (-0.003882, -0.048116, 1.051998)),
    "deutan": ((0.367322, 0.860646, -0.227968), (0.280085, 0.672501, 0.047413), (-0.011820, 0.042940, 0.968881)),
    "tritan": ((1.255528, -0.076749, -0.178779), (-0.078411, 0.930809, 0.147602), (0.004733, 0.691367, 0.303900)),
}


def lab(hex_value, vision=None):
    r, g, b = (palette.linear(c) for c in palette.channels(hex_value))
    if vision:
        m = MACHADO[vision]
        r, g, b = (max(0.0, min(1.0, m[i][0] * r + m[i][1] * g + m[i][2] * b)) for i in range(3))
    l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b
    m_ = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b
    s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b
    l, m_, s = (math.copysign(abs(v) ** (1 / 3), v) for v in (l, m_, s))
    return (
        0.2104542553 * l + 0.7936177850 * m_ - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m_ + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m_ - 0.8086757660 * s,
    )


def distance(a, b, vision=None):
    return 100 * math.dist(lab(a, vision), lab(b, vision))


def worst_distance(colors):
    return min(distance(a, b, v) for a, b in itertools.combinations(colors, 2) for v in (None, "protan", "deutan", "tritan"))


def hue_clear(hex_value):
    _, chroma, hue = palette.to_oklch(hex_value)
    degrees = math.degrees(hue) % 360
    return chroma < 0.05 or all(min(abs(degrees - h), 360 - abs(degrees - h)) > clear for h, clear in MONEY_HUES.values())


# Where the search starts: hues chosen to sit with the indigo and cerulean
# anchors, as (OKLCH lightness, chroma, hue in degrees) for light mode. Dark
# mode starts from the same hues, lighter.
SEEDS = [
    (0.61, 0.15, 310),  # orchid
    (0.60, 0.13, 52),   # copper
    (0.42, 0.12, 340),  # plum
    (0.44, 0.08, 195),  # pine
    (0.50, 0.03, 255),  # slate
    (0.52, 0.09, 122),  # olive
]


HUE_DRIFT = 12


def make(big_l, chroma, hue):
    while chroma > 0 and not palette.in_gamut(big_l, chroma, math.radians(hue)):
        chroma -= 0.005
    return palette.to_hex([palette.gamma(max(0.0, min(1.0, c))) for c in palette.from_oklch(big_l, chroma, math.radians(hue))])


def allowed(hex_value, surfaces):
    return hue_clear(hex_value) and min(palette.contrast(hex_value, s) for s in surfaces) >= 3.1


def refine(anchors, seeds, surfaces, lightness_shift, band, fixed_hues=0):
    """Hill-climbs each seed's lightness, chroma and hue to push the closest pair apart."""
    points = [(l + lightness_shift, c, h) for l, c, h in seeds]
    def colors(ps):
        return anchors + [make(*q) for q in ps]
    def score(ps):
        # A harmonious band: no near-blacks, near-whites or neon.
        for (l, c, h), (_, seed_c, seed_h) in zip(ps, seeds):
            neutral = seed_c < 0.05
            drift = min(abs(h - seed_h), 360 - abs(h - seed_h))
            if not (band[0] <= l <= band[1] and drift <= HUE_DRIFT):
                return -math.inf
            if not ((0.015 <= c <= 0.045) if neutral else (0.06 <= c <= 0.16)):
                return -math.inf
        cs = colors(ps)
        if not all(allowed(c, surfaces) for c in cs[len(anchors):]):
            return -math.inf
        # A soft minimum, so raising any of several tied closest pairs counts.
        return -sum(math.exp(-distance(a, b, v) / 2) for a, b in itertools.combinations(cs, 2) for v in (None, "protan", "deutan", "tritan"))
    best = score(points)
    steps = [(d * k, 0, 0) for d in (0.01, 0.03) for k in (1, -1)] + [(0, d * k, 0) for d in (0.01, 0.03) for k in (1, -1)] + [(0, 0, d * k) for d in (3, 9) for k in (1, -1)]
    improved = True
    while improved:
        improved = False
        for i in range(len(points)):
            for dl, dc, dh in steps:
                trial = list(points)
                l, c, h = trial[i]
                if i < fixed_hues and (dh or dc):
                    continue
                trial[i] = (l + dl, max(0.0, c + dc), (h + dh) % 360)
                value = score(trial)
                if value > best + 1e-4:
                    points, best, improved = trial, value, True
    return colors(points), worst_distance(colors(points))


def lift(light, levels=(0.64, 0.68, 0.72, 0.76, 0.80, 0.84), tries=4000):
    """Dark mode keeps each light color's hue, caps chroma at 0.13 so nothing glows,
    and picks lightness from a few steps to push the closest pair apart."""
    rng = random.Random(7)
    lch = [palette.to_oklch(c) for c in light]
    best = None
    for _ in range(tries):
        steps = [rng.choice(levels) for _ in light]
        colors = [make(l, min(c, 0.13), math.degrees(h) % 360) for l, (_, c, h) in zip(steps, lch)]
        if not all(allowed(c, DARK_CHART) for c in colors):
            continue
        score = worst_distance(colors)
        if best is None or score > best[0]:
            best = (score, colors)
    return best[1], best[0]


DEBT_PAYDOWN = 0x8B5E34


def near_debt_paydown(hex_value):
    """Debt paydown is brown in every chart, so no category may read as it."""
    _, _, a = palette.to_oklch(hex_value)
    _, _, b = palette.to_oklch(DEBT_PAYDOWN)
    apart = abs(math.degrees(a) - math.degrees(b)) % 360
    return min(apart, 360 - apart) < 20


def ninth(light, dark):
    """The core has nine chart series. No ninth hue clears the money states and
    the distance floors, so the ninth reuses a hue at a lightness far from it."""
    best = None
    for hue in range(0, 360, 3):
        for big_l in (0.38, 0.42, 0.46, 0.5, 0.54, 0.58, 0.62, 0.66):
            for chroma in (0.03, 0.06, 0.09, 0.12, 0.15):
                l = make(big_l, chroma, hue)
                if not allowed(l, LIGHT_CHART) or near_debt_paydown(l):
                    continue
                _, c, h = palette.to_oklch(l)
                for dark_l in (0.64, 0.68, 0.72, 0.76, 0.80, 0.84):
                    d = make(dark_l, min(c, 0.13), math.degrees(h) % 360)
                    if not allowed(d, DARK_CHART):
                        continue
                    score = min(worst_distance(light + [l]) / 7, worst_distance(dark + [d]) / 5)
                    if best is None or score > best[0]:
                        best = (score, l, d)
    return best[1], best[2]


def main():
    light, light_score = refine(ANCHORS_LIGHT, SEEDS, LIGHT_CHART, 0, band=(0.38, 0.66))
    dark, dark_score = lift(light)
    extra_light, extra_dark = ninth(light, dark)
    light, dark = light + [extra_light], dark + [extra_dark]
    light_score, dark_score = worst_distance(light), worst_distance(dark)
    for name, colors, surfaces, score in (("light", light, LIGHT_CHART, light_score), ("dark", dark, DARK_CHART, dark_score)):
        print(f"{name}: closest pair under any vision {score:.1f} (OKLab x100)")
        pairs = sorted(((distance(a, b, v), v or "normal", a, b) for a, b in itertools.combinations(colors, 2) for v in (None, "protan", "deutan", "tritan")))[:3]
        for d, v, a, b in pairs:
            print(f"    close: #{a:06X} #{b:06X} {v} {d:.1f}")
        for c in colors:
            _, chroma, hue = palette.to_oklch(c)
            print(f"  #{c:06X}  hue {math.degrees(hue) % 360:5.1f}  contrast {min(palette.contrast(c, s) for s in surfaces):.2f}")


if __name__ == "__main__":
    main()
