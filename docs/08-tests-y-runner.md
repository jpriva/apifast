# 8. Tests y runner

Las pruebas se escriben primero de forma **declarativa**: aserciones y capturas dentro del archivo de la request, sin código. El scripting en JavaScript llega en una fase posterior (ver [ADR 0006](adr/0006-scripting-diferido.md)).

## Aserciones

Cada aserción tiene una expresión, un operador y, según el operador, un valor esperado.

```yaml
runtime:
  assertions:
    - expression: res.status
      operator: eq
      value: "200"
    - expression: res.headers.content-type
      operator: contains
      value: application/json
    - expression: res.body.items
      operator: length
      value: "10"
    - expression: res.responseTime
      operator: lt
      value: "500"
    - expression: res.body.error
      operator: isUndefined
```

### Expresiones

| Expresión | Valor |
|---|---|
| `res.status` | Código de estado |
| `res.statusText` | Texto de estado |
| `res.headers.<nombre>` | Header de la respuesta (sin distinguir mayúsculas) |
| `res.body` | Cuerpo completo |
| `res.body.<ruta>` | Campo del cuerpo JSON; admite índices (`items[0].id`) y JSONPath (`items[*].id`, `..id`, `items[?@.ok]`) |
| `res.responseTime` | Duración en milisegundos |
| `res.size` | Tamaño del cuerpo en bytes |

El valor esperado admite plantillas (`{{expected_id}}`).

### Operadores

| Grupo | Operadores |
|---|---|
| Igualdad | `eq`, `neq` |
| Comparación | `gt`, `gte`, `lt`, `lte`, `between` |
| Pertenencia | `in`, `notIn`, `contains`, `notContains` |
| Texto | `startsWith`, `endsWith`, `matches`, `notMatches` |
| Tamaño | `length`, `isEmpty`, `isNotEmpty` |
| Existencia | `isNull`, `isUndefined`, `isDefined`, `isTruthy`, `isFalsy` |
| Tipo | `isJson`, `isNumber`, `isString`, `isBoolean`, `isArray` |

Es la misma lista que usa Bruno, para que las colecciones importadas funcionen sin cambios.

### Comparación de tipos

- El valor esperado se escribe como texto y se convierte al tipo del valor real antes de comparar (`"200"` frente al número `200`).
- Si la conversión no es posible, la aserción falla con un mensaje que muestra ambos tipos.
- Un valor esperado entre comillas (`"'200'"`) es siempre texto, nunca número.
- Los números se comparan por valor (`200` y `200.0` son iguales) y los objetos sin importar el orden de las claves; un array o un objeto esperado se escribe en JSON.
- Un valor que no existe (un header o un campo ausente) es `undefined`: cumple `isUndefined`, `isFalsy`, `notIn`, `notContains`, `notMatches` y `eq undefined`, y falla el resto.

### Detalles de los operadores

| Operador | Detalle |
|---|---|
| `gt`, `gte`, `lt`, `lte` | El valor real debe ser número, o texto que se lea como número (`"9.5"`) |
| `between` | Dos valores separados por coma, ambos incluidos: `200,299` |
| `in`, `notIn` | Valores separados por coma (`200,201,204`) o un array JSON |
| `contains` | Texto dentro de texto, un elemento en un array o una clave en un objeto |
| `matches` | Expresión regular de Rust (crate `regex`): tiempo lineal, sin *backtracking*, con límite de tamaño |
| `length`, `isEmpty` | Caracteres de un texto, elementos de un array o claves de un objeto |
| `isTruthy`, `isFalsy` | Como en JavaScript: `false`, `0`, `""`, `null` y `undefined` son falsos |
| `isJson` | Un objeto o un array |

Un operador desconocido se conserva en el archivo y la aserción falla diciendo que no lo conoce.

### Lectura de la respuesta

- `res.statusText` es la frase estándar del código (`OK`, `Not Found`); HTTP/2 no envía frase.
- Un header repetido se lee con sus valores unidos por `, `.
- El cuerpo se lee como JSON una sola vez por respuesta, con un lector propio del dominio (`apifast-domain::Json`, sin `serde`) que conserva el texto de los números y rechaza anidamientos de más de 128 niveles. Si no es JSON, `res.body` es su texto y `res.body.<ruta>` falla con "the body is not JSON".
- Las rutas usan puntos e índices (`items[0].id`); `["a.b"]` nombra una clave con puntos.
- Una ruta que usa sintaxis de JSONPath (`*`, `..`, un filtro `[?…]`, un slice `[1:3]`, una unión `[0,2]` o un índice negativo) se lee como el JSONPath `$<ruta>`: `res.body.items[*].id` es `$.items[*].id`. Si selecciona un valor, es ese valor; si selecciona varios, un array con ellos; si no selecciona nada, es indefinido (`isUndefined` pasa). Ver [JSONPath y XPath](#jsonpath-y-xpath).
- Un cuerpo demasiado grande para leerlo entero hace fallar las aserciones sobre `res.body`, con su motivo; las demás se evalúan igual.

## Capturas

Una captura extrae un valor de la respuesta y lo guarda en una variable.

```yaml
runtime:
  actions:
    - type: set-variable
      phase: after-response
      selector:
        expression: res.body.access_token
        method: jsonq
      variable:
        name: access_token
        scope: runtime
```

| Ámbito (`scope`) | Vida de la variable |
|---|---|
| `runtime` | Hasta cerrar la colección o terminar la ejecución del runner |
| `request` | Solo durante esta request |
| `environment` | Se escribe en el entorno activo (pide confirmación en la GUI) |

Una captura cuyo destino es una variable secreta guarda el valor en la bóveda, nunca en claro.

Comportamiento actual:

- Las capturas corren tras guardar el cuerpo, y lo capturado ya está disponible para las aserciones del mismo envío (`value: "{{user_id}}"`).
- El valor toma el tipo de lo capturado: texto, número o booleano; un objeto, un array o `null` se guardan como variable de tipo objeto (JSON).
- Una captura sin valor (`res.body.token` ausente) no cambia la variable y se informa como aviso; no hace fallar la request.
- Una captura sobre una variable secreta conserva la marca de secreto: su valor se enmascara en resultados, historial y requests siguientes. Por ahora queda en memoria; escribirla en la bóveda llega con la interfaz de tests.
- Ámbito `environment`: por ahora vive como `runtime`. Escribirlo en el entorno, con confirmación en la interfaz, llega con la pestaña Tests.
- En la interfaz, las variables de runtime duran mientras la colección está abierta; en la CLI, lo que dura `send` (y en `run`, toda la ejecución).
- El cuerpo se lee para las comprobaciones solo si la request tiene alguna activa, y como mucho 16 MiB (`CHECK_LIMIT`).
- Los valores real y esperado de los resultados llegan ya enmascarados con los secretos de la request y los capturados.

## JSONPath y XPath

El crate `apifast-query` evalúa las dos lenguas sobre el cuerpo de una respuesta. Lo usan `res.body.<ruta>` en aserciones y capturas, `response(path=…)` en plantillas ([docs/05](05-variables-y-plantillas.md)) y, más adelante, el filtro del visor de respuestas.

**JSONPath** sigue el RFC 9535:

| Sintaxis | Ejemplo |
|---|---|
| Nombre, comodín, índice (negativo desde el final) | `$.store.book[*].author`, `$.items[-1]` |
| Nombre entre comillas, con escapes JSON | `$['a.b']`, `$["it's"]` |
| Descendientes | `$..price` |
| Slice con paso, unión | `$.items[1:5:2]`, `$.items[0,2]` |
| Filtro con `==`, `!=`, `<`, `<=`, `>`, `>=`, `&&`, `\|\|`, `!` y paréntesis | `$.items[?@.price < 10 && !@.sold]` |
| Funciones `length`, `count`, `match`, `search`, `value` | `$.items[?match(@.name, 'b.*')]` |

- También acepta la forma `[?(…)]` de Jayway y Bruno, y un `-` dentro de un nombre tras el punto (`$.first-name`).
- Los números se comparan por valor (`1 == 1.0`). Comparar con algo que no existe solo es cierto si el otro lado tampoco existe.
- Una consulta de comparación que selecciona varios valores no compara (el RFC la rechaza; aquí no coincide).
- Las expresiones regulares de `match` (entera) y `search` (parcial) tienen un límite de 1 MiB compiladas; una inválida es un error.

**XPath** es la versión 1.0 completa salvo las variables (`$x`):

- Rutas absolutas y relativas, `//`, `.`, `..`, `@`, todos los ejes (`ancestor::`, `following-sibling::`, `preceding::`…), los tests `*`, `text()`, `node()`, `comment()` y `processing-instruction()`, predicados de posición y de valor, uniones con `|`.
- Las funciones de cadena, número y booleano de XPath 1.0 (`count`, `sum`, `contains`, `substring`, `normalize-space`, `translate`, `name`, `local-name`…), más `ends-with`.
- Un nombre sin prefijo coincide por nombre local en cualquier espacio de nombres: `//Body/Token` encuentra `<s:Body><Token xmlns="urn:auth">` sin declarar nada. Con prefijo (`//s:Body`), el prefijo es el que declara el documento.
- El resultado es el texto de cada nodo seleccionado, en orden del documento, o el número, la cadena o el booleano que da la expresión (`count(//item)` da `3`).
- Se admite un DTD; `roxmltree` limita la expansión de entidades.

**Comunes:**

- Una expresión que empieza por `$` es JSONPath; cualquier otra, XPath. En `response(path=…)`, sobre un cuerpo JSON, una ruta sin `$` es la forma simple (`data.items[0]`).
- Un error de sintaxis dice dónde: `` invalid JSONPath `$.a[`: expected a selector at character 5 ``.
- Cada consulta tiene un presupuesto de 20 millones de visitas a valores o nodos y un anidamiento máximo de 64 niveles, así que una expresión costosa sobre un cuerpo grande se detiene con un error en vez de bloquear.

## Orden de evaluación en un envío

1. Variables previas de la request.
2. Envío.
3. Capturas.
4. Aserciones.
5. (F9) Scripts posteriores y tests en JavaScript.

### `apifast send`

Muestra cada aserción en stderr (`pass` o `FAIL` con su motivo) y cada captura sin valor como aviso; con `--quiet`, solo los fallos. Si una aserción falla, termina con código 1.

## Runner de colección

Ejecuta varias requests en secuencia y agrega los resultados.

| Opción | Descripción |
|---|---|
| Alcance | Colección, carpeta (recursiva o no) o selección manual |
| Orden | Por `seq`, respetando la jerarquía |
| Filtro por tags | Incluir o excluir |
| Pausa entre requests | Milisegundos |
| Detener al primer fallo | Sí o no |
| Iteraciones | Número de repeticiones |
| Datos | Archivo CSV o JSON; una iteración por fila, con las columnas como variables |

Las ejecuciones dirigidas por datos son una mejora respecto a Bruno, que no las incluye en su versión de código abierto.

### Cómo funciona

El caso de uso `run_collection` (`apifast-app`) es el mismo para la CLI y la interfaz:

- **Selección:** la colección entera o una carpeta (`folder`), con sus subcarpetas o sin ellas (`recursive`), en el orden del árbol: por `seq`, luego por nombre, y cada carpeta en su lugar. Una carpeta que no existe es un error antes de enviar nada.
- **Tags:** con `tags`, solo corren las requests que tienen al menos uno; con `exclude_tags`, no corre ninguna que tenga alguno. Sin distinguir mayúsculas.
- **Omitidas:** las requests filtradas por tags, las de tipos que aún no se pueden enviar (GraphQL…) y los archivos ilegibles no corren y quedan en el informe con su motivo.
- **Variables:** cada request ve lo que capturaron las anteriores, también entre iteraciones. El informe devuelve los valores finales.
- **Errores:** una request que no se puede enviar (variable sin resolver, red…) queda en el informe con su error y la ejecución sigue.
- **`bail`:** detiene la ejecución tras la primera request con un error o una aserción fallida.
- **Pausa:** `delay_ms` se espera entre dos requests, no antes de la primera. La espera es un puerto (`Pause`), así que las pruebas no esperan.
- **Iteraciones:** la selección entera se repite `iterations` veces; cada paso del informe dice a qué iteración pertenece.
- **Datos:** con `data`, la selección corre una vez por fila y se ignora `iterations`. Al empezar cada fila, sus columnas pasan a variables runtime y reemplazan las de la fila anterior; una captura con el mismo nombre la sobrescribe hasta la fila siguiente.

### Archivos de datos

`parse_data` (`apifast-app`, sin E/S) lee el texto. El formato sale de la extensión (`.csv` o `.json`, sin distinguir mayúsculas):

- **CSV** (RFC 4180): la primera fila son los nombres de las columnas. Campos entre comillas dobles cuando llevan comas, saltos de línea o comillas (`""`). Admite `\r\n`, un BOM inicial y líneas en blanco, que se saltan. Todos los valores son texto.
- **JSON:** un array de objetos; cada clave es una variable. Números y booleanos conservan su tipo, y un objeto o un array se vuelve una variable de tipo objeto (`{{user.name}}`).
- **Errores**, con dónde: `line 3 has 1 fields; the header has 2`, `line 2: a quote is not closed`, `column 2 has no name`, `` the column `a` appears twice ``, `item 2 is number, not an object`, `the data file has no rows`.

```csv
email,password
ana@example.com,"s3cr3t, really"
bo@example.com,hunter2
```
- **Cancelación:** se comprueba antes de cada request y corta la que está en vuelo; el informe queda marcado como cancelado.
- **Progreso:** un callback recibe `Started` y `Finished` de cada request, para mostrar el avance en vivo.
- **Resultado:** el informe tiene los pasos (respuesta, capturas, aserciones y duración), las omitidas, la duración total y si se detuvo por `bail` o por cancelación. `summary()` da los totales y `succeeded()` dice si todo se envió y todas las aserciones pasaron.

### Resultado

Por request: estado, duración, aserciones (aprobadas y fallidas con valor esperado y real) y capturas realizadas. Por ejecución: totales, duración y las requests omitidas.

En la interfaz, el runner muestra el progreso en vivo y permite abrir la respuesta de cualquier paso.

## CLI

La CLI usa los mismos casos de uso que la interfaz.

```
apifast send <archivo.yml> [--env <nombre>]
apifast run <ruta> [opciones]
apifast import <formato> <origen> --out <directorio>
apifast vault <subcomando>
```

### `apifast run`

```
apifast run [carpeta] [opciones]
```

La carpeta es la colección o una de sus carpetas; por defecto, la actual. La colección es la carpeta más cercana hacia arriba con `opencollection.yml`. Un archivo no se ejecuta con `run`: para una request está `apifast send`.

| Opción | Descripción |
|---|---|
| `-e`, `--env <nombre>` | Entorno activo |
| `--var <k=v>` (o `--env-var`) | Sobrescribe una variable (repetible) |
| `-r`, `--recursive` | Incluye subcarpetas |
| `--tags <a,b>` / `--exclude-tags <a,b>` | Filtro por tags |
| `--bail` | Detiene al primer fallo |
| `--delay <ms>` | Pausa entre requests |
| `--iterations <n>` | Repeticiones (al menos 1; no se combina con `--data`) |
| `--data <archivo>` | CSV o JSON para iterar |
| `--reporter <tipo>=<ruta>` | `json`, `junit` o `html` (repetible); crea las carpetas que falten |
| `--allow-unresolved` | Las variables sin resolver son advertencia |
| `--no-color`, `-q`/`--quiet`, `-v`/`--verbose` | Salida |

Pendientes: `--env-file` (un `.env` adicional) y `--insecure`/`--cacert`, que llegan con la configuración de red de F5; hoy los certificados salen de la configuración de la colección.

#### Salida en consola

Una línea por request al terminar, con su ruta, duración y estado; debajo, las aserciones que fallan, las capturas sin valor y el error de una request que no se pudo enviar. Al final, las omitidas y el resumen:

```
Running Shop with environment local
PASS  Login login.yml · 12 ms · 200
FAIL  Missing users/missing.yml · 4 ms · 404
      ✗ res.status eq 200: expected eq 200, got 404
ERROR Create users/create.yml · 0 ms
      variable `name` is not defined
SKIP  graphql.yml · graphql requests cannot run yet
3 requests: 1 passed, 1 failed, 1 with errors, 1 skipped · 1/2 assertions · 16 ms
```

- Con varias iteraciones, cada una empieza con `Iteration 2 of 3`.
- `--quiet` muestra solo los fallos y el resumen; `--verbose`, también las aserciones que pasan y los valores capturados (los secretos, ocultos).
- Colores solo en una terminal, y nunca con `--no-color` o la variable `NO_COLOR`.

### Códigos de salida

| Código | Significado |
|---|---|
| 0 | Todas las aserciones pasaron |
| 1 | Al menos una aserción falló |
| 2 | Error de ejecución (red, archivo inválido, variable sin resolver) |
| 3 | Error de uso (argumentos) |

Se aplica el código más grave: una request que no se pudo enviar da 2 aunque otras tengan aserciones fallidas. Una ejecución cancelada con Ctrl+C también da 2. El 3 vale para todos los comandos: `apifast send` usaba antes el 2.

### Reporters

| Formato | Uso |
|---|---|
| Consola | Resumen legible; es la salida por defecto |
| JSON | Resultado completo para procesar |
| JUnit XML | Integración con sistemas de CI |
| HTML | Informe autocontenido |

Los reporters enmascaran los secretos igual que la interfaz.

Son funciones puras de `apifast-app` (`ReportFormat::render`), así que la interfaz podrá exportar los mismos informes.

- **JSON:** colección, entorno, inicio (ISO 8601), duración, `succeeded`, `bailed`, `cancelled`, los totales en `summary` y, por request, iteración, ruta, nombre, estado, tiempo, tamaño, aserciones (expresión, operador, esperado, actual, mensaje) y capturas (variable, ámbito, valor o error). Las omitidas, con su motivo.
- **JUnit:** un `testsuite` por request enviada (con la iteración en el nombre si hay varias) y un `testcase` por aserción; una request sin aserciones es un caso `request`, y una que no se pudo enviar, un `<error>`. Las omitidas van en un `testsuite` `skipped`. Los atributos `tests`, `failures`, `errors`, `skipped`, `time` y `timestamp` son los que leen GitHub Actions, GitLab y Jenkins.
- **HTML:** una página sin recursos externos ni scripts, con tema claro y oscuro: totales, y cada request plegable con sus aserciones y capturas; las que fallan empiezan desplegadas.
- No incluyen URLs: su query puede llevar secretos.

#### En CI

```yaml
# GitHub Actions
- name: API tests
  run: apifast run -r -e ci --reporter junit=reports/api.xml
  env:
    APIFAST_COLLECTION_KEY: ${{ secrets.APIFAST_COLLECTION_KEY }}
    APIFAST_NO_KEYRING: "1"
- uses: mikepenz/action-junit-report@v5
  if: always()
  with:
    report_paths: reports/api.xml
```

## Scripting en JavaScript (fase F9)

Cuando las aserciones declarativas no alcanzan, se podrá escribir JavaScript.

- **Motor:** QuickJS embebido (`apifast-script-js`), sin acceso al sistema de archivos ni a la red salvo por la API expuesta.
- **Carga bajo demanda:** el motor se inicializa solo cuando se ejecuta una request con scripts. Una colección sin scripts no paga ese costo.
- **Límites:** tiempo máximo de ejecución y memoria máxima por script.
- **API compatible con Bruno** para facilitar la importación:

| Objeto | Funciones principales |
|---|---|
| `req` | Leer y modificar URL, método, headers y body antes del envío |
| `res` | `getStatus()`, `getHeader()`, `getBody()`, `getResponseTime()` |
| `bru` | `getVar`/`setVar`, `getEnvVar`/`setEnvVar`, `sleep`, `setNextRequest` |
| `test`, `expect` | Tests con aserciones estilo chai |

- **Ubicación en el archivo:** el bloque `runtime.scripts` de OpenCollection, que apifast ya conserva desde la fase F1.
- **Niveles:** scripts de colección, carpeta y request, con el orden "sándwich" de Bruno (antes del envío de afuera hacia adentro; después, de adentro hacia afuera).

Hasta la fase F9, una request con scripts muestra un aviso: los scripts se conservan pero no se ejecutan.
