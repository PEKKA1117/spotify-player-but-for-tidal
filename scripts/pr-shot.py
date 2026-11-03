#!/usr/bin/env python3
"""Screenshot a tmux pane for a PR's "Hands-on check" (see CLAUDE.md).

Usage:
  scripts/pr-shot.py NAME OUT_DIR TMUX_TARGET     capture the pane, keep NAME.ansi
  scripts/pr-shot.py NAME OUT_DIR --from FILE     re-render a kept .ansi capture

Writes OUT_DIR/NAME.png (and NAME.html, NAME.ansi next to it). Needs Python's
`rich`, `tmux`, `chromium` and ImageMagick's `magick`. Run it with `python3 -I`.

How it renders: `tmux capture-pane -e -p` gives the pane with its colours as
ANSI; `rich` parses it into styled segments; every character goes into a cell
of fixed width (1 cell for a narrow character, 2 for a wide one, as the
terminal counts them), so CJK rows line up whatever the font's widths are;
headless chromium screenshots the page, and `magick -trim` crops it.
(`rich`'s own `export_svg` lets CJK rows drift, which is why it is not used.)

The run, step by step (scenarios come from the spec's "Test plan" manual checks):

  # the maintainer's player holds the audio device: stop it, start it again after
  systemctl --user stop tidal-player
  B=<worktree>/target/release/tidal-player       # cargo build --release first
  S=<scratch dir>; mkdir -p -m 700 $S/state $S/shots $S/cfg
  cp ~/.config/tidal-player/app.toml $S/cfg/     # only app.toml: default keys and theme
  R=$(mktemp -d); chmod 700 $R
  E="env TIDAL_PLAYER_RUNTIME_DIR=$R TIDAL_PLAYER_STATE_DIR=$S/state TIDAL_PLAYER_MPRIS=off \
       TIDAL_PLAYER_REMEMBER_PLAYBACK=off TIDAL_PLAYER_COVER_IMG=off"
  tmux new-session -d -s tp -x 150 -y 42 "$E $B -c $S/cfg; sleep 600"
  # or a daemon and clients, when the spec's checks involve them:
  #   tmux new-session -d -s tp -n daemon -x 150 -y 42 "$E $B daemon -c $S/cfg; sleep 600"
  #   tmux new-window -t tp -n a "$E $B -c $S/cfg; sleep 600"
  #   $E $B playback load <track ids>             # one-shot commands, same runtime dir
  k() { for x in "$@"; do tmux send-keys -t tp "$x"; sleep 0.4; done; }
  k g s; tmux send-keys -t tp -l "query"; k Enter; sleep 3
  python3 -I scripts/pr-shot.py 01-search-results $S/shots tp
  # two panes side by side in one image:
  #   magick a.png b.png -background '#1e1e1e' -splice 0x16 -append -trim +repage ab.png
  # after: quit the TUIs, Ctrl-C the daemon, tmux kill-session -t tp, rm -rf $R,
  # systemctl --user start tidal-player
"""

import html
import os
import subprocess
import sys

from rich.cells import cell_len
from rich.console import Console
from rich.terminal_theme import SVG_EXPORT_THEME as THEME
from rich.text import Text

COLS = 150  # the check's terminal width
CW, LH, FS = 12, 24, 20  # cell width, line height, font size (px)
BG = THEME.background_color
FG = THEME.foreground_color


def hexc(c):
    return "#%02x%02x%02x" % tuple(c)


def mix(a, b, t):
    return tuple(round(x * (1 - t) + y * t) for x, y in zip(a, b))


def cell_css(style):
    """CSS for one styled segment: colours (reverse and dim applied), bold, italic."""
    fg = style.color.get_truecolor(THEME, True) if style and style.color else FG
    bg = style.bgcolor.get_truecolor(THEME, False) if style and style.bgcolor else None
    if style and style.reverse:
        fg, bg = (bg or BG), fg
    if style and style.dim:
        fg = mix(fg, bg or BG, 0.5)
    css = f"color:{hexc(fg)};" + (f"background:{hexc(bg)};" if bg else "")
    if style and style.bold:
        css += "font-weight:bold;"
    if style and style.italic:
        css += "font-style:italic;"
    return css


def render_rows(ansi):
    console = Console(width=COLS, force_terminal=True, color_system="truecolor", file=open("/dev/null", "w"))
    rows = []
    for line in Text.from_ansi(ansi.rstrip("\n")).split("\n", allow_blank=True):
        cells = []
        for seg in line.render(console):
            css = cell_css(seg.style)
            for ch in seg.text:
                w = cell_len(ch)
                if w == 0:
                    continue
                text = "&nbsp;" if ch == " " else html.escape(ch)
                cells.append(f'<span style="{css}width:{w * CW}px">{text}</span>')
        rows.append('<div class="r">' + "".join(cells) + "</div>")
    return rows


def page(name, rows):
    width = COLS * CW
    return f"""<html><head><meta charset="utf-8"><style>
body{{margin:0;background:#1e1e1e}}
.win{{display:inline-block;margin:0;background:{hexc(BG)};border-radius:10px;padding:0 0 14px 0;border:1px solid #3a3a3a}}
.bar{{height:44px;position:relative;font:bold 18px Arial,sans-serif;color:#c5c8c6;text-align:center;line-height:44px}}
.dot{{position:absolute;top:15px;width:14px;height:14px;border-radius:7px}}
.t{{padding:0 14px;width:{width}px;font-family:'DejaVu Sans Mono','Noto Sans Mono CJK TC',monospace;font-size:{FS}px}}
.r{{height:{LH}px;line-height:{LH}px;white-space:pre;display:flex}}
.r span{{display:inline-block;overflow:visible;text-align:left;flex:none}}
</style></head><body><div class="win"><div class="bar">
<span class="dot" style="left:18px;background:#ff5f57"></span><span class="dot" style="left:40px;background:#febc2e"></span><span class="dot" style="left:62px;background:#28c840"></span>{html.escape(name)}</div>
<div class="t">{"".join(rows)}</div></div></body></html>"""


def main(argv):
    if len(argv) == 4:
        name, out, target = argv[1:]
        ansi = subprocess.run(
            ["tmux", "capture-pane", "-t", target, "-e", "-p"], capture_output=True, text=True, check=True
        ).stdout
        with open(f"{out}/{name}.ansi", "w") as f:
            f.write(ansi)
    elif len(argv) == 5 and argv[3] == "--from":
        name, out = argv[1:3]
        with open(argv[4]) as f:
            ansi = f.read()
    else:
        sys.exit(__doc__.split("\n\n")[1])

    out = os.path.abspath(out)
    rows = render_rows(ansi)
    with open(f"{out}/{name}.html", "w") as f:
        f.write(page(name, rows))
    png = f"{out}/{name}.png"
    subprocess.run(
        ["chromium", "--headless", "--disable-gpu", "--hide-scrollbars", f"--screenshot={png}",
         f"--window-size={COLS * CW + 60},{len(rows) * LH + 120}", f"file://{out}/{name}.html"],
        capture_output=True,
        check=True,
    )
    subprocess.run(["magick", png, "-fuzz", "2%", "-trim", "+repage", png], check=True)
    print(png)


if __name__ == "__main__":
    main(sys.argv)
