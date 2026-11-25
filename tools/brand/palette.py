"""Derive Veritas's text-safe brand shades.

Each brand colour keeps its OKLCH hue. Its lightness moves only as far as it
must for the shade to reach the target contrast on every surface it sits on;
chroma shrinks only when a shade would leave sRGB.

    python3 tools/brand/palette.py
"""

import math

BRAND = {
    "primary": 0x23169C,
    "secondary": 0x007EA7,
    "bad": 0xDB162F,
    "good": 0x56E39F,
    "warning": 0xF5B700,
}

LIGHT_SURFACES = {
    "window": (0xF3F3F6, 1.0),
    "grouped": (0xFCFCFD, 1.0),
    "raised": (0xFBFBFC, 1.0),
    "field": (0xFDFDFE, 1.0),
}
DARK_SURFACES = {
    "window": (0x1E1E22, 1.0),
    "grouped": (0x27272C, 1.0),
    "raised": (0x2E2E34, 1.0),
    "field": (0x1A1A1E, 1.0),
}

TEXT = 4.5
ACCENT_SOFT_LIGHT = 0.1
ACCENT_SOFT_DARK = 0.14
UI = 3.0
MARGIN = 0.1


def channels(hex_value):
    return [((hex_value >> shift) & 0xFF) / 255 for shift in (16, 8, 0)]


def to_hex(rgb):
    r, g, b = (max(0, min(255, round(c * 255))) for c in rgb)
    return (r << 16) | (g << 8) | b


def linear(c):
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def gamma(c):
    return 12.92 * c if c <= 0.0031308 else 1.055 * c ** (1 / 2.4) - 0.055


def luminance(hex_value):
    r, g, b = (linear(c) for c in channels(hex_value))
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contrast(a, b):
    high, low = sorted((luminance(a), luminance(b)), reverse=True)
    return (high + 0.05) / (low + 0.05)


def to_oklch(hex_value):
    r, g, b = (linear(c) for c in channels(hex_value))
    l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b
    m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b
    s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b
    l, m, s = (math.copysign(abs(v) ** (1 / 3), v) for v in (l, m, s))
    big_l = 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s
    a = 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s
    bb = 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s
    return big_l, math.hypot(a, bb), math.atan2(bb, a)


def from_oklch(big_l, chroma, hue):
    a, b = chroma * math.cos(hue), chroma * math.sin(hue)
    l = (big_l + 0.3963377774 * a + 0.2158037573 * b) ** 3
    m = (big_l - 0.1055613458 * a - 0.0638541728 * b) ** 3
    s = (big_l - 0.0894841775 * a - 1.2914855480 * b) ** 3
    rgb = (
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    )
    return rgb


def in_gamut(big_l, chroma, hue):
    return all(-1e-6 <= c <= 1 + 1e-6 for c in from_oklch(big_l, chroma, hue))


def shade(big_l, chroma, hue):
    while chroma > 0 and not in_gamut(big_l, chroma, hue):
        chroma -= 0.001
    return to_hex([gamma(max(0.0, min(1.0, c))) for c in from_oklch(big_l, chroma, hue)])


def worst(hex_value, surfaces):
    return min(contrast(hex_value, s) for s, _ in surfaces.values())


def derive(base, surfaces, target, lighter):
    if worst(base, surfaces) >= target + MARGIN:
        return base
    big_l, chroma, hue = to_oklch(base)
    step = 0.002 if lighter else -0.002
    while 0 < big_l < 1:
        big_l += step
        candidate = shade(big_l, chroma, hue)
        if worst(candidate, surfaces) >= target + MARGIN:
            return candidate
    raise ValueError(f"no shade of #{base:06X} reaches {target}")


def over(top, alpha, bottom):
    mixed = [t * alpha + b * (1 - alpha) for t, b in zip(channels(top), channels(bottom))]
    return to_hex(mixed)


def with_selection(surfaces, accent, alpha):
    """Rows tint with the accent at low alpha when selected, so text must read there too."""
    result = dict(surfaces)
    for name, (hex_value, _) in surfaces.items():
        result[f"{name} selected"] = (over(accent, alpha, hex_value), 1.0)
    return result


def main():
    accent_dark = derive(BRAND["primary"], DARK_SURFACES, TEXT, lighter=True)
    while True:
        dark = with_selection(DARK_SURFACES, accent_dark, ACCENT_SOFT_DARK)
        settled = derive(BRAND["primary"], dark, TEXT, lighter=True)
        if settled == accent_dark:
            break
        accent_dark = settled
    light = with_selection(LIGHT_SURFACES, BRAND["primary"], ACCENT_SOFT_LIGHT)
    print(f"{'colour':<10} {'brand':<8} {'light text':<11} {'min':>5} {'dark text':<10} {'min':>5}")
    for name, base in BRAND.items():
        light_text = derive(base, light, TEXT, lighter=False)
        dark_text = derive(base, dark, TEXT, lighter=True)
        print(
            f"{name:<10} #{base:06X}  #{light_text:06X}     {worst(light_text, light):5.2f} "
            f"#{dark_text:06X}    {worst(dark_text, dark):5.2f}"
        )
    print(f"\nselected rows: light #{light['grouped selected'][0]:06X}, dark #{dark['raised selected'][0]:06X}")


if __name__ == "__main__":
    main()
