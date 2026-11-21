#!/usr/bin/env sh
# Fetches appimagetool and the AppImage runtime into the folder given
# (target/appimage/tools by default), at the versions pinned here, and checks
# them against their SHA-256 before they can run. Prints the two variables
# packaging/linux/appimage/build.sh takes.
#
# To update: the release's assets list their digest on GitHub
# (api.github.com/repos/<repo>/releases/tags/<tag>). The "continuous"
# releases are rebuilt in place, so they are not pinned.
set -eu

APPIMAGETOOL_VERSION=1.9.1
APPIMAGETOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
RUNTIME_VERSION=20251108
RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d

dir=${1:-target/appimage/tools}
mkdir -p "$dir"
tool="$dir/appimagetool-$APPIMAGETOOL_VERSION-x86_64.AppImage"
runtime="$dir/runtime-$RUNTIME_VERSION-x86_64"

fetch() { # url, file, sha256
    if [ ! -f "$2" ] || ! echo "$3  $2" | sha256sum -c --status; then
        curl -sSfL -o "$2.part" "$1"
        echo "$3  $2.part" | sha256sum -c --quiet
        mv "$2.part" "$2"
    fi
}

fetch "https://github.com/AppImage/appimagetool/releases/download/$APPIMAGETOOL_VERSION/appimagetool-x86_64.AppImage" \
    "$tool" "$APPIMAGETOOL_SHA256"
fetch "https://github.com/AppImage/type2-runtime/releases/download/$RUNTIME_VERSION/runtime-x86_64" \
    "$runtime" "$RUNTIME_SHA256"
chmod +x "$tool"

echo "APPIMAGETOOL=$(cd "$dir" && pwd)/$(basename "$tool")"
echo "APPIMAGE_RUNTIME=$(cd "$dir" && pwd)/$(basename "$runtime")"
