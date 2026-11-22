#!/usr/bin/env sh
# Builds apifast and installs it for the current user, without root: the
# interface and the command line in ~/.local/bin, and the menu entry, metainfo
# and icon in ~/.local/share. The binaries it replaces are kept as
# apifast-gui.prev and apifast.prev; `--rollback` puts them back.
# `--uninstall` removes everything. PREFIX changes where (default: ~/.local).
set -eu

cd "$(dirname "$0")/../.."
id=com.tryxtek.apifast
linux=packaging/linux
prefix=${PREFIX:-$HOME/.local}
sizes="16 24 32 48 64 128 256 512"
files="bin/apifast-gui bin/apifast share/applications/$id.desktop share/metainfo/$id.metainfo.xml share/icons/hicolor/scalable/apps/$id.svg"
for size in $sizes; do
    files="$files share/icons/hicolor/${size}x${size}/apps/$id.png"
done

refresh() {
    # The menu picks the change up. No icon cache is written: a stale one in
    # ~/.local would hide other applications' icons, and without one the
    # folder is read as it is.
    command -v update-desktop-database >/dev/null && update-desktop-database -q "$prefix/share/applications" || true
}

version_of() { # binary
    "$1" --version 2>/dev/null | cut -d' ' -f2- || echo "?"
}

case "${1:-}" in
    --uninstall)
        for file in $files bin/apifast-gui.prev bin/apifast.prev; do
            rm -f "$prefix/$file"
        done
        refresh
        echo "apifast removed from $prefix"
        exit 0
        ;;
    --rollback)
        if [ ! -x "$prefix/bin/apifast.prev" ] || [ ! -x "$prefix/bin/apifast-gui.prev" ]; then
            echo "no earlier version kept in $prefix/bin" >&2
            exit 1
        fi
        from=$(version_of "$prefix/bin/apifast")
        for bin in apifast apifast-gui; do
            mv -f "$prefix/bin/$bin.prev" "$prefix/bin/$bin"
        done
        echo "apifast $from -> $(version_of "$prefix/bin/apifast") (rolled back)"
        exit 0
        ;;
esac

cargo build --release --locked -p apifast-cli -p apifast-gui
from=""
if [ -x "$prefix/bin/apifast" ] && [ -x "$prefix/bin/apifast-gui" ]; then
    # What it replaces, to roll back to.
    from=$(version_of "$prefix/bin/apifast")
    cp -f "$prefix/bin/apifast" "$prefix/bin/apifast.prev"
    cp -f "$prefix/bin/apifast-gui" "$prefix/bin/apifast-gui.prev"
fi
built=${CARGO_TARGET_DIR:-target}/release
install -Dm755 "$built/apifast-gui" "$prefix/bin/apifast-gui"
install -Dm755 "$built/apifast" "$prefix/bin/apifast"
install -Dm644 "$linux/$id.desktop" "$prefix/share/applications/$id.desktop"
install -Dm644 "$linux/$id.metainfo.xml" "$prefix/share/metainfo/$id.metainfo.xml"
install -Dm644 "$linux/icons/hicolor/scalable/apps/$id.svg" "$prefix/share/icons/hicolor/scalable/apps/$id.svg"
for size in $sizes; do
    icon=icons/hicolor/${size}x${size}/apps/$id.png
    install -Dm644 "$linux/$icon" "$prefix/share/$icon"
done
refresh
to=$(version_of "$prefix/bin/apifast")
if [ -n "$from" ]; then
    echo "apifast $from -> $to in $prefix (the earlier one is kept: --rollback)"
else
    echo "apifast $to installed in $prefix: run apifast-gui, or open it from the menu"
fi
case ":$PATH:" in
    *":$prefix/bin:"*) ;;
    *) echo "note: $prefix/bin is not on your PATH" ;;
esac
