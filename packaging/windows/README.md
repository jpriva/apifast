# Windows packaging

What apifast needs to be installed on Windows, and how its packages are built.

| File | What it is |
|---|---|
| `apifast.ico` | The icon, 16 to 256 px, made from the SVG by `packaging/icons.sh`. The build scripts of `apifast-gui` and `apifast-cli` put it in the executables (`crates/apifast-cli/build_resources.rs`), and the installer uses it as its own. |
| `apifast.iss` | The installer, for [Inno Setup 6](https://jrsoftware.org/isinfo.php). |
| `build.ps1` | Builds apifast and makes the portable `.zip` and the installer. |

## Building

Inno Setup 6, once:

```powershell
winget install JRSoftware.InnoSetup
```

Then, from anywhere in the repository:

```powershell
packaging\windows\build.ps1            # builds, then packages
packaging\windows\build.ps1 -NoBuild   # packages what target\release has
```

It leaves in `target\packages`:

- `apifast-<version>-windows-x86_64.zip`: the two executables and the licenses, to unpack anywhere.
- `apifast-<version>-windows-x86_64-setup.exe`: the installer.

Without Inno Setup the `.zip` is still made, and the script fails saying what is missing. `$env:ISCC` names the compiler (`ISCC.exe`) when it is somewhere unusual.

## What the installer does

- Installs for the current user, without administrator rights, in `%LOCALAPPDATA%\Programs\apifast`. Its first dialog offers installing for all users instead (in `C:\Program Files\apifast`), which asks for them.
- Adds apifast to the Start menu and, if chosen, to the desktop.
- Adds its folder to the `PATH` (a task, on by default), so `apifast` runs from any terminal opened afterwards: the user's `PATH`, or the machine's when installed for all users. Uninstalling removes it.
- Registers an uninstaller in "Installed apps".
- Speaks English and Spanish, following the system.

Running a newer installer updates what is installed: the `AppId` of `apifast.iss` is what tells Windows it is the same application, so it must not change.

Silent, for scripts: `apifast-<version>-windows-x86_64-setup.exe /VERYSILENT /CURRENTUSER`.

## Pending

- Code signing: without it, SmartScreen may warn the first time the installer runs.
- Building the installer in CI (`.github/workflows/packages.yml` makes only the Linux packages).
