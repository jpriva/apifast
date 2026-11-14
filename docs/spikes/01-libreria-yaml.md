# Spike 1: librería YAML

- **Fecha:** 2026-11-14
- **Resultado:** se elige **`oneyaml` 1.2.0**, guardando mediante su modo documento sin pérdida y JSON Patch.
- **Código:** `spikes/yaml-spike/` (desechable). Se reproduce con:

```
cd spikes/yaml-spike
cargo run --release -- ../../../bruno
```

## Qué se comparó

| Librería | Versión | Licencia | MSRV |
|---|---|---|---|
| `yaml_serde` | 0.10.7 | MIT OR Apache-2.0 | 1.82 |
| `serde-saphyr` | 2.1.1 | MIT OR Apache-2.0 | 1.89 |
| `oneyaml` | 1.2.0 | MIT OR Apache-2.0 | 1.85 |

`serde-saphyr` se probó con sus opciones por defecto y con dos ajustes para parecerse a la salida de Bruno (listas indentadas y sin plegar cadenas largas). `oneyaml` se probó de dos formas: con serde (como las otras) y con su `Document`, que edita el texto sin reescribirlo.

**Corpus:** 430 archivos OpenCollection reales de los tests de Bruno (`bruno/tests/` y `bruno/packages/`), 201 de ellos requests HTTP. Tres archivos están rotos a propósito (fixtures de errores) y todas las librerías los rechazan.

## Resultados

### Lectura y round-trip

| Librería | Lee bien | Datos iguales tras guardar | Texto idéntico tras guardar |
|---|---|---|---|
| `yaml_serde` | 427/427 | 100 % | 4 % |
| `serde-saphyr` (defecto) | 427/427 | 100 % | 14 % |
| `serde-saphyr` (ajustado) | 427/427 | 100 % | 22 % |
| `oneyaml` (serde) | 427/427 | 100 % | 9 % |
| **`oneyaml` (Document)** | 427/427 | — | **100 %** |

Las tres librerías leen exactamente los mismos datos de todos los archivos. Con serde, ninguna conserva el texto: pierden las líneas en blanco que Bruno inserta entre bloques, los comentarios y, según la librería, el estilo de las listas.

Con un DTO tipado que guarda las claves desconocidas con `#[serde(flatten)]`, **todas** conservan el 100 % de los datos de las 199 requests HTTP. Conservar claves desconocidas no es un problema en ninguna.

### La prueba que decide: editar un campo y guardar

Se cambia solo la URL de cada request y se cuenta cuántas líneas cambian en el diff. Lo ideal son 2 (una que sale, una que entra).

| Librería | Líneas cambiadas (media) | Peor caso |
|---|---|---|
| `yaml_serde` | 21,9 | 597 |
| `serde-saphyr` (defecto) | 21,5 | 597 |
| `serde-saphyr` (ajustado) | 4,5 | 29 |
| `oneyaml` (serde) | 5,4 | 17 |
| **`oneyaml` (Document)** | **2,0** | **2** |

Con serde, cada guardado reescribe el archivo: abrir una colección de Bruno y cambiar una URL produce un diff de decenas de líneas, y cada guardado posterior de Bruno lo revierte. Con `Document`, el diff es siempre de 2 líneas.

### Archivo escrito a mano (comentarios, líneas en blanco, bloques)

| Librería | Líneas cambiadas | Comentarios | Líneas en blanco | Bloque `\|-` | Línea larga intacta |
|---|---|---|---|---|---|
| `yaml_serde` | 19 | se pierden | se pierden | sí | no |
| `serde-saphyr` (defecto) | 21 | se pierden | se pierden | sí | no (la pliega con `>-`) |
| `serde-saphyr` (ajustado) | 9 | se pierden | se pierden | sí | no |
| `oneyaml` (serde) | 11 | se pierden | se pierden | sí | no |
| **`oneyaml` (Document)** | **2** | **se conservan** | **se conservan** | sí | **sí** |

### Editar listas (headers, params, aserciones)

| Método con `oneyaml` | Resultado |
|---|---|
| Merge patch (RFC 7396) | Reemplaza la lista entera: pierde los comentarios de sus elementos y cambia las comillas |
| `set_path("http.headers[1].value", …)` | Cambia solo esa línea |
| JSON Patch (RFC 6902) por elemento | Cambia solo esa línea; añadir al final agrega solo las líneas nuevas |

**Conclusión:** el adaptador debe calcular un **JSON Patch por elemento** entre el modelo leído y el modelo a guardar, no un merge patch.

### Interpretación de escalares

Cadenas como `"yes"`, `"on"`, `"0777"`, `"null"` o `"{{host}}/v1"`, escritas por cada librería y leídas por todas:

- `oneyaml` y `serde-saphyr` las escriben siempre de forma que cualquier lector las lee como texto.
- `yaml_serde` escribe `yes`, `on` y `no` sin comillas. `serde-saphyr` las lee como booleanos.

Lectura de valores sin comillas (`a: yes`, `c: 0777`) en un tipo genérico:

| Librería | `yes` / `on` | `0777` |
|---|---|---|
| `yaml_serde` | texto | texto |
| `serde-saphyr` | **booleano** (YAML 1.1) | número decimal 777 |
| `oneyaml` | texto (YAML 1.2) | número 777 |

Bruno lee con la librería JavaScript `yaml`, que por defecto sigue YAML 1.2: `yes` es texto. `serde-saphyr` puede discrepar si no se activa `strict_booleans`. En campos tipados como `String` todas conservan el texto original.

### Rendimiento y tamaño

Corpus completo (268 KB) leído y escrito 20 veces, compilación release:

| Librería | Leer | Escribir | Leer + escribir tipado | Archivo de 1 MB |
|---|---|---|---|---|
| `yaml_serde` | 92 ms | 57 ms | 128 ms | 11,5 ms |
| `serde-saphyr` | 176 ms | 21 ms | 159 ms | 3,4 ms |
| `oneyaml` (serde) | 79 ms | 28 ms | 101 ms | 2,3 ms |
| `oneyaml` (Document) | 91 ms (leer + escribir) | — | — | 1,2 ms |

Las cuatro sobran: leer los 430 archivos una vez cuesta entre 4 y 9 ms.

Binario mínimo que lee y escribe un archivo (perfil con LTO, `strip` y `panic = "abort"`), frente a uno que solo usa `serde_json` (374 KB):

| Librería | Tamaño | Aumento | Dependencias | Memoria pico con 1 MB |
|---|---|---|---|---|
| `yaml_serde` | 531 KB | +157 KB | 14 | 5,7 MB |
| `serde-saphyr` | 1339 KB | +965 KB | 19 | 6,4 MB |
| `oneyaml` | 2227 KB | +1853 KB | 10 | 6,5 MB |

`oneyaml` es la más pesada en binario (incluye el editor de documentos y el sistema de diagnósticos), pero son menos de 2 MB de un presupuesto de 25 MB. En memoria las tres son equivalentes.

## Decisión

Se adopta **`oneyaml`**:

- **Leer:** con serde hacia los DTO de `apifast-storage-fs`.
- **Guardar:** releer el archivo del disco como `Document`, calcular un JSON Patch por elemento entre el DTO leído y el DTO a guardar, aplicarlo y escribir de forma atómica. Así no hace falta mantener los documentos en memoria.
- **Archivos nuevos:** se escriben con serde, con un formato que imite el de Bruno (líneas en blanco entre bloques).

Consecuencias para el formato:

- Se cumple el round-trip sin pérdida: abrir y guardar sin cambios no modifica el archivo.
- Los comentarios escritos a mano sobreviven a las ediciones desde la interfaz.
- Editar en apifast una colección compartida con Bruno produce diffs mínimos.

## Riesgos y plan alternativo

| Riesgo | Mitigación |
|---|---|
| `oneyaml` es nueva y sin historial en producción (lo dice su propia documentación) | La librería solo se usa dentro de `apifast-storage-fs`; el corpus de round-trip de F1 detecta regresiones al actualizarla |
| Un solo mantenedor u organización | Licencia MIT OR Apache-2.0: se puede mantener un fork |
| El cálculo del JSON Patch por elemento es código propio | Pruebas de propiedades: aplicar el patch al documento produce los mismos datos que el DTO nuevo |
| Reordenar elementos de una lista | Usar operaciones `move` de JSON Patch; verificar en F1 que conservan los comentarios |

**Plan alternativo:** `serde-saphyr` con `compact_list_indent: false`, `folded_wrap_chars` sin límite y `strict_booleans: true`. Pierde comentarios y produce diffs de unas 4,5 líneas por edición, pero lee correctamente todo el corpus. `yaml_serde` queda descartada por los diffs de más de 20 líneas.

## Fuera del alcance de este spike

- No se probó un archivo editado a la vez por Bruno y apifast con Bruno real.
- Las afirmaciones de conformidad de `oneyaml` (402/402 casos de la suite oficial de YAML) son de su documentación; no se verificaron aquí.
