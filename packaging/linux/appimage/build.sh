#!/usr/bin/env sh
# Builds the apifast AppImage into target/packages: the interface and the
# command line, with the desktop entry, metainfo and icon of packaging/linux.
# Nothing else goes in: the binaries link only the C library, and what the
# interface loads at run time (Vulkan or EGL, Wayland or X11) must be the
# system's own, as the graphics drivers are.
#
# Needs appimagetool (https://github.com/AppImage/appimagetool), on the PATH
# or in $APPIMAGETOOL, and the AppImage runtime
# (https://github.com/AppImage/type2-runtime) in $APPIMAGE_RUNTIME: given, so
# appimagetool downloads nothing. packaging/linux/appimage/tools.sh fetches
# both, pinned and checked. Without FUSE, set APPIMAGE_EXTRACT_AND_RUN=1.
# `--appdir-only` stops once target/appimage/AppDir is assembled.
set -eu

cd "$(dirname "$0")/../../.."
id=com.tryxtek.apifast
linux=packaging/linux
appdir=target/appimage/AppDir
out=target/packages

cargo build --release --locked -p apifast-cli -p apifast-gui

rm -rf "$appdir"
install -Dm755 "$linux/appimage/AppRun" "$appdir/AppRun"
install -Dm755 target/release/apifast-gui "$appdir/usr/bin/apifast-gui"
install -Dm755 target/release/apifast "$appdir/usr/bin/apifast"
install -Dm644 "$linux/$id.desktop" "$appdir/usr/share/applications/$id.desktop"
install -Dm644 "$linux/$id.metainfo.xml" "$appdir/usr/share/metainfo/$id.metainfo.xml"
install -Dm644 "$linux/icons/hicolor/scalable/apps/$id.svg" "$appdir/usr/share/icons/hicolor/scalable/apps/$id.svg"
install -Dm644 LICENSE-MIT "$appdir/usr/share/licenses/apifast/LICENSE-MIT"
install -Dm644 LICENSE-APACHE "$appdir/usr/share/licenses/apifast/LICENSE-APACHE"
# appimagetool takes the desktop entry and the icon from the AppDir's root.
ln -s "usr/share/applications/$id.desktop" "$appdir/$id.desktop"
ln -s "usr/share/icons/hicolor/scalable/apps/$id.svg" "$appdir/$id.svg"
ln -s "$id.svg" "$appdir/.DirIcon"
# appimagetool looks for the metainfo by its former name only.
ln -s "$id.metainfo.xml" "$appdir/usr/share/metainfo/$id.appdata.xml"

if [ "${1:-}" = "--appdir-only" ]; then
    echo "$appdir"
    exit 0
fi

version=$(cargo metadata --no-deps --format-version 1 \
    | sed -n 's/.*"name":"apifast-gui","version":"\([^"]*\)".*/\1/p')
mkdir -p "$out"
# The metainfo is checked here, offline; appimagetool would also check that
# its URLs answer, which makes the build depend on the network.
if command -v appstreamcli >/dev/null; then
    appstreamcli validate --no-net "$appdir/usr/share/metainfo/$id.metainfo.xml"
fi
ARCH=x86_64 "${APPIMAGETOOL:-appimagetool}" --no-appstream --runtime-file "${APPIMAGE_RUNTIME:?the AppImage runtime file}" \
    "$appdir" "$out/apifast-$version-x86_64.AppImage"
ls -l "$out"
