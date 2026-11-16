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
| `res.body.<ruta>` | Campo del cuerpo JSON; admite índices (`items[0].id`) |
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

## Orden de evaluación en un envío

1. Variables previas de la request.
2. Envío.
3. Capturas.
4. Aserciones.
5. (F9) Scripts posteriores y tests en JavaScript.

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

| Opción | Descripción |
|---|---|
| `--env <nombre>` | Entorno activo |
| `--env-var <k=v>` | Sobrescribe una variable (repetible) |
| `--env-file <ruta>` | Archivo `.env` adicional |
| `-r`, `--recursive` | Incluye subcarpetas |
| `--tags <a,b>` / `--exclude-tags <a,b>` | Filtro por tags |
| `--bail` | Detiene al primer fallo |
| `--delay <ms>` | Pausa entre requests |
| `--iterations <n>` | Repeticiones |
| `--data <archivo>` | CSV o JSON para iterar |
| `--reporter <tipo>=<ruta>` | `json`, `junit` o `html` (repetible) |
| `--insecure` | No valida certificados |
| `--cacert <ruta>` | Autoridad certificadora adicional |
| `--allow-unresolved` | Las variables sin resolver son advertencia |
| `--no-color`, `--quiet`, `--verbose` | Salida |

### Códigos de salida

| Código | Significado |
|---|---|
| 0 | Todas las aserciones pasaron |
| 1 | Al menos una aserción falló |
| 2 | Error de ejecución (red, archivo inválido, variable sin resolver) |
| 3 | Error de uso (argumentos) |

### Reporters

| Formato | Uso |
|---|---|
| Consola | Resumen legible; es la salida por defecto |
| JSON | Resultado completo para procesar |
| JUnit XML | Integración con sistemas de CI |
| HTML | Informe autocontenido |

Los reporters enmascaran los secretos igual que la interfaz.

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
