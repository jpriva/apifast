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

### Tipos de body

| `type` | `data` |
|---|---|
| `json`, `text`, `xml`, `sparql` | Texto |
| `form-urlencoded` | Lista de `name` / `value` |
| `multipart-form` | Lista de partes; cada parte es texto o archivo con `contentType` |
| `file` | Ruta relativa a la colección y `contentType` |

### Tipos de auth

`inherit`, `none`, `basic`, `bearer`, `apikey`, `digest`, `oauth2`, `awsv4`, `ntlm`. Los campos de cada uno siguen los nombres de OpenCollection (por ejemplo `username` y `password` en `basic`).

Los campos sensibles de auth **no guardan el secreto**: referencian una variable secreta, por ejemplo `token: "{{access_token}}"`.

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
├── keys/<collection-id>.key          # clave de la colección, envuelta
├── vault/<collection-id>.vault       # secretos solo locales, cifrados
├── cookies/<collection-id>.jar       # cookies, cifradas
├── history/<collection-id>.jsonl     # índice de ejecuciones
├── responses/<collection-id>/        # cuerpos de respuesta
└── ui-state.yml                      # pestañas abiertas, tamaños de paneles
~/.config/apifast/
├── settings.yml
└── keymap.yml
```

En macOS y Windows se usan los directorios equivalentes de cada plataforma.
