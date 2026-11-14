# 11. Calidad y clean code

Convenciones de código, estrategia de pruebas y comprobaciones automáticas del proyecto.

## Convenciones de código

### Idioma

- **Código, identificadores, comentarios, rustdoc, nombres de tests, mensajes de error y de log, y mensajes de commit:** inglés.
- **README del repositorio y de cada crate, comentarios de configuración (`Cargo.toml`, `deny.toml`, CI):** inglés.
- **Documentación de diseño (`docs/`), ADRs y resultados de spikes:** español. Se traducirán si el proyecto busca colaboradores.
- **Textos de la interfaz:** en español e inglés, siempre a través del sistema de traducción, nunca escritos directamente en el código.

### Nombres

- Tipos del dominio con el vocabulario del producto: `Collection`, `Folder`, `Request`, `Environment`, `Variable`, `Secret`.
- Casos de uso con verbo: `SendRequest`, `SaveRequest`, `RunCollection`.
- Puertos con el rol que cumplen: `CollectionRepository`, `HttpTransport`, `SecretVault`.
- Adaptadores con la tecnología: `YamlCollectionRepository`, `ReqwestTransport`, `KeyringKeyStore`.

### Tamaño y forma

- Funciones cortas que hacen una cosa. Si necesita un comentario para separar secciones, se divide.
- Un módulo por concepto. Como guía, un archivo que pasa de unas 400 líneas se revisa para dividirlo.
- Sin módulos "utils" genéricos; cada función auxiliar vive junto a lo que ayuda.
- Dependencias explícitas por constructor; sin estado global.

### Tipos

- **Estados inválidos irrepresentables.** `Auth` y `Body` son enums; no hay mapas de cadenas.
- **Newtypes** para identificadores y valores con reglas (`CollectionId`, `VariableName`, `HeaderName`).
- **Transiciones en el tipo.** `ResolvedRequest` solo se obtiene resolviendo la herencia, y es lo único que acepta el transporte.
- **`Secret`** no se puede imprimir ni serializar por accidente.

### Errores

- `thiserror` en las librerías; `anyhow` solo en los binarios.
- Sin `unwrap` ni `expect` en código de producción, salvo invariantes documentadas.
- Sin `panic` como control de flujo.
- Cada error lleva contexto suficiente para mostrar un mensaje útil (qué archivo, qué variable).
- Ningún error contiene el valor de un secreto.

### `unsafe`

- `#![forbid(unsafe_code)]` en todos los crates, salvo donde sea imprescindible (por ejemplo, mapear un archivo en memoria), aislado en un módulo pequeño y comentado.

### Comentarios y documentación

- Los comentarios explican el porqué, no el qué.
- Todo elemento público de un crate tiene documentación.
- Cada crate tiene un `README` breve con su responsabilidad y sus dependencias permitidas.

## Reglas de arquitectura

Las reglas de dependencia de [arquitectura](03-arquitectura.md) se comprueban automáticamente:

- `apifast-arch-tests` lee `cargo metadata` y falla si:
  - `apifast-domain` depende de otro crate del workspace, si `apifast-app` depende de un adaptador o si un adaptador depende de otro;
  - el dominio usa tokio, serde, reqwest, egui u oneyaml;
  - un crate externo se usa fuera de su dueño (reqwest, hyper y rustls solo en `apifast-http`; oneyaml solo en `apifast-storage-fs`; egui, eframe y wgpu solo en `apifast-gui`; etc.). Así cada tecnología se cambia en un solo sitio y `apifast-app` queda sin entrada/salida;
  - un crate que no es driver usa `anyhow`;
  - un crate no tiene capa asignada, no hereda los lints del workspace (`[lints] workspace = true`), no tiene README o no usa la licencia del proyecto.
- Un doctest `compile_fail` comprueba que `ResolvedRequest` no se puede construir fuera de `resolve()`.
- `cargo deny` controla licencias, avisos de seguridad y dependencias duplicadas.

## Estrategia de pruebas

| Nivel | Qué cubre | Herramientas |
|---|---|---|
| Unitarias | Dominio, plantillas, aserciones, herencia | Tests de Rust; rápidas y sin entrada/salida |
| De propiedades | Round-trip de plantillas y de archivos; el parser no entra en pánico | `proptest` |
| De instantáneas | Serialización YAML, reporters | `insta` |
| De contrato | Cada adaptador cumple su puerto; la misma batería se ejecuta contra la implementación real y la de memoria | Tests genéricos sobre el trait |
| De integración | Casos de uso con adaptadores reales contra un servidor local | Servidor HTTP de pruebas, directorios temporales |
| De extremo a extremo | CLI sobre colecciones de ejemplo; se comparan salida y código de retorno | Colecciones en `tests/` |
| De interfaz | Lógica de paneles sin ventana; pruebas visuales de los componentes clave | Utilidades de pruebas de egui |
| De compatibilidad | Colecciones reales de Bruno se leen y se reescriben sin pérdida | Corpus tomado de `bruno/packages/bruno-tests/` |
| Criptográficas | Vectores fijos del sobre, manipulación detectada, clave incorrecta | Tests con valores conocidos |
| Fuzzing | Parser de plantillas, lector YAML, importadores | `cargo fuzz` |
| Rendimiento | Presupuestos de [rendimiento](10-rendimiento-y-memoria.md) | `criterion`, scripts de memoria |

### Criterios

- El dominio y las plantillas se desarrollan con las pruebas primero.
- Toda corrección de un bug llega con una prueba que lo reproduce.
- Las pruebas no dependen de la red externa ni del keyring real.
- Se prioriza cubrir comportamientos, no alcanzar un porcentaje.

## Comprobaciones automáticas

En cada cambio:

| Comprobación | Comando |
|---|---|
| Formato | `cargo fmt --check` |
| Lints | `cargo clippy --all-targets -- -D warnings` |
| Pruebas | `cargo test --workspace` |
| Arquitectura | Test de reglas de dependencia |
| Dependencias | `cargo deny check` |
| Seguridad | `cargo deny check advisories` (misma base RustSec que `cargo audit`) |
| Documentación | `cargo doc --no-deps` sin advertencias |

El CI está en `.github/workflows/ci.yml`. Los lints están en `[workspace.lints]` del `Cargo.toml` raíz, y las reglas de licencias y orígenes en `deny.toml`. La versión de Rust está fijada en `rust-toolchain.toml` (ahora 1.99.1) y es la misma en local y en el CI, para que una versión nueva con lints nuevos no rompa el CI sin cambios en el código. `rust-version` coincide con ella: apifast se distribuye como binario y no necesita compilar con versiones antiguas. Se sube a propósito cada 2 o 3 versiones, en un commit `build: bump Rust to 1.xx` que también corrige los lints nuevos.

En la rama principal, además: benchmarks con comparación, compilación en Linux, macOS y Windows, y tamaño del binario.

### Lints

- `clippy::pedantic` activado de forma selectiva.
- Prohibidos: `unwrap_used`, `expect_used`, `dbg_macro`, `print_stdout` fuera de la CLI, `todo`.
- `missing_docs` en los crates de dominio y aplicación.

## Flujo de trabajo

- Ramas cortas y cambios pequeños, cada uno con sus pruebas.
- Mensajes de commit con formato convencional (`feat:`, `fix:`, `docs:`, `refactor:`).
- Una decisión que cambia la arquitectura se registra como ADR en `docs/adr/`.
- Los cambios en el formato de archivos actualizan [formato de archivos](04-formato-de-archivos.md) y añaden casos al corpus de round-trip.

## Seguridad

- Revisión específica de todo cambio en `apifast-vault`.
- Los secretos nunca aparecen en pruebas con valores reales.
- Las dependencias criptográficas se actualizan de forma deliberada, comprobando los vectores fijos.
- Los registros de la aplicación no incluyen headers de autorización, cookies ni cuerpos, salvo en un modo de depuración explícito y con enmascarado.

## Definición de terminado

Una tarea está terminada cuando:

1. El comportamiento tiene pruebas y pasan.
2. Las comprobaciones automáticas pasan.
3. No rompe ningún presupuesto de rendimiento.
4. La documentación afectada está actualizada.
5. No introduce `unwrap`, `unsafe` ni dependencias nuevas sin justificar.
