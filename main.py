"""Live Wallpaper - launcher shim.

The real app lives in the wallmotion package (split from the monolithic
main.py with no behavior change). Run: python main.py

Qt is imported lazily so that --version/--help work even where Qt
cannot load (headless machines, missing system GL libraries).
"""

import sys


def main():
    if any(a in ("--version", "--help", "-h") for a in sys.argv[1:]):
        from wallmotion import cli as cli_mod
        try:
            args = cli_mod.parse_args(sys.argv[1:])
        except SystemExit as e:
            sys.exit(e.code)  # --help already printed
        if getattr(args, "version", False):
            from wallmotion.utils import app_version
            print(f"WallMotion {app_version()}")
            return
    from wallmotion.ui import main as ui_main
    ui_main()


if __name__ == "__main__":
    main()
