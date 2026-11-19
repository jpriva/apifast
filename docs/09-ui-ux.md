# 9. UI y UX

La interfaz se construye con egui (eframe). El objetivo es que resulte familiar a quien viene de Postman, Bruno o Yaak y que se pueda manejar por completo con el teclado.

## Principios

1. **Enviar es lo primero.** Abrir la aplicación, escribir una URL y pulsar `Ctrl+Enter` debe funcionar sin crear nada antes.
2. **Lo que ves es lo que se envía.** Las variables muestran su valor resuelto y su origen al pasar el cursor.
3. **Nada se pierde.** Los cambios sin guardar sobreviven al cierre; las pestañas se restauran al abrir.
4. **Teclado completo.** Toda acción tiene atajo y aparece en la paleta de comandos.
5. **Sin sorpresas con secretos.** Siempre ocultos hasta que el usuario pide verlos.

## Layout principal

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ Mi API                         [ Buscar / comandos  Ctrl+K ]    ● produccion ▾│
├───────────────────┬──────────────────────────────────────────────────────────┤
│ Mi equipo ▾       │  login ×   crear usuario ●   Pagos · listar ×      +     │
│ [+ Colección] [⌂] ├──────────────────────────────────────────────────────────┤
│ ▾ Mi API          │ [POST ▾] {{host}}/v1/users/:orgId              [ Enviar ] │
│   ▾ auth          ├──────────────────────────────────────────────────────────┤
│      POST login   │ Params  Headers(2)  Body  Auth  Vars  Tests  Docs  Ajustes│
│      POST refresh │ ┌──────────────────────────────────────────────────────┐ │
│   ▾ usuarios      │ │ {                                                    │ │
│      GET  listar  │ │   "name": "{{$randomFullName}}",                     │ │
│      POST crear   │ │   "email": "{{user_email}}"                          │ │
│ ▸ Pagos           │ │ }                                                    │ │
│ ▸ Legacy  ⚠       │ └──────────────────────────────────────────────────────┘ │
│                   ├──────────────────────────────────────────────────────────┤
│                   │ 201 Created    182 ms    1.2 kB        Tests 2/2 ✓       │
│                   │ Body  Headers(8)  Cookies  Timeline  Tests               │
│ Colecciones │ Hist│ ┌──────────────────────────────────────────────────────┐ │
│                   │ │ { "id": 981, "name": "Ana Ruiz" }                    │ │
│                   │ └──────────────────────────────────────────────────────┘ │
├───────────────────┴──────────────────────────────────────────────────────────┤
│ main ↑1  ·  2 sin guardar  ·  bóveda desbloqueada                            │
└──────────────────────────────────────────────────────────────────────────────┘
```

| Zona | Contenido |
|---|---|
| Barra superior | Nombre de la colección de la pestaña activa, búsqueda y paleta de comandos, selector de entorno de esa colección con su color |
| Barra lateral | Selector de workspace, árbol con una raíz por colección, historial de la colección activa. Se puede ocultar y redimensionar |
| Pestañas | Una por request abierta, de cualquier colección; un punto indica cambios sin guardar. El nombre de la colección aparece en el tooltip, y delante del nombre ("Billing / Health") cuando dos pestañas de colecciones distintas se llaman igual |
| Barra de URL | Método, URL con resaltado de variables, botón de enviar o cancelar. Pegar un comando cURL convierte la request (ver abajo) |
| Panel de request | Pestañas de edición |
| Panel de respuesta | Estado, tiempo, tamaño y pestañas de visualización |
| Barra de estado | Rama de git, cambios sin guardar, estado de la bóveda |

Los paneles de request y respuesta se pueden disponer uno encima del otro o lado a lado (`Ctrl+J`).

**Pegar un cURL en la barra de URL.** Si lo que se pega en el campo de la URL empieza por `curl `, no se pega como texto: la request pasa a ser la del comando (método, URL, params, headers, body y auth), y conserva su nombre, su posición, sus tests, sus ajustes, su documentación y sus ejemplos. Queda como un cambio sin guardar, que se puede deshacer descartándolo. Se intercepta el pegado antes de que lo vea el campo, que es de una línea y perdería los saltos de línea del comando. Una nota sobre las pestañas dice las opciones que no se conservaron (`-k`, `--proxy …`) o por qué el comando no se puede leer; desaparece al editar la URL. Escribir `curl` a mano no convierte nada.

**Copiar como cURL.** En el menú de cada request del árbol ("Copy as cURL") y en la paleta, para la pestaña activa. Copia al portapapeles la request **tal como se enviaría**: con el entorno activo, las variables de runtime, los secretos y el borrador de la pestaña, para pegarla en una terminal. Por eso lleva los secretos en claro; es una acción explícita del usuario. No envía nada: con OAuth 2.0 usa el token guardado si sigue valiendo (si no, sale sin él), y un `response(...)` que necesitaría enviar otra request queda vacío. Lo que no se pudo llenar queda vacío y el aviso lo dice ("Copied as cURL, with something missing: …"). El comando usa opciones largas, una por línea, con los valores entre comillas simples; las rutas relativas de los archivos pasan a absolutas, y Digest, NTLM y AWS SigV4 salen como `--digest`, `--ntlm` y `--aws-sigv4`.

## Arranque y workspaces

La aplicación entra siempre directamente al visor, sin pantalla de bienvenida ([ADR 0009](adr/0009-workspaces-y-varias-colecciones.md)):

- Abre el último workspace usado, con sus colecciones desplegadas y sus pestañas como se dejaron.
- La primera vez crea "My workspace" en el directorio de datos (`default-workspace/`), que es también la carpeta donde "New collection…" propone crear las colecciones.
- Sin colecciones, el visor muestra un estado vacío con "New collection…" y "Open collection…".
- `apifast-gui <carpeta>`: si la carpeta tiene `workspace.yml`, abre ese workspace; si es una colección, la añade al workspace activo.

La cabecera de la barra lateral tiene:

- El **selector de workspace**, con New workspace…, Open workspace…, Rename y Forget (lo quita de la lista de esta máquina, sin borrar nada).
- **New collection…**: nombre y carpeta padre (por defecto, la del workspace); crea `<padre>/<nombre>` con su `opencollection.yml` y la añade al workspace.
- **Open collection…**: elige una carpeta. Si no tiene `opencollection.yml`, un aviso ofrece crear una ahí.

## Barra lateral

- Una raíz por cada colección del workspace, en su orden. Una raíz plegada no está cargada; desplegarla la carga. Una colección cuya carpeta ya no existe, o que no se puede leer, se marca con su error.
- Menú de la raíz: nueva request, nueva carpeta, entornos, clave de la colección, mostrar la carpeta, cerrar (la descarga y cierra sus pestañas, preguntando si hay cambios sin guardar) y quitar del workspace (no borra archivos).
- Árbol con carpetas plegables. Solo se dibujan las filas visibles.
- Arrastrar y soltar para reordenar (actualiza `seq`) y mover entre carpetas (mueve el archivo), dentro de la misma colección.
- El historial lateral es el de la colección de la pestaña activa, con su nombre encima.
- Arriba del árbol, "New request" y "New folder" crean en la colección enfocada (la de la pestaña activa, o la última elegida en el árbol); desde el menú de una raíz, en esa colección.
- Una raíz en rojo no se pudo cargar (el motivo, al pasar el ratón); en ámbar, la colección comparte secretos y falta su clave en esta máquina.
- Soltar un elemento sobre la raíz de su colección lo lleva al nivel superior; no se puede soltar en otra colección.
- Menú contextual: nueva request, nueva carpeta, duplicar, renombrar, borrar, ejecutar carpeta, mostrar en el explorador de archivos, copiar como cURL.
- Filtro rápido por nombre, método o tag (`Ctrl+F`).
- Los archivos con error de parseo aparecen marcados y se pueden abrir como texto.

## Panel de request

| Pestaña | Contenido |
|---|---|
| Params | Tabla de parámetros de query y de ruta, sincronizada con la URL |
| Headers | Tabla clave/valor; los heredados aparecen atenuados con su origen |
| Body | Selector de tipo y editor: código, tabla de formulario, partes multipart o archivo (ver abajo) |
| Auth | Selector de tipo y sus campos; "heredada" muestra de quién. Con OAuth 2.0 (propia o heredada), el estado del token guardado ("valid for 52 min", "expired", ninguno) y los botones "Get new token" y "Forget token" |
| Vars | Variables de la request y capturas |
| Tests | Aserciones (expresión, operador, valor esperado) y capturas (expresión, variable, ámbito) |
| Docs | Markdown con vista previa |
| Ajustes | Timeout, redirecciones, TLS; cada uno con "heredar" |
| Examples | Ejemplos guardados (`Examples (2)`): cada uno plegado con su estado, nombre editable y botón para borrarlo; al abrirlo, la descripción editable, la request enviada, los headers y el body de la respuesta, de solo lectura |

### Bodies multipart y de archivo

- **Multipart:** una fila por parte con casilla, nombre, tipo ("Text" o "Files"), `Content-Type` (vacío: lo elige apifast) y botón para quitarla. Una parte de texto tiene su valor con las variables resaltadas; una de archivos, su lista (nombre, y la ruta al pasar el cursor) con "Choose files…", que abre el diálogo del sistema y admite varios. "Add part" añade una parte. Cambiar de formulario a multipart conserva los campos como partes de texto, y al revés se conservan las partes de texto.
- **Archivo:** la ruta (editable a mano o con "Choose…") y su `Content-Type`.
- Un archivo dentro de la colección se guarda con su ruta relativa y con `/`, así la colección sigue funcionando en otra máquina; uno de fuera, con la ruta absoluta. Como Bruno.
- El diálogo se abre en otro hilo: la interfaz no se bloquea mientras está abierto.
- Un body de archivo de Bruno con varias variantes muestra la seleccionada; al guardar, solo cambia esa y las demás se conservan.

### Tablas clave/valor

- Casilla para habilitar o deshabilitar cada fila.
- Siempre hay una fila vacía al final para añadir.
- Modo de edición masiva: la tabla como texto, una línea por entrada con el formato `nombre: valor`; las que empiezan por `//` se conservan sin enviarse.
- Autocompletado de nombres de header y de variables.

### Pestaña Tests

- Dos tablas con la misma mecánica que las de clave/valor (casilla, fila vacía al final, botón para quitar): **aserciones** y **capturas**. La pestaña cuenta las que están habilitadas: `Tests (3)`.
- La expresión sugiere `res.status`, `res.headers.`, `res.body.`, `res.responseTime` y `res.size` mientras se escribe.
- El operador es un menú con los 28 de Bruno. Al elegir uno que no lleva valor (`isNull`, `isEmpty`…), el campo del valor esperado se sustituye por "no value" y el valor se descarta; al volver a uno que lo lleva, empieza vacío.
- El valor esperado es un campo de plantilla: resalta las variables como el resto.
- El ámbito de una captura es `runtime`, `request` o `environment`.

### Editor de código

- Resaltado para JSON, XML, SPARQL y texto con plantillas; GraphQL, cuando exista ese tipo de body.
- Formatear (`Alt+Shift+F` o el botón Format) JSON y XML, con dos espacios de sangría. El formateador solo mueve espacios, así que acepta `{{variables}}` donde va un valor (`"id": {{id}}`). Si los corchetes o las etiquetas no cuadran, o algo queda sin cerrar, no cambia nada y dice en qué línea está el problema.
- Buscar y reemplazar (pendiente).
- Las variables se resaltan: las que se resuelven, sobre un tinte de su color; las que no existen, en otro color y subrayadas, para que el color no sea la única señal; las que se generan al enviar (`{{$...}}`, `{{?...}}`, `process.env`, funciones), en un tercer color.
- Al pasar el cursor sobre una variable: valor, ámbito de origen y enlace para editarla.

La edición con resaltado se limita a unos 200 KB: con más texto, egui tarda demasiado en cada tecla ([spike 2](spikes/02-interfaz-egui-vs-iced.md)). Por encima de ese tamaño, el body se edita como texto plano o se abre en el visor de solo lectura.

## Panel de respuesta

| Pestaña | Contenido |
|---|---|
| Body | Vista formateada, cruda o previsualización (imagen, HTML). Filtro JSONPath o XPath |
| Headers | Tabla de headers recibidos |
| Cookies | Cookies que fija la respuesta (`Cookies (2)`), leídas como las lee el tarro: nombre, valor, dominio, ruta, caducidad y atributos. Las respuestas abiertas desde el historial no tienen: los `Set-Cookie` se guardan ocultos |
| Timeline | Eventos del envío con sus tiempos |
| Tests | Aserciones (primero las que fallan, con su motivo) y capturas con su valor; la pestaña dice `Tests 2/3` en verde o en rojo |

- Los cuerpos grandes se muestran por ventanas; el formateo se desactiva por encima de un umbral (2 MB) y se ofrece verlos en crudo o guardarlos. La vista "Pretty" formatea y colorea JSON y XML; si no se pueden formatear (HTML con `<br>`, JSON roto), se muestran tal cual, coloreados y con un aviso. Los bodies binarios no se dibujan: se indica su tamaño.
- El Timeline lista, con su tiempo desde el inicio, cada request que sale y, debajo, su conexión: `DNS api.test → 10.0.0.1 (1.5 ms)`, `Connected to 10.0.0.1:443 (0.8 ms)` y `TLS handshake, TLSv1.3 (4.1 ms)`, o `Reused connection to 10.0.0.1:443`; luego cada redirección seguida, la llegada de los headers, el final del body y, si llegó comprimido, `Decompressed gzip: 3.1 KB`. Sobre las pestañas, el tamaño de un cuerpo comprimido dice los dos: `12.0 KB (3.1 KB gzip)`.
- **Filtro del body:** sobre un body JSON o XML que se muestra formateado (hasta el umbral de 2 MB), un campo filtra con JSONPath (`$.items[*].id`, `$..name`, `$.items[?@.active]`) o XPath (`//item/@id`, `count(//item)`). En JSON muestra el valor seleccionado, o un array con varios, formateado y coloreado; en XML, el texto de cada nodo, uno por línea. Encima dice cuántos seleccionó, y una expresión inválida muestra dónde falla. Con el campo vacío se ve el body entero. El filtro se mantiene al cambiar de respuesta, para comparar el mismo campo entre envíos.
- La pestaña Tests enmascara el valor de una captura secreta. Una respuesta abierta desde el historial no tiene resultados: no se guardan en él.
- **Capturas para el entorno:** las de ámbito `environment` se usan enseguida como variables de runtime, y un aviso sobre el editor dice cuáles se capturaron para el entorno activo, con "Save to environment" y "Discard". Al guardar, cada valor se escribe en el archivo del entorno, conservando su tipo; si la variable es secreta, va a la bóveda (local), nunca al archivo. Las capturas de envíos seguidos se acumulan en el mismo aviso. Sin entorno activo no hay aviso.
- Acciones: copiar, guardar a archivo, guardar como ejemplo, "guardar como variable" desde un valor seleccionado.
- **Guardar como ejemplo** ("Save as example", junto a "Copy"): añade la respuesta a los ejemplos de la request como un cambio más, con un nombre según su estado (`200 OK`, `200 OK (2)`…). La request queda con cambios sin guardar y el ejemplo llega al archivo con `Ctrl+S`, como cualquier edición; así nunca pisa un borrador. Se guarda la request tal como está escrita y la respuesta sin sus `Set-Cookie` (son sesiones, y el archivo va a git). Un body binario o de más de 1 MB no se guarda, y el aviso dice por qué.
- Selector de respuestas anteriores de la misma request.

## Entornos y variables

- El selector de entorno está siempre visible y muestra el color del entorno. Un entorno marcado como "de cuidado" (por ejemplo producción) tiñe la barra superior (pendiente: OpenCollection no tiene esa marca; se decidirá dónde guardarla).
- Los cambios del editor de entornos se acumulan en una copia y se escriben con Save. Así una tabla a medio editar no cambia lo que usa un envío en curso.
- Editor de entornos (`Ctrl+E`): lista de entornos a la izquierda y tabla de variables a la derecha.
- Columnas de la tabla: habilitada, nombre, valor, tipo y clase de secreto (normal, solo local, compartido).
- Marcar una variable como secreta la hace **solo local**. Compartirla con el equipo es una acción aparte (la casilla Shared, con una explicación breve de lo que implica).
- El valor de un secreto se escribe en un campo oculto. Su estado (guardado en esta máquina, compartido, bloqueado porque falta la clave de la colección, o sin valor) se ve en el campo vacío, al pasar el ratón y en su nombre accesible. Reveal lo muestra 15 segundos.
- Los valores guardados que ya no usa ninguna variable secreta se listan bajo la tabla, con un botón para olvidarlos.
- Vista "todas las variables": cada variable efectiva con su valor y el ámbito del que procede.

## Ajustes de la colección

"Collection settings…" en el menú de la raíz de una colección cargada abre sus ajustes: el proxy (el de la aplicación, ninguno o el suyo: protocolo, host, puerto, credenciales y hosts directos) y los certificados de cliente (una fila por certificado: activo, host, certificado y clave PEM; "Add certificate" añade uno). Se guarda en `opencollection.yml` al pulsar "Save", y los envíos siguientes ya van por él. Un aviso recuerda que la contraseña queda en el archivo, como en Bruno. Debajo, las sobrescrituras de DNS: una fila por entrada (activa, host e IP, con "not an IP address" en rojo si no lo es) y "Add override"; se guardan en `.apifast/collection.yml`.

En los ajustes de la aplicación, la sección TLS tiene un archivo de CA adicional (con "Choose…") y "Do not check certificates", que muestra un aviso en rojo mientras está activa. La sección Proxy elige el del sistema, ninguno o uno manual, para las colecciones que heredan el suyo. Su contraseña se guarda aparte con "Save password", sellada; el campo muestra "saved, sealed" cuando hay una.

## Cookies

"Cookies…" en el menú de la raíz de una colección cargada, o la acción "Cookies" de la paleta, abre el tarro de la colección enfocada:

- Las cookies por dominio, con nombre, valor (cortado; entero al pasar el cursor), ruta, caducidad en UTC ("Session" si no tiene, "Expired" en rojo si ya caducó) y atributos (`subdomains`, `Secure`, `HttpOnly`, `SameSite`).
- "Edit" y "Delete" en cada cookie, "Delete domain" en cada dominio, "Add cookie" y "Delete all" (con confirmación).
- El formulario edita nombre, valor, dominio, ruta, caducidad (`2030-01-31 12:00:00`, vacía para una de sesión), `SameSite` y las casillas "Only this host", "Secure" y "HttpOnly". Dice qué campo está mal y no guarda hasta que todo es válido. Cambiar el nombre, el dominio o la ruta sustituye a la cookie editada.
- Cada cambio se guarda enseguida, sellado.

Los ajustes de la aplicación tienen una sección Cookies con "Send cookies" y "Store cookies".

## Runner

"Run…" en el menú de la raíz de una colección o de una carpeta abre la ventana del runner sobre ella.

- **Formulario:** subcarpetas, tags que incluir y que excluir (separados por comas), pausa entre requests, iteraciones, archivo de datos (CSV o JSON, con el selector del sistema; con datos, las iteraciones se desactivan) y detener al primer fallo. "Run" lo lanza con el entorno activo de la colección y sus variables de runtime.
- **Durante la ejecución:** barra de progreso (`3 / 10`), la request que se está enviando con su iteración, y "Stop". La ejecución sigue en segundo plano si se cierra la ventana; solo hay una a la vez.
- **Resultados:** una fila por request terminada: PASS, FAIL o ERROR, nombre (con la iteración si hay varias), ruta, estado y duración; debajo, las aserciones que fallan o el error. El nombre es un enlace que abre la request con esa respuesta y sus resultados en la pestaña Tests. Al terminar: los totales, las omitidas con su motivo y "Save report:" en HTML, JUnit o JSON, que pide dónde guardarlo.
- Cada request del runner queda en el historial, y lo capturado sigue disponible para los envíos siguientes. Las capturas de ámbito `environment` del runner no se ofrecen para guardar en el entorno.

## Importar

"Import…", junto a "New collection…" y "Open collection…" en la barra lateral, o la acción "Import…" de la paleta, abre la ventana de importación:

1. Se elige el archivo, con el diálogo del sistema o escribiendo la ruta: una exportación de Postman (colección v2.0 o v2.1), con sus entornos si se quiere, una de Insomnia (v4 o v5), una especificación OpenAPI 3 en JSON o YAML, o una colección de Bruno en `.bru` (su carpeta, con "Folder…"). El formato se reconoce solo.
2. "Review" lee los archivos y muestra el plan: la colección con cuántas carpetas, requests y entornos crea, la ruta de cada una, y lo que no se conservó ("Not kept"), con dónde estaba. Un archivo que no es de Postman dice por qué.
3. La carpeta de la colección nueva se propone a partir de su nombre, dentro del workspace activo; las dos se pueden cambiar.
4. "Import" escribe exactamente el plan revisado y abre la colección en el workspace. Si los archivos cambiaron desde la revisión, hay que revisarlos otra vez. Nunca escribe sobre una carpeta que ya tiene colección: el plan sigue ahí para elegir otra.

La lectura y el plan los hace el núcleo, fuera del hilo de la interfaz; la colección leída se guarda hasta que se importa o se cierra la ventana.

### Importar otra vez

"Import again…", en el menú de una colección importada del árbol, trae una versión nueva de su fuente sin pisar lo editado ([ADR 0010](adr/0010-reimportacion.md)):

1. Propone el archivo de la última importación (o la carpeta, para Bruno); se puede elegir otro.
2. "Review" muestra cuántas requests son nuevas, cambiaron o siguen igual, y una casilla por cada una que no sigue igual: nueva, cambiada en la fuente, cambiada en la fuente y aquí, borrada aquí, o ya no está en la fuente. Vienen marcadas las nuevas y las cambiadas solo en la fuente.
3. "Apply" aplica lo marcado y recarga la colección. Lo no marcado se vuelve a ofrecer la próxima vez.

En una colección que no salió de una importación, la ventana lo dice y no ofrece nada.

## Bóveda

- La clave de la colección no se pide nunca a quien la creó (se genera con el primer secreto compartido) ni a una máquina que ya la tiene: queda guardada, envuelta con la clave maestra.
- Al abrir una request de una colección con secretos compartidos y sin clave en esta máquina, un aviso no bloqueante sobre la request ofrece pegar la clave; su raíz en el árbol lleva una marca.
- La ventana "Collection key" (menú de la raíz o paleta, para la colección activa) muestra la clave para pasarla al equipo cuando no se recuerda, con botón de copiar, y la rota tras una confirmación que explica que el equipo necesitará la nueva y que los valores antiguos siguen en el historial de git.
- Diálogo "Compartir clave": muestra la clave legible, con botón de copiar y advertencia sobre cómo compartirla.
- Los campos secretos tienen un botón de revelar que se vuelve a ocultar solo.
- Indicador del estado de la bóveda en la barra superior (todavía no hay barra de estado): aparece cuando la clave maestra está en el archivo de respaldo en lugar del keyring, con la explicación al pasar el ratón.

## Paleta de comandos

`Ctrl+K` abre un buscador único para:

- saltar a cualquier request por nombre o URL,
- ejecutar acciones (nueva request, cambiar entorno, importar, abrir ajustes),
- cambiar de colección.

Búsqueda difusa, resultados recientes primero y el atajo de cada acción a la derecha.

## Atajos por defecto

| Acción | Atajo |
|---|---|
| Enviar request | `Ctrl+Enter` |
| Cancelar envío | `Esc` |
| Guardar / guardar todo | `Ctrl+S` / `Ctrl+Shift+S` |
| Paleta de comandos | `Ctrl+K` |
| Cambio rápido de request | `Ctrl+P` |
| Nueva request | `Ctrl+N` |
| Cerrar pestaña / reabrir | `Ctrl+W` / `Ctrl+Shift+T` |
| Pestaña siguiente / anterior | `Ctrl+Tab` / `Ctrl+Shift+Tab` |
| Ir a la pestaña N | `Ctrl+1` … `Ctrl+9` |
| Foco en la URL | `Ctrl+L` |
| Editor de entornos | `Ctrl+E` |
| Filtrar barra lateral | `Ctrl+F` |
| Mostrar u ocultar barra lateral | `Ctrl+B` |
| Cambiar disposición de paneles | `Ctrl+J` |
| Duplicar / renombrar | `Ctrl+D` / `F2` |
| Formatear body | `Alt+Shift+F` |
| Zoom | `Ctrl+=` / `Ctrl+-` / `Ctrl+0` |
| Mostrar historial | (sin atajo) |
| Editor de atajos | (sin atajo; desde la paleta) |

En macOS, `Ctrl` se sustituye por `Cmd`. Todos los atajos se pueden reasignar desde el editor de atajos; se guardan en `keymap.yml`, solo los que cambian:

```yaml
send: Ctrl+R
palette: [Ctrl+K, Ctrl+Shift+P]
reopen_tab: []        # sin atajo
```

Mientras la paleta está abierta o el editor espera una combinación, los demás atajos no actúan. `Esc` solo cancela un envío cuando hay uno en curso; si no, queda libre para cerrar diálogos.

## Estados y mensajes

| Situación | Comportamiento |
|---|---|
| Envío en curso | El botón pasa a "Cancelar"; el timeline se llena en vivo |
| Error de red | Mensaje claro en el panel de respuesta, con el detalle en el timeline |
| Variable sin resolver | Aviso antes de enviar, con la lista |
| Archivo cambiado desde fuera | Si no hay cambios locales, se recarga; si los hay, se ofrece comparar |
| Archivo borrado desde fuera | La pestaña queda como "sin archivo" y se puede volver a guardar |
| Colección vacía | Pantalla inicial con "nueva request", "importar" y "abrir carpeta" |

## Temas y accesibilidad

- Tema claro y oscuro, siguiendo el del sistema por defecto; se puede fijar uno desde la paleta.
- Las ventanas usan el color de fondo y los campos el de superficie, con borde, para que un campo se distinga siempre de lo que lo rodea.
- Las tablas con campos editables se componen fila a fila con anchos repartidos, no con `egui::Grid`: en una celda de grid un campo solo recibe el ancho que su columna midió el frame anterior y no crece.
- Escala de interfaz configurable en los ajustes (`Ctrl+,`), que también siguen `Ctrl+=`, `Ctrl+-` y `Ctrl+0`; la escala aumenta el texto con todo lo demás.
- Los colores de método y de estado nunca son el único indicador: siempre van con texto.
- Contraste WCAG AA (4,5:1) en ambos temas, comprobado por un test sobre los tokens de `theme/`: cada color de texto sobre cada fondo, y el texto de los badges sobre su propio tinte.

## Organización del código de la interfaz

- La interfaz no guarda el modelo. Guarda identificadores, borradores de edición y vistas.
- Cada panel es un módulo con su estado propio y una función de dibujo.
- Las acciones del usuario se convierten en comandos que se envían al núcleo; la interfaz nunca hace entrada/salida.
- El núcleo publica instantáneas inmutables del estado y pide el repintado.

### Componentes: atomic design

`apifast-gui` organiza sus componentes con atomic design, adaptado al modo inmediato de egui. En egui no hay un árbol de componentes que persista entre frames: un componente es una función o un struct que dibuja y devuelve una `egui::Response`, y el estado vive en el modelo de la aplicación, no dentro del componente.

| Nivel | Carpeta | Forma en egui | Ejemplos |
|---|---|---|---|
| Tokens | `theme/` | Constantes y funciones que construyen `egui::Style` y `Visuals` | Colores por método HTTP, por estado y por entorno; espaciados; radios; tipografías |
| Átomos | `atoms/` | Structs que implementan `egui::Widget`: `ui.add(MethodBadge::new(&method))` | Badge de método, chip de estado, botón con icono, campo con resaltado de `{{variables}}` |
| Moléculas | `molecules/` | Funciones o structs que combinan átomos y reciben `&mut` solo de su parte del estado | Fila clave/valor con checkbox, método + URL, selector de entorno |
| Organismos | `organisms/` | Componentes con lógica propia que **devuelven eventos** en lugar de modificar el modelo | Tabla clave/valor, árbol de la colección, pestañas de request, visor de respuesta |
| Plantillas | `layout/` | La disposición de paneles (`SidePanel`, `TopBottomPanel`, `CentralPanel`) | Barra lateral, editor y respuesta |
| Pantallas | `screens/` | La plantilla con datos reales y la traducción de eventos a comandos del núcleo | Ventana principal, pantalla inicial |

Reglas:

- **Los colores y medidas salen solo de `theme/`.** Ningún componente escribe un color o un espaciado a mano, así los temas claro y oscuro son un cambio de valores.
- **Los organismos devuelven eventos** (`HeaderEdited`, `RowRemoved`, `RequestSelected`...). La pantalla los traduce a comandos del núcleo. Ningún componente hace entrada/salida ni conoce los puertos.
- **Los textos visibles llegan ya traducidos** como parámetros; los componentes no eligen idioma.
- **Coste por frame:** todo se ejecuta en cada frame, así que los átomos no asignan memoria ni formatean texto si no cambió. Los organismos de listas largas (árbol, visor de respuesta) dibujan solo lo visible.
- **Sin abstraer de más:** un componente se crea cuando se repite o tiene reglas propias. Un envoltorio de `ui.label()` sin nada más no es un átomo.

Herramientas:

- **Galería de componentes:** un binario de desarrollo (`cargo run -p apifast-gui --example gallery`) muestra cada átomo, molécula y organismo en todos sus estados y en ambos temas, como Storybook. Cada componente nuevo se añade a la galería.
- **Pruebas por componente** con `egui_kittest` (de la misma versión que egui): interacción simulada (pulsar, escribir, navegar con teclado) y consultas al árbol de accesibilidad, sin levantar la aplicación entera ni necesitar GPU.

Trampa conocida: `ui.set_visuals(..)` compila, pero a través de `Deref<Target = Context>` cambia el tema de toda la aplicación desde el frame siguiente. Para una zona se usa `*ui.visuals_mut() = ..`. `clippy.toml` prohíbe `Context::set_visuals`.

Más detalles de rendimiento en [rendimiento y memoria](10-rendimiento-y-memoria.md).
