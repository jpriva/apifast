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
- Una captura sobre una variable secreta conserva la marca de secreto: su valor se enmascara en resultados, historial y requests siguientes. Queda en memoria, salvo que su ámbito sea `environment` y el usuario la guarde: entonces va a la bóveda.
- Ámbito `environment`: se usa enseguida como `runtime`. En la interfaz, un aviso ofrece escribirlo en el entorno activo ([docs/09](09-ui-ux.md#panel-de-respuesta)); la CLI no escribe nunca en los entornos.
- En la interfaz, las variables de runtime duran mientras la colección está abierta; en la CLI, lo que dura `send` (y en `run`, toda la ejecución).
- El cuerpo se lee para las comprobaciones solo si la request tiene alguna activa, y como mucho 16 MiB (`CHECK_LIMIT`).
- Los valores real y esperado de los resultados llegan ya enmascarados con los secretos de la request y los capturados.

## JSONPath y XPath

El crate `apifast-query` evalúa las dos lenguas sobre el cuerpo de una respuesta. Lo usan `res.body.<ruta>` en aserciones y capturas, `response(path=…)` en plantillas ([docs/05](05-variables-y-plantillas.md)) y el filtro del visor de respuestas ([docs/09](09-ui-ux.md#panel-de-respuesta)).

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

1. Scripts previos (`before-request`): colección, carpetas de la más externa a la más interna, request. Ven y cambian la request resuelta, sin renderizar.
2. Variables y plantillas: se renderiza la request, con lo que fijaron los scripts.
3. Envío.
4. Scripts posteriores (`after-response`): request, carpetas de la más interna a la más externa, colección.
5. Capturas.
6. Aserciones, que ven las variables fijadas por los scripts.
7. Scripts de tests (`tests`), en el mismo orden que los posteriores.

### `apifast send`

Muestra cada aserción en stderr (`pass` o `FAIL` con su motivo) y cada captura sin valor como aviso; con `--quiet`, solo los fallos. Lo mismo con los tests de los scripts (`pass  test: …`), lo que escribieron en la consola (`console  …`) y los errores de los que no terminaron. Si una aserción o un test falla, o un script posterior no termina, termina con código 1.

Una request WebSocket se conecta ([docs/07](07-motor-de-red.md#websocket-f7)): envía sus mensajes en orden y sigue abierta hasta que el servidor cierra o `Ctrl+C` la cierra. Cada mensaje recibido va a stdout en su línea, a medida que llega (los binarios, en hexadecimal), así que la salida se puede encadenar. En stderr van los enviados (`> …`), los que no se pudieron enviar (`warning: …`), el cierre y el resumen; con `--quiet`, solo los avisos.

```text
$ apifast send live.yml -e local
> {"op":"subscribe"}
{"price": 101.2}
{"price": 101.4}
^C
closed 1000
101 Switching Protocols · 5230 ms · 412 B
```

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
- **Omitidas:** las requests filtradas por tags, las WebSocket (una conexión no es un paso del runner), las de tipos que aún no se pueden enviar (gRPC) y los archivos ilegibles no corren y quedan en el informe con su motivo.
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
apifast describe <archivo.yml> [--env <nombre>]
apifast run <ruta> [opciones]
apifast import <formato> <origen> --out <directorio>
apifast export postman --out <archivo> [--env <nombre>]...
apifast vault <subcomando>
```

Una request gRPC se llama ([docs/07](07-motor-de-red.md#grpc-f8)) con sus mensajes en orden: solo el primero si el cliente del método no transmite, y si transmite, todos y luego cierra su lado. Cada respuesta va a stdout en una línea de JSON, a medida que llega, así que se puede encadenar con `jq`. En stderr van los enviados (`> …`), los que no se pudieron enviar y, al final, el estado con el tiempo; con `--quiet`, solo los avisos y un estado que no sea `OK`. Con `--fail`, un estado distinto de `OK` termina con código 4.

```text
$ apifast send hello.yml -e local
> {"greeting": "Ana"}
{"reply":"hello Ana"}
OK · 14 ms
```

### `apifast describe`

Lista lo que puede llamar una request gRPC: los métodos de su `.proto` o, sin uno, los de su servidor por reflection ([docs/07](07-motor-de-red.md#grpc-f8)). Una línea por método en stdout: su ruta, cómo transmite y los mensajes que recibe y devuelve. Acepta las mismas opciones de entorno, colección, variables y TLS que `send`.

```text
$ apifast describe hello.yml -e local
/hello.Greeter/SayHello       unary             hello.HelloRequest -> hello.HelloReply
/hello.Greeter/LotsOfReplies  server-streaming  hello.HelloRequest -> hello.HelloReply
```

### `apifast import`

```
apifast import postman <colección.json> [--env <entorno.json>]... --out <carpeta> [--dry-run]
```

Crea una colección nueva en `--out` a partir de una exportación de Postman (colección v2.0 o v2.1, también envuelta en `{"collection": …}` como la da su API) y, con `--env`, de sus entornos. Antes de escribir imprime el plan: cada carpeta y request con la ruta que tendrá, los entornos, y lo que no se conservó, con dónde estaba (`Users / Get user: a \`test\` script is not imported…`). Con `--dry-run` se queda ahí. Sobre una carpeta que ya tiene una colección solo escribe si salió de una importación, y entonces la actualiza ([Importar otra vez](#importar-otra-vez)).

Lo que se traduce de Postman (con las decisiones del importador de Bruno):

- Carpetas y requests en su orden (`seq`), con nombres de archivo sacados del nombre y únicos entre hermanos.
- Auth de colección, carpeta y request: basic, bearer, digest, NTLM, API key, AWS SigV4 y OAuth 2.0 (sus cuatro flujos, PKCE, dónde va el token y los parámetros adicionales). Sin `auth`, hereda; `noauth`, ninguna. Las demás (OAuth 1, Hawk, EdgeGrid…) quedan sin auth y con aviso.
- URL como texto o por partes, path params con su valor, params de query desactivados, headers (también desactivados), descripción como documentación.
- Bodies: raw según su lenguaje o el `Content-Type` (JSON, XML o texto), urlencoded, form-data (texto y archivos), archivo, GraphQL (su operación y sus variables).
- `protocolProfileBehavior`: `followRedirects`, `maxRedirects` y `disableUrlEncoding` (como `encodeUrl: false`); los demás, con aviso.
- Las respuestas guardadas, como ejemplos de la request.
- Variables de colección. `{{$guid}}` pasa a `{{$randomUUID}}`; una variable dinámica que apifast no genera se avisa.
- Entornos: sus variables; las de tipo `secret` se declaran secretas sin valor (el valor nunca va a un archivo) y se avisa para ponerlo en la bóveda.
- Los scripts `prerequest` y `test` se traducen a la API de Bruno, como lo hace su importador: `pm.environment.get` → `bru.getEnvVar`, `pm.collectionVariables.set` → `bru.setVar`, `pm.test` → `test`, `pm.expect` → `expect`, `pm.response.json()` → `res.getBody()`, `pm.response.to.have.status(…)` → `expect(res.getStatus()).to.equal(…)`, `pm.execution.skipRequest()` → `bru.runner.skipRequest()`, el antiguo `tests["…"] = …` como un `test`, y el resto de la tabla de Bruno. Una línea que sigue usando `pm` o `postman` (como `pm.sendRequest`) queda comentada, para que el script corra y la línea no se pierda, y se avisa cuántas. Los scripts de otros eventos se avisan.

```
apifast import openapi <spec.yaml|spec.json> --out <carpeta> [--dry-run]
```

Una especificación OpenAPI 3 (3.0 o 3.1), en JSON o YAML. Swagger 2.0 todavía no: se rechaza diciéndolo. Las decisiones, con las de Bruno donde las tiene:

- Cada request empieza por `{{baseUrl}}`, una variable de la colección con el primer `server` (sus variables, con su valor por defecto). Con varios servidores, además, un entorno por servidor, con su descripción como nombre.
- Una carpeta por el primer `tag` de cada operación, en el orden de `tags`; las operaciones sin tag van en la raíz. El nombre es el `summary` (su primera línea), el `operationId` o `MÉTODO ruta`.
- `{id}` en la ruta pasa a ser un path param (`:id`) con su ejemplo. Los params de query y de header se listan todos, activos si son obligatorios, con su ejemplo, su `default` o el primer valor de `enum`; los de cookie se avisan.
- El body del primer content type: JSON, formulario, multipart (las propiedades `binary` como partes de archivo, por elegir), archivo para `octet-stream`, o texto; con el ejemplo de la especificación o uno construido desde el esquema (`$ref`, `allOf`, `oneOf`/`anyOf` y los formatos habituales). Un content type que apifast no pone solo va como header. XML se deja vacío, con aviso.
- La seguridad, global (en la colección) o de la operación (en la request; `security: []` es ninguna): basic, bearer, digest, API key en header o query y OAuth 2.0 (su primer flujo, con los scopes pedidos). Las credenciales son variables por llenar (`{{token}}`, `{{apiKey}}`, `{{clientId}}`…), nunca valores. OpenID Connect y las API key en cookie se avisan.
- Las respuestas con contenido, como ejemplos de la request.
- Solo se siguen los `$ref` locales (`#/…`); uno externo se avisa.

```
apifast import bruno <carpeta> --out <carpeta> [--dry-run]
```

Una colección de Bruno en su formato anterior, `.bru` (la carpeta con `bruno.json`). Las colecciones de Bruno en YAML no necesitan importarse: se abren tal cual. El parser sigue la gramática de Bruno (`bruno-lang/v2/src/bruToJson.js`): bloques de pares (`headers { nombre: valor }`, `~` delante para desactivar, claves entre comillas, valores de varias líneas entre `'''`, anotaciones como `@description(…)` y los tipos `@number`, `@boolean`, `@object`), de texto (`body:json { … }`) y de lista (`vars:secret [ a, b ]`).

- `bruno.json` da el nombre; `collection.bru`, los headers, la auth y las variables que todo hereda; el `folder.bru` de cada carpeta, su nombre, su posición y lo que heredan sus requests.
- Cada request: método y URL, path params con su valor y params de query desactivados, headers, body (JSON, texto, XML, SPARQL, formulario, multipart con `@file(a|b)` y `@contentType(…)`, archivo, GraphQL), auth (basic, bearer, digest, NTLM, API key, AWS SigV4, OAuth 2.0), variables, aserciones (`res.status: eq 200`), `vars:post-response` como capturas, ajustes, tags y documentación. El orden es el `seq` de Bruno.
- Los entornos de `environments/`: sus variables, y las de `vars:secret` declaradas secretas sin valor (Bruno guarda los valores aparte); se avisa para ponerlos en la bóveda.
- Los scripts (`script:pre-request`, `script:post-response`) y los bloques `tests` se importan tal cual: ya usan la API de Bruno. Una request que no es HTTP (WebSocket, gRPC) se deja fuera con aviso, igual que un archivo que no se puede leer, con su línea.
- Se ignoran las carpetas ocultas y `node_modules`.

```
apifast import insomnia <export.json|export.yaml> --out <carpeta> [--dry-run]
```

Una exportación de Insomnia: la v4 (JSON, una lista de recursos enlazados por `parentId`, en el orden de `metaSortKey`) o la v5 (YAML, `collection.insomnia.rest/5.x`, un árbol). Con las decisiones del importador de Bruno:

- `{{ _.host }}` pasa a `{{host}}`. Las etiquetas de plantilla de Insomnia (`{% response … %}`, `{% uuid %}`…) no tienen equivalente: se avisan.
- Los params de query, que Insomnia guarda aparte, se unen a la URL (los desactivados quedan como params desactivados); los path params conservan su valor.
- Headers, body según su `mimeType` (JSON, XML, texto, formulario, multipart con archivos, archivo, GraphQL; uno que no es `{"query": …}` queda como JSON con aviso), auth (basic, bearer, digest, NTLM, API key, AWS IAM como SigV4, OAuth 2.0 con PKCE; una auth desactivada es ninguna; vacía, hereda), `encodeUrl` y `followRedirects`, la descripción como documentación.
- Las carpetas conservan su auth y su entorno, como variables de la carpeta.
- Los entornos: el base y uno por cada sub-entorno, mezclado sobre el base. Un valor anidado es una variable de tipo objeto (`{{user.name}}` lo lee), no se aplana en claves con puntos.
- Los scripts se avisan: el importador todavía no los traslada; las requests de WebSocket y gRPC se dejan fuera con aviso.

#### Importar otra vez

```
apifast import <formato> <archivo> --out <colección importada> [--dry-run] [--overwrite] [--prune]
```

Si `--out` es una colección que salió de una importación, la actualiza en lugar de negarse ([ADR 0010](adr/0010-reimportacion.md)). Cada importación deja en `.apifast/import.yml` qué escribió; la siguiente compara cada request como vino entonces, como está ahora y como la trae la fuente, y lo imprime:

```
Updating pets (last imported from ../specs/pets.yaml): 1 new, 2 changed, 5 unchanged
  pets/list-pets.yml: changed in the source
  pets/get-pet.yml: changed in the source and here [skip]
  pets/adopt.yml: new
  (--overwrite takes the source's version of what was edited or deleted here)
```

- Lo nuevo se crea y lo que cambió solo en la fuente se actualiza, conservando su posición.
- Lo que cambió también aquí, y lo que se borró aquí, se salta; `--overwrite` toma la versión de la fuente.
- Lo que la fuente ya no tiene se conserva; `--prune` lo borra.
- Lo saltado se vuelve a mostrar en la siguiente reimportación.
- Los entornos solo se añaden.

Una carpeta con una colección que no se importó (sin `.apifast/import.yml`) se rechaza.

### `apifast export`

```
apifast export postman [--collection <carpeta>] --out <archivo.json> [--env <nombre>]...
```

Escribe la colección (la de `--collection`, por defecto la carpeta actual o la que la contiene) como una colección de Postman v2.1, y cada entorno de `--env` junto a ella como `<nombre>.postman_environment.json`, el nombre que usa Postman. Reemplaza los archivos que ya existan. Después imprime lo que no se conservó, con dónde estaba.

Es el camino inverso del importador, así que lo exportado se vuelve a importar igual: probado con las 33 colecciones del corpus de Postman (importar, exportar e importar otra vez da el mismo árbol y las mismas requests) y de extremo a extremo (exportar, importar lo exportado como lo haría Postman y enviar).

- Carpetas y requests en el orden del árbol, con método, URL (también desarmada en `host`, `path`, `query` y `variable`, como la escribe Postman), headers, body (JSON, XML, texto, formulario, multipart con archivos, archivo), auth (basic, bearer, digest, NTLM, API key, AWS SigV4, OAuth 2.0 con sus flujos, PKCE y parámetros adicionales; heredada, no se escribe), documentación y ejemplos guardados.
- Los params de query desactivados y los path params, que la URL no muestra, van en su lista.
- `followRedirects`, `maxRedirects` y `encodeUrl` como `protocolProfileBehavior`; el timeout no tiene equivalente por request y se avisa.
- Postman no hereda headers ni tiene variables de carpeta: los headers de la colección y de las carpetas se copian en cada request, y las variables de carpeta pasan a la colección (si el nombre ya existe, se avisa y no se exporta).
- Un entorno que extiende a otro se exporta con las variables de los dos mezcladas. Los secretos van sin valor, de tipo `secret`.
- Se avisan las aserciones, las capturas, las variables de request y las etiquetas, que Postman no tiene fuera de los scripts, y los scripts, que usan la API de Bruno y no se traducen de vuelta; las de WebSocket y gRPC se dejan fuera. GraphQL va con el modo `graphql` de Postman.

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
| `--cacert <archivo>` | PEM con autoridades que se confían además de las del sistema (repetible) |
| `--insecure` | No comprueba el certificado del servidor; avisa en stderr |
| `--allow-unresolved` | Las variables sin resolver son advertencia |
| `--no-color`, `-q`/`--quiet`, `-v`/`--verbose` | Salida |

Pendiente: `--env-file` (un `.env` adicional). `--cacert` e `--insecure` también valen para `apifast send`; los certificados de cliente salen de la colección.

#### Salida en consola

Una línea por request al terminar, con su ruta, duración y estado; debajo, las aserciones que fallan, las capturas sin valor y el error de una request que no se pudo enviar. Al final, las omitidas y el resumen:

```
Running Shop with environment local
PASS  Login login.yml · 12 ms · 200
FAIL  Missing users/missing.yml · 4 ms · 404
      ✗ res.status eq 200: expected eq 200, got 404
ERROR Create users/create.yml · 0 ms
      variable `name` is not defined
SKIP  socket.yml · websocket requests cannot run yet
3 requests: 1 passed, 1 failed, 1 with errors, 1 skipped · 1/2 assertions · 16 ms
```

- Con varias iteraciones, cada una empieza con `Iteration 2 of 3`.
- `--quiet` muestra solo los fallos y el resumen; `--verbose`, también las aserciones que pasan y los valores capturados (los secretos, ocultos).
- Colores solo en una terminal, y nunca con `--no-color` o la variable `NO_COLOR`.

### Códigos de salida

| Código | Significado |
|---|---|
| 0 | Todas las aserciones y los tests de los scripts pasaron |
| 1 | Al menos una aserción o un test falló, o un script posterior no terminó |
| 2 | Error de ejecución (red, archivo inválido, variable sin resolver, un script previo que falló) |
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
- **Límites:** tiempo máximo de ejecución y memoria máxima por script (`ScriptLimits`; por defecto 10 s y 64 MB). El tiempo cuenta también las esperas: un manejador de interrupción de QuickJS corta el código que sigue corriendo, y un plazo corta el que sigue esperando una promesa. Ninguno de los dos se puede atrapar con `try`. La recursión profunda da un error del script, no un fallo del programa. Cancelar el envío detiene el script igual.
- **Aislamiento:** cada script corre en un runtime propio, creado al empezar y descartado al terminar; no queda nada entre un script y otro. No hay módulos (`import`, `require`), ni `std`, `os` o `fetch`.
- **Forma:** el script es el cuerpo de una función `async`, como en Bruno: puede usar `await`, y `return` lo termina. Corre en modo no estricto.
- **Consola:** `console.log`, `info`, `warn`, `error` y `debug` se guardan con su nivel, también cuando el script falla; las cadenas van tal cual y los demás valores en JSON. Se guarda hasta 1 MB de texto.
- **Errores:** de sintaxis (antes de ejecutar nada), excepciones y promesas rechazadas, con el tipo y la línea del script (`TypeError: cannot read property 'name' of null at line 3`), tiempo agotado, memoria agotada y cancelación.
- **API compatible con Bruno** para facilitar la importación. El motor recibe la request, la respuesta y las variables como datos (`ScriptInput`) y devuelve lo que el script cambió (`ScriptOutcome`); no llama de vuelta a la aplicación mientras corre.

| Objeto | Funciones |
|---|---|
| `req` | `getName`, `getUrl`/`setUrl`, `getMethod`/`setMethod`, `getHeader`/`getHeaders`/`setHeader`/`setHeaders`/`deleteHeader`, `getBody`/`setBody`, `getTimeout`/`setTimeout`, `getHost`, `getPath` (con los parámetros de ruta puestos), `getQueryString`, `getPathParams`, `getTags`, y las propiedades `url`, `method`, `headers`, `body`, `timeout`. Antes del envío los textos van sin renderizar (`{{host}}/users`), porque el script corre antes que las plantillas; después de la respuesta, la URL, el método y las cabeceras son los que se enviaron, ya renderizados, como en Bruno. `getBody` da el JSON ya leído cuando el cuerpo es JSON |
| `res` | `getStatus`, `getStatusText`, `getHeader`, `getHeaders` (nombres en minúscula), `getBody`/`setBody`, `getResponseTime`, `getUrl`, `getSize`, las propiedades `status`, `statusText`, `headers`, `body`, `responseTime`, `url`, y `res('data.items[0].id')` para leer una ruta del cuerpo. No existe antes del envío |
| `bru` | `getVar`/`setVar`/`hasVar`/`deleteVar`/`deleteAllVars`/`getAllVars` (runtime), `getEnvVar`/`setEnvVar` (con `{persist: true}` para guardarla en el archivo)/`hasEnvVar`/`deleteEnvVar`, `getAllEnvVars`/`deleteAllEnvVars`, `getCollectionVar`/`setCollectionVar`/`hasCollectionVar`/`deleteCollectionVar`/`getAllCollectionVars`/`deleteAllCollectionVars` (los cambios valen para el envío y siguen como runtime en la ejecución; el archivo de la colección no cambia), `getRequestVar`, `getFolderVar`, `getEnvName`, `getCollectionName`, `getProcessEnv`, `interpolate` (con la precedencia de las plantillas y las variables dinámicas, como `{{$randomInt}}`), `isSafeMode` (siempre `true`: un script no llega a archivos ni a la red), `sleep`, `setNextRequest` y `runner.setNextRequest`/`skipRequest`/`stopExecution` |
| Temporizadores | `setTimeout`/`clearTimeout` y `setInterval`/`clearInterval`. El script espera a los `setTimeout` pendientes antes de terminar, como Node, dentro del límite de tiempo; a los intervalos no. Si el callback de un temporizador lanza un error, ese es el error del script, y lo que el script cambió se conserva |
| `test` | `test(nombre, fn)`: pasa salvo que `fn` lance o su promesa se rechace; se esperan los tests asíncronos antes de terminar |
| `expect` | La parte de chai que usan los scripts, con sus mensajes (`expected 404 to equal 200`): `equal`, `eql`, `a`/`an`, `include`, `above`/`below`/`least`/`most`/`within`/`closeTo`, `lengthOf`, `property` (también `nested` y `own`), `keys`, `members`, `oneOf`, `match`, `string`, `instanceOf`, `throw`, `satisfy`, `ok`, `true`, `false`, `null`, `undefined`, `NaN`, `exist`, `empty`, con `not`, `deep`, `any`/`all` y las palabras de enlace; `expect.fail` |

Las variables conservan su tipo: `bru.getEnvVar('retries') + 1` suma si la variable es un número, y `bru.setVar('user', {…})` la guarda como objeto. Un nombre de variable solo admite letras, dígitos, `_`, `-` y `.`. Lo que hizo un script que falla (variables, tests) se conserva; lo de uno que se detuvo por tiempo, memoria o cancelación, no.

Todavía no están: `bru.sendRequest` y `bru.runRequest`, `require` de librerías (lodash, moment, crypto-js…), el tarro de cookies (`bru.cookies`), las variables globales (`bru.getGlobalEnvVar`…), `req.headerList` y `res.headerList`, los plugins de chai `jsonBody` y `jsonSchema`, `bru.getSecretVar`, las etiquetas heredadas de las carpetas en `req.getTags`, y los cuerpos que no son texto (formularios, multipart) en `req.getBody` y `req.setBody`. En una ejecución, lo que `bru.setEnvVar` o `bru.setCollectionVar` cambió llega a las siguientes requests como runtime: sus plantillas lo ven, pero `bru.getEnvVar` y `bru.getCollectionVar` leen el valor del archivo.

- **Ubicación en el archivo:** el bloque `runtime.scripts` de OpenCollection, que apifast ya conserva desde la fase F1.
- **Niveles:** scripts de colección, carpeta y request, con el orden "sándwich" de Bruno (antes del envío de afuera hacia adentro; después, de adentro hacia afuera).

### Ejecución en un envío

- **Niveles y orden "sándwich":** ver [Orden de evaluación en un envío](#orden-de-evaluación-en-un-envío). Cada script corre por separado, con lo que dejaron los anteriores: la request que cambiaron y las variables que fijaron. Varios bloques del mismo tipo en un archivo se juntan en uno.
- **Tipos que se ejecutan:** `before-request`, `after-response` y `tests`. Los demás (los de gRPC de Bruno, como `grpc:before-call-start`) se conservan y no se ejecutan; una llamada gRPC no ejecuta scripts todavía. Una conexión WebSocket ejecuta los previos, sobre su handshake.
- **Errores:** si un script previo falla, la request no se envía y el error dice cuál (`the request script before the request failed: …`). Si falla uno posterior o de tests, el envío sigue: el error queda en el resultado y la request cuenta como fallida en el runner.
- **Variables:** las de `bru.setVar` y `bru.setEnvVar` siguen como valores de runtime en las siguientes requests, igual que las capturas. Las de `setEnvVar(…, {persist: true})` se ofrecen para guardar en el entorno, como las capturas de ámbito `environment` (en la interfaz, con la misma confirmación).
- **Runner:** `bru.runner.skipRequest()` en un script previo deja la request sin enviar y la cuenta como omitida; `bru.runner.stopExecution()` termina la ejecución después de la request; `bru.setNextRequest(nombre)` sigue por la request con ese nombre (o termina la iteración si no existe). Los tests de los scripts cuentan en el resumen (`3/4 tests`), en el código de salida y en los reportes: JSON (`tests`, `scriptErrors`, y `summary.tests`), JUnit (un caso `test: …` por test, y un error por script que no terminó) y HTML.
- **Editor:** la pestaña Scripts de la request tiene un editor por tipo (Before request, After response, Test script), con resaltado de JavaScript; las plantillas `{{…}}` no se marcan como variables, porque en un script no se renderizan. Su contador dice cuántos tipos tienen código. Una request WebSocket solo muestra el previo, y una gRPC no tiene la pestaña. Los scripts de una carpeta o de la colección se editan en una ventana, desde "Scripts…" en el menú del árbol, y se guardan en su `folder.yml` u `opencollection.yml`. Si un archivo tiene varios scripts de un tipo, se edita el primero y se avisa; vaciar uno lo quita del archivo.
- **Interfaz:** la pestaña Tests de la respuesta muestra los tests de los scripts (primero los que fallan), los errores y la consola, y su contador suma aserciones y tests.
