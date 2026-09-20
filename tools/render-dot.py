#!/usr/bin/env python

"""Render .dot files to .png with graphviz.

Graphviz is not necessarily on PATH, so the renderer is looked for where it installs, and
--dot names it when it is somewhere else.
"""

import argparse
import shutil
import subprocess
import sys
import traceback

from pathlib import Path

WHERE_DOT_INSTALLS = (
    Path(r"C:/Program Files/Graphviz/bin/dot.exe"),
    Path(r"C:/Program Files (x86)/Graphviz/bin/dot.exe"),
)

###############################################################################
# Core functions
###############################################################################


def find_dot(named=None):
    if named:
        at = Path(named)
        if not at.is_file():
            raise FileNotFoundError(f"--dot names {at}, which is not a file")
        return at
    on_path = shutil.which("dot")
    if on_path:
        return Path(on_path)
    for at in WHERE_DOT_INSTALLS:
        if at.is_file():
            return at
    looked = ", ".join(str(at) for at in WHERE_DOT_INSTALLS)
    raise FileNotFoundError(f"no graphviz: not on PATH, and not at {looked}. Name it with --dot")


def render(paths, out_dir=None, dot=None):
    renderer = find_dot(dot)
    for name in paths:
        source = Path(name)
        if not source.is_file():
            raise FileNotFoundError(f"no such .dot file: {source}")
        target = (Path(out_dir) if out_dir else source.parent) / (source.stem + ".png")
        target.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run([str(renderer), "-Tpng", str(source), "-o", str(target)], check=True)
        print(target)


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("paths", nargs="+", help="The .dot files to render")
    parser.add_argument("--out-dir", default=None, help="Where the .png files go")
    parser.add_argument("--dot", default=None, help="Where graphviz's dot is, if not found")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    parser = get_parser()
    args = parser.parse_args(argv)
    try:
        render(args.paths, out_dir=args.out_dir, dot=args.dot)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
