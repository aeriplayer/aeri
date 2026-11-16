"""Generates aeri's logo: a round maple tabletop with a blue resin river through it.

Deterministic (seeded), so the output only changes when this script does.
Usage: python3 -I docs/assets/logo_v1.py docs/assets/logo-v1.svg
"""

import math
import random
import sys

SIZE = 512
CX = CY = SIZE / 2
RADIUS = 236

MAPLE_LIGHT = "#F1D3A2"
MAPLE = "#E3B97F"
MAPLE_EDGE = "#B8834A"
GRAIN_DARK = "#C9965A"
GRAIN_LIGHT = "#F6DEB4"
RESIN_DEEP = "#14558F"
RESIN = "#2A78BE"
RESIN_SHALLOW = "#5FA9E0"
RESIN_RIM = "#A9D6F5"

rng = random.Random(7)


BEND = 62


def centerline(t, bend=BEND):
    """The river's centre at t in [0, 1], running top-left to bottom-right in an S.

    Grain far from the river follows a gentler copy (smaller `bend`): offsetting the
    river's own curve that far would fold it into cusps.
    """
    x0, y0, x1, y1 = -40, 70, SIZE + 40, SIZE - 70
    x = x0 + (x1 - x0) * t
    y = y0 + (y1 - y0) * t + bend * math.sin(t * 2 * math.pi)
    return x, y


def normal(t, bend=BEND, eps=1e-3):
    ax, ay = centerline(max(t - eps, 0), bend)
    bx, by = centerline(min(t + eps, 1), bend)
    dx, dy = bx - ax, by - ay
    length = math.hypot(dx, dy)
    return -dy / length, dx / length


def wobble(seed, amplitude, waves=3):
    """A smooth random offset along the river: a few low-frequency sines."""
    r = random.Random(seed)
    terms = [(r.uniform(1, 5), r.uniform(0, 2 * math.pi), r.uniform(0.3, 1)) for _ in range(waves)]
    total = sum(w for _, _, w in terms)
    return lambda t: amplitude * sum(w * math.sin(f * 2 * math.pi * t + p) for f, p, w in terms) / total


def offset_curve(distance, wiggle, samples=60, bend=BEND):
    points = []
    for i in range(samples + 1):
        t = i / samples
        x, y = centerline(t, bend)
        nx, ny = normal(t, bend)
        d = distance(t) + wiggle(t)
        points.append((x + nx * d, y + ny * d))
    return points


def smooth_path(points, closed=False, digits=1):
    """Catmull-Rom through the points, as cubic Beziers."""
    pts = points + points[:3] if closed else [points[0]] + points + [points[-1]]
    f = f"{{:.{digits}f}}".format
    out = [f"M{f(pts[1][0])},{f(pts[1][1])}"]
    for i in range(1, len(pts) - 2):
        p0, p1, p2, p3 = pts[i - 1], pts[i], pts[i + 1], pts[i + 2]
        c1 = (p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6)
        c2 = (p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6)
        out.append(f"C{f(c1[0])},{f(c1[1])} {f(c2[0])},{f(c2[1])} {f(p2[0])},{f(p2[1])}")
    return " ".join(out) + (" Z" if closed else "")


def half_width(t):
    """The river widens and narrows as it crosses, like a real live-edge gap."""
    return 34 + 14 * math.sin(t * math.pi * 3 + 0.6) ** 2


left_edge = wobble(11, 7, waves=5)
right_edge = wobble(23, 7, waves=5)
left_bank = offset_curve(lambda t: -half_width(t), left_edge)
right_bank = offset_curve(half_width, right_edge)
river = smooth_path(left_bank + right_bank[::-1], closed=True)


def grain_lines():
    lines = []
    for side in (-1, 1):
        bank = left_edge if side < 0 else right_edge
        d = 0.0
        while d < 360:
            d += rng.uniform(7, 16)
            fade = d / 360
            bend = BEND * (1 - 0.8 * min(1, fade * 1.6))
            line_wobble = wobble(rng.randrange(10**6), 2 + 9 * fade, waves=4)
            dist = lambda t, d=d: side * (half_width(t) + d)
            points = offset_curve(
                dist, lambda t, lw=line_wobble, b=bank: b(t) * (1 - fade) + lw(t), samples=28, bend=bend
            )
            colour = GRAIN_DARK if rng.random() < 0.62 else GRAIN_LIGHT
            width = rng.uniform(0.8, 2.6)
            opacity = rng.uniform(0.35, 0.8)
            lines.append(
                f'<path d="{smooth_path(points, digits=0)}" stroke="{colour}" stroke-width="{width:.2f}" '
                f'stroke-opacity="{opacity:.2f}" fill="none" stroke-linecap="round"/>'
            )
    return "\n    ".join(lines)


def resin_streaks():
    streaks = []
    for i in range(5):
        frac = (i + 1) / 6 * 2 - 1
        wig = wobble(100 + i, 4)
        points = offset_curve(lambda t, f=frac: f * half_width(t) * 0.75, wig)
        streaks.append(
            f'<path d="{smooth_path(points)}" stroke="{RESIN_SHALLOW}" stroke-width="{rng.uniform(1, 2.5):.2f}" '
            f'stroke-opacity="{rng.uniform(0.15, 0.35):.2f}" fill="none"/>'
        )
    return "\n    ".join(streaks)


svg = f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {SIZE} {SIZE}" width="{SIZE}" height="{SIZE}" role="img" aria-labelledby="title">
  <title id="title">aeri</title>
  <defs>
    <clipPath id="top"><circle cx="{CX}" cy="{CY}" r="{RADIUS}"/></clipPath>
    <radialGradient id="maple" cx="42%" cy="38%" r="70%">
      <stop offset="0" stop-color="{MAPLE_LIGHT}"/>
      <stop offset="1" stop-color="{MAPLE}"/>
    </radialGradient>
    <linearGradient id="resin" gradientUnits="userSpaceOnUse" x1="150" y1="360" x2="362" y2="152">
      <stop offset="0" stop-color="{RESIN_SHALLOW}"/>
      <stop offset="0.35" stop-color="{RESIN}"/>
      <stop offset="0.5" stop-color="{RESIN_DEEP}"/>
      <stop offset="0.65" stop-color="{RESIN}"/>
      <stop offset="1" stop-color="{RESIN_SHALLOW}"/>
    </linearGradient>
    <radialGradient id="sheen" cx="35%" cy="30%" r="75%">
      <stop offset="0" stop-color="#FFFFFF" stop-opacity="0.22"/>
      <stop offset="0.6" stop-color="#FFFFFF" stop-opacity="0"/>
    </radialGradient>
  </defs>
  <circle cx="{CX}" cy="{CY}" r="{RADIUS + 10}" fill="{MAPLE_EDGE}"/>
  <g clip-path="url(#top)">
    <rect width="{SIZE}" height="{SIZE}" fill="url(#maple)"/>
    {grain_lines()}
    <path d="{river}" fill="url(#resin)"/>
    {resin_streaks()}
    <path d="{river}" fill="none" stroke="{RESIN_RIM}" stroke-width="3" stroke-opacity="0.55"/>
    <rect width="{SIZE}" height="{SIZE}" fill="url(#sheen)"/>
  </g>
  <circle cx="{CX}" cy="{CY}" r="{RADIUS}" fill="none" stroke="#FFFFFF" stroke-opacity="0.25" stroke-width="1.5"/>
</svg>
"""

with open(sys.argv[1], "w", encoding="utf-8") as out:
    out.write(svg)
