#!/usr/bin/env bash
# Package the AppDir built by build-appdir.sh into a self-contained .deb.
#
# Usage: make-deb.sh <version> [AppDir] [outdir]
#
# The whole bundle is installed under /opt/wallmotion and launched through
# AppRun, so bundled libraries (libmpv, ffmpeg, GTK, ...) are used instead of
# whatever the distro happens to provide.
set -euo pipefail

version="${1:?usage: make-deb.sh <version> [AppDir] [outdir]}"
appdir="${2:-AppDir}"
outdir="${3:-dist}"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

if [ ! -x "$appdir/AppRun" ]; then
    echo "missing $appdir/AppRun (run build-appdir.sh first)" >&2
    exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
pkg="$work/wallmotion"

install -d "$pkg/DEBIAN" "$pkg/opt" "$pkg/usr/bin" \
    "$pkg/usr/share/applications" \
    "$pkg/usr/share/icons/hicolor/256x256/apps"

cp -a "$appdir" "$pkg/opt/wallmotion"

cat > "$pkg/usr/bin/wallmotion-settings" <<'EOF'
#!/bin/sh
if [ -x /opt/wallmotion/usr/bin/ffmpeg ]; then
    export WALLMOTION_FFMPEG=/opt/wallmotion/usr/bin/ffmpeg
fi
exec /opt/wallmotion/AppRun "$@"
EOF
chmod 755 "$pkg/usr/bin/wallmotion-settings"

install -m 644 installer/linux/wallmotion-settings.desktop "$pkg/usr/share/applications/"
install -m 644 assets/icon.png "$pkg/usr/share/icons/hicolor/256x256/apps/wallmotion-settings.png"

installed_kb="$(du -sk "$pkg" | cut -f1)"

cat > "$pkg/DEBIAN/control" <<EOF
Package: wallmotion
Version: $version
Architecture: amd64
Section: graphics
Priority: optional
Maintainer: Jurek1357 <https://github.com/Jurek1357/WallMotion-PC>
Installed-Size: $installed_kb
Depends: libc6, libgcc-s1
Recommends: xdg-utils
Homepage: https://github.com/Jurek1357/WallMotion-PC
Description: Video and image wallpapers for your desktop
 Settings window with a wallpaper library, YouTube downloads, rotation,
 per-monitor selection and autostart.
 .
 Bundles libmpv, ffmpeg, libxdo and the GTK runtime, so no distro
 multimedia packages are needed.
EOF

install -d "$outdir"
deb="$outdir/wallmotion_${version}_amd64.deb"
dpkg-deb --build --root-owner-group "$pkg" "$deb" >/dev/null
echo "$deb"
