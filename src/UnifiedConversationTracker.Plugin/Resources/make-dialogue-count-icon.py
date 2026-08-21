#!/usr/bin/env python

"""Draws the left-speech-bubble icon the HUD dialogue count is prefixed with.

Solid white on transparent, no outline, so the game's own tint colour can be applied
to it at runtime. Drawn oversampled and downsampled, which is the anti-aliasing.
"""

import argparse
import sys
import traceback

# Pillow is not a dependency of this repo - nothing here is built from Python, and
# the icon it draws is committed. Install it only if the icon has to be redrawn.
from PIL import Image, ImageDraw  # ty: ignore[unresolved-import]

###############################################################################
# Core functions
###############################################################################

SIZE = 128
OVERSAMPLE = 8
WHITE = (255, 255, 255, 255)


def draw_bubble(size=SIZE, oversample=OVERSAMPLE):
    s = size * oversample
    img = Image.new("RGBA", (s, s), (255, 255, 255, 0))
    d = ImageDraw.Draw(img)

    def px(*values):
        # Coordinates below are written against a 128-unit square.
        return [v * s / SIZE for v in values]

    # The body: a rounded rectangle sitting in the upper four-fifths.
    d.rounded_rectangle(px(9, 14, 119, 92), radius=(22 * s / SIZE), fill=WHITE)

    # The tail: a spur off the bottom-left corner, pointing down and left.
    d.polygon(
        [
            tuple(px(30, 84)),
            tuple(px(14, 116)),
            tuple(px(62, 89)),
        ],
        fill=WHITE,
    )

    return img.resize((size, size), Image.LANCZOS)


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("out", help="Where to write the .png")
    parser.add_argument("--size", default=SIZE, type=int, help="Output edge length in pixels")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    try:
        icon = draw_bubble(size=args.size)
        icon.save(args.out)
        print(f"wrote {args.out} {icon.size}")
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
