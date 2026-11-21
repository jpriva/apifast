# Linux packaging

What apifast needs to be installed on Linux, shared by every package format,
and how the packages are built.

## Shared files

| File | Installed as | What it is |
|---|---|---|
| `com.tryxtek.apifast.desktop` | `/usr/share/applications/` | The menu entry. Runs `apifast-gui`. |
| `com.tryxtek.apifast.metainfo.xml` | `/usr/share/metainfo/` | AppStream metadata, for software centers and Flathub. |
| `icons/hicolor/scalable/apps/com.tryxtek.apifast.svg` | `/usr/share/icons/hicolor/scalable/apps/` | The icon. **A placeholder for now**: replace it with the real one, keeping its name. PNG sizes (`icons/hicolor/<n>x<n>/apps/com.tryxtek.apifast.png`) can be added next to it, and to the package assets. |

The application id is `com.tryxtek.apifast`: the desktop file, the AppStream id
and the window's app id (`with_app_id` in `apifast-gui`) all use it, so the
desktop shows the window with its entry and icon. Changing it breaks existing
installations.

Check them after a change:

```sh
desktop-file-validate packaging/linux/com.tryxtek.apifast.desktop
appstreamcli validate --no-net packaging/linux/com.tryxtek.apifast.metainfo.xml
```

## On this machine

To build apifast and use it, without root or a package:

```sh
packaging/linux/install-local.sh              # ~/.local/bin/apifast-gui and apifast, menu entry and icon
packaging/linux/install-local.sh --uninstall
```

`PREFIX` changes where (default: `~/.local`). Built here, it runs here: no
need for the glibc rules of the packages below. The `.rpm` (or `.deb`) built
by `build.sh` installs on this same system too (`sudo dnf install
./target/packages/apifast-<v>-1.x86_64.rpm`), and so does the AppImage, which
needs no installing.

## Packages

One package, `apifast`, holds the interface (`/usr/bin/apifast-gui`) and the
command line (`/usr/bin/apifast`).

```sh
cargo install --locked cargo-deb cargo-generate-rpm
packaging/linux/build.sh      # target/packages/apifast_<v>-1_amd64.deb and apifast-<v>-1.x86_64.rpm
```

- **.deb** with [cargo-deb](https://github.com/kornelski/cargo-deb), **.rpm** with
  [cargo-generate-rpm](https://github.com/cat-in-136/cargo-generate-rpm). Their
  settings are in `crates/apifast-gui/Cargo.toml` (`package.metadata.deb` and
  `package.metadata.generate-rpm`).
- The binaries link only the C library. What the interface loads at run time
  (Vulkan or EGL, Wayland or X11, xkbcommon) goes as recommended packages;
  xkbcommon is required.
- The binaries need the glibc of the system they were built on (a build on
  Fedora with glibc 2.44 does not run on Ubuntu 22.04). Release packages are
  built on the oldest system supported, Ubuntu 22.04 (glibc 2.35), which is
  what the `.deb` declares.
- The `.rpm` has one text for its summary and description: cargo-generate-rpm
  takes no longer description.

`packaging/linux/check-glibc.sh [max]` says which glibc each binary needs and
fails if it is newer than `max` (2.35 by default).

## In CI

`.github/workflows/packages.yml` builds the packages on Ubuntu 22.04 (glibc
2.35), checks the desktop entry, the metainfo and the glibc the binaries need,
and keeps the packages as an artifact of the run. It runs on changes to the
packaging, by hand, and on a tag `v<version>`, which must match the
workspace's version. Nothing is published anywhere.

## AppImage

```sh
eval "$(packaging/linux/appimage/tools.sh)"      # appimagetool and the runtime, pinned and checked
export APPIMAGETOOL APPIMAGE_RUNTIME
packaging/linux/appimage/build.sh                # target/packages/apifast-<v>-x86_64.AppImage
packaging/linux/appimage/build.sh --appdir-only  # only target/appimage/AppDir
```

- One file with the interface and the command line, and the same desktop
  entry, metainfo and icon. Nothing else is bundled: the binaries link only the
  C library, and the graphics libraries (Vulkan or EGL, Wayland or X11) must be
  the system's, as its drivers are.
- `AppRun` runs the interface, with its arguments (a collection folder). Called
  through a link named `apifast`, it runs the command line:
  `ln -s ~/Apps/apifast-x86_64.AppImage ~/.local/bin/apifast`.
- `tools.sh` fetches [appimagetool](https://github.com/AppImage/appimagetool)
  1.9.1 and the [AppImage runtime](https://github.com/AppImage/type2-runtime)
  20251108 into `target/appimage/tools`, and checks their SHA-256 before they
  can run. Both are the newest numbered releases; the `continuous` ones are
  rebuilt in place, so they cannot be pinned. The runtime is given to
  appimagetool, which then downloads nothing. To update, change the versions
  and hashes in `tools.sh` (each release asset lists its digest on GitHub).
- The metainfo is validated offline by `build.sh`; appimagetool's own check
  (`--no-appstream` turns it off) would also need its URLs to answer. It looks
  for the metainfo by its former name, `com.tryxtek.apifast.appdata.xml`, a
  link to it in the AppDir.
- Without FUSE, set `APPIMAGE_EXTRACT_AND_RUN=1`. As the other packages, a
  release AppImage is built on Ubuntu 22.04.

## Other formats

Flatpak will use the same desktop file, metainfo and icons, with its build
files in `flatpak/` when it is added.
