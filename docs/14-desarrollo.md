# Desarrollo

Cómo ejecutar apifast desde el código, probarlo sin tocar los datos de una instalación, y qué comprobar antes de un commit. Para compilar en release y armar paquetes, ver [docs/13](13-compilar-y-empaquetar.md).

## Requisitos

- Las dependencias del sistema de [docs/13, "Dependencias del sistema"](13-compilar-y-empaquetar.md#dependencias-del-sistema).
- [rustup](https://rustup.rs). La versión de Rust la fija `rust-toolchain.toml`, y `rustup` la instala sola la primera vez que se compila.
- Para las comprobaciones: `cargo install --locked cargo-deny`.

## Ejecutar desde el código

```sh
cp .env.example .env                  # una vez: las variables de desarrollo (ver abajo)
tools/dev.sh                          # la interfaz, en debug
tools/dev.sh gui ~/mi-coleccion       # la interfaz, abriendo esa colección
tools/dev.sh --release                # la interfaz, optimizada
tools/dev.sh cli run ~/mi-coleccion   # la CLI, con sus argumentos
tools/dev.sh cli --help
```

`tools/dev.sh` carga el `.env` y ejecuta `cargo run` del paquete que toca (`apifast-gui` o `apifast-cli`). Se puede lanzar desde cualquier carpeta: las rutas que se le pasan son relativas a donde se está.

Al arrancar, dice en la terminal qué `.env` cargó y qué carpetas de datos y de ajustes usa:

```
dev.sh: env file: /home/…/apifast/.env
dev.sh: data: /home/…/apifast/target/dev/data
dev.sh: settings: /home/…/apifast/target/dev/config
```

**Debug o release.** La compilación debug tarda menos, pero la aplicación va más lenta: se nota en cuerpos grandes, en el resaltado y en los scripts. Para ver cómo se siente instalada, o para medir, se usa `--release` (o `APIFAST_DEV_RELEASE=1` en el `.env`).

Sin el script funciona igual, pero sin el `.env`:

```sh
cargo run -p apifast-gui                   # la interfaz
cargo run -p apifast-gui -- ~/mi-coleccion
cargo run -p apifast-cli -- --help         # la CLI
```

## El archivo `.env`

Guarda las variables con las que se ejecuta apifast en desarrollo. Se crea copiando `.env.example`, que documenta cada una. git ignora `.env`, así que cada uno tiene el suyo.

```sh
cp .env.example .env
```

Formato:

- Una variable por línea: `CLAVE=valor`. También se acepta `export CLAVE=valor`.
- El valor puede ir entre comillas, simples o dobles. Las comillas se quitan.
- Las líneas vacías y las que empiezan por `#` se ignoran.
- Una línea que no sea `CLAVE=valor` detiene el script y dice cuál es.

Reglas:

- **La terminal gana.** Si una variable ya tiene valor en la terminal, el `.env` no la cambia. Así se puede probar algo una vez sin editarlo: `APIFAST_TIMINGS=1 tools/dev.sh`.
- **Las rutas relativas son del repositorio.** `APIFAST_DATA_DIR=target/dev/data` es `<repositorio>/target/dev/data` aunque el script se lance desde otra carpeta.
- **Se lee, no se ejecuta.** El script no hace `source` del archivo: un `$(…)` o un `;` en un valor quedan como texto.
- **Otro archivo:** `APIFAST_ENV_FILE=~/otro.env tools/dev.sh`.
- **Solo lo lee `tools/dev.sh`.** Un apifast instalado no lee ningún `.env`: un `.env` en la carpeta de una colección no cambia cómo funciona.

Las variables:

| Variable | Para qué | En `.env.example` |
|---|---|---|
| `APIFAST_DATA_DIR` | Dónde van el historial, las respuestas, las sesiones, el espacio de trabajo y los archivos de la bóveda. | `target/dev/data` |
| `APIFAST_CONFIG_DIR` | Dónde van `settings.yml` y `keymap.yml`. | `target/dev/config` |
| `APIFAST_DEV_RELEASE` | `1`: compila optimizado, como `--release`. | `0` |
| `APIFAST_NO_KEYRING` | `1`: la clave maestra de la bóveda va en un archivo de la carpeta de datos, no en el llavero del sistema. Así las pruebas no llenan el llavero de claves. | `1` |
| `APIFAST_LOG` | El filtro del registro: un nivel, o `objetivo=nivel` separados por comas. Manda sobre el ajuste "Registro". Ver [El registro](#el-registro). | comentada |
| `APIFAST_TIMINGS` | `1`: escribe en la terminal cuánto tardaron el primer frame y el árbol de la colección. | comentada |
| `LC_MESSAGES` | El idioma de la interfaz cuando su ajuste dice "el del sistema": `en_US.UTF-8`, `es_CO.UTF-8`. No sirve `LANG`: la terminal siempre lo tiene, y gana. | comentada |
| `APIFAST_COLLECTION_KEY` | CLI: abre los secretos compartidos de una colección sin guardar su clave. | comentada |

## El registro

apifast escribe lo que hace, con niveles: `error`, `warn`, `info`, `debug` y `trace`. Todos los crates emiten eventos con `tracing` (`tracing::info!(…)`); solo `apifast-log` decide adónde van. También llegan los mensajes de las bibliotecas que usan el crate `log`, como wgpu, winit y reqwest.

**Adónde va:**

- **La interfaz** escribe en la terminal (con colores si es una) y en `logs/apifast.log`, dentro de la carpeta de datos. El archivo se mueve a `apifast.log.1` al pasar de 5 MB, así que nunca ocupa más de unos 10 MB. Con el `.env` de ejemplo queda en `target/dev/data/logs/apifast.log`; en la instalación, en `~/.local/share/apifast/logs/apifast.log`.
- **La CLI** solo escribe en la terminal, y solo si `APIFAST_LOG` tiene valor. Sin ella, su salida no cambia.

**Qué nivel:**

- Por defecto, `warn`: los errores y lo que puede ir mal.
- En la interfaz, Ajustes (`Ctrl+,`) → "Registro" cambia el nivel de los crates de apifast en el momento, y muestra la ruta del archivo. Las bibliotecas quedan en `warn`.
- `APIFAST_LOG` manda sobre el ajuste, y el ajuste se desactiva. Acepta un nivel (`APIFAST_LOG=debug`, que vale también para las bibliotecas) o una lista `objetivo=nivel` (`APIFAST_LOG=warn,apifast_gui=debug,apifast_http=trace`). Los objetivos son los nombres de los crates con `_`. Si no se entiende, se avisa y se usa el ajuste.

```sh
APIFAST_LOG=debug tools/dev.sh                          # todo en debug, también wgpu y reqwest
APIFAST_LOG=warn,apifast_http=debug tools/dev.sh cli send ping.yml
tail -f target/dev/data/logs/apifast.log                # lo que escribe la interfaz
```

**Qué escribe:**

- `info`: el arranque y el backend gráfico, las migraciones, las colecciones abiertas, cada envío (la ruta de la request, el estado, el tiempo y el tamaño), los commits y los cambios de rama.
- `warn`: los errores que muestra la interfaz (en su idioma), los envíos fallidos y las colecciones que no se pueden abrir.
- `debug`: cada envío en el adaptador HTTP, con el método y el host.

**Avisos de bibliotecas que no son problemas.** Algunos avisos de wgpu y de arboard son normales en muchos sistemas. `apifast-log` los cambia por un evento `info` que explica qué pasó y qué se hizo (el de la ventana transparente, que sí es un problema, se queda en `warn` y dice qué hacer), con el objetivo `apifast_log::explained`. Con el nivel por defecto (`warn`) no se ven; con `info` o más, sí:

| Aviso original | Por qué pasa | Lo que se escribe en su lugar |
|---|---|---|
| `wgpu_hal::vulkan`: `… vkCreateInstance in ICD /usr/lib64/libvulkan_dzn.so. Skipping this driver.`, y la línea `objects: …` que lo sigue | El cargador de Vulkan prueba todos los drivers instalados. Dozen (`dzn`, Vulkan sobre DirectX 12) solo funciona en Windows y WSL, así que lo descarta y se usa otro. | `vulkan: the driver libvulkan_dzn.so (Dozen, …) only works on Windows and WSL, so the Vulkan loader skipped it …`. Para que el cargador ni lo intente: desinstalarlo o `VK_LOADER_DRIVERS_DISABLE=*dzn*`. Otro driver que no arranca se explica igual, con su ruta. |
| `arboard`: `… Falling back to the X11 clipboard protocol …` | GNOME no ofrece el protocolo de portapapeles de Wayland para aplicaciones (`ext-data-control` o `wlr-data-control`), así que se usa el de XWayland. Copiar y pegar funcionan igual. | `clipboard: the desktop does not offer Wayland's clipboard protocol …` |
| `egui_wgpu`: `Transparent window was requested, but the active wgpu surface does not support a CompositeAlphaMode with transparency.` | El driver no puede dibujar una ventana transparente, que la barra de título propia necesita para sus esquinas y su sombra: se ven como un margen oscuro. Este sí es un problema, así que sigue en `warn`. | `window: the graphics driver cannot draw a transparent window, …` y cómo apagar "Draw the title bar in apifast". |

Se reconocen en `crates/apifast-log/src/explained.rs`: añadir otro es una regla más en `explain`, con su prueba. Los demás mensajes de las bibliotecas pasan tal cual.

**Lo que no escribe:** valores secretos, ni la URL completa de un envío (su query y su ruta pueden llevar tokens). Quien añade un evento cuida esto: los errores de apifast ya cumplen la regla, y se registra la ruta de la request o el host, no la URL. Las bibliotecas no siguen la regla: en `debug` o `trace`, reqwest y hyper pueden escribir URLs. Para un reporte de error, mejor subir solo los crates de apifast, con el ajuste o nombrándolos en `APIFAST_LOG`.

## Datos separados de la instalación

Sin `APIFAST_DATA_DIR` ni `APIFAST_CONFIG_DIR`, la versión de desarrollo usa las mismas carpetas que un apifast instalado: `~/.local/share/apifast` y `~/.config/apifast`. Comparte con él el historial, los ajustes, el espacio de trabajo y la bóveda. Además, como el desarrollo suele ir por delante, su versión puede migrar esos datos y el instalado los vería después como de una versión más nueva ([docs/13, "Los datos al actualizar"](13-compilar-y-empaquetar.md#los-datos-al-actualizar)).

Con el `.env` de ejemplo todo queda en `target/dev/`. Para empezar de cero:

```sh
rm -rf target/dev
```

Las colecciones no se copian: son carpetas del usuario, y la versión de desarrollo las edita igual que la instalada. Para probar sin miedo, se usa una copia.

## Colecciones para probar

- Una colección de Bruno en YAML se abre directamente. Una de Bruno en `.bru`, de Postman, de Insomnia o una especificación OpenAPI 3 se importa desde el menú de la colección, "Import…".
- `crates/apifast-storage-fs/tests/corpus/bruno/`: colecciones reales de Bruno que usan las pruebas de compatibilidad.
- Una colección grande, para el rendimiento:

  ```sh
  python3 tools/bench/make_collection.py /tmp/c1000 1000
  tools/dev.sh --release gui /tmp/c1000
  ```

## Antes de un commit

Lo mismo que comprueba el CI:

```sh
cargo fmt --all --check
rustfmt --edition 2024 --check crates/apifast-gui/src/strings/*.rs   # los idiomas, donde cargo fmt no llega
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p apifast-cli --no-default-features --all-targets -- -D warnings
cargo clippy -p apifast-gui --no-default-features --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo deny check
cargo test --workspace --no-fail-fast
```

Para ver solo el resumen de las pruebas, con los fallos:

```sh
cargo test --workspace --no-fail-fast 2>&1 \
  | grep -E "test result|FAILED|panicked|overflow|error: test failed|SIGABRT|^error" \
  | grep -v "panicked at crates/apifast-git-contract" \
  | awk '/test result/ {p+=$4; f+=$6} /FAILED|panicked|overflow|error: test failed|SIGABRT|^error/ {print} END {print "passed", p, "failed", f}'
```

Se buscan también `overflow` y `SIGABRT`: un binario de pruebas que se cae no escribe su `test result`, y sus pruebas no se cuentan. Las líneas `panicked at crates/apifast-git-contract` son esperadas: la batería de git comprueba que un adaptador roto falla.

### Las pruebas de la interfaz

Están en un solo binario, `crates/apifast-gui/tests/gui/`, y usan `egui_kittest`: dibujan la interfaz sin ventana y la recorren por el árbol de accesibilidad, buscando los textos de `strings::EN`. No abren nada en la pantalla.

```sh
cargo test -p apifast-gui --test gui              # todas
cargo test -p apifast-gui --test gui flow         # las que tienen "flow" en el nombre
```

Un texto nuevo de la interfaz va en `crates/apifast-gui/src/strings/en.rs` y en `es.rs` (ver [docs/09, "Idiomas"](09-ui-ux.md#idiomas)); las pruebas lo buscan por su campo (`EN.send`), no por la frase.

### Las capturas

`tests/gui/snapshots.rs` dibuja algunas partes de la interfaz con `wgpu`, en los dos temas, y compara cada imagen con la guardada en `crates/apifast-gui/tests/snapshots/`. Encuentran lo que el árbol de accesibilidad no ve:
- un elemento desalineado o un botón cortado;
- un glifo que falta en las fuentes: epaint entra en pánico solo al dibujarlo.

Las imágenes se dibujan con lavapipe, el Vulkan por CPU de Mesa, que `egui_kittest` prefiere si está instalado. En Fedora viene en `mesa-vulkan-drivers`, y en Debian y Ubuntu en el paquete del mismo nombre; el CI lo instala.

Otros drivers y sistemas dibujan algunos píxeles distintos. Por eso:
- en macOS y Windows estas pruebas se ignoran;
- en Linux sin lavapipe pueden fallar por diferencias mínimas.

Tras un cambio que altera el aspecto a propósito, se regeneran las imágenes y se revisan antes del commit:

```sh
UPDATE_SNAPSHOTS=1 cargo test -p apifast-gui --test gui snapshots
```

Si una falla, junto a la imagen quedan `<nombre>.new.png`, con lo que se dibujó, y `<nombre>.diff.png`, con las diferencias. Git las ignora.

## Medir el rendimiento

Las herramientas y cómo se usan están en `tools/bench/README.md`, y los presupuestos y las últimas mediciones en [docs/10](10-rendimiento-y-memoria.md). Siempre en release. Algunas abren la ventana real unos segundos.

## Qué funciona y qué no

En Linux, la aplicación está completa hasta la fase F10 del [roadmap](12-roadmap.md):

- Peticiones HTTP, WebSocket, SSE, gRPC y GraphQL.
- Entornos y variables, la bóveda de secretos (locales y compartidos con el equipo), cookies y autenticación (Basic, Bearer, API key, Digest, NTLM, AWS SigV4, OAuth 2.0).
- Aserciones, capturas y scripts en JavaScript compatibles con Bruno.
- El runner, con informes JUnit y JSON.
- Importar de Bruno, Postman, Insomnia, OpenAPI 3 y cURL, y exportar a Postman.
- git: estado, diff por campos, commit y cambio de rama.
- Interfaz en español y en inglés, versiones y actualizaciones, y paquetes `.rpm`, `.deb` y AppImage.

Lo que falta o tiene límites:

- **Windows y macOS:** compilan y pasan las pruebas en el CI, pero no se han probado en una máquina real ni tienen instalador.
- **Traducción:** los resultados de las aserciones ("expected eq 201, got 200") y los fallos de los scripts siguen en inglés.
- **Pendientes anotados en cada fase del roadmap:**
  - certificados PKCS#12 y claves cifradas;
  - Swagger 2;
  - `require` de librerías en los scripts;
  - scripts de gRPC;
  - la configuración de proxy del escritorio;
  - entre otros, anotados en el roadmap.
- **Falta la guía de usuario y la auditoría de seguridad de la bóveda.**
- **Uso diario:** las pruebas automáticas no cubren cómo se siente en el día a día. Lo que resulte raro o incómodo se anota y se corrige.
