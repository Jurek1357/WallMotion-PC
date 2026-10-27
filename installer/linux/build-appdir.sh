#!/usr/bin/env bash
# Build a self-contained AppDir for wallmotion-settings with linuxdeploy.
#
# Usage: build-appdir.sh [AppDir] [appimage-output]
#
# The AppDir bundles libmpv, ffmpeg, libxdo, the GTK runtime and their
# non-system dependencies. It is used both for the AppImage and for the
# self-contained .deb (see make-deb.sh).
set -euo pipefail

appdir="${1:-AppDir}"
image="${2:-}"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

bin="wallmotion-rs/target/release/wallmotion-settings"
if [ ! -x "$bin" ]; then
    echo "missing $bin (build with: cargo build -p wallmotion-settings --release)" >&2
    exit 1
fi

for lib in /usr/lib/x86_64-linux-gnu/libmpv.so.2 \
           /usr/lib/x86_64-linux-gnu/libxdo.so.3; do
    if [ ! -e "$lib" ]; then
        echo "missing $lib (install libmpv-dev and libxdo-dev)" >&2
        exit 1
    fi
done

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

cp assets/icon.png "$work/wallmotion-settings.png"

wget -q https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage -O "$work/linuxdeploy"
wget -q https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh -O "$work/linuxdeploy-plugin-gtk.sh"
chmod +x "$work/linuxdeploy" "$work/linuxdeploy-plugin-gtk.sh"

extra_libs=()
for lib in /usr/lib/x86_64-linux-gnu/libayatana-appindicator3.so.1 \
           /usr/lib/x86_64-linux-gnu/libappindicator3.so.1; do
    if [ -e "$lib" ]; then
        extra_libs+=(-l "$lib")
        break
    fi
done

args=(--appdir "$appdir"
    -e "$bin"
    -e /usr/bin/ffmpeg
    -l /usr/lib/x86_64-linux-gnu/libmpv.so.2
    -l /usr/lib/x86_64-linux-gnu/libxdo.so.3
    "${extra_libs[@]}"
    -d installer/linux/wallmotion-settings.desktop
    -i "$work/wallmotion-settings.png"
    --plugin gtk)

export DEPLOY_GTK_VERSION=3
export PATH="$work:$PATH"

"$work/linuxdeploy" "${args[@]}"

if [ ! -x "$appdir/AppRun" ] || [ ! -x "$appdir/usr/bin/wallmotion-settings" ]; then
    echo "linuxdeploy did not produce a usable AppDir at $appdir" >&2
    exit 1
fi

missing="$(ldd "$appdir/usr/bin/wallmotion-settings" | grep 'not found' || true)"
if [ -n "$missing" ]; then
    echo "unresolved libraries in AppDir:" >&2
    echo "$missing" >&2
    exit 1
fi

if [ -n "$image" ]; then
    OUTPUT="$image" "$work/linuxdeploy" --appdir "$appdir" --output appimage
fi
