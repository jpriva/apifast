#!/usr/bin/env sh
# Makes every size and format of apifast's icon from its source,
# packaging/linux/icons/hicolor/scalable/apps/com.tryxtek.apifast.svg:
#
#   linux/icons/hicolor/<n>x<n>/apps/com.tryxtek.apifast.png  for desktops that
#       do not draw SVG (X11, some docks), and the window's own icon (128)
#   windows/apifast.ico   16 to 256 px in one file
#   macos/icon-1024.png   what the .icns is made from (docs/13)
#
# What it writes is kept in git: building apifast does not need these tools.
# Run it again after changing the SVG. Needs ImageMagick 7 (`magick`) built
# with librsvg, the renderer the desktop uses; on Fedora,
# `sudo dnf install ImageMagick`.
set -eu

cd "$(dirname "$0")"
id=com.tryxtek.apifast
svg=linux/icons/hicolor/scalable/apps/$id.svg
sizes="16 24 32 48 64 128 256 512"

command -v magick >/dev/null || {
    echo "magick (ImageMagick 7) is needed" >&2
    exit 1
}
magick -list format | grep -q 'RSVG' || {
    echo "magick has no librsvg: it would draw the icon with its own SVG renderer" >&2
    exit 1
}

# One size, as an 8-bit RGBA PNG without dates, so the same SVG gives the
# same bytes.
png() { # size output
    magick -background none -density 384 "RSVG:$svg" -resize "$1x$1" \
        -strip -define png:color-type=6 -define png:bit-depth=8 \
        -define png:exclude-chunks=date,time "$2"
}

for size in $sizes; do
    dir=linux/icons/hicolor/${size}x${size}/apps
    mkdir -p "$dir"
    png "$size" "$dir/$id.png"
done

mkdir -p windows macos
png 1024 macos/icon-1024.png
magick $(for size in 16 24 32 48 64 128 256; do echo "linux/icons/hicolor/${size}x${size}/apps/$id.png"; done) \
    windows/apifast.ico

echo "icons made from $svg: hicolor $sizes, windows/apifast.ico, macos/icon-1024.png"
