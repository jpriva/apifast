# 13. Compilar y empaquetar

Los comandos para compilar apifast y armar sus paquetes a mano: `.rpm`, `.deb` y AppImage en Linux, y la aplicación en Windows y macOS. No se publica nada en ningún sitio: todo queda en `target/`.

Todos los comandos se ejecutan desde la raíz del repositorio. Los de Linux están probados en Fedora 45 y en un contenedor de Ubuntu 22.04. Los de Windows y macOS no se han ejecutado en esas máquinas: el CI compila y prueba el código en las dos, pero no arma ningún paquete.

## Qué se compila

| Binario | Qué es |
|---|---|
| `apifast-gui` | La interfaz de escritorio |
| `apifast` | La línea de comandos (`apifast send`, `apifast run`, `apifast git`…) |

Las features de Cargo `grpc` y `git` vienen activas por defecto. Para dejar alguna fuera:

```sh
cargo build --release --locked -p apifast-cli -p apifast-gui --no-default-features --features grpc   # sin git
cargo build --release --locked -p apifast-cli -p apifast-gui --no-default-features                   # sin gRPC ni git
```

## Requisitos comunes

- **Rust con rustup.** La versión está fijada en `rust-toolchain.toml` (1.99.1). Se instala sola con:

  ```sh
  rustup toolchain install
  ```

- **Un compilador de C.** QuickJS, el motor de los scripts, se compila desde C.
- **`git`**, solo para las pruebas: crean repositorios con él. La aplicación no lo necesita.

## Linux

### Dependencias del sistema

Fedora:

```sh
sudo dnf install gcc binutils curl desktop-file-utils appstream
```

Ubuntu o Debian:

```sh
sudo apt install build-essential pkg-config binutils curl desktop-file-utils appstream
```

`desktop-file-utils` y `appstream` sirven para validar la entrada de menú y el metainfo. `curl` lo usa la herramienta del AppImage, y `objdump` (de `binutils`) la comprobación de glibc.

### Compilar

```sh
cargo build --release --locked -p apifast-cli -p apifast-gui
target/release/apifast --version
target/release/apifast-gui
```

### Instalar en la propia máquina, sin root

Compila e instala en `~/.local`: los binarios en `~/.local/bin`, y la entrada de menú, el metainfo y el icono en `~/.local/share`.

```sh
packaging/linux/install-local.sh
packaging/linux/install-local.sh --uninstall
PREFIX=/opt/apifast packaging/linux/install-local.sh   # en otro sitio
```

`~/.local/bin` tiene que estar en el `PATH` para lanzar `apifast` y `apifast-gui` desde una terminal.

### `.rpm` y `.deb`

Las herramientas, una vez:

```sh
cargo install --locked cargo-deb@3.8.0 cargo-generate-rpm@0.21.1
```

Validar la entrada de menú y el metainfo:

```sh
desktop-file-validate packaging/linux/com.tryxtek.apifast.desktop
appstreamcli validate --no-net packaging/linux/com.tryxtek.apifast.metainfo.xml
```

Armar los dos paquetes:

```sh
packaging/linux/build.sh
```

Esto deja `target/packages/apifast-<versión>-1.x86_64.rpm` y `target/packages/apifast_<versión>-1_amd64.deb`. Cada uno lleva `apifast-gui` y `apifast` en `/usr/bin`, la entrada de menú, el metainfo, el icono y las licencias.

Ver qué contienen:

```sh
rpm -qpi target/packages/apifast-*.rpm                 # metadatos
rpm -qpl target/packages/apifast-*.rpm                 # archivos
rpm -qp --requires target/packages/apifast-*.rpm       # dependencias
dpkg-deb --info target/packages/apifast_*.deb          # en Debian/Ubuntu
dpkg-deb --contents target/packages/apifast_*.deb
```

Instalar y desinstalar:

```sh
sudo dnf install ./target/packages/apifast-*.x86_64.rpm   # Fedora
sudo dnf remove apifast
sudo apt install ./target/packages/apifast_*_amd64.deb    # Ubuntu/Debian
sudo apt remove apifast
```

### Paquetes que sirvan en otros sistemas: compilar en Ubuntu 22.04

Un binario necesita la glibc del sistema donde se compiló. Si se compila en Fedora 45 (glibc 2.44), no arranca en Ubuntu 22.04 (glibc 2.35). Para que los paquetes sirvan en cualquier sistema con glibc 2.35 o más nueva, se compilan en Ubuntu 22.04. Es lo que hace el workflow `.github/workflows/packages.yml`, y también se puede hacer en local con un contenedor.

Comprobar qué glibc piden los binarios:

```sh
packaging/linux/check-glibc.sh 2.35    # falla si alguno pide una más nueva
```

Con podman (o docker), en un contenedor de Ubuntu 22.04. El código se copia dentro del contenedor, así sus binarios no se mezclan con los de `target/`:

```sh
mkdir -p /tmp/apifast-out
tar --exclude=./target --exclude=./.git -cf /tmp/apifast-src.tar .
cat > /tmp/apifast-build.sh <<'EOF'
set -eu
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq --no-install-recommends file ca-certificates curl build-essential pkg-config binutils desktop-file-utils appstream
curl -sSf https://sh.rustup.rs | sh -s -- -y -q --profile minimal --default-toolchain none
. "$HOME/.cargo/env"
mkdir -p /work && cd /work && tar -xf /in/src.tar
rustup toolchain install
cargo install --locked cargo-deb@3.8.0 cargo-generate-rpm@0.21.1
packaging/linux/build.sh
export APPIMAGE_EXTRACT_AND_RUN=1
eval "$(packaging/linux/appimage/tools.sh)"
export APPIMAGETOOL APPIMAGE_RUNTIME
packaging/linux/appimage/build.sh
packaging/linux/check-glibc.sh 2.35
cp target/packages/* /out/
EOF
podman run --rm \
  -v /tmp/apifast-src.tar:/in/src.tar:ro,Z \
  -v /tmp/apifast-build.sh:/in/build.sh:ro,Z \
  -v /tmp/apifast-out:/out:Z \
  docker.io/library/ubuntu:22.04 sh /in/build.sh
ls -l /tmp/apifast-out
```

Tarda unos 5 minutos la primera vez: descarga la imagen, Rust y las crates.

### AppImage

Las herramientas (`appimagetool` 1.9.1 y el runtime del AppImage 20251108) se descargan con su versión y su SHA-256 fijados en `packaging/linux/appimage/tools.sh`, y no se ejecutan si el hash no coincide:

```sh
eval "$(packaging/linux/appimage/tools.sh)"
export APPIMAGETOOL APPIMAGE_RUNTIME
packaging/linux/appimage/build.sh
```

Esto deja `target/packages/apifast-<versión>-x86_64.AppImage`. Para armar solo el AppDir, sin empaquetarlo:

```sh
packaging/linux/appimage/build.sh --appdir-only   # target/appimage/AppDir
```

Usarlo:

```sh
chmod +x target/packages/apifast-*-x86_64.AppImage
target/packages/apifast-*-x86_64.AppImage                       # la interfaz
ln -s "$PWD"/target/packages/apifast-*-x86_64.AppImage ~/.local/bin/apifast
apifast --version                                                # la CLI, por el enlace
```

En un sistema sin FUSE (contenedores, algunos CI), se ejecuta con `APPIMAGE_EXTRACT_AND_RUN=1`, tanto `appimagetool` como el AppImage resultante.

## Windows

Sin probar en una máquina Windows: el CI compila y pasa las pruebas en `windows-latest`, pero todavía no hay instalador.

### Requisitos

1. [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) con la carga de trabajo **"Desarrollo para el escritorio con C++"**: el enlazador MSVC, el SDK de Windows y el compilador de C para QuickJS.
2. [rustup](https://rustup.rs) (`rustup-init.exe`), con la toolchain por defecto `x86_64-pc-windows-msvc`.
3. Git, para clonar el repositorio.

### Compilar (PowerShell)

```powershell
rustup toolchain install
cargo build --release --locked -p apifast-cli -p apifast-gui
.\target\release\apifast.exe --version
.\target\release\apifast-gui.exe
```

### Armar un `.zip` portable

```powershell
$version = (cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).packages |
    Where-Object name -eq 'apifast-gui' | Select-Object -ExpandProperty version
$dir = "target\packages\apifast-$version-windows-x86_64"
New-Item -ItemType Directory -Force $dir | Out-Null
Copy-Item target\release\apifast-gui.exe, target\release\apifast.exe, LICENSE-MIT, LICENSE-APACHE $dir
Compress-Archive -Force -Path "$dir\*" -DestinationPath "$dir.zip"
```

Para usarlo, se descomprime donde se quiera. Para lanzar `apifast` desde cualquier terminal, se añade esa carpeta al `PATH` del usuario.

### Pendiente en Windows

- **La interfaz abre también una ventana de consola**: `apifast-gui` no declara `windows_subsystem = "windows"` todavía.
- No tiene icono en el `.exe` ni entrada en el menú Inicio. Tampoco hay instalador (MSI o MSIX) ni firma de código; sin firma, SmartScreen puede avisar la primera vez.

## macOS

Sin probar en un Mac: el CI compila y pasa las pruebas en `macos-latest`, pero todavía no arma la aplicación.

### Requisitos

```sh
xcode-select --install                                  # herramientas de línea de comandos de Xcode
curl -sSf https://sh.rustup.rs | sh                     # rustup
rustup toolchain install
```

### Compilar

Para el procesador del Mac donde se compila:

```sh
cargo build --release --locked -p apifast-cli -p apifast-gui
target/release/apifast --version
```

Un binario universal (Apple Silicon e Intel):

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --locked -p apifast-cli -p apifast-gui --target aarch64-apple-darwin
cargo build --release --locked -p apifast-cli -p apifast-gui --target x86_64-apple-darwin
mkdir -p target/universal
for bin in apifast apifast-gui; do
  lipo -create -output target/universal/$bin \
    target/aarch64-apple-darwin/release/$bin target/x86_64-apple-darwin/release/$bin
done
lipo -info target/universal/apifast-gui
```

### Armar `apifast.app`

Con el binario de `target/release` (o, si se hizo, el universal de `target/universal`):

```sh
version=$(cargo metadata --no-deps --format-version 1 \
  | python3 -c "import json,sys; print(next(p['version'] for p in json.load(sys.stdin)['packages'] if p['name']=='apifast-gui'))")
app=target/packages/apifast.app
rm -rf "$app" && mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/apifast-gui "$app/Contents/MacOS/"
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>com.tryxtek.apifast</string>
  <key>CFBundleName</key><string>apifast</string>
  <key>CFBundleDisplayName</key><string>apifast</string>
  <key>CFBundleExecutable</key><string>apifast-gui</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF
plutil -lint "$app/Contents/Info.plist"
codesign --force --deep --sign - "$app"                 # firma ad hoc, sin cuenta de desarrollador
open "$app"
```

El icono va en `Contents/Resources/apifast.icns`, con `<key>CFBundleIconFile</key><string>apifast</string>` en el `Info.plist`. Se genera desde un PNG de 1024×1024 cuando exista el icono definitivo:

```sh
mkdir apifast.iconset
for size in 16 32 128 256 512; do
  sips -z $size $size icon-1024.png --out apifast.iconset/icon_${size}x${size}.png
  sips -z $((size*2)) $((size*2)) icon-1024.png --out apifast.iconset/icon_${size}x${size}@2x.png
done
iconutil -c icns apifast.iconset -o "$app/Contents/Resources/apifast.icns"
```

### La CLI y un `.dmg`

```sh
sudo cp target/release/apifast /usr/local/bin/          # la CLI en el PATH
hdiutil create -volname apifast -srcfolder target/packages/apifast.app \
  -ov -format UDZO target/packages/apifast-$version-macos.dmg
```

### Pendiente en macOS

- **Sin firma de desarrollador ni notarización**, Gatekeeper bloquea la aplicación al abrirla desde un archivo descargado. Para usarla igual: clic derecho y "Abrir", o `xattr -dr com.apple.quarantine apifast.app`. Firmar y notarizar requiere una cuenta de Apple Developer.
- No tiene icono hasta que exista el definitivo.

## Comprobar lo construido

```sh
target/release/apifast --version
target/release/apifast git --help
packaging/linux/check-glibc.sh 2.35      # Linux: la glibc que piden los binarios
ls -l target/packages                    # los paquetes armados
```
