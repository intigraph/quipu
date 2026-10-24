#!/usr/bin/env python3
"""Record the README demo: run real `qp` commands, write a cast and an animated SVG.

Every line of command output in the recording is what the installed release
printed. Only the typing pace of the commands and the pauses are synthetic.

    python3 docs/demos/readme-episode/record.py [--qp PATH]

Writes session.cast, terminal.svg and shop.nt next to this script. The store is
created in a throwaway directory with a throwaway HOME, so no user configuration
is read and nothing outside that directory is written.
"""

import argparse
import html
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
FIXTURES = ("trace.json", "shapes.ttl", "guess.ttl")
CALLS = ("PREFIX shop: <https://example.org/shop/> "
         "PREFIX prov: <http://www.w3.org/ns/prov#> "
         "PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#> "
         "SELECT ?service ?calls ?source WHERE { ?s shop:calls ?o ; rdfs:label ?service ; "
         "prov:wasGeneratedBy/prov:wasAssociatedWith ?source . ?o rdfs:label ?calls } "
         "ORDER BY ?calls")
BEFORE = ("PREFIX shop: <https://example.org/shop/> "
          "PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#> "
          "SELECT ?service ?calls WHERE { ?s shop:calls ?o ; rdfs:label ?service . "
          "?o rdfs:label ?calls }")

# A known stderr notice, true but beside the point here (this throwaway store has no
# embedding model). session.cast keeps it; terminal.svg leaves it out, and the
# demo README says so.
SVG_OMIT = "automatic embedding unavailable"

# (narration, command as shown, argv)
STEPS = [
    ("An agent traced one checkout request and records what it saw.",
     "qp episode trace.json --base-ns https://example.org/shop/ "
     "--timestamp 2026-01-05T09:00:00Z --db shop.db",
     ["episode", "trace.json", "--base-ns", "https://example.org/shop/",
      "--timestamp", "2026-01-05T09:00:00Z", "--db", "shop.db"]),
    ("It guesses a cache exists, with no description. The rule refuses it.",
     "qp knot guess.ttl --shapes shapes.ttl --db shop.db",
     ["knot", "guess.ttl", "--shapes", "shapes.ttl", "--db", "shop.db"]),
    ("Any agent can read it back with its source. $CALLS: who calls whom, from where.",
     'qp read "$CALLS" --db shop.db',
     ["read", CALLS, "--db", "shop.db"]),
    ("Time is part of every fact. $BEFORE: the same question, before the trace.",
     'qp read "$BEFORE" --valid-at 2026-01-04T00:00:00Z --db shop.db',
     ["read", BEFORE, "--valid-at", "2026-01-04T00:00:00Z", "--db", "shop.db"]),
]

COLS, ROWS = 100, 28
TYPE_CPS = 45        # synthetic typing speed, characters per second
PAUSE_AFTER = 1.6    # seconds after each step's output
HOLD_END = 4.0       # seconds the final frame stays before the loop restarts


def run_steps(qp):
    """Run every step for real; return [(narration, shown, stderr, stdout, rc)]."""
    out = []
    with tempfile.TemporaryDirectory(prefix="quipu-readme-demo-") as tmp:
        work = Path(tmp)
        home = work / "home"
        home.mkdir()
        for name in FIXTURES:
            shutil.copy(HERE / name, work / name)
        env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": str(home),
               "LANG": "C.UTF-8", "NO_COLOR": "1"}
        version = subprocess.run([qp, "--version"], capture_output=True, text=True,
                                 env=env, cwd=work, check=True).stdout.splitlines()[0]
        for narration, shown, argv in STEPS:
            proc = subprocess.run([qp, *argv], capture_output=True, text=True,
                                  env=env, cwd=work)
            out.append((narration, shown, proc.stderr, proc.stdout, proc.returncode))
        export = subprocess.run([qp, "export", "--format", "ntriples", "--db", "shop.db"],
                                capture_output=True, text=True, env=env, cwd=work, check=True)
        (HERE / "shop.nt").write_text(export.stdout)
    return version, out


def expand(text):
    return [line.expandtabs(8) for line in text.rstrip("\n").splitlines()]


def timeline(results):
    """Return [(time, kind, text)], where kind is note|cmd|err|out|bad."""
    t, events = 0.6, []
    for narration, shown, stderr, stdout, _rc in results:
        if events:
            events.append((t, "out", ""))
        events.append((t, "note", "# " + narration))
        t += 0.5
        events.append((t, "cmd", shown))
        t += len(shown) / TYPE_CPS + 0.4
        for line in expand(stderr):
            bad = "failed" in line or "Violation" in line
            events.append((t, "bad" if bad else "err", line))
            t += 0.05
        for line in expand(stdout):
            kind = "bad" if ("failed" in line or "Violation" in line) else "out"
            events.append((t, kind, line))
            t += 0.08
        t += PAUSE_AFTER
    return events, t + HOLD_END


def write_cast(events, total, version):
    header = {"version": 2, "width": COLS, "height": ROWS,
              "title": f"Quipu: an agent records an episode ({version})",
              "env": {"TERM": "xterm-256color"}}
    lines = [json.dumps(header)]
    for t, kind, text in events:
        if kind == "cmd":
            for i, ch in enumerate("$ " + text):
                lines.append(json.dumps([round(t + i / TYPE_CPS, 3), "o", ch]))
            lines.append(json.dumps([round(t + (len(text) + 2) / TYPE_CPS, 3), "o", "\r\n"]))
        else:
            lines.append(json.dumps([round(t, 3), "o", text + "\r\n"]))
    lines.append(json.dumps([round(total, 3), "o", ""]))
    (HERE / "session.cast").write_text("\n".join(lines) + "\n")


COLOURS = {"note": "#8b8fa8", "cmd": "#f2f2f7", "err": "#5f6280",
           "out": "#d6d7e6", "bad": "#ff6b6b"}


def write_svg(events, total, version):
    """One line per event; each appears at its time, then everything loops."""
    events = [e for e in events if not e[2].startswith(SVG_OMIT)]
    lh, cw, pad, top = 19, 8.4, 18, 46
    # Scroll by rows: keep the last ROWS lines visible, as a terminal would.
    width = int(COLS * cw + 2 * pad)
    height = top + ROWS * lh + pad
    css, body = [], []
    pct = lambda t: round(100 * t / total, 3)
    for i, (t, kind, text) in enumerate(events):
        start = pct(t)
        if kind == "cmd":
            # Reveal the typed command left to right with a clip that widens in steps.
            n = len(text) + 2
            end = pct(t + n / TYPE_CPS)
            css.append(f"@keyframes c{i}{{0%,{start}%{{width:0}}{end}%,98%{{width:{n * cw:.1f}px}}"
                       f"99%,100%{{width:0}}}}")
            css.append(f".c{i}{{animation:c{i} {total:.2f}s steps({n},end) infinite}}")
        css.append(f"@keyframes l{i}{{0%,{round(max(start - 0.01, 0), 3)}%{{opacity:0}}{start}%,98%{{opacity:1}}"
                   f"99%,100%{{opacity:0}}}}")
        css.append(f".l{i}{{animation:l{i} {total:.2f}s linear infinite}}")
    # Lines scroll up when the screen is full: shift the whole text block.
    shifts, row = [], 0
    for i, (t, _kind, _text) in enumerate(events):
        if row >= ROWS:
            shifts.append((t, row - ROWS + 1))
        row += 1
    scroll = [f"0%{{transform:translateY(0)}}"]
    for t, rows_up in shifts:
        p = pct(t)
        scroll.append(f"{round(max(p - 0.01, 0), 3)}%{{transform:translateY({-(rows_up - 1) * lh}px)}}")
        scroll.append(f"{p}%{{transform:translateY({-rows_up * lh}px)}}")
    scroll.append(f"98%{{transform:translateY({-(shifts[-1][1] if shifts else 0) * lh}px)}}")
    scroll.append("99%,100%{transform:translateY(0)}")
    css.append("@keyframes scroll{" + "".join(scroll) + "}")
    css.append(f".screen{{animation:scroll {total:.2f}s steps(1,end) infinite}}")
    css.append("@media (prefers-reduced-motion: reduce){.screen,[class^=l],[class^=c]"
               "{animation:none!important;opacity:1!important}}")
    for i, (t, kind, text) in enumerate(events):
        y = top + (i + 1) * lh - 5
        colour = COLOURS[kind]
        if kind == "cmd":
            body.append(f'<clipPath id="k{i}"><rect class="c{i}" x="{pad}" y="{y - lh + 4}" '
                        f'height="{lh}" width="{(len(text) + 2) * cw:.1f}"/></clipPath>')
            body.append(f'<text class="l{i}" x="{pad}" y="{y}" clip-path="url(#k{i})">'
                        f'<tspan fill="#ffc24d">$ </tspan><tspan fill="{colour}">'
                        f'{html.escape(text)}</tspan></text>')
        else:
            body.append(f'<text class="l{i}" x="{pad}" y="{y}" fill="{colour}">'
                        f'{html.escape(text)}</text>')
    label = (f"A terminal recording of {version}. An agent records a traced checkout request "
             "as an episode with its source; a guessed service with no description is refused "
             "by a rule; a query returns each dependency with the trace it came from; and a "
             "read at a time before the trace returns nothing.")
    svg = f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" role="img" aria-label="{html.escape(label)}">
<style>
text{{font-family:JetBrains Mono,Menlo,Consolas,monospace;font-size:13.5px;white-space:pre}}
{chr(10).join(css)}
</style>
<rect width="{width}" height="{height}" rx="10" fill="#14141f"/>
<rect width="{width}" height="32" rx="10" fill="#20203a"/><rect y="22" width="{width}" height="10" fill="#20203a"/>
<circle cx="20" cy="16" r="6" fill="#ff5f57"/><circle cx="40" cy="16" r="6" fill="#febc2e"/><circle cx="60" cy="16" r="6" fill="#28c840"/>
<text x="{width / 2}" y="21" fill="#8b8fa8" text-anchor="middle" style="font-size:12px">{html.escape(version)}: an agent records an episode</text>
<clipPath id="term"><rect x="0" y="34" width="{width}" height="{height - 34}"/></clipPath>
<g clip-path="url(#term)"><g class="screen">
{chr(10).join(body)}
</g></g>
</svg>
"""
    (HERE / "terminal.svg").write_text(svg)


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--qp", default=shutil.which("qp") or shutil.which("quipu"))
    args = ap.parse_args()
    if not args.qp:
        sys.exit("record.py: qp (or quipu) is not on PATH; pass --qp")
    version, results = run_steps(args.qp)
    expected = [0, 1, 0, 0]
    got = [rc for *_, rc in results]
    if got != expected:
        sys.exit(f"record.py: step exit codes {got}, expected {expected}; not writing assets")
    events, total = timeline(results)
    write_cast(events, total, version)
    write_svg(events, total, version)
    print(f"wrote session.cast, terminal.svg and shop.nt ({version}, {total:.1f}s loop)")


if __name__ == "__main__":
    main()
