#!/usr/bin/env sh
# Builds apifast and installs it for the current user, without root: the
# interface and the command line in ~/.local/bin, and the menu entry, metainfo
# and icon in ~/.local/share. `--uninstall` removes them. PREFIX changes where
# (default: ~/.local).
set -eu

cd "$(dirname "$0")/../.."
id=com.tryxtek.apifast
linux=packaging/linux
prefix=${PREFIX:-$HOME/.local}
files="bin/apifast-gui bin/apifast share/applications/$id.desktop share/metainfo/$id.metainfo.xml share/icons/hicolor/scalable/apps/$id.svg"

refresh() {
    # The menu picks the change up. No icon cache is written: a stale one in
    # ~/.local would hide other applications' icons, and without one the
    # folder is read as it is.
    command -v update-desktop-database >/dev/null && update-desktop-database -q "$prefix/share/applications" || true
}

if [ "${1:-}" = "--uninstall" ]; then
    for file in $files; do
        rm -f "$prefix/$file"
    done
    refresh
    echo "apifast removed from $prefix"
    exit 0
fi

cargo build --release --locked -p apifast-cli -p apifast-gui
install -Dm755 target/release/apifast-gui "$prefix/bin/apifast-gui"
install -Dm755 target/release/apifast "$prefix/bin/apifast"
install -Dm644 "$linux/$id.desktop" "$prefix/share/applications/$id.desktop"
install -Dm644 "$linux/$id.metainfo.xml" "$prefix/share/metainfo/$id.metainfo.xml"
install -Dm644 "$linux/icons/hicolor/scalable/apps/$id.svg" "$prefix/share/icons/hicolor/scalable/apps/$id.svg"
refresh
echo "apifast installed in $prefix: run apifast-gui, or open it from the menu"
case ":$PATH:" in
    *":$prefix/bin:"*) ;;
    *) echo "note: $prefix/bin is not on your PATH" ;;
esac
