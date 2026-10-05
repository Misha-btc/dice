#!/usr/bin/env python3
"""Regenerate the README figures: python3 docs/img/make_figures.py

Writes banner, roll-under and odds as -light/-dark SVG pairs (the README picks
one with <picture> + prefers-color-scheme). Colors follow the game UI's
palette; the chart hues were checked with the dataviz palette validator
(light #e63800/#00a565 on #fcfbf8, dark #f25022/#12a96c on #18171a: all pass).
The numbers come from the contract: 65,536 roll outcomes, 1% edge, targets
1..64,238 (x1.01 .. x64,880).
"""
import math
import os

OUT = os.path.dirname(os.path.abspath(__file__))
FONT = "Inter, -apple-system, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif"
MONO = "'JetBrains Mono', 'SF Mono', Menlo, Consolas, monospace"

THEMES = {
    "light": dict(bg="#fcfbf8", border="#ebe6dd", ink="#343433", ink2="#6b6a68",
                  muted="#9a9791", grid="#ebe6dd", accent="#e63800", win="#00a565",
                  lose="#ece7de", die2="#343433", pip="#fcfbf8", chip="#ffffff"),
    "dark": dict(bg="#18171a", border="#2d2b30", ink="#efece6", ink2="#b3b0aa",
                 muted="#807d78", grid="#2d2b30", accent="#f25022", win="#12a96c",
                 lose="#2e2c31", die2="#efece6", pip="#18171a", chip="#211f24"),
}

EDGE = 0.99
SPACE = 65_536
T_MAX = 64_238


def fmt(n):
    return f"{n:,}"


def svg(w, h, body, title):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" '
            f'viewBox="0 0 {w} {h}" role="img" aria-label="{title}">\n'
            f'<title>{title}</title>\n{body}\n</svg>\n')


def card(w, h, c):
    return (f'<rect x="1" y="1" width="{w - 2}" height="{h - 2}" rx="22" '
            f'fill="{c["bg"]}" stroke="{c["border"]}" stroke-width="2"/>')


def text(x, y, s, c, size=16, weight=400, color="ink", anchor="start", family=FONT, extra=""):
    return (f'<text x="{x}" y="{y}" font-family="{family}" font-size="{size}" '
            f'font-weight="{weight}" fill="{c[color]}" text-anchor="{anchor}" {extra}>{s}</text>')


def die(cx, cy, size, angle, fill, pip, pips):
    half = size / 2
    r = size * 0.075
    spots = {
        3: [(-1, -1), (0, 0), (1, 1)],
        5: [(-1, -1), (1, -1), (0, 0), (-1, 1), (1, 1)],
    }[pips]
    off = size * 0.27
    dots = "".join(
        f'<circle cx="{cx + dx * off:.1f}" cy="{cy + dy * off:.1f}" r="{r:.1f}" fill="{pip}"/>'
        for dx, dy in spots)
    return (f'<g transform="rotate({angle} {cx} {cy})">'
            f'<rect x="{cx - half}" y="{cy - half}" width="{size}" height="{size}" '
            f'rx="{size * 0.2:.1f}" fill="{fill}"/>{dots}</g>')


def banner(c):
    w, h = 1280, 320
    chips = ["1 tx = 1 bet = 1 result", "LP-owned bankroll", "immutable contracts"]
    x, parts = 384, []
    for label in chips:
        cw = 18 + len(label) * 9.6 + 18
        parts.append(f'<rect x="{x}" y="196" width="{cw:.0f}" height="38" rx="19" '
                     f'fill="{c["chip"]}" stroke="{c["border"]}" stroke-width="1.5"/>')
        parts.append(text(x + cw / 2, 221, label, c, 17, 500, "ink", "middle"))
        x += cw + 12
    body = "\n".join([
        card(w, h, c),
        die(176, 150, 150, -14, c["die2"], c["pip"], 3),
        die(236, 186, 150, 9, c["accent"], c["pip"], 5),
        text(384, 128, "Dice", c, 92, 800),
        text(386, 170, "Instant roll-under dice on Bitcoin · ALKANES metaprotocol", c, 26, 400, "ink2"),
        *parts,
        text(1236, 286, "roll = sha256(blockhash ‖ txid) mod 65,536", c, 16, 400, "muted", "end", MONO),
    ])
    return svg(w, h, body, "Dice — instant roll-under dice on Bitcoin")


def roll_under(c):
    w, h = 1200, 440
    x0, x1, y, bh = 80, 1120, 176, 46
    span = x1 - x0
    target = 32_768
    xt = x0 + span * target / SPACE
    roll = 21_403
    xr = x0 + span * roll / SPACE
    gap = 2
    payout = EDGE * SPACE / target
    ticks = "".join(
        text(x0 + span * v / SPACE, y + bh + 30, fmt(v), c, 14, 400, "muted", "middle")
        for v in (0, 16_384, 32_768, 49_152, 65_536))
    cols = [
        ("Win chance", "T ÷ 65,536", FONT),
        ("Payout, stake included", "stake × 0.99 × 65,536 ÷ T", FONT),
        ("The roll", "sha256(blockhash ‖ txid) mod 65,536", MONO),
    ]
    colx = [80, 420, 790]
    col_parts = []
    for (label, value, fam), cx in zip(cols, colx):
        col_parts.append(text(cx, 336, label, c, 15, 500, "ink2"))
        col_parts.append(text(cx, 366, value, c, 20 if fam == FONT else 17, 600, "ink", family=fam))
    body = "\n".join([
        card(w, h, c),
        text(80, 70, "Roll under your target", c, 28, 700),
        text(80, 100, "The block that mines your bet decides the roll — nobody can know it in advance.", c, 17, 400, "ink2"),
        f'<rect x="{x0}" y="{y}" width="{xt - x0 - gap / 2:.1f}" height="{bh}" rx="10" fill="{c["win"]}"/>',
        f'<rect x="{xt + gap / 2:.1f}" y="{y}" width="{x1 - xt - gap / 2:.1f}" height="{bh}" rx="10" fill="{c["lose"]}"/>',
        # white on the green fill in both themes (bold 18px clears 3:1)
        f'<text x="{(x0 + xt) / 2}" y="{y + 29}" font-family="{FONT}" font-size="18" '
        f'font-weight="700" fill="#ffffff" text-anchor="middle">WIN · roll &lt; T</text>',
        text((xt + x1) / 2, y + 29, "LOSE · roll ≥ T", c, 18, 700, "ink2", "middle"),
        f'<line x1="{xt:.1f}" y1="{y - 40}" x2="{xt:.1f}" y2="{y + bh + 8}" stroke="{c["ink"]}" stroke-width="2"/>',
        # one row: the roll's label runs left of its marker, the target's right of its line
        text(xt + 10, y - 26, f"your target T = {fmt(target)}: 50% chance, pays ×{payout:.2f}", c, 15, 600),
        f'<path d="M{xr - 8:.1f},{y - 16} L{xr + 8:.1f},{y - 16} L{xr:.1f},{y - 4} Z" fill="{c["ink"]}"/>',
        text(xr + 4, y - 26, f"this block's roll: {fmt(roll)} → win", c, 14, 500, "ink2", "end"),
        ticks,
        f'<line x1="80" y1="300" x2="1120" y2="300" stroke="{c["grid"]}" stroke-width="1"/>',
        *col_parts,
    ])
    return svg(w, h, body, "Roll under your target: win if the block's roll is below T")


def odds(c):
    w, h = 1200, 620
    px0, px1, py0, py1 = 120, 1110, 150, 500
    ly1 = 5.0   # log10 multiplier: x1 .. x100,000

    def X(p):   # win chance, linear 0..100%
        return px0 + (px1 - px0) * p

    def Y(m):   # multiplier, log scale
        return py1 - (py1 - py0) * math.log10(m) / ly1

    grid, labels = [], []
    for pct in (0, 25, 50, 75, 100):
        x = X(pct / 100)
        grid.append(f'<line x1="{x:.1f}" y1="{py0}" x2="{x:.1f}" y2="{py1}" stroke="{c["grid"]}" stroke-width="1"/>')
        labels.append(text(x, py1 + 26, f"{pct}%", c, 14, 400, "muted", "middle"))
    for d in range(int(ly1) + 1):
        y = Y(10 ** d)
        grid.append(f'<line x1="{px0}" y1="{y:.1f}" x2="{px1}" y2="{y:.1f}" stroke="{c["grid"]}" stroke-width="1"/>')
        labels.append(text(px0 - 14, y + 5, f"×{fmt(10 ** d)}", c, 14, 400, "muted", "end"))

    p_min, p_max = 1 / SPACE, T_MAX / SPACE
    near = [p_min * (0.02 / p_min) ** (i / 40) for i in range(41)]          # the steep longshot end
    rest = [0.02 + (p_max - 0.02) * i / 60 for i in range(1, 61)]
    line = " ".join(f"{X(p):.1f},{Y(EDGE / p):.1f}" for p in near + rest)
    area = f"{X(p_min):.1f},{py1} {line} {X(p_max):.1f},{py1}"

    # (win chance, label, sub-label, placement) — label the story points only.
    # Above the flat right end ("above"), right of the steep left end ("right").
    marks = [
        (p_min, "0.0015% → ×64,880", "target 1, the jackpot", "right"),
        (0.01, "1% → ×99", None, "right"),
        (0.10, None, None, None),
        (0.25, None, None, None),
        (0.50, "50% → ×1.98", "a coin flip", "above"),
        (p_max, "98% → ×1.01", "the safest bet", "above-end"),
    ]
    mark_parts = []
    for p, lab, sub, where in marks:
        x, y = X(p), Y(EDGE / p)
        mark_parts.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="6" fill="{c["accent"]}" '
                          f'stroke="{c["bg"]}" stroke-width="2.5"/>')
        if not lab:
            continue
        if where == "right":
            mark_parts.append(text(x + 14, y + 5, lab, c, 16, 700))
            if sub:
                mark_parts.append(text(x + 14, y + 25, sub, c, 14, 400, "ink2"))
        else:
            anchor = "middle" if where == "above" else "end"
            lx = x if anchor == "middle" else x + 6
            mark_parts.append(text(lx, y - 40, lab, c, 16, 700, "ink", anchor))
            mark_parts.append(text(lx, y - 20, sub, c, 14, 400, "ink2", anchor))

    body = "\n".join([
        card(w, h, c),
        text(80, 66, "Any odds, the same 1% edge", c, 28, 700),
        text(80, 96, "Payout multiplier (log scale) for every win chance: multiplier = 0.99 ÷ chance, "
                     "so on average every bet returns 99% of its stake.", c, 16, 400, "ink2"),
        *grid,
        f'<polygon points="{area}" fill="{c["accent"]}" fill-opacity="0.10"/>',
        f'<polyline points="{line}" fill="none" stroke="{c["accent"]}" stroke-width="2.5" '
        f'stroke-linejoin="round" stroke-linecap="round"/>',
        *mark_parts,
        *labels,
        text((px0 + px1) / 2, py1 + 58, "win chance", c, 15, 500, "ink2", "middle"),
        text(40, (py0 + py1) / 2, "payout multiplier", c, 15, 500, "ink2", "middle",
             extra=f'transform="rotate(-90 40 {(py0 + py1) / 2})"'),
    ])
    return svg(w, h, body, "Payout multiplier vs win chance: x1.01 at 98% up to x64,880 at 0.0015%")


def main():
    for name, make in (("banner", banner), ("roll-under", roll_under), ("odds", odds)):
        for mode, c in THEMES.items():
            path = os.path.join(OUT, f"{name}-{mode}.svg")
            with open(path, "w", encoding="utf-8") as f:
                f.write(make(c))
            print("wrote", os.path.relpath(path))


if __name__ == "__main__":
    main()
