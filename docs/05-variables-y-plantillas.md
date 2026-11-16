# 5. Variables y plantillas

Las plantillas permiten insertar valores dinámicos en la URL, los parámetros, los headers, el body y los campos de autenticación. La sintaxis base es la de Bruno (`{{ }}`); las funciones con argumentos nombrados vienen de Yaak.

## Sintaxis

| Forma | Ejemplo | Significado |
|---|---|---|
| Variable | `{{host}}` | Valor de la variable `host` |
| Ruta en objeto | `{{user.address.city}}` | Campo de una variable de tipo objeto |
| Variable dinámica | `{{$randomUUID}}` | Valor generado en cada envío |
| Pregunta | `{{?Código OTP}}` | Pide el valor al usuario antes de enviar |
| Entorno del proceso | `{{process.env.API_KEY}}` | Variable de `.env` o del sistema |
| Función | `{{ response(request='auth/login', path='$.token') }}` | Llamada con argumentos nombrados |
| Escape | `\{{literal}}` | Texto literal, sin interpolar |

Reglas:

- Los nombres de variable admiten letras, dígitos, `_`, `-` y `.`.
- Una función se reconoce por los paréntesis. Sus argumentos siempre son nombrados.
- Los valores de los argumentos pueden ser cadenas entre comillas simples, `true`, `false`, `null`, otra variable u otra función.
- Una etiqueta mal formada se trata como texto literal y se señala en el editor; no interrumpe el envío.

## Ámbitos y precedencia

Cuando el mismo nombre existe en varios ámbitos, gana el más específico. De menor a mayor prioridad:

| # | Ámbito | Dónde se define |
|---|---|---|
| 1 | Global | Entorno global del workspace |
| 2 | Colección | `request.variables` en `opencollection.yml` |
| 3 | Entorno activo | `environments/<nombre>.yml`, incluyendo lo heredado por `extends` |
| 4 | Carpetas | `request.variables` en cada `folder.yml`, de la más externa a la más interna |
| 5 | Request | `runtime.variables` en el archivo de la request |
| 6 | Runtime | Capturas de respuestas anteriores (en memoria) |
| 7 | Pregunta | Valores introducidos por el usuario para este envío |

`process.env.*` es un espacio de nombres aparte y no participa en esta cadena.

Las variables deshabilitadas (`disabled: true`) se ignoran.

### Procedencia

El resolvedor no devuelve solo el valor: devuelve el valor **y su origen**. La interfaz lo usa para:

- mostrar al pasar el cursor de dónde viene cada variable,
- indicar cuándo un valor está siendo sobrescrito por otro ámbito,
- saltar al archivo que la define.

### Herencia entre entornos

Un entorno puede declarar `extends` con el nombre de otro. Las variables del padre se aplican primero y las del hijo las sobrescriben. Los ciclos se detectan al cargar y se reportan como error.

## Variables tipadas

Una variable puede ser texto, número, booleano u objeto:

```yaml
variables:
  - name: page_size
    value:
      type: number
      data: "50"
  - name: feature_flags
    value:
      type: object
      data: '{"beta": true}'
```

- Al interpolar en texto, el valor se convierte a cadena; los objetos se insertan como JSON.
- Las aserciones y, más adelante, los scripts reciben el valor con su tipo real.

### Rutas dentro de un objeto

`{{feature_flags.beta}}` lee un campo del valor JSON de una variable; un segmento numérico indexa un array (`{{user.tags.0}}`). Reglas:

- Una variable cuyo nombre contiene puntos tiene prioridad: si existe `api.v`, `{{api.v}}` es ella. Si no, gana el prefijo más largo que sea una variable (`api.v` antes que `api` para `{{api.v.x}}`).
- El valor de la variable se renderiza antes de leerlo, así que puede contener plantillas.
- Un campo que no existe, o un valor que no es JSON, es una variable sin definir, con el nombre completo en el mensaje.
- Un campo de un secreto es secreto: se enmascara igual.
- Las capturas de un objeto o un array crean variables de tipo objeto, así que `{{user.id}}` funciona tras capturar `res.body.user`.

## Variables dinámicas

Se generan en cada envío. Conjunto inicial:

| Variable | Valor |
|---|---|
| `{{$randomUUID}}` | UUID versión 4 |
| `{{$timestamp}}` | Segundos desde la época Unix |
| `{{$isoTimestamp}}` | Fecha y hora en ISO 8601 |
| `{{$randomInt}}` | Entero entre 0 y 1000 |
| `{{$randomEmail}}`, `{{$randomFullName}}`, `{{$randomWord}}` | Datos de prueba |

El resto de las variables de Bruno se añade según demanda para facilitar la importación.

## Funciones

| Función | Argumentos | Resultado |
|---|---|---|
| `response` | `request`, `path`, `refresh`, `ttl` | Valor extraído de la respuesta de otra request |
| `header` | `request`, `name`, `refresh`, `ttl` | Header de la respuesta de otra request |
| `base64` | `value`, `mode` | Codifica o decodifica |
| `hash` | `value`, `algorithm` | SHA-256, SHA-1 o MD5 en hexadecimal |
| `hmac` | `value`, `key`, `algorithm` | HMAC en hexadecimal o base64 |
| `urlencode` | `value` | Codificación de URL |
| `now` | `format`, `offset` | Fecha y hora con formato |
| `file` | `path` | Contenido de un archivo relativo a la colección |
| `jwt` | `payload`, `secret`, `algorithm` | Token JWT firmado |

### Encadenamiento de requests

`response(...)` permite que una request use el resultado de otra sin scripts:

```
{{ response(request='auth/login', path='$.access_token', refresh='ttl', ttl='300') }}
```

- `request` es la ruta relativa dentro de la colección, sin extensión.
- `path` es una expresión JSONPath (o XPath si la respuesta es XML).
- `refresh` controla cuándo se vuelve a ejecutar la request referenciada:

| Valor | Comportamiento |
|---|---|
| `never` | Usa la última respuesta guardada; error si no hay |
| `missing` | Ejecuta solo si no hay respuesta guardada (por defecto) |
| `ttl` | Ejecuta si la última respuesta tiene más de `ttl` segundos |
| `always` | Ejecuta siempre |

En la vista previa de la interfaz, `always` y `ttl` se comportan como `missing` para no disparar requests al escribir.

Se detectan ciclos entre requests encadenadas y se reportan como error.

#### Cómo funciona

- El renderizado es síncrono y enviar no lo es, así que se renderiza por rondas: una llamada cuya respuesta no está a mano anota la request y falla esa ronda; el envío ejecuta lo anotado y vuelve a renderizar. Por eso también funcionan las llamadas dentro de variables de entorno (`token: "{{ response(request='auth/login', path='$.token') }}"`). Hay un máximo de 16 rondas.
- La dependencia se envía con el mismo entorno y las mismas variables de runtime. Si ella misma usa `response(...)`, se resuelve igual, de forma recursiva.
- Cada dependencia se envía una sola vez por envío, aunque se use varias veces y aunque sea `always`.
- La "respuesta guardada" es la última del historial cuyo cuerpo sigue guardado. La CLI no escribe historial, así que en `apifast send` toda dependencia `missing` o `ttl` se envía (una vez).
- Las dependencias quedan en el historial y la interfaz las muestra. Sus capturas y aserciones se ejecutan pero no se informan.
- Un ciclo se rechaza antes de enviar nada: `requests depend on each other: a.yml → b.yml → a.yml`. Si una dependencia falla, el error la nombra: `` `login.yml`, used by response(): ... ``.
- `path` admite, por ahora, la forma simple de JSONPath (`$.data.token`, `$.items[0].id`); sin `path`, el resultado es el cuerpo entero. El JSONPath completo y XPath llegan con su tarea.
- `header` une con `, ` los valores repetidos. En el historial los `Set-Cookie` se guardan ocultos, así que `header(name='set-cookie')` solo funciona con una respuesta enviada en el mismo envío.
- Solo se lee para esto un cuerpo de hasta 16 MiB.

## Secretos en plantillas

Los secretos **siempre son variables**. Un campo sensible referencia la variable (`{{api_key}}`) y la variable está marcada como secreta en el entorno. No existe una sintaxis para incrustar texto cifrado dentro de una plantilla.

Consecuencias:

- El valor descifrado solo existe en memoria durante el render.
- El resultado del render queda marcado como "contiene secretos", y el timeline, el historial y los logs lo enmascaran.
- En el editor el valor aparece oculto, con un botón para revelarlo temporalmente.

## Variables sin resolver

Si una variable no existe en ningún ámbito:

- En el editor se resalta en otro color.
- Antes de enviar, la interfaz avisa y lista las que faltan; el usuario puede enviar igual.
- En la CLI es un error por defecto; `--allow-unresolved` lo convierte en advertencia.

## Diseño del motor (`apifast-template`)

- **Parser manual** que produce una lista de tokens: `Texto`, `Etiqueta` o `Fin`. Una etiqueta contiene un valor: cadena, booleano, nulo, variable, variable dinámica, pregunta o función con argumentos.
- **Árbol reversible:** convertir los tokens de vuelta a texto reproduce la plantilla original. Esto permite renombrar variables y reescribir plantillas sin alterar el resto.
- **Render recursivo:** el valor de una variable puede contener otras plantillas. Hay un límite de profundidad (50) para cortar ciclos.
- **Entrada/salida por trait:** el renderer recibe un `Host` que aporta la hora, la aleatoriedad, `process.env`, las preguntas y las funciones. Así el crate sigue siendo de dominio y las pruebas usan un `Host` falso. Las funciones puras podrán vivir en el crate; las que necesitan entrada/salida (`response`, `file`) las aporta la capa de aplicación.
- **Dos modos:** estricto (falla en el primer problema) y permisivo (deja vacío y devuelve la lista de problemas), este último para la vista previa. Los problemas nunca incluyen el valor de un secreto, solo su nombre.
- **Ciclos:** además del límite de profundidad, un ciclo entre variables (`a → b → a`) se informa como tal.
- **Escape:** `\{{` siempre produce `{{` literal.
- **Sin asignaciones innecesarias:** un texto sin etiquetas se devuelve tal cual, sin copiar.

## Ejemplo completo

Con estas definiciones:

| Ámbito | Variable | Valor |
|---|---|---|
| Colección | `api_version` | `v1` |
| Entorno `local` | `host` | `http://localhost:8080` |
| Entorno `produccion` | `host` | `https://api.ejemplo.com` |
| Carpeta `usuarios` | `resource` | `users` |
| Runtime | `user_id` | `981` |

La URL `{{host}}/{{api_version}}/{{resource}}/{{user_id}}` con el entorno `produccion` activo se convierte en `https://api.ejemplo.com/v1/users/981`.
