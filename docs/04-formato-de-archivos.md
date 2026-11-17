# 4. Formato de archivos

Las colecciones se guardan como archivos YAML compatibles con **OpenCollection**, el formato que Bruno usa por defecto en sus versiones recientes. Los archivos son la única fuente de verdad (ver [ADR 0003](adr/0003-archivos-como-fuente-de-verdad.md)).

## Principios

1. **Un archivo por request.** Los cambios se revisan uno a uno en git.
2. **Carpetas = directorios.** La jerarquía se ve en el explorador de archivos.
3. **Sin ruido en los diffs.** No hay marcas de tiempo ni identificadores por request; el orden de las claves es estable.
4. **Nada sensible en claro.** Los secretos van cifrados o no van (ver [bóveda](06-boveda-de-secretos.md)).
5. **Compatibilidad hacia adelante.** Las claves desconocidas se conservan al reescribir un archivo.

## Layout de una colección

```
mi-api/
├── opencollection.yml        # metadatos y valores por defecto de la colección
├── .env                      # opcional, no se versiona
├── .apifast/                 # datos propios de apifast (se versiona)
│   ├── collection.yml        # identificador estable y challenge de la bóveda
│   └── secrets/
│       └── produccion.yml    # secretos compartidos, cifrados
├── environments/
│   ├── local.yml
│   └── produccion.yml
├── auth/
│   ├── folder.yml            # metadatos y valores por defecto de la carpeta
│   ├── login.yml
│   └── refresh.yml
└── usuarios/
    ├── folder.yml
    ├── listar.yml
    └── crear.yml
```

- El nombre del archivo es un *slug* del nombre de la request. El nombre visible está en `info.name`.
- El orden dentro de una carpeta lo da `info.seq`; a igual `seq`, orden alfabético.
- Se ignoran `.git`, `node_modules` y los patrones listados en la configuración de la colección.

## `opencollection.yml`

```yaml
opencollection: "1.0.0"
info:
  name: Mi API

request:
  headers:
    - name: accept
      value: application/json
  auth:
    type: bearer
    token: "{{access_token}}"
  variables:
    - name: api_version
      value: v1
    - name: page_size
      value:
        type: number
        data: "50"

extensions:
  bruno:
    ignore:
      - .apifast
```

| Campo | Descripción |
|---|---|
| `opencollection` | Versión de la especificación |
| `info.name` | Nombre visible |
| `request.*` | Headers, auth, variables y ajustes que heredan todas las requests |
| `extensions.bruno.ignore` | Lista de rutas que Bruno no muestra. apifast añade `.apifast` para que Bruno no la trate como una carpeta de requests |

El archivo es OpenCollection puro: apifast no escribe claves propias en él.

## `folder.yml`

```yaml
info:
  name: Usuarios
  type: folder
  seq: 2

request:
  headers:
    - name: x-team
      value: plataforma
  variables:
    - name: resource
      value: users
```

## Request HTTP

```yaml
info:
  name: Crear usuario
  type: http
  seq: 2
  tags:
    - smoke

http:
  method: POST
  url: "{{host}}/{{api_version}}/users/:orgId"
  params:
    - name: orgId
      value: "42"
      type: path
    - name: notify
      value: "true"
      type: query
      disabled: true
  headers:
    - name: content-type
      value: application/json
  body:
    type: json
    data: |-
      {
        "name": "{{$randomFullName}}",
        "email": "{{user_email}}"
      }
  auth:
    type: inherit

runtime:
  assertions:
    - expression: res.status
      operator: eq
      value: "201"
    - expression: res.body.id
      operator: isDefined
  actions:
    - type: set-variable
      phase: after-response
      selector:
        expression: res.body.id
        method: jsonq
      variable:
        name: user_id
        scope: runtime

settings:
  timeout: 10000
  followRedirects: true
  maxRedirects: 5

docs: |-
  Crea un usuario en la organización indicada.
```

### Bloques

| Bloque | Contenido |
|---|---|
| `info` | `name`, `type` (`http`, `graphql`, `websocket`, `grpc`), `seq`, `tags`, `description` |
| `http` | `method`, `url`, `params`, `headers`, `body`, `auth` |
| `runtime` | `variables`, `assertions`, `actions` (capturas), `scripts` |
| `settings` | `timeout`, `followRedirects`, `maxRedirects`, `encodeUrl` |
| `docs` | Documentación en Markdown |

Cualquier entrada de lista (`params`, `headers`, variables, aserciones) admite `disabled: true`.

Los parámetros de query se envían desde la URL. Bruno los duplica en `http.params` para su interfaz; apifast nunca los envía dos veces, y el editor de F2 mantendrá sincronizadas la URL y la lista, como Bruno. Los parámetros de ruta (`type: path`) sustituyen los segmentos `:nombre` de la URL.

### Tipos de body

| `type` | `data` |
|---|---|
| `json`, `text`, `xml`, `sparql` | Texto |
| `form-urlencoded` | Lista de `name` / `value` |
| `multipart-form` | Lista de partes; cada parte es texto o archivo con `contentType` |
| `file` | Ruta relativa a la colección y `contentType` |

### Tipos de auth

`inherit`, `none`, `basic`, `bearer`, `apikey`, `digest`, `oauth2`, `awsv4`, `ntlm`. Los campos de cada uno siguen los nombres de OpenCollection (por ejemplo `username` y `password` en `basic`).

```yaml
auth:
  type: awsv4
  accessKeyId: "{{aws_access_key}}"
  secretAccessKey: "{{aws_secret_key}}"
  sessionToken: ""
  service: "execute-api"
  region: "eu-west-1"
  profileName: ""
```

### Certificados de cliente

En `opencollection.yml`, como Bruno:

```yaml
config:
  clientCertificates:
    - domain: api.example.com     # con puerto (`localhost:8443`) y comodines (`*.example.com`)
      type: pem
      certificateFilePath: certs/client.pem
      privateKeyFilePath: certs/client-key.pem
    - domain: "*.corp"
      type: pkcs12
      pkcs12FilePath: certs/client.pfx
      passphrase: "{{pfx_pass}}"
      disabled: true
```

Las rutas son relativas a la colección. La lista se reescribe entera al cambiar, y sin certificados la clave se quita.

### Proxy de la colección

En `opencollection.yml`, como lo escribe Bruno:

```yaml
config:
  proxy:
    inherit: false        # true: el proxy de la aplicación
    disabled: true        # opcional: sin proxy, gane lo que gane `inherit`
    config:
      protocol: socks5    # http, https, socks4 o socks5
      hostname: proxy.corp
      port: "1080"
      auth:
        username: ana
        password: ""      # en claro, como en Bruno
        disabled: true    # opcional: no enviar las credenciales
      bypassProxy: localhost, .internal
```

Sin bloque, o con uno que no se entiende, la colección hereda el de la aplicación. Una colección sin bloque solo lo gana al cambiar su proxy. Los valores no admiten `{{variables}}` todavía.

`oauth2` es un bloque anidado, como lo escribe Bruno:

```yaml
auth:
  type: oauth2
  flow: authorization_code        # client_credentials, resource_owner_password_credentials, implicit
  authorizationUrl: https://id.example.com/authorize
  accessTokenUrl: https://id.example.com/token
  refreshTokenUrl: ""             # opcional; si falta, la del token
  callbackUrl: http://localhost:8081/callback
  credentials:
    clientId: my-app
    clientSecret: "{{client_secret}}"
    placement: body               # o basic_auth_header
  resourceOwner:                  # solo en el flujo password
    username: ana
    password: "{{password}}"
  additionalParameters:
    authorizationRequest:
      - name: audience
        placement: query          # header, query o body
        value: api
  scope: read write
  state: xyz
  pkce: {}                        # `disabled: true` lo apaga; sin bloque, apagado
  tokenConfig:
    id: credentials               # nombre del token en la caché
    placement:
      header: Bearer              # o `query: access_token`
    source: access_token          # o id_token
  settings:
    autoFetchToken: true
    autoRefreshToken: false
```

- Lo que falta toma los valores por defecto de Bruno: token en el header con `Bearer`, se obtiene solo y no se refresca, PKCE apagado.
- Al editar, solo se reescriben las claves que cambian dentro del bloque; las que apifast no modela se conservan. Los textos vacíos no se escriben, como hace Bruno.
- Los parámetros adicionales se leen y se conservan; la interfaz aún no los edita.
- `implicit` se lee y se conserva, pero no se envía: es un flujo obsoleto.

Los campos sensibles de auth **no guardan el secreto**: referencian una variable secreta, por ejemplo `token: "{{access_token}}"`.

### Semántica al leer (igual que Bruno)

`apifast-storage-fs` lee los archivos como los lee Bruno (`bruno-filestore/src/formats/yml`), para que una colección se comporte igual en las dos herramientas:

| Caso | Significado |
|---|---|
| Request o carpeta sin `auth` | Sin autenticación. Solo `auth: inherit` hereda |
| `folder.yml` sin `auth` | Anula la auth de la colección en esa carpeta. Una carpeta sin `folder.yml` hereda |
| Colección sin `auth` | Sin autenticación (valor por defecto) |
| Auth en `runtime.auth` (OpenCollection v3 rc1) | Se lee; al cambiar la auth se escribe en `http.auth` |
| Auth de un tipo no implementado (`oauth2`, `wsse`…) | Se conserva intacta; enviar la request da un error claro |
| Request sin bloque `settings` | Todos los ajustes se heredan |
| `settings` con `timeout: 0` | Sin límite de tiempo |
| `settings` con `timeout: inherit` | Se hereda |
| `settings` sin `timeout` | 0 (sin límite), como en Bruno |
| Request `graphql`, `grpc` o `websocket` | Aparece en el árbol; abrirla da "no soportado todavía" |
| Archivo YAML inválido | Aparece en el árbol como inválido; no impide abrir el resto |

OpenCollection no tiene ajustes de envío en carpetas ni en la colección, así que en archivos de Bruno solo la request los define. Sin bloque `settings`, apifast usa su valor por defecto de 30 s; Bruno usa su preferencia global (0, sin límite).

### Guardado

Guardar nunca reescribe un archivo desde cero:

1. Se relee el archivo del disco.
2. Se escriben solo los campos que cambiaron. Los elementos de las listas se alinean por LCS, así que cada uno conserva sus claves desconocidas (por ejemplo `description`) aunque se añadan o quiten otros.
3. La diferencia se aplica como JSON Patch por elemento sobre el texto original, de modo que los comentarios y el formato del resto se conservan.
4. Se relee el resultado y se comprueba que es exactamente lo que se quería guardar; si no, se devuelve un error y no se escribe nada.
5. Se escribe de forma atómica (archivo temporal, `fsync` y `rename`), conservando los permisos.

Guardar una request sin cambios no toca el archivo. Los archivos nuevos se escriben como Bruno: comillas dobles y una línea en blanco entre bloques. El corpus de 436 archivos de Bruno se relee y se guarda sin cambiar un byte (`crates/apifast-storage-fs/tests/corpus`).

## Entornos

`environments/produccion.yml`:

```yaml
name: Producción
color: "#d9534f"
extends: local

variables:
  - name: host
    value: https://api.ejemplo.com
  - name: max_retries
    value:
      type: number
      data: "3"
  - name: api_key
    secret: true
  - name: personal_token
    secret: true
```

Un secreto se declara igual que en Bruno: `secret: true` y sin valor. Dónde está el valor depende de su clase:

| Caso | En el archivo del entorno | Dónde está el valor |
|---|---|---|
| Variable normal | `value` en claro | En el archivo |
| Secreto solo local (por defecto) | `secret: true` | En la bóveda local de cada usuario |
| Secreto compartido | `secret: true` | En `.apifast/secrets/<entorno>.yml`, cifrado con la clave de la colección |

Un secreto es compartido si tiene una entrada en `.apifast/secrets/`; si no, es solo local.

## `.env`

Archivo `clave=valor` clásico. Sus valores se leen con `{{process.env.NOMBRE}}`. Precedencia: `.env` de la colección sobre variables de entorno del sistema. No se versiona; apifast sugiere añadirlo a `.gitignore` al crearlo.

## Carpeta `.apifast/`

Los datos propios de apifast no se escriben dentro de los archivos OpenCollection. Van en una carpeta aparte, dentro de la colección, que **se versiona en git**.

El motivo: Bruno reconstruye cada archivo desde su modelo interno al guardar y elimina cualquier clave que no conozca (comprobado en `bruno/packages/bruno-filestore/src/formats/yml/stringifyEnvironment.ts` y `bruno/packages/bruno-filestore/src/formats/yml/stringifyCollection.ts`). Con una carpeta aparte, Bruno y apifast pueden trabajar sobre la misma colección sin pisarse.

`.apifast/collection.yml`:

```yaml
format: 1
id: col_7Hq2mXv9Rk
vault:
  challenge: "enc:v1:Zm9vYmFyYmF6cXV4Zm9vYmFyYmF6cXV4Zm9vYmFyYmF6cXV4"
```

| Campo | Descripción |
|---|---|
| `format` | Versión del formato de `.apifast/` |
| `id` | Identificador estable de la colección. No cambia al mover la carpeta. Lo usa la bóveda |
| `vault.challenge` | Texto cifrado con la clave de la colección; permite detectar una clave incorrecta |

`.apifast/secrets/produccion.yml`:

```yaml
environment: produccion
secrets:
  - name: api_key
    value: "enc:v1:c2VjcmV0by1jaWZyYWRvLWRlLWVqZW1wbG8tbm8tcmVhbA"
```

Reglas:

- La carpeta se crea la primera vez que hace falta (al asignar el identificador). Una colección de Bruno sin `.apifast/` se abre con normalidad.
- Al crearla, apifast añade `.apifast` a `extensions.bruno.ignore` en `opencollection.yml`. Bruno no ignora las carpetas ocultas por sí solo y, sin esa entrada, la mostraría como una carpeta de requests.
- Al renombrar o borrar un entorno o una variable secreta, apifast actualiza el archivo correspondiente de `.apifast/secrets/`.
- Una entrada huérfana (secreto cifrado cuya variable ya no existe, por ejemplo tras editar con Bruno) se señala en el editor de entornos y se puede limpiar.

## Workspace (`workspace.yml`)

Un workspace agrupa colecciones ([ADR 0009](adr/0009-workspaces-y-varias-colecciones.md)). Es una carpeta con un `workspace.yml`, el mismo formato que usa Bruno, que se puede versionar en git junto a sus colecciones:

```
mi-equipo/
├── workspace.yml
├── environments/          # entornos globales del workspace (Bruno); se conservan
└── collections/
    ├── shop/              # una colección: opencollection.yml, requests...
    └── pagos/
```

```yaml
opencollection: 1.0.0
info:
  name: "Mi equipo"
  type: workspace
collections:
  - name: "Shop API"
    path: "collections/shop"
  - name: "Legacy"
    path: "/home/ana/apis/legacy"
specs:
docs: ''
```

| Campo | Significado |
|---|---|
| `info.name` | Nombre del workspace |
| `info.type` | Siempre `workspace` |
| `collections[].path` | Carpeta de la colección: relativa a la del workspace si está dentro, absoluta si está fuera |
| `collections[].name` | Nombre que Bruno muestra; apifast lo escribe al añadir y muestra el de la propia colección |
| `specs`, `docs` | De Bruno; se conservan sin cambios |

- Al añadir o quitar una colección se cambian solo sus líneas, como en el resto de archivos.
- Una misma colección puede aparecer en varios workspaces.
- Los entornos de `environments/` son los globales del workspace. Por ahora se conservan sin usarse; su uso como ámbito `Global` llega en una tarea posterior.
- Lo personal (qué colecciones están desplegadas, las pestañas abiertas) no va en este archivo: ver "Estado local".
- Cada colección sigue guardando su sesión: sus borradores y su entorno activo. Las pestañas abiertas y su orden son del workspace. Un borrador de una request sin pestaña (por ejemplo, tras cerrar su colección) vuelve al abrir esa request.

## Compatibilidad con Bruno

| Dirección | Estado |
|---|---|
| apifast lee una colección YAML de Bruno | Directo, sin importador |
| apifast lee una colección `.bru` de Bruno | Mediante el importador (fase F6) |
| Bruno lee una colección de apifast | Directo. Los secretos aparecen sin valor, como cualquier secreto de Bruno en una máquina nueva |
| Bruno reescribe archivos de la colección | Seguro: no toca `.apifast/` y los archivos no contienen claves de apifast |
| Bruno renombra un entorno o una variable secreta | El secreto compartido queda huérfano; apifast lo detecta y ofrece reasignarlo |

Los bloques `runtime.scripts` de Bruno se conservan intactos aunque apifast no los ejecute hasta la fase F9.

## Reglas de lectura y escritura

- **Escritura atómica:** se escribe en un archivo temporal del mismo directorio y se renombra sobre el destino, conservando permisos.
- **Round-trip sin pérdida:** leer un archivo y guardarlo sin cambios produce exactamente el mismo contenido. Se comprueba con tests sobre un corpus de archivos.
- **Guardado por edición, no por reescritura:** al guardar, se relee el archivo del disco como documento, se calcula un JSON Patch por elemento entre los datos leídos y los nuevos, y se aplica. Solo cambian las líneas editadas; se conservan comentarios, líneas en blanco, comillas y estilos de bloque. Ver el [spike 1](spikes/01-libreria-yaml.md).
- **Archivos nuevos:** se escriben con el mismo estilo que Bruno (listas indentadas y una línea en blanco entre bloques).
- **Orden de claves estable:** el serializador escribe siempre en el mismo orden.
- **Claves desconocidas:** se guardan en un mapa de "extras" del DTO y se vuelven a escribir.
- **Errores de parseo:** el archivo aparece en el árbol marcado con error y se puede abrir como texto; nunca se sobrescribe automáticamente.
- **Archivos grandes:** por encima de un umbral (2 MB por defecto) solo se cargan los metadatos hasta que el usuario abre la request.

## Versionado y migraciones

- `format` en `.apifast/collection.yml` empieza en `1`.
- Un cambio incompatible incrementa el número y añade una migración que se aplica al abrir la colección, con confirmación del usuario.
- Una colección con formato más nuevo que la aplicación se abre en modo solo lectura.

## Estado local (fuera de la colección)

El historial, las respuestas, las cookies y la bóveda local viven en el directorio de datos del usuario, nunca dentro de la colección. Ver [ADR 0008](adr/0008-estado-local.md).

```
~/.local/share/apifast/
├── master.key                        # clave maestra, solo si no hay keyring (0600)
├── keys/<collection-id>.key          # clave de la colección, envuelta
├── vault/<collection-id>.vault       # secretos solo locales, cifrados (JSON, 0600)
├── cookies/<collection-id>.jar       # cookies, cifradas
├── oauth2/<collection-id>.tokens     # tokens OAuth 2.0, cada uno sellado (JSON, 0600)
├── settings.sealed                   # valores de los ajustes sellados, como la contraseña del proxy (0600)
├── history/<collection-id>.jsonl     # índice de ejecuciones
├── responses/<collection-id>/        # cuerpos de respuesta
├── sessions/<collection-id>/         # pestañas abiertas de la colección
│   ├── session.yml                   # rutas de las pestañas, la activa y el entorno
│   └── drafts/<n>.yml                # cambios sin guardar de la pestaña n, como archivo de request
├── workspaces.yml                    # workspaces conocidos y el último activo
├── workspaces/<hash-de-la-ruta>/
│   └── state.yml                     # colecciones desplegadas, orden de pestañas y la activa
├── default-workspace/                # "My workspace", creado en el primer arranque
│   └── workspace.yml
└── ui-state.yml                      # tamaños de paneles
~/.config/apifast/
├── settings.yml
└── keymap.yml                        # atajos cambiados por el usuario, por acción
```

En macOS y Windows se usan los directorios equivalentes de cada plataforma.

`settings.yml` guarda todos los ajustes, con sus valores por defecto si no se cambiaron:

```yaml
theme: system           # system, light o dark
ui_scale: 1             # de 0.75 a 2
pretty_limit_mb: 2      # bodies formateados y coloreados hasta este tamaño
history_entries: 1000   # envíos que se conservan en el historial
keep_days: 30           # días que se conservan el historial y los cuerpos
responses_mb: 512       # espacio máximo para cuerpos de respuesta
proxy: system           # system, off o manual: el proxy de la aplicación
proxy_protocol: http    # http, https, socks4 o socks5
proxy_host: ""
proxy_port: ""
proxy_auth: false
proxy_username: ""
proxy_bypass: ""        # hosts que van directos, separados por comas
ca_file: ""             # PEM con autoridades que se confían además de las del sistema
tls_insecure: false     # no comprobar los certificados de los servidores
```

La contraseña del proxy de la aplicación no está en `settings.yml`: se guarda sellada con la clave maestra en `settings.sealed`, en el directorio de datos.

`APIFAST_CONFIG_DIR` sustituye a `~/.config/apifast/`, como `APIFAST_DATA_DIR` al directorio de datos.

### Historial

Cada envío terminado añade una línea JSON a `history/<collection-id>.jsonl`; un envío cancelado no la añade:

```json
{"v":1,"at":1763200000000,"request":"users/get.yml","method":"GET","url":"{{host}}/users/1",
 "response":{"status":200,"version":"HTTP/1.1","url":"http://localhost:8080/users/1","redirects":0,
   "headers":[["content-type","application/json"]],"elapsed_ms":12,"size":21,"body":"<id>",
   "timeline":[{"type":"request","at_us":0,"method":"GET","url":"http://localhost:8080/users/1"},
               {"type":"headers","at_us":11250,"status":200},{"type":"body","at_us":12000,"bytes":21}]}}
```

- Un envío fallido lleva `"error": "<mensaje>"` en lugar de `response`. Los mensajes de error nunca contienen secretos.
- `url` es la URL tal como está escrita, con sus `{{variables}}` sin resolver.
- El historial no se cifra, así que:
  - si la request llevaba un secreto, los valores de la query de las URLs resueltas se sustituyen por `[hidden]`;
  - los valores de `Set-Cookie` se sustituyen siempre.
- `body` es el id del cuerpo en `responses/`. Puede haberse borrado por los límites, y entonces la entrada sigue en el historial sin cuerpo.
- Las líneas que no se pueden leer (de un formato `v` más nuevo o cortadas por un cierre inesperado) se ignoran. Si una línea quedó cortada, la siguiente escritura empieza en una línea nueva.
- Al abrir la colección se recortan el historial (1000 entradas, 30 días) y los cuerpos (1000, 512 MB, 30 días). Recortar reescribe el archivo aparte y lo renombra.

Una colección sin identificador en `.apifast/` usa como clave un hash de su ruta. La sesión se escribe poco después de cada cambio y al cerrar la aplicación. Los borradores tienen el mismo formato que los archivos de la colección; si uno no se puede leer, solo se pierde ese borrador, no la sesión.
