# 2. Análisis de Bruno y Yaak

Este documento resume qué hace bien cada proyecto, qué hace mal y qué adopta apifast. Las rutas son relativas a `/home/jpriva/dev/apitest/`.

- Bruno: commit `09625f67c`, licencia MIT (`bruno/license.md`).
- Yaak: commit `6a34d0a4`, licencia MIT (`yaak/LICENSE`). Solo los binarios precompilados de Yaak requieren licencia comercial.

## Comparativa general

| Aspecto | Bruno | Yaak | apifast |
|---|---|---|---|
| Tecnología | Electron + React + Redux | Tauri (webview) + React + núcleo Rust | Rust + egui, sin webview |
| Fuente de verdad | Archivos | SQLite; los archivos son un espejo | Archivos |
| Formato en disco | `.bru` (DSL) y YAML OpenCollection | Un YAML plano por modelo: `yaak.<id>.yaml` | YAML OpenCollection |
| Jerarquía en disco | Carpetas = directorios | Directorio plano; jerarquía por `folderId` | Carpetas = directorios |
| Secretos | Solo nombres en disco; valores locales | Cifrados dentro de los archivos | Ambos modelos |
| Cifrado | safeStorage; respaldo AES-CBC con IV fijo | XChaCha20-Poly1305, clave en keyring | XChaCha20-Poly1305, clave en keyring |
| Scripting | JavaScript (QuickJS o Node vm) | No tiene; funciones de plantilla por plugin | Declarativo; JavaScript después |
| Plugins | No | Proceso Node permanente | No en 1.0 |
| CLI | Sí, con motor duplicado | Sí, comparte crates | Sí, mismo motor |
| Autenticación | Nativa en el proceso principal | Toda por plugins Node | Nativa en Rust |

## Bruno

### Lo que se adopta

| Idea | Dónde está en Bruno |
|---|---|
| Un archivo por request, carpetas como directorios, orden con `seq` | `bruno/packages/bruno-filestore/src/formats/yml/` |
| Formato YAML OpenCollection (`opencollection.yml`, `folder.yml`) | `bruno/packages/bruno-filestore/src/formats/yml/stringifyCollection.ts` |
| Sin identificadores de request en disco; UID en memoria por ruta | `bruno/packages/bruno-electron/src/cache/requestUids.js` |
| Herencia de headers, variables y auth (colección → carpeta → request) | `bruno/packages/bruno-electron/src/utils/collection.js` |
| Cadena de precedencia de variables | `bruno/packages/bruno-electron/src/ipc/network/interpolate-vars.js` |
| Interpolación `{{var}}` recursiva con protección de ciclos | `bruno/packages/bruno-common/src/interpolate/index.ts` |
| Namespace `{{process.env.X}}` con `.env` por capas | `bruno/packages/bruno-electron/src/store/process-env.js` |
| Entornos con `extends` | `bruno/packages/bruno-common/src/utils/environment-inheritance.ts` |
| Variables dinámicas `{{$randomUUID}}` | `bruno/packages/bruno-common/src/utils/faker-functions.ts` |
| Variables de pregunta `{{?Texto}}` | `bruno/packages/bruno-common/src/utils/prompt-variables.ts` |
| Variables tipadas (número, booleano, objeto) | `bruno/packages/bruno-tests/yml-collection/opencollection.yml` |
| Aserciones declarativas con lista fija de operadores | `bruno/packages/bruno-js/src/runtime/assert-runtime.js` |
| Capturas de variables desde la respuesta | `bruno/packages/bruno-filestore/src/formats/yml/common/actions.ts` |
| CLI con reporters JUnit, JSON y HTML, tags y `--bail` | `bruno/packages/bruno-cli/src/commands/run.js` |
| Carga progresiva y watcher con espera de escritura estable | `bruno/packages/bruno-electron/src/app/collection-watcher.js` |
| Certificados cliente por dominio | `bruno/packages/bruno-electron/src/ipc/network/cert-utils.js` |
| Importadores (Postman, Insomnia, OpenAPI, WSDL) | `bruno/packages/bruno-converters/src/` |

También se adoptan de su interfaz: ejemplos de respuesta guardados, documentación Markdown por request, tags para filtrar ejecuciones, "guardar como variable" desde una respuesta, editor masivo de tablas clave/valor y colores por entorno.

### Lo que se evita

1. **Estado completo en la interfaz.** El store de Redux contiene colecciones, borradores y respuestas enteras (`bruno/packages/bruno-app/src/providers/ReduxStore/slices/collections/index.js`, más de 4500 líneas). Los cuerpos de respuesta se guardan en base64.
2. **Copiar la colección entera en cada envío.** Cada request hace `cloneDeep(collection)` y la envía por IPC (`bruno/packages/bruno-app/src/providers/ReduxStore/slices/collections/actions.js`).
3. **Motor duplicado.** La CLI tiene su propia copia de la tubería de ejecución (`bruno/packages/bruno-cli/src/runner/`).
4. **Cifrado de respaldo débil.** AES-256-CBC con IV de ceros y clave derivada del identificador de máquina (`bruno/packages/bruno-electron/src/utils/encryption.js`). Además los secretos se indexan por ruta absoluta: al mover la carpeta se pierden.
5. **Dos formatos en disco.** `.bru` y YAML conviven, con migraciones y parsers heredados.
6. **Archivos gigantes.** El módulo de red tiene más de 2300 líneas (`bruno/packages/bruno-electron/src/ipc/network/index.js`).

### Lo que Bruno no tiene en código abierto

- Ejecuciones dirigidas por datos (CSV/JSON).
- Integración real con gestores de secretos externos: existe la gramática y un cliente de Vault, pero nada lo invoca.

## Yaak

### Lo que se adopta

| Idea | Dónde está en Yaak |
|---|---|
| Sobre cifrado XChaCha20-Poly1305 con nonce de 24 bytes | `yaak/crates/yaak-crypto/src/encryption.rs` |
| Clave maestra en el keyring del sistema, con respaldo en archivo 0600 | `yaak/crates/yaak-crypto/src/master_key.rs` |
| Clave por workspace envuelta con la maestra | `yaak/crates/yaak-crypto/src/workspace_key.rs` |
| *Challenge* sincronizado para detectar una clave incorrecta | `yaak/crates/yaak-crypto/src/manager.rs` |
| Clave compartible legible (base32 en grupos) | `yaak/crates/yaak-crypto/src/workspace_key.rs` |
| Parser de plantillas con árbol de tokens reversible | `yaak/crates/yaak-templates/src/parser.rs` |
| Render asíncrono y recursivo con límite de profundidad | `yaak/crates/yaak-templates/src/renderer.rs` |
| Ajustes heredables con procedencia (`ResolvedSetting`) | `yaak/crates/yaak-models/src/models.rs` |
| Timeline tipado por respuesta | `yaak/crates/yaak-models/src/models.rs` |
| Cuerpos de respuesta en streaming a disco | `yaak/crates/yaak/src/send.rs` |
| Redirecciones manuales con eventos en el timeline | `yaak/crates/yaak-http/src/transaction.rs` |
| Resolver DNS con sobrescrituras por host | `yaak/crates/yaak-http/src/dns.rs` |
| Configuración TLS y certificados cliente | `yaak/crates/yaak-tls/src/lib.rs` |
| Escritura atómica (temporal + rename) | `yaak/crates/yaak-sync/src/sync.rs` |
| Importación como plan revisable | `yaak/crates/yaak/src/import.rs` |
| JSONPath y XPath nativos | `yaak/crates/yaak-filter/` |
| gRPC con reflection y códec dinámico | `yaak/crates/yaak-grpc/src/` |
| WebSocket con historial de mensajes | `yaak/crates/yaak-ws/src/` |

También se adoptan de su interfaz: paleta de comandos, cambio rápido entre requests recientes, atajos reasignables, entornos públicos y privados, y encadenamiento de requests con la función `response(...)` y política de refresco.

### Lo que se evita

1. **Base de datos como fuente de verdad.** La sincronización en dos sentidos necesita una tabla de estado, checksums y "gana el último que escribió" según el reloj. Con archivos como fuente de verdad todo eso desaparece.
2. **Layout plano con identificadores.** `yaak.<id>.yaml` no es navegable ni se revisa bien en git. Los archivos incluyen `createdAt` y `updatedAt`, que generan ruido y conflictos.
3. **Proceso Node permanente.** Hasta Basic y Bearer pasan por IPC a un plugin (`yaak/crates/yaak-plugins/src/nodejs.rs`). También hay un `protoc` auxiliar y se requiere el binario `git` del sistema.
4. **Pool de conexiones desactivado** para medir el tiempo de DNS (`yaak/crates/yaak-http/src/client.rs`): cada request paga el handshake TCP y TLS completo.
5. **Dos pilas TLS.** rustls cuando valida certificados y native-tls cuando no.
6. **Tipos débiles.** `authentication` y `body` son `BTreeMap<String, Value>`.
7. **Lógica repartida.** La orquestación de la sincronización vive en JavaScript del webview.
8. **Perfil de release sin ajustar.** No hay `lto`, `codegen-units` ni `panic` configurados en `yaak/Cargo.toml`.

## Síntesis: qué toma apifast de cada uno

| Área | Origen | Decisión en apifast |
|---|---|---|
| Layout y formato en disco | Bruno | YAML OpenCollection, carpetas como directorios |
| Fuente de verdad | Bruno | Archivos |
| Herencia y precedencia de variables | Bruno | Se adopta, con procedencia visible (Yaak) |
| Aserciones y capturas | Bruno | Se adoptan |
| Criptografía | Yaak | Se adopta casi tal cual |
| Secretos solo locales | Bruno | Se adoptan como opción por variable |
| Plantillas con funciones | Yaak | Se adoptan sobre la sintaxis `{{ }}` de Bruno |
| Timeline y streaming a disco | Yaak | Se adoptan |
| Autenticación | Ambos | Nativa en Rust, con tipos fuertes |
| CLI | Ambos | Mismo motor que la GUI |
| Scripting | Bruno | Diferido; QuickJS bajo demanda |
| Plugins | Yaak | Fuera de la 1.0 |
