# ADR 0002: Formato en disco YAML compatible con OpenCollection

- **Estado:** aceptado; librería YAML elegida en el [spike 1](../spikes/01-libreria-yaml.md)
- **Fecha:** 2026-11-14

## Contexto

Las colecciones se guardan como archivos. Se consideraron tres formatos: YAML tipo OpenCollection (el que Bruno usa por defecto en sus versiones recientes), TOML propio y un lenguaje propio similar al `.bru` clásico.

## Decisión

YAML OpenCollection puro: `opencollection.yml`, `folder.yml`, un archivo por request y `environments/*.yml`.

Los datos propios de apifast (identificador de colección, *challenge* y secretos cifrados) **no** se escriben en esos archivos. Van en una carpeta `.apifast/` dentro de la colección, versionada en git.

### Por qué una carpeta aparte

Se revisó el serializador de Bruno: al guardar reconstruye cada archivo desde su modelo interno y descarta toda clave desconocida, incluida cualquier extensión propia. Con claves `x-apifast` dentro de los archivos, abrir y guardar la colección con Bruno habría borrado los secretos cifrados y el identificador. Con `.apifast/` aparte, ambas herramientas conviven sobre la misma carpeta.

Bruno no ignora las carpetas ocultas, así que apifast añade `.apifast` a `extensions.bruno.ignore`, que Bruno sí conserva.

## Consecuencias

**A favor**

- apifast abre colecciones YAML de Bruno sin importador.
- Formato conocido, legible y con buen soporte en editores.
- Los bloques de texto multilínea sirven para bodies, scripts y documentación.
- No hay que diseñar ni mantener una gramática propia.

**En contra**

- YAML tiene ambigüedades conocidas (tipos implícitos, indentación). Se mitiga escribiendo siempre las cadenas problemáticas entre comillas.
- El ecosistema YAML en Rust es menos sólido que el de TOML: la librería histórica está sin mantenimiento y la elegida es reciente.
- Se depende de una especificación externa que puede cambiar.
- El valor cifrado de un secreto queda separado de la definición de su variable; apifast debe mantenerlos sincronizados al renombrar, y detectar entradas huérfanas si el cambio se hizo con Bruno.

## Librería YAML

Se elige **`oneyaml`** tras medir las tres candidatas con 430 archivos reales de Bruno ([spike 1](../spikes/01-libreria-yaml.md)):

- Lee con serde hacia los DTO.
- Guarda editando el documento original con JSON Patch por elemento. Una edición de un campo cambia exactamente 2 líneas del archivo, y se conservan comentarios, líneas en blanco y estilos. Con serde puro, las otras librerías cambian de 4,5 a 22 líneas de media.
- Plan alternativo: `serde-saphyr` ajustada.

Candidatas evaluadas:

| Crate | A favor | En contra |
|---|---|---|
| `yaml_serde` | Continuación de `serde_yaml` publicada por la organización oficial de YAML; misma API, con valor genérico para conservar claves desconocidas | No conserva comentarios ni formato; poco control del estilo de salida |
| `serde-saphyr` | Rust puro, tipada, con envoltorios para bloques literales y comentarios por campo | Sin valor genérico: conservar claves desconocidas exige más trabajo; no es edición sin pérdida |
| `oneyaml` | La única que ofrece edición sin pérdida conservando comentarios, además de límites de recursos | Muy nueva; sus garantías son declaraciones propias sin historial en producción |

Descartadas: `serde_yml` (retirada por un aviso de seguridad), `yaml_lib` (publicada hace días, un solo mantenedor, sin evidencia independiente) y `yaml-rust2` como opción principal (madura, pero de bajo nivel y sin serde: obligaría a escribir a mano todo el mapeo). `serde_norway` y `serde_yaml_ng` quedan como reserva equivalente a `yaml_serde`.

La librería solo se usa dentro de `apifast-storage-fs`, así que cambiarla más adelante no afecta al resto del sistema.

## Alternativas descartadas

- **TOML propio:** excelente soporte en Rust, pero sin compatibilidad con Bruno y con listas de tablas más incómodas de leer.
- **Lenguaje tipo `.bru`:** el más agradable de leer, pero exige parser y serializador propios, y el propio Bruno lo está dejando atrás.

## Reglas derivadas

- Round-trip sin pérdida, verificado con un corpus.
- Las claves desconocidas se conservan.
- Sin marcas de tiempo ni identificadores por request en los archivos.
