# Builds the Windows packages of apifast into target\packages: a portable
# .zip and, with Inno Setup 6, an installer, each with the interface
# (apifast-gui.exe) and the command line (apifast.exe).
#
#   packaging\windows\build.ps1            # the .zip and the installer
#   packaging\windows\build.ps1 -NoBuild   # package what target\release has
#
# Needs Inno Setup 6 for the installer (`winget install JRSoftware.InnoSetup`);
# without it, only the .zip is made and the script fails at the end. ISCC
# names its compiler when it is not in the PATH or in its usual folders.
param(
    [switch]$NoBuild
)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..\..')

# Stops at the first command that fails, as `set -e` does.
function Invoke-Checked([string]$Command, [string[]]$Arguments) {
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command failed ($LASTEXITCODE)"
    }
}

if (-not $NoBuild) {
    # Every crate of Cargo.lock, also those of other systems: the build
    # script of apifast-gui reads them offline to list the licenses.
    Invoke-Checked cargo @('fetch', '--locked')
    Invoke-Checked cargo @('build', '--release', '--locked', '-p', 'apifast-cli', '-p', 'apifast-gui')
}

$metadata = cargo metadata --no-deps --format-version 1 --locked | ConvertFrom-Json
$version = ($metadata.packages | Where-Object name -eq 'apifast-gui').version
if (-not $version) {
    throw 'cargo metadata did not give the version of apifast-gui'
}

$out = 'target\packages'
$name = "apifast-$version-windows-x86_64"
$dir = Join-Path $out $name
if (Test-Path $dir) {
    Remove-Item -Recurse -Force $dir
}
New-Item -ItemType Directory -Force $dir | Out-Null
Copy-Item target\release\apifast-gui.exe, target\release\apifast.exe, LICENSE-MIT, LICENSE-APACHE $dir
Compress-Archive -Force -Path "$dir\*" -DestinationPath "$dir.zip"
Remove-Item -Recurse -Force $dir

$iscc = @(
    $env:ISCC,
    (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source,
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $iscc) {
    Get-ChildItem $out
    throw 'Inno Setup 6 (ISCC.exe) is needed for the installer: winget install JRSoftware.InnoSetup'
}
Invoke-Checked $iscc @("/DAppVersion=$version", '/Qp', 'packaging\windows\apifast.iss')

Get-ChildItem $out
