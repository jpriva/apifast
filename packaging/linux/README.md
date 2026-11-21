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

## Other formats

AppImage and Flatpak use the same desktop file, metainfo and icons; each gets
its own folder here (`appimage/`, `flatpak/`) with its build files when it is
added.
