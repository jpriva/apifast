# 12. Roadmap

El desarrollo se divide en once fases. Cada una entrega algo utilizable y tiene un criterio de salida comprobable. No se fijan fechas: el tamaño relativo (S, M, L, XL) orienta el esfuerzo.

## Resumen

| Fase | Nombre | Tamaño | Depende de | Entrega |
|---|---|---|---|---|
| F0 | Fundaciones | S | — | Documentación, ADRs y spikes |
| F1 | Núcleo sin interfaz | L | F0 | `apifast send` desde un archivo |
| F2 | GUI mínima | XL | F1 | Aplicación usable a diario |
| F3 | Bóveda | M | F1 | Secretos cifrados |
| F4 | Tests y runner | L | F1 | Colecciones ejecutables en CI |
| F5 | Red avanzada | L | F1 | Autenticación completa, proxy, certificados |
| F6 | Importación y exportación | M | F1 | Migración desde otras herramientas |
| F7 | GraphQL, SSE y WebSocket | L | F2 | Tres protocolos más |
| F8 | gRPC | L | F2 | gRPC completo |
| F9 | Scripting JavaScript | L | F4 | Scripts compatibles con Bruno |
| F10 | Versión 1.0 | M | Todas | Release estable |

```mermaid
flowchart LR
    F0 --> F1
    F1 --> F2
    F1 --> F3
    F1 --> F4
    F1 --> F5
    F1 --> F6
    F2 --> F7
    F2 --> F8
    F4 --> F9
    F2 --> F10
    F3 --> F10
    F5 --> F10
    F6 --> F10
    F7 --> F10
    F8 --> F10
    F9 --> F10
```

Tras F1, las fases F2 a F6 son en gran parte independientes entre sí. El orden recomendado para una sola persona es el numérico. **Hito "MVP":** F1 + F2 + F3.

---

## F0 — Fundaciones

**Objetivo:** cerrar el diseño y eliminar las incógnitas técnicas antes de escribir código de producto.

### Tareas

- [x] Análisis de Bruno y Yaak.
- [x] Documentación de diseño y ADRs.
- [x] **Spike 1: librería YAML.** Elegida `oneyaml`, guardando con JSON Patch sobre el documento original. Resultados: [spikes/01-libreria-yaml.md](spikes/01-libreria-yaml.md).
- [x] **Spikes 2 y 3: interfaz y renderer.** Elegido egui con wgpu; presupuesto de memoria medido en PSS; edición con resaltado limitada a unos 200 KB. Resultados: [spikes/02-interfaz-egui-vs-iced.md](spikes/02-interfaz-egui-vs-iced.md).
- [x] **Spike 4: compatibilidad con Bruno.** Resuelto leyendo su código: Bruno descarta las claves desconocidas al guardar. Los datos de apifast van en `.apifast/` ([ADR 0002](adr/0002-formato-yaml-opencollection.md)).
- [x] Comprobar en Bruno real que respeta `extensions.bruno.ignore` para `.apifast`. Confirmado en Bruno 4.3.0: [spikes/03-bruno-ignora-apifast.md](spikes/03-bruno-ignora-apifast.md).
- [ ] Medir Bruno y Yaak en la misma máquina (RAM en reposo, arranque) como línea base.
- [x] Crear el workspace de Cargo vacío con CI: formato, lints, pruebas, `cargo deny`; compilación en Linux, macOS y Windows. Esqueletos de los crates de F1, test de reglas de arquitectura (`apifast-arch-tests`) y Rust fijado en `rust-toolchain.toml`.
- [x] Repositorio git inicializado y archivos de licencia (MIT y Apache-2.0).

### Criterio de salida

Cada spike tiene un resultado escrito con cifras, la librería YAML está elegida y el ADR 0004 está confirmado o sustituido.

---

## F1 — Núcleo sin interfaz

**Objetivo:** un esqueleto funcional de extremo a extremo, sin GUI.

### Tareas

- [x] `apifast-domain`: entidades, `Auth`, `Body`, `Setting<T>`, resolución de herencia con procedencia. Solo lo de F1: request HTTP y auth Basic, Bearer y API key; el resto llega en su fase.
- [x] `apifast-template`: parser, árbol reversible, render recursivo, variables dinámicas básicas (`$randomUUID`, `$timestamp`, `$isoTimestamp`, `$randomInt`).
- [x] `apifast-app`: puertos, `OpenCollection`, `LoadRequest`, `SaveRequest`, `ResolveVariables`, `SendRequest`. `RenderedRequest` es el único tipo que acepta el transporte. El watcher de `OpenCollection` llega en F2 y el timeline por canal en F5.
- [x] `apifast-storage-fs`: lectura con `oneyaml` y guardado con JSON Patch por elemento sobre el documento releído del disco; lectura y escritura de `opencollection.yml`, `folder.yml`, requests y entornos; carpeta `.apifast/` con el identificador de colección; escritura atómica; conservación de claves desconocidas. Cada guardado se verifica releyendo el resultado antes de escribir.
- [x] `apifast-http`: envío con reqwest y rustls, redirecciones manuales, timeout, cancelación, cuerpo en streaming a disco. Errores clasificados (DNS, conexión, TLS, timeout, protocolo) sin la URL en el mensaje.
- [x] Autenticación básica: Basic, Bearer, API key, heredada. Se aplica al renderizar, en `apifast-app`; un esquema sin implementar da error en vez de enviar sin auth.
- [x] `apifast-history`: almacén de respuestas en archivos (`FsResponseStore`): escritura por partes con búfer fijo, lectura por ventanas, archivos parciales invisibles hasta terminar y borrados si el envío falla, y limpieza por número, tamaño y antigüedad. El historial de ejecuciones en JSON Lines queda en F2, con su panel.
- [x] `apifast-cli`: `apifast send <archivo> --env <nombre>`, con `--var`, `--allow-unresolved`, `-i`, `--fail` y cancelación con Ctrl+C. El body va a stdout y el resumen a stderr. No escribe nada en la colección.
- [x] Implementaciones en memoria de todos los puertos (`apifast_app::memory`).
- [x] Corpus de round-trip con colecciones YAML de Bruno: 436 archivos copiados al repositorio (los fixtures de Bruno y una colección guardada por la aplicación Bruno 4.3.0), que se guardan sin cambiar un byte, y 1028 ediciones verificadas. Incluye archivos con `config.proxy`, `bundled` y `settings` por defecto.
- [x] Test de reglas de arquitectura (adelantado en F0, `apifast-arch-tests`).

### Criterio de salida

- `apifast send` envía una request leída de un archivo, con variables de entorno, y muestra la respuesta.
- Todos los archivos del corpus se leen y se reescriben sin diferencias.
- El dominio no depende de ningún crate de entrada/salida.

---

## F2 — GUI mínima

**Objetivo:** una aplicación de escritorio con la que trabajar a diario.

**Estado:** completada el 2026-11-16.

### Tareas

- [x] `apifast-gui`: ventana, bucle reactivo, canal de comandos y de instantáneas con el núcleo. La ventana abre una carpeta desde la pantalla de bienvenida o con `apifast-gui <carpeta>`; el botón "Browse..." abre el selector de carpetas del sistema (`rfd`, por el portal de escritorio en Linux).
- [x] Estructura de componentes con atomic design ([UI y UX](09-ui-ux.md#componentes-atomic-design)): tokens en `theme/`, átomos, moléculas, organismos que devuelven eventos, plantillas y pantallas.
- [x] Galería de componentes (`--example gallery`) y pruebas por componente con `egui_kittest`.
- [x] Barra lateral: árbol virtualizado, crear, renombrar, mover, borrar, reordenar. Menú contextual, arrastrar y soltar, y "subir" y "bajar" para hacerlo sin ratón. Crear y renombrar ponen al archivo el *slug* del nombre; mover y reordenar reescriben solo los `seq` que cambian.
- [x] Pestañas con borradores y restauración al abrir. Método y URL ya se editan; `Ctrl+S`, `Ctrl+Shift+S` y `Ctrl+W`; cerrar una pestaña con cambios pide confirmación. Enviar usa lo que hay en pantalla, guardado o no.
- [x] Barra de URL con resaltado de variables. Las definidas van sobre un tinte de su color, las que faltan subrayadas y las que se generan al enviar en otro color. Al pasar el cursor se ven el valor (los secretos, ocultos) y el ámbito de origen. El átomo `TemplateField` sirve también para los demás campos con plantillas.
- [x] Panel de request: Params, Headers, Body (JSON, texto, formulario), Auth, Ajustes. La query de la URL y la tabla de params se mantienen sincronizadas, y los params de ruta salen de los segmentos `:nombre`. Los headers heredados aparecen atenuados con su origen y tachados si la request los sustituye; la auth y cada ajuste muestran de quién heredan. Los bodies multipart y de archivo se muestran, pero todavía no se editan.
- [x] Tablas clave/valor con edición masiva: casilla, nombre, valor con resaltado, quitar y fila vacía al final. Un interruptor las pasa a texto (`nombre: valor` por línea, `//` para las desactivadas) y las filas siguen al texto mientras se escribe. Se sugieren nombres de header conocidos y, tras `{{`, las variables definidas: flechas para elegir, Enter o clic para aceptar, Esc para cerrar.
- [x] Editor de código con resaltado y formateo, con límite de unos 200 KB para la edición con resaltado. Colorea JSON, XML y SPARQL, y encima las variables como en la barra de URL, con el mismo hover. El botón Format o `Alt+Shift+F` formatean JSON y XML con dos espacios aunque haya `{{variables}}` donde va un valor; si la estructura está rota, dice en qué línea. El resaltado se guarda hasta que cambia el texto, y por encima de 200 KB el body se edita sin resaltar. Buscar y reemplazar queda pendiente.
- [x] Visor de respuestas por ventanas; si el texto de egui no alcanza el presupuesto, dibujo wgpu propio con `PaintCallback`. No hizo falta: con 100 MB, desplazarse cuesta unos 0,2 ms de CPU por frame con el texto de egui. El visor lee el body del `ResponseStore` sin guardarlo en memoria. Lo indexa 8 MB por frame, mostrando ya lo que tiene, y parte las líneas de más de 2 KB en filas. Guarda un punto de control cada 64 filas (unos 3 MB para 100 MB de líneas cortas) y en cada frame lee solo las filas visibles. La instantánea ya no copia los primeros 256 KB de cada respuesta.
- [x] Panel de respuesta: Body por ventanas, Headers, Timeline básico. Detecta el lenguaje por `Content-Type` o por cómo empieza el body, y muestra JSON y XML de hasta 2 MB formateados y coloreados ("Pretty"). Cualquier body se ve tal cual ("Raw") por ventanas. Los binarios se anuncian en vez de dibujarse, y un botón copia el body entero (hasta 16 MB). El transporte anota en `ResponseHead::timeline`, en microsegundos, cada request que sale, cada redirección seguida, la llegada de los headers y el final del body.
- [x] Historial de ejecuciones en JSON Lines (`HistoryStore` en `apifast-history`, ADR 0008) y su panel; limpieza de respuestas al arrancar con `FsResponseStore::prune`.
  - Cada envío terminado deja una línea: la request tal como está escrita, y la cabecera, la Timeline y el id del body, o el error. Los cancelados no dejan línea.
  - Al abrir la colección, una tarea aparte recorta el historial (1000 entradas, 30 días) y las respuestas (1000, 512 MB, 30 días). Después restaura la última respuesta de cada pestaña abierta.
  - La barra lateral tiene una pestaña History, filtrable, con las entradas más nuevas arriba; elegir una muestra esa respuesta en la pestaña de su request.
  - Las líneas ilegibles se saltan y desaparecen al recortar.
- [x] Selector y editor de entornos; vista de variables con procedencia.
  - El selector muestra el color del entorno activo.
  - El editor se abre con `Ctrl+E` o con el botón Environments. Tiene la lista de entornos a la izquierda (crear) y, a la derecha, el entorno elegido: nombre (Enter para renombrar), color, `extends` y la tabla de variables (casilla, nombre, valor con resaltado, tipo, secreto, quitar).
  - Los cambios van a una copia que se guarda con Save o se descarta con Revert. Borrar pide confirmación.
  - Renombrar sigue a los entornos que lo extienden, y su archivo pasa a llamarse como el entorno, como en Bruno. No se puede borrar un entorno que otro extiende, ni guardar un `extends` que no existe o forma un ciclo.
  - La vista All variables muestra cada variable que ve la request activa, con su valor, de dónde viene y qué ámbitos sustituye.
  - Marcar una variable como secreta espera a la bóveda (F3): sin ella, solo borraría el valor del archivo.
- [x] `apifast-watch`: recarga ante cambios externos y resolución de conflictos con borradores.
  - `FsWatcher` (con `notify`) agrupa las ráfagas: espera 200 ms de calma, con un máximo de 2 s, y entrega rutas relativas. Ignora `.git`, `.apifast`, `node_modules` y los temporales de editores y de las escrituras atómicas propias.
  - El núcleo relee el árbol y, si cambió `opencollection.yml`, un `folder.yml`, `.env` o `environments/`, también los entornos y lo heredado.
  - Para cada pestaña abierta cuyo archivo cambió:
    - si el archivo dice lo mismo que ya tiene, era una escritura propia y no se hace nada;
    - si no tiene borrador, toma lo nuevo;
    - si tiene borrador, mantiene el borrador en pantalla y ofrece "Keep my changes" o "Load from disk";
    - si el archivo se borró, se cierra si estaba limpia, y con borrador se queda (guardar lo vuelve a escribir).
- [x] Paleta de comandos y atajos reasignables.
  - Todas las acciones están en un registro único (`actions.rs`), con su nombre estable para `keymap.yml` y sus atajos por defecto. Funcionan los de la tabla de `docs/09-ui-ux.md` salvo Duplicar, Renombrar, Filtrar barra lateral y Cambiar disposición, que esperan a sus funciones; el zoom lo da egui.
  - `Ctrl+K` abre la paleta: acciones con su atajo, requests y entornos, con búsqueda difusa y lo último elegido primero. `Ctrl+P` busca solo requests.
  - El editor de atajos (desde la paleta, "Keyboard shortcuts") cambia uno pulsando la combinación nueva, avisa si se la quita a otra acción y permite restablecer el predeterminado.
  - `~/.config/apifast/keymap.yml` guarda solo lo que difiere de lo predeterminado; las entradas que no se entienden se ignoran y se avisa en pantalla.
- [x] Temas claro y oscuro.
  - Por defecto se sigue el tema del sistema. Desde la paleta se elige "Theme: follow the system", "light" o "dark"; las tres acciones admiten atajo. La elección se guarda con los ajustes.
  - Repaso visual con capturas renderizadas en los dos temas:
    - Las tablas clave/valor y la de variables ya no usan `egui::Grid`: un campo en una celda solo recibe el ancho que su columna midió antes, así que nunca crecía (nombres cortados y valores partidos en varias líneas). Ahora las filas se reparten el ancho disponible.
    - Las ventanas usan el color de fondo y los campos el de superficie, con borde: en el tema claro los campos dentro de una ventana no se veían.
    - Los botones de cerrar y quitar son el átomo `CloseButton`: una × dibujada, sin fondo hasta pasar el ratón.
- [x] Ajustes de la aplicación.
  - Ventana Settings (`Ctrl+,` o desde la paleta):
    - tema y escala de la interfaz (también `Ctrl+=`, `Ctrl+-` y `Ctrl+0`, que se guardan igual);
    - tamaño máximo de un body en la vista Pretty;
    - entradas del historial, días que se conservan y espacio para cuerpos de respuesta;
    - un botón para restablecer los valores por defecto.
  - Se guardan en `~/.config/apifast/settings.yml` al cambiar, en un hilo aparte, y al soltar el ratón si se arrastra un control.
  - Los límites de almacenamiento llegan al núcleo con `Command::Configure` antes de abrir la colección y se aplican al abrirla.
  - Los valores fuera de rango se ajustan al rango; los que no se entienden se ignoran y se avisa en pantalla.

### Criterio de salida

- El flujo crear → editar → guardar → enviar → inspeccionar funciona sin tocar archivos a mano.
- Se cumplen los presupuestos de RAM en reposo y de arranque.
- Un cambio hecho con otro editor o con git se refleja sin reiniciar.

**Resultado:** los tres se cumplen.

- El flujo completo lo recorren las pruebas de `apifast-gui` (`tests/flow.rs` contra un servidor real, y `tests/core.rs`): crear, editar, guardar, enviar e inspeccionar, sin tocar archivos.
- Presupuestos medidos con `tools/bench/`: 50 MB PSS en reposo con 100 requests, primera ventana en 43–64 ms. Detalle en [rendimiento](10-rendimiento-y-memoria.md#mediciones-de-f2).
- `apifast-watch` refleja los cambios externos; con un borrador, la interfaz pregunta qué conservar.

**Queda fuera de F2**, a retomar en la fase que lo necesite o en un repaso de la interfaz:

| Pendiente | Motivo |
|---|---|
| Buscar y reemplazar en el editor | No bloquea el uso diario |
| Duplicar, renombrar con `F2`, filtro `Ctrl+F` y disposición lado a lado (`Ctrl+J`) | Funciones de la barra lateral y del diseño aún sin hacer; sus atajos se añaden con ellas. La disposición lado a lado pasa a las disposiciones editables de F10 |
| Autocompletado de variables en el editor de varias líneas | Enter y las flechas chocan con la edición |
| Entorno "de cuidado" que tiñe la barra | OpenCollection no tiene esa marca; falta decidir dónde guardarla |
| Marcar variables como secretas | Llega con la bóveda (F3) |
| Selector de respuestas anteriores en el panel de respuesta | El historial ya permite abrirlas; retomado en F10 como historial al estilo de IntelliJ |
| Bloqueo de archivo del historial entre dos instancias | Cada línea se escribe de una vez; basta en la práctica |
| Contraste 3:1 de los bordes de los campos | Los campos se distinguen por el fondo; se revisará en el repaso de accesibilidad |

---

## F3 — Bóveda

**Objetivo:** ningún secreto en claro.

**Estado:** completada el 2026-11-16.

### Tareas

- [x] `apifast-vault`: sobre XChaCha20-Poly1305 con datos asociados y vector fijo de prueba. El cifrado se comprueba con el vector publicado (apéndice A.3.1 de `draft-irtf-cfrg-xchacha-03`) y el formato `enc:v1:` con un vector propio. Un valor abierto con otra clave, otra variable, otro entorno u otra colección, o con un byte cambiado, se rechaza. Las claves se borran de memoria al liberarse y no se imprimen.
- [x] `KeyStore` sobre keyring, con respaldo en archivo `0600` e indicador en la interfaz. La clave maestra se busca en el keyring (crate `keyring`, Secret Service en Linux) y luego en `master.key`; una nueva va al keyring, o al archivo si el sistema no tiene keyring. Un keyring que existe pero falla o está bloqueado es un error, nunca un paso al archivo. `master_key` devuelve dónde está la clave para el indicador, que se dibujará con la interfaz de la bóveda.
- [x] Clave de colección: generación, envoltura, *challenge*. `CollectionKeyStore` guarda cada clave en `keys/<collection-id>.key`, cifrada con la maestra y atada a su colección por los datos asociados, con permisos `0600`. El *challenge* son 32 bytes aleatorios cifrados con la clave de la colección, en `vault.challenge` de `.apifast/collection.yml`; abrirlo dice si una clave es la correcta. Generar la clave al guardar el primer secreto compartido llega con la tarea de secretos.
- [x] Secretos solo locales (por defecto) y secretos compartidos en `.apifast/secrets/`; detección de entradas huérfanas.
  - [x] Solo locales: `vault/<collection-id>.vault` (JSON, `0600`), cada valor en su sobre con la clave maestra y atado a colección, entorno y variable. El envío pide los valores por el puerto `SecretStore`, solo para las variables secretas sin valor; la GUI y la CLI usan `VaultSecrets`, que lee la clave maestra la primera vez que hace falta y nunca la crea al leer.
  - [x] Compartidos en `.apifast/secrets/<entorno>.yml`, con la clave de la colección generada al compartir el primero.
    - El envío busca primero el secreto local y luego el compartido. `apifast-vault` lee los archivos de la colección por el puerto `SharedSecretFiles`, que implementa `FsCollection`.
    - Compartir crea la clave de la colección y el *challenge* la primera vez. Si la colección ya tiene *challenge* y esta máquina no tiene su clave, compartir falla: la clave se pide al equipo, nunca se crea otra.
    - `use_collection_key` acepta la clave del equipo solo si abre el *challenge*.
    - Pasar un secreto de local a compartido o al revés lo quita del otro sitio.
    - Un secreto compartido sin la clave en esta máquina hace fallar el envío con ese motivo, en vez de enviarse sin él.
  - [x] Entradas huérfanas: secretos guardados cuya variable ya no existe o ya no es secreta. `VaultSecrets::orphans` las lista, locales y compartidas, y `forget` las borra; mostrarlas y limpiarlas desde el editor de entornos llega con la interfaz de la bóveda. Al renombrar un entorno, sus secretos se cifran de nuevo con el nombre nuevo (los datos asociados lo incluyen), cada uno en su sitio. Renombrar una variable secreta deja su valor huérfano hasta volver a escribirlo: desde un archivo no se distingue un renombrado de un borrado más un alta.
- [x] Clave compartible: exportar, importar y validar. `AFK-` y 54 caracteres Crockford en 9 grupos de 6: 52 de la clave y 2 de control (10 bits de un CRC-16) que detectan una errata antes de probar la clave. La lectura ignora mayúsculas, guiones y espacios, y toma `I`/`L` por `1` y `O` por `0`. `export_collection_key` la muestra; `import_collection_key` la lee, la valida contra el *challenge* y la guarda.
- [x] Rotación de clave y conversión entre modelos. `rotate_collection_key` abre todos los secretos compartidos antes de escribir nada, guarda la clave nueva y conserva la anterior como `keys/<id>.previous.key` hasta terminar de cifrar de nuevo los secretos y el *challenge*. Si se corta a mitad, los valores se leen con la nueva o con la anterior y basta con repetirla. Devuelve la clave nueva en formato compartible para el equipo. La conversión entre modelos ya la hacen `set_local` y `share`.
- [x] Tipo `Secret` y enmascarado en timeline, historial, logs y CLI.
  - `Secret` (en el dominio) borra su valor de memoria al liberarse y al imprimirse solo muestra `••••••`; la bóveda devuelve los valores así.
  - El render recoge el valor de cada secreto que usa (el valor ya renderizado, si se compone de otras variables). La request renderizada y el resultado del envío lo llevan consigo.
  - `mask` sustituye cada valor, y su forma codificada en URL, por `••••••`.
  - El historial y su Timeline enmascaran las URLs alcanzadas (la final, cada request y cada redirección) y, además, ocultan los valores de su query. La request renderizada se imprime enmascarada, así que no llega a un log.
  - La CLI no imprime nada de la request, solo la respuesta y un resumen sin URL.
  - Queda fuera: el valor existe en memoria como texto normal mientras se renderiza y se envía, y un cuerpo de respuesta que repita un secreto se guarda tal cual (ver `docs/06`).
- [x] Interfaz: columna de clase de secreto, revelar, diálogos de clave, estado de la bóveda.
  - [x] Núcleo: la instantánea lleva la vista de la bóveda (dónde está el valor de cada variable secreta, entradas huérfanas, si la colección comparte secretos y tiene clave, si la maestra está en archivo), calculada solo con nombres: no abre ningún valor ni pregunta al keyring. Comandos para guardar un valor (local o compartido), revelarlo, olvidar una entrada, y mostrar, pegar y rotar la clave; lo revelado y la clave mostrada se ocultan solos a los 15 s. Una colección sin identificador lo recibe al guardar su primer secreto, y su sesión, historial y respuestas pasan del hash de la ruta al identificador. `Core::start_with(.., SystemKeyring::off(), ..)` usa solo el archivo de la clave maestra: así las pruebas nunca tocan el keyring del usuario.
  - [x] Editor de entornos: marcar secreta (local o compartida), escribir el valor oculto, revelar, estado y huérfanas.
    - La casilla Secret ya funciona. Si la variable tenía valor, ese valor se guarda en la bóveda al pulsar Save y el archivo queda solo con el nombre.
    - Cada secreto tiene un campo oculto para escribir un valor nuevo, la casilla Shared (marcada si el valor ya es compartido) y Reveal, activo si hay valor guardado.
    - El estado (guardado aquí, compartido, bloqueado sin la clave o sin valor) aparece en el campo vacío, al pasar el ratón y en su nombre accesible.
    - Save guarda el entorno y los valores escritos, cada uno en su sitio; Revert descarta ambos.
    - Debajo, las entradas huérfanas con un botón Forget.
  - [x] Diálogos de la clave de la colección e indicador de la bóveda.
    - Una colección que comparte secretos sin la clave en esta máquina muestra un aviso con un campo oculto para pegarla.
    - La ventana "Collection key" (desde la paleta) muestra la clave con un botón para copiarla, y la rota tras confirmar lo que implica. Sin secretos compartidos, explica que la clave se crea al compartir el primero.
    - La barra superior avisa cuando la clave maestra está en el archivo de respaldo y no en el keyring.
- [x] CLI: `apifast vault` y `APIFAST_COLLECTION_KEY`. `vault set` (local o `--shared`), `status`, y `key show`, `import` y `rotate`; valores y claves solo por la entrada estándar. `APIFAST_COLLECTION_KEY` abre los secretos compartidos en una ejecución sin guardar la clave, y `APIFAST_NO_KEYRING=1` deja la clave maestra en su archivo.
- [x] ~~Cifrado del tarro de cookies.~~ Pasa a F5, donde está hecho: apifast todavía no guarda cookies, y el tarro llega allí ("Tarro de cookies con editor"). Se cifrará con la clave maestra al crearlo.

### Criterio de salida

- Buscar un secreto conocido en la colección, el directorio de datos y los logs no devuelve resultados.
- Una clave incorrecta se detecta y un valor manipulado no se descifra.
- Dos usuarios comparten una colección con secretos intercambiando solo la clave.

**Resultado:** los tres se cumplen, cada uno con su prueba.

- `apifast-cli/tests/vault.rs` guarda un secreto local y otro compartido, envía requests que los usan y busca sus bytes en todos los archivos de la colección y del directorio de datos y en la salida: no aparecen. Queda fuera, por diseño, el cuerpo de una respuesta que repita un secreto: se guarda tal cual llega ([bóveda](06-boveda-de-secretos.md)).
- Las pruebas del sobre rechazan una clave incorrecta, un valor con un byte cambiado y un valor movido a otra variable, entorno o colección; la clave compartible detecta una errata antes de probarla.
- `apifast-cli/tests/vault.rs` y `apifast-vault/tests/shared.rs` recorren dos máquinas sobre la misma colección: la segunda abre los secretos compartidos con solo la clave, sea pegada o en `APIFAST_COLLECTION_KEY`.

El cifrado del tarro de cookies pasa a F5, donde llega el tarro.

---

## Workspaces y varias colecciones

**Objetivo:** trabajar con varias colecciones a la vez, agrupadas en workspaces compatibles con Bruno, entrando directamente al visor ([ADR 0009](adr/0009-workspaces-y-varias-colecciones.md)).

**Estado:** completada el 2026-11-16. Se intercaló en F4, después de las funciones `response` y `header` y antes de JSONPath; nace de probar la interfaz: sin colección solo había una bienvenida, no se podía crear una colección y abrir otra cerraba la anterior.

### Tareas

- [x] Diseño documentado: ADR 0009, formato de `workspace.yml` y estado local ([formato](04-formato-de-archivos.md)), interfaz ([UI](09-ui-ux.md)), clave de la colección ([bóveda](06-boveda-de-secretos.md)).
- [x] Arranque directo al visor, con estado vacío; crear y abrir colecciones desde dentro (diálogo "New collection", acciones y paleta). Sin pantalla de bienvenida. La barra lateral vacía ofrece "New collection…" y "Open collection…" (también en la paleta; abrir es `Ctrl+O`). El diálogo pide nombre y carpeta padre, que por defecto es la del workspace por defecto (`default-workspace` en el directorio de datos) o la de la colección abierta, y crea `<padre>/<nombre-en-minúsculas-con-guiones>`. Abrir una carpeta sin `opencollection.yml` muestra el error con el botón "Create one here". Crear nunca escribe sobre una colección existente. Hasta la tarea del núcleo, abrir otra colección sigue cerrando la anterior.
- [x] `workspace.yml` en `apifast-storage-fs`: leer, añadir, quitar y renombrar conservando el resto del archivo; estado personal por workspace y lista de workspaces conocidos. Todo `workspace.yml` del corpus de Bruno se guarda byte a byte. Puerto `WorkspaceRepository` (con `MemoryWorkspace`) y adaptador `FsWorkspace`, que reutiliza el guardado por parches de las colecciones: al añadir una colección se respeta hasta el estilo de comillas del archivo. `entry_for` escribe la ruta relativa si la colección está dentro de la carpeta del workspace, y `resolve` la vuelve a absoluta. `FsWorkspaceState` y `FsKnownWorkspaces` guardan lo personal; sus rutas están en `apifast_history::dirs`.
- [x] Núcleo de la GUI con un workspace activo y varias colecciones: pestañas y comandos por colección, carga perezosa, entorno activo por colección, cerrar y quitar colecciones, crear, abrir y cambiar de workspace, restaurar al reiniciar, `workspace.yml` vigilado sin recursión. Las pestañas se identifican por colección y ruta (`TabKey`). El snapshot mantiene los campos de "la colección enfocada" (la de la pestaña activa, o la última elegida), así que la barra superior, los entornos, la bóveda y el historial siguen la pestaña activa sin más cambios. `Restore` abre el último workspace (o crea "My workspace") después de los ajustes; cualquier comando que lo necesite también lo hace. Cerrar una colección guarda sus borradores en su sesión y vuelven al abrir la request. La lista de workspaces conocidos vive en el directorio de datos, no en el de configuración: es estado. `FsWatcher::start_shallow` vigila solo `workspace.yml`. Hasta la tarea de interfaz, el árbol muestra la colección enfocada.
- [x] Interfaz: selector de workspace, una raíz por colección en el árbol con su menú, barra superior e historial de la colección activa, pestañas con su colección, paleta con todas las requests cargadas. El selector de la cabecera de la barra lateral cambia de workspace y ofrece New workspace…, Open workspace… y Rename workspace…; debajo, New collection… y Open collection…. Cada raíz se despliega con un clic (y se carga entonces). Una raíz que no carga se ve en rojo, con el motivo al pasar el ratón, y una que necesita su clave en ámbar. El menú de la raíz tiene New request, New folder, Environments, Collection key, Close, Remove from workspace y Copy folder path. Arrastrar solo funciona dentro de la colección, y soltar sobre la raíz lleva el elemento a su nivel superior. Dos pestañas con el mismo nombre de colecciones distintas llevan delante el nombre de la colección; el tooltip siempre lo dice. "Forget" de un workspace existe en el núcleo pero aún no tiene botón.
- [x] Clave de la colección por colección: nunca se pide a quien la creó ni dos veces en la misma máquina; verla, copiarla y rotarla desde el menú de la raíz. Fijado con pruebas. Una prueba recorre dos máquinas (dos directorios de datos): quien comparte el primer secreto no escribe la clave y, tras reiniciar, no se le pide; la otra máquina ve la marca en la raíz, pega la clave una vez y tras reiniciar ya no se le pide. El aviso para pegarla es el de la colección enfocada.
- [x] Medición de memoria con un workspace de tres colecciones y una cargada. Medido con tres colecciones de 1000 requests sin desplegar: 51,2 MB, lo mismo que con una colección pequeña cargada (52,9 MB). El binario sube a 24,6 MiB, cerca del límite ([rendimiento](10-rendimiento-y-memoria.md)).

Siguiente, fuera de esta sección: los entornos globales del workspace (`environments/` de Bruno) como ámbito `Global`.

### Criterio de salida

- `apifast-gui` sin argumentos entra al visor; desde dentro se crean y abren colecciones y workspaces.
- Dos colecciones abiertas a la vez, cada una con su entorno, y al reiniciar todo vuelve como estaba.
- Un workspace creado en Bruno se abre en apifast con sus colecciones, y uno guardado por apifast se abre en Bruno.
- La clave de una colección no se pide a quien la creó ni a una máquina que ya la importó.

**Resultado:** los cuatro se cumplen. Las pruebas de flujo arrancan sin colección, crean colecciones y un segundo workspace desde la interfaz; las del núcleo cubren dos colecciones con su entorno, el reinicio, cerrar y quitar, el cambio de workspace y la clave en dos máquinas. Los `workspace.yml` del corpus de Bruno se leen y se guardan byte a byte, y lo que apifast escribe usa el mismo formato; abrirlo en Bruno queda por comprobar a mano.

---

## F4 — Tests y runner

**Objetivo:** usar una colección como suite de pruebas.

**Estado:** completada el 2026-11-16. Los dos criterios de salida se comprueban en las pruebas de extremo a extremo de la CLI (`crates/apifast-cli/tests/run.rs`): la ejecución termina con 1 cuando una aserción falla, y el informe JUnit es XML válido con los atributos que leen los sistemas de CI (se valida con XPath). Falta verlo en un pipeline real: el repositorio aún no tiene remoto, así que `.github/workflows/ci.yml` no ha corrido nunca; [docs/08](08-tests-y-runner.md#en-ci) trae el ejemplo para GitHub Actions.

### Tareas

- [x] Rutas en variables de tipo objeto (`{{user.address.city}}`), que necesitan las capturas: requiere leer JSON fuera del dominio o un lector propio en `apifast-template`. Pasado desde F1. Hecho con el lector `Json` del dominio: gana una variable con puntos en su nombre, luego el prefijo más largo; un segmento numérico indexa arrays y el campo de un secreto es secreto.
- [x] Variables dinámicas de datos de prueba (`$randomEmail`, `$randomFullName`, `$randomWord`). Pasado desde F1. Con los nombres de Postman y Bruno: nombre, apellido, nombre completo, usuario, email (siempre en `example.com`), palabra y palabras, ciudad, país, teléfono, booleano y alfanumérico, de listas fijas y con la aleatoriedad del `Host`.
- [x] Evaluador de aserciones con todos los operadores. Las aserciones y capturas de Bruno (`runtime.assertions` y las acciones `set-variable`) se leen y se guardan en su sitio; las acciones de otro tipo se conservan. Un lector JSON propio en el dominio (`Json`) sirve al evaluador y servirá a las rutas en variables objeto. `check` evalúa los 28 operadores contra `ResponseFacts` con conversión al tipo del valor real y mensajes con ambos valores.
- [x] Capturas con selector JSON; variables de runtime. `send_request` ejecuta las capturas y luego las aserciones (con el valor esperado renderizado, ya con lo capturado) y las devuelve enmascaradas en `SendOutcome`. `apply_captures` guarda los valores para los envíos siguientes: la interfaz los mantiene mientras la colección está abierta, y `apifast send` muestra los resultados y termina con 1 si una aserción falla. Las de ámbito `environment` se escriben en el entorno, y las secretas en la bóveda, cuando el usuario lo acepta en la interfaz.
- [x] Funciones de plantilla `response` y `header`, con política de refresco y detección de ciclos. Renderizado por rondas: lo que falta se envía y se vuelve a renderizar, con la misma lógica recursiva, así que también sirven dentro de variables de entorno. `never`, `missing`, `ttl` y `always` sobre la última respuesta del historial; cada dependencia se envía una vez por envío y queda en el historial, que la interfaz muestra. Los ciclos se rechazan antes de enviar nada. `path` admite JSONPath y XPath desde la tarea siguiente.
- [x] JSONPath y XPath. Nuevo crate de dominio `apifast-query`: JSONPath según el RFC 9535 (más la forma `[?(…)]` de Jayway y Bruno) sobre el lector `Json`, y XPath 1.0 completo salvo variables sobre `roxmltree`, con nombres sin prefijo que coinciden en cualquier espacio de nombres. Presupuesto de 20 millones de visitas y anidamiento máximo de 64 por consulta. `response(path=…)` lo usa (varios valores dan un array JSON), y `res.body.<ruta>` en aserciones y capturas pasa a JSONPath cuando la ruta usa su sintaxis.
- [x] Caso de uso `RunCollection`: orden, tags, pausa, `--bail`, iteraciones. `run_collection` en `apifast-app`: colección o carpeta (recursiva o no) en el orden del árbol, tags que incluyen o excluyen, omitidas con su motivo, capturas que pasan de una request a la siguiente, errores que no detienen salvo con `bail`, pausa por un puerto `Pause`, cancelación y eventos de progreso. Devuelve un `RunReport` con totales.
- [x] Ejecución dirigida por datos (CSV y JSON). `parse_data` lee CSV (RFC 4180, con cabecera) o un array JSON de objetos, con errores que dicen la línea o el elemento. `RunOptions.data` hace una iteración por fila, con sus columnas como variables runtime.
- [x] `apifast run` con reporters de consola, JSON, JUnit y HTML. Los reporters son funciones puras de `apifast-app` (para que la interfaz pueda exportarlos); la consola muestra una línea por request en vivo. Códigos de salida 0, 1, 2 y 3, y el 3 pasa a ser el de uso en todos los comandos. Pendiente: `--env-file`; `--insecure` y `--cacert` llegaron con los certificados de F5.
- [x] Interfaz: pestaña Tests, resultados en la respuesta, vista del runner. La pestaña Tests del editor edita aserciones y capturas; la de la respuesta muestra los resultados, los fallos primero; un aviso guarda las capturas de ámbito `environment` en el entorno o, si son secretas, en la bóveda. La ventana del runner ("Run…" en el menú de una colección o carpeta) lanza la ejecución en segundo plano, muestra el progreso y los resultados, abre cada paso con su respuesta y guarda los informes. Corregido de paso: `run_collection` ordena el árbol por `seq`, como la interfaz, y no en el orden de los archivos.
- [x] Filtro de respuesta por JSONPath o XPath en el visor. Un campo sobre el body formateado (JSON o XML, hasta el umbral de 2 MB) con `apifast-query`: el valor o los valores seleccionados, formateados, cuántos son, y el error con su posición si la expresión no vale.

### Criterio de salida

- Una colección de ejemplo se ejecuta en un pipeline de CI y falla con código 1 cuando una aserción falla.
- El informe JUnit se muestra correctamente en un sistema de CI.

---

## F5 — Red avanzada

**Objetivo:** cubrir los escenarios de red de entornos corporativos.

### Tareas

- [x] Digest, NTLM y AWS SigV4. Las tres se aplican en el transporte (`TransportAuth`), sobre la request tal como sale. AWS SigV4 firma cada request al mismo origen; Digest responde al 401 (MD5, SHA-256 y `-sess`, `qop=auth`) reenviando una vez; NTLMv2 hace el intercambio en una conexión, con un cliente HTTP/1.1 propio. Cada una tiene una prueba de integración contra un servidor local, además de los ejemplos oficiales (documentación de AWS, RFC 2617 y 7616, [MS-NLMP] 4.2.4).
- [x] OAuth 2.0: authorization code con PKCE, client credentials, password; caché cifrada y refresco. Por fases:
  - [x] A. Modelo completo (`OAuth2Auth`): se lee y se guarda con el bloque de Bruno, cambiando solo las claves afectadas, y se edita en la pestaña Auth con los campos de cada flujo.
  - [x] B. Obtener y refrescar el token (client credentials y password) y aplicarlo al enviar. Antes de renderizar, el envío toma el token guardado, lo refresca o pide uno nuevo, y lo pone en el header o la query como variable secreta. Por ahora los tokens se guardan en memoria (`MemoryTokens`). Probado con un servidor guionizado (caché, refresco, errores, query, `id_token`) y de extremo a extremo con la CLI contra un servidor OAuth local.
  - [x] C. Caché de tokens cifrada con la clave maestra; ver y borrar el token en la interfaz. `FsTokenStore` sella cada token en `oauth2/<colección>.tokens`, para cualquier colección (no necesita id); la interfaz lo usa y sobrevive a un reinicio, la CLI sigue en memoria. La pestaña Auth muestra el estado del token y obtiene uno nuevo o lo olvida.
  - [x] D. Authorization code con PKCE: navegador del sistema y servidor local temporal para el *callback*. Puerto `Authorizer` en la aplicación y `LoopbackAuthorizer` en el transporte (callback solo en `localhost`, 5 minutos, cancelable); `state` comprobado y PKCE S256. La CLI solo abre el navegador en una terminal.
- [x] Proxy: sistema y manual (HTTP, HTTPS, SOCKS5), exclusiones, por colección. Proxy de la aplicación en los ajustes (sistema, ninguno o manual, con la contraseña sellada aparte) y de cada colección en `config.proxy`, como Bruno (heredar, ninguno o propio), editable en "Collection settings…". HTTP, HTTPS, SOCKS4 y SOCKS5, credenciales y exclusiones; el transporte de cada colección se reconstruye al cambiar. Pendientes: registrarlo en el timeline, `{{variables}}` en sus campos y la configuración de proxy del escritorio.
- [x] Autoridades certificadoras adicionales y certificados cliente por host. CA adicional en los ajustes y `--cacert` (repetible), sin validación con aviso (`--insecure`), y certificados PEM por host (con puerto y comodines) en `config.clientCertificates`, editables en "Collection settings…": un cliente por certificado, elegido en cada salto. Probado contra un servidor TLS local que exige certificado de cliente. Pendiente: PKCS#12 y claves cifradas, que dan un error que explica cómo convertirlos.
- [x] Tarro de cookies con editor, cifrado con la clave maestra (movido de F3). Un tarro por colección según RFC 6265 en el dominio, aplicado en cada salto del transporte (también redirecciones y retos), con "Send cookies" y "Store cookies" en los ajustes, sellado en `cookies/<colección>.jar`, en memoria en la CLI, editor por dominio y pestaña Cookies en la respuesta ([docs/07](07-motor-de-red.md#cookies)). Pendiente: lista de sufijos públicos (`publicsuffix`), si se ve necesaria.
- [x] Sobrescrituras de DNS. Por colección, en `.apifast/collection.yml` (OpenCollection no tiene dónde, y así se versionan), editables en "Collection settings…". Host con comodín, una o varias IP; las aplica el resolutor del transporte, conservando el host para `Host`, SNI y el certificado, y el timeline las señala ([docs/07](07-motor-de-red.md#dns)).
- [x] Timeline completo con conector instrumentado (DNS, conexión, TLS, reutilización). Con el pool activo: un resolutor propio, una capa sobre el conector de reqwest y el almacén de sesiones de rustls anotan en una sonda por envío (`task_local`); la configuración de rustls la construye apifast para colgar ese almacén ([docs/07](07-motor-de-red.md#implementado)). Pendiente: la suite y el certificado del servidor en el evento TLS, y el evento de proxy.
- [x] Descompresión manual, para registrar el tamaño comprimido y el descomprimido (en F1 la hace reqwest). gzip, deflate, brotli y zstd con decodificadores de escritura, en streaming hacia el almacén; el tamaño en la red va en el evento `decompressed` y la interfaz muestra los dos ([docs/07](07-motor-de-red.md#respuesta)).
- [x] `encodeUrl` como en Bruno. La tarea decía "respetar `encodeUrl: false`", pero al revisar Bruno resultó al revés: desactivado, Bruno (axios) solo normaliza la URL con WHATWG, como ya hacía apifast; activado (el valor por defecto), además aplica `encodeURIComponent` a cada segmento de la ruta y a cada nombre y valor de la query. Ahora apifast hace lo mismo al renderizar ([docs/07](07-motor-de-red.md#codificación-de-la-url-encodeurl)), y el enmascarado de secretos reconoce cualquier mezcla de bytes tal cual y `%XX`.
- [x] Multipart y cuerpos de archivo en streaming. El envío en streaming ya estaba desde F1 (reqwest lee las partes y el archivo del disco por trozos); faltaba editarlos, que F2 había dejado fuera: la interfaz edita partes de texto y de archivos, y el archivo de un body, con el diálogo del sistema y rutas relativas a la colección ([docs/09](09-ui-ux.md#bodies-multipart-y-de-archivo)). Un body de archivo sin tipo va como `application/octet-stream`, como en Bruno.
- [x] Ejemplos de respuesta guardados. Con el formato de Bruno (`examples` en el archivo de la request, [docs/04](04-formato-de-archivos.md#ejemplos)); "Save as example" en la respuesta los añade como una edición de la request, y la pestaña Examples los muestra, renombra y borra ([docs/09](09-ui-ux.md#panel-de-request)). Sirven después para el servidor de mocks.

### Criterio de salida

- Cada tipo de autenticación tiene una prueba de integración contra un servidor local.
- El timeline muestra tiempos de DNS, conexión y TLS con el pool de conexiones activo.

Los dos se cumplen: cada esquema (Digest, NTLM, AWS SigV4, OAuth 2.0 en sus tres flujos) se prueba contra un servidor local, y el timeline muestra DNS, conexión y TLS con el pool activo, marcando las conexiones reutilizadas. Quedan pendientes menores anotados en cada tarea: la suite y el certificado en el evento TLS, el evento de proxy, `{{variables}}` en el proxy y en las sobrescrituras de DNS, la configuración de proxy del escritorio, PKCS#12 y claves cifradas, y la lista de sufijos públicos de las cookies.

---

## F6 — Importación y exportación

**Objetivo:** que migrar a apifast sea sencillo.

### Tareas

- [x] Importadores: Bruno `.bru`, Postman (colecciones y entornos), Insomnia, OpenAPI 3, cURL.
  - [x] Insomnia: exportaciones v4 (JSON) y v5 (YAML), en la CLI y en la interfaz, que la reconoce ([docs/08](08-tests-y-runner.md#apifast-import)). Probado con las ocho exportaciones del corpus de Bruno (las rotas a propósito se rechazan) y de extremo a extremo: importar y enviar, con el param de query unido a la URL.
  - [x] cURL: `parse_curl` (pegar en la barra de URL); un archivo de comandos no tiene importador propio: cada comando es una request.
  - [x] Bruno `.bru`: la carpeta de la colección, con su `collection.bru`, `folder.bru` y entornos ([docs/08](08-tests-y-runner.md#apifast-import)), en la CLI y en la interfaz. Probado con la colección de pruebas de Bruno entera (247 archivos, más de 200 requests, ninguno ilegible) y de extremo a extremo: importar y enviar, con el header heredado de `collection.bru`.
  - [x] OpenAPI 3 (3.0 y 3.1), en JSON o YAML ([docs/08](08-tests-y-runner.md#apifast-import)), en la CLI y en la interfaz, que reconoce el formato. Probado con 19 especificaciones del corpus de Bruno (y dos rotas a propósito, que se rechazan) y de extremo a extremo: importar un YAML y enviar. Pendiente: Swagger 2.0 y los `$ref` externos.
  - [x] Postman: colecciones v2.0 y v2.1 y entornos ([docs/08](08-tests-y-runner.md#apifast-import)). Probado con 33 exportaciones del corpus de Bruno, que se importan sin rutas repetidas, y de extremo a extremo: importar y enviar.
- [x] Plan de importación revisable: qué se crea, avisos, elementos no soportados.
  - [x] El plan y su aplicación (`plan_import`, `apply_import`) y `apifast import`, que lo imprime y escribe (o no, con `--dry-run`).
  - [x] El diálogo de importación de la interfaz ([docs/09](09-ui-ux.md#importar)): elegir la exportación y sus entornos, revisar el plan y la carpeta, importar y abrir.
- [x] Reimportación que actualiza en lugar de duplicar ([ADR 0010](adr/0010-reimportacion.md)).
  - [x] Clave estable por request en cada importador, registro en `.apifast/import.yml`, y `plan_reimport`/`apply_reimport`: crear, actualizar, conflicto, borrado aquí, retirado de la fuente; lo no elegido se vuelve a ofrecer.
  - [x] `apifast import` sobre una colección importada la actualiza, con `--overwrite` y `--prune` ([docs/08](08-tests-y-runner.md#importar-otra-vez)). Probado de extremo a extremo con una especificación que cambia y una request editada.
  - [x] Revisar y aplicar una reimportación en la interfaz: "Import again…" en el menú de la colección, con una casilla por request ([docs/09](09-ui-ux.md#importar-otra-vez)). La fuente se guarda relativa a la colección, para proponerla en cualquier clon.
- [x] Pegar un comando cURL en la barra de URL crea la request. `parse_curl` en `apifast-import`: palabras como las separa un shell POSIX (comillas, `\`, `$'…'`, continuaciones de línea) y las opciones de Bruno y alguna más (`-X`, `-H`, `-A`, `-e`, `-b`, `-u` con `--digest`/`--ntlm`, `--oauth2-bearer`, `-d` y sus variantes juntadas con `&`, `--json`, `-F`/`--form-string`, `@archivo` como body de archivo, `-G`, `-I`, opciones cortas juntas como `-sSL` o `-XPOST`). Las que no cambian la request se saltan con su valor; las demás se listan, nunca se pierden en silencio. Es también la base del importador de cURL.
- [x] Copiar como cURL. `render_for_copy` en `apifast-app` renderiza la request como la enviaría un envío, pero sin red: token OAuth 2.0 solo si está guardado, `response(...)` sin enviar, y en modo tolerante, con lo que falta listado. `to_curl` en `apifast-import` la escribe con opciones largas y comillas de shell; probado de ida y vuelta con `parse_curl` y con el curl real. En el menú de cada request del árbol y en la paleta ([docs/09](09-ui-ux.md#layout-principal)).
- [x] Exportar a Postman: colección v2.1 y entornos, con `apifast export postman` ([docs/08](08-tests-y-runner.md#apifast-export)) y "Export to Postman…" en el menú de la colección ([docs/09](09-ui-ux.md#exportar-a-postman)). Probado de ida y vuelta con las 33 colecciones del corpus de Postman y de extremo a extremo.
- [x] Generación de código para los lenguajes más usados: JavaScript (`fetch`), Python (`requests`) y Go (`net/http`), además de cURL. `to_code` en `apifast-import`, sobre el mismo renderizado que copiar como cURL; en el submenú "Copy as" de cada request y en la paleta ([docs/09](09-ui-ux.md#layout-principal)). Lo que el cliente de un lenguaje no hace se avisa. Probado ejecutando el código de Python y Go contra un servidor. Otros lenguajes, según demanda.

### Criterio de salida

Una colección real de cada origen se importa y sus requests se envían correctamente; lo no soportado se informa de forma explícita.

Se cumple: cada origen tiene una prueba de extremo a extremo que importa y envía (Postman, OpenAPI, Bruno `.bru` e Insomnia con `apifast import` y `apifast send`; un comando cURL pegado en la barra de URL), y los corpus de Bruno (33 colecciones de Postman, 19 especificaciones OpenAPI, su colección de pruebas `.bru` entera y 8 exportaciones de Insomnia) se importan sin errores. Lo que no tiene lugar en apifast se lista con dónde estaba, en el plan de la CLI y en el diálogo. Quedan pendientes anotados: Swagger 2.0 y los `$ref` externos de OpenAPI.

---

## F7 — GraphQL, SSE y WebSocket

### Tareas

- [x] GraphQL: editores de query y variables, introspección en caché, explorador de esquema, autocompletado.
  - [x] El body GraphQL (`Body::GraphQl`): se envía como JSON, con las variables validadas; editores de la operación (con resaltado propio) y de las variables ([docs/09](09-ui-ux.md#editor-de-código)). Los importadores de Postman, Insomnia y Bruno `.bru` lo conservan como GraphQL, la exportación a Postman usa su modo `graphql`, y copiar como código lo envía como JSON. Probado de extremo a extremo con `apifast send`.
  - [x] Introspección en caché y explorador de esquema ([docs/07](07-motor-de-red.md#graphql-f7), [docs/09](09-ui-ux.md#editor-de-código)): la request misma pide el esquema, que se guarda por URL y entorno en los datos locales; el explorador navega de tipo en tipo. Probado contra un servidor local: pedirlo, encontrarlo guardado en otra sesión y un servidor con la introspección desactivada.
  - [x] Autocompletado de la operación con el esquema ([docs/09](09-ui-ux.md#editor-de-código)): campos según el tipo de la selección, argumentos, `$variables`, valores de enum y tipos, con una lectura ligera del texto hasta el cursor. Probado con casos de cada contexto y en el editor (flechas, Enter, Esc y `Ctrl+Espacio`).
- [x] SSE: parseo en vivo y visor de eventos ([docs/07](07-motor-de-red.md#server-sent-events-f7), [docs/09](09-ui-ux.md#panel-de-respuesta)): parser incremental según el estándar, lista en vivo con filtro y detalle, "Stop" que conserva lo recibido y un timeout que no corta el stream. Probado con trozos de todos los tamaños, contra un servidor que no cierra (transporte y núcleo) y con `apifast send` y `Ctrl+C`.
- [x] `apifast-ws`: conectar, enviar, cerrar; historial de mensajes; herencia de headers y autenticación.
  - [x] Transporte y caso de uso ([docs/07](07-motor-de-red.md#websocket-f7)): el handshake va por `apifast-http` (proxy, TLS, certificados, DNS y cookies), los mensajes se renderizan al salir y el log de la conexión se guarda en disco como el cuerpo de su respuesta, con los secretos enmascarados. `apifast send` se conecta, envía los mensajes y muestra lo que llega en vivo. Probado contra un servidor local (eco, binario, cierre del servidor y del cliente, un servidor que no cambia de protocolo) y de extremo a extremo con la CLI.
  - [x] En la GUI ([docs/09](09-ui-ux.md#requests-websocket)): "Connect" y "Disconnect" en la barra de URL, "Send" junto al mensaje mostrado (tal como está en pantalla) y la pestaña Messages en vivo, con filtro y detalle; cerrada la conexión, o abierta desde el historial (con `WS`), se lee del log guardado. Probado en el núcleo contra un servidor local (conectar, enviar, desconectar, el log y el historial) y en el panel de respuesta.
- [x] Formato de archivo para requests `graphql` y `websocket`.
  - [x] `graphql`, el de Bruno ([docs/04](04-formato-de-archivos.md#requests-graphql)): sus archivos del corpus se guardan byte a byte y pasan las ediciones; cambiar el body a GraphQL o desde él renombra el bloque en su lugar. Probado también con las pruebas de propiedades.
  - [x] `websocket`, el de Bruno ([docs/04](04-formato-de-archivos.md#requests-websocket), [ADR 0011](adr/0011-websocket-como-body.md)): una request con los mensajes como body; sus archivos del corpus se guardan byte a byte y se editan en su lugar (un mensaje plano o variantes), sin `method` ni `params`. Se abre en la GUI con su editor de mensajes y `WS` en el árbol y la barra de URL; el runner y la exportación la dejan fuera con aviso. Conectarse llega con `apifast-ws`.

### Criterio de salida

Cada protocolo tiene su editor y su visor, y sus archivos pasan el round-trip.

---

## F8 — gRPC

### Tareas

- [x] Formato de archivo para requests `grpc`, el de Bruno ([docs/04](04-formato-de-archivos.md#requests-grpc), [ADR 0012](adr/0012-grpc-como-body.md)): una request con la llamada como body (método, tipo, `.proto` y mensajes) y la metadata como headers; sus 27 archivos del corpus se guardan byte a byte y se editan en su lugar. Se abre en la GUI con su editor (`gRPC` en el árbol y la barra de URL, pestaña Metadata); enviarla da un error claro, y el runner y la exportación la dejan fuera con aviso.
- [x] `apifast-grpc`: reflection (v1 y v1alpha) y carga de `.proto` ([docs/07](07-motor-de-red.md#grpc-f8)): la conexión la abre `apifast-http` (TLS, certificados y DNS de la colección), la metadata son los headers heredados, y `apifast describe` lista los métodos. Probado contra un servidor local con reflection v1 y v1alpha (con un interceptor que exige el token), con un `.proto` con imports y de extremo a extremo con la CLI.
- [x] Decidir cómo compilar `.proto` sin un `protoc` externo: protox ([ADR 0013](adr/0013-protox-para-proto.md)).
- [x] Unario, streaming de servidor, de cliente y bidireccional.
  - [x] Transporte, caso de uso y CLI ([docs/07](07-motor-de-red.md#grpc-f8), [docs/08](08-tests-y-runner.md#apifast-send)): los cuatro modos como un stream HTTP/2 en cada sentido, mensajes JSON convertidos con prost-reflect, el log de la llamada como el de WebSocket y el estado al final. `apifast send` la llama e imprime cada respuesta en una línea. Probado contra un servidor local con los cuatro modos (con `.proto` y por reflection), un estado de error, la cancelación y de extremo a extremo con la CLI.
  - [x] En la GUI ([docs/09](09-ui-ux.md#requests-grpc)): "Load methods" y "Pick a method", "Call" (envía el mensaje mostrado), "Send" y "End stream" para un cliente que transmite, "Cancel", y la pestaña Messages en vivo; la llamada en el historial con `gRPC`. Probado en el núcleo contra un servidor local (cargar los métodos, llamar con streaming del cliente, terminar el stream, el log y el historial), en el editor y en el panel de respuesta.
- [x] Editor de mensajes JSON con esquema para autocompletar ([docs/09](09-ui-ux.md#requests-grpc)): la descripción trae los campos de los mensajes (nombre en JSON, tipo, lista o mapa, valores de enum), y el editor completa campos, valores de enum y booleanos según el tipo de entrada del método, con una lectura ligera del JSON hasta el cursor. Probado con casos de cada contexto (anidados, listas, mapas, plantillas) y en el editor.
- [x] Metadata, TLS y autenticación reutilizando el motor ([docs/07](07-motor-de-red.md#grpc-f8)): la conexión de `apifast-http` ahora pasa por el proxy (las reglas de reqwest, con `CONNECT` sobre HTTP o HTTPS, SOCKS5 y SOCKS4a), y tonic no pone TLS propio. Probado con proxies mínimos locales (`CONNECT` con credenciales, un `407`, las exclusiones, SOCKS5 con usuario y contraseña), una llamada a través del proxy de la colección y un servidor en TLS que exige certificado de cliente (con él, sin él y sin la CA).
- [x] Inicialización diferida y feature de Cargo ([docs/10](10-rendimiento-y-memoria.md#funciones-opcionales)): el transporte se crea en cada uso; la feature `grpc` (por defecto) de la CLI y la GUI trae `apifast-grpc`, y sin ella las requests gRPC dicen que la build no lo tiene. Medido: un envío HTTP ocupa la misma memoria con y sin gRPC, y la CLI sin él pesa 2,3 MB menos. El CI compila y prueba los drivers sin la feature.

### Criterio de salida

Los cuatro modos funcionan contra un servidor de pruebas, y abrir la aplicación sin usar gRPC no aumenta la memoria.

Se cumple: los cuatro modos funcionan contra un servidor local (con `.proto` y por reflection v1 y v1alpha, en texto plano, en TLS con certificado de cliente y a través de un proxy), desde el transporte, el núcleo de la GUI y `apifast send`. La memoria se midió con la CLI: un envío HTTP ocupa lo mismo con gRPC compilado que sin él, porque nada de gRPC corre hasta usarlo; la GUI crea el transporte de la misma forma. Quedan pendientes anotados: los importadores de Bruno `.bru` e Insomnia todavía omiten las requests gRPC, los scripts de sus ciclos esperan a F9, y el proxy HTTPS y SOCKS4a no tienen prueba propia.

---

## F9 — Scripting JavaScript

### Tareas

- [x] `apifast-script-js` con QuickJS, límites de tiempo y memoria. Puerto `ScriptEngine` en `apifast-app`, con `CannedScripts` en memoria; `QuickJs` crea un runtime por script, con límite de tiempo (esperas incluidas), de memoria y de pila, y guarda la consola.
- [x] API `req`, `res`, `bru`, `test` y `expect`. En JavaScript sobre los datos que da el motor (`ScriptInput`), devolviendo la request cambiada, las variables, los tests y lo pedido al runner (`ScriptOutcome`). Pendientes: `require` de librerías y los cuerpos multipart y de archivo (`bru.sendRequest`, `runRequest` y las cookies llegaron con la compatibilidad con Bruno).
- [x] Scripts de colección, carpeta y request con orden "sándwich". Se leen y se escriben en los archivos; los previos cambian la request resuelta antes de renderizarla, y los posteriores y de tests corren después de la respuesta. El runner aplica sus variables y respeta `skipRequest`, `stopExecution` y `setNextRequest`, y sus tests cuentan en la CLI, los reportes y la pestaña Tests. Pendiente: los scripts de gRPC.
- [x] Editor de scripts con resaltado. Pestaña Scripts en la request y ventana "Scripts…" para carpetas y colección, con un editor por tipo y resaltado de JavaScript.
- [x] Traducción de scripts de Postman en el importador, con la tabla de reemplazos de Bruno; lo que no tiene equivalente queda comentado y avisado. El importador de `.bru` trae los scripts tal cual. Pendientes: los scripts de Insomnia y la traducción inversa al exportar a Postman.
- [x] Carga del motor solo al ejecutar el primer script. No hay estado global: el runtime se crea al empezar cada script, y una request sin scripts (o solo con scripts de tipos que apifast no ejecuta) no llama al motor. Medido con la CLI: unos 2 MB más de memoria máxima, solo cuando corre un script.
- [x] Compatibilidad con las colecciones de prueba de Bruno (`bruno-tests`), por partes:
  - [x] API barata y errores: `req.getHost`, `getPath`, `getQueryString`, `getPathParams` y `getTags`; variables de colección (`setCollectionVar` y compañía), `getAllEnvVars` y `deleteAllEnvVars`, `isSafeMode`; temporizadores (`setTimeout`, `setInterval`); variables dinámicas en `bru.interpolate`. Después de la respuesta, `req` es la request enviada, ya renderizada.
  - [x] `req.headerList` y `res.headerList`; los plugins de chai `jsonBody` y `jsonSchema` (este con un validador draft-07 propio, en lugar de ajv). Las cabeceras desactivadas llegaron en una parte posterior.
  - [x] `bru.sendRequest` y `bru.runRequest`, por un puerto nuevo, `ScriptHost`: el motor atiende las llamadas del script por un canal mientras corre.
  - [x] Cookies (`bru.cookies` y `bru.cookies.jar()`), sobre el tarro de la colección que el transporte expone.
  - [x] En una ejecución, `bru.getEnvVar` y `bru.getCollectionVar` ven lo que cambiaron los scripts de requests anteriores (`SendOptions.script_changes`). Variables globales para la ejecución. Etiquetas de carpeta, heredadas por sus requests en el runner y en `req.getTags`. `await test(…)` espera al test.
  - [x] Las cabeceras desactivadas en `req.headerList`, `req.setBody` en formularios y `req.getExecutionMode`.
  - [x] Aserciones sobre la request enviada: `req.method`, `req.url`, `req.headers.<nombre>` y `req.body.<ruta>`.

### Criterio de salida

Las colecciones de prueba de Bruno con scripts se ejecutan con los mismos resultados, y una colección sin scripts no carga el motor.

Se cumple, con las diferencias que quedan documentadas abajo. Una colección sin scripts no carga el motor: una request sin scripts no lo llama, y el motor cuesta unos 2 MB de memoria solo cuando corre un script.

La colección `bruno-tests` se importó con `apifast import bruno` y se corrió con `apifast run --allow-unresolved`, porque Bruno deja sin resolver las variables que faltan. Se usó un servidor local que imita `/ping` y `/api/echo/json` del de Bruno, porque sin Node no se puede levantar el suyo. Los tests de `sendRequest` llamaron de verdad a `echo.usebruno.com`.

En `scripting/api` y `scripting/js` pasan 315 de 330 tests y las 15 aserciones; al empezar, 54 de 235. Lo que no pasa, por causa:

- **Diferencias de entorno, no de apifast** (10 tests): esperan el entorno Prod (su host y su nombre: `getEnvName`, `getEnvVar`, `interpolate`, `getHost`, `getUrl`, `res.getUrl`, `deleteAllEnvVars`), el tamaño exacto de una respuesta de Express (`getHeaders`) o un endpoint que el servidor local no imita (`/api/mix`, en `Redirect Cookie Save`).
- **A propósito** (2): `req.getHeader` no distingue mayúsculas, como HTTP (`case-insensitive-write`); `bru.isSafeMode()` da `true`, porque los scripts de apifast siempre corren aislados, y los tests de Bruno esperan su modo desarrollador.
- **Detalles que no se igualan** (3): `res.getResponseTime()` da 0 contra un servidor local, porque se mide en milisegundos enteros; `res.setBody(undefined)` y un array con `undefined` no sobreviven al paso del cuerpo de un script al siguiente (el propio Bruno avisa que ahí sus modos difieren).

Fuera de esta colección quedan anotados: `require` de librerías y los módulos de Node (`scripting/node-builtins`, `npm-modules`, `local-modules`), `bru.getSecretVar`, los cuerpos multipart en los scripts, los scripts de gRPC y de Insomnia, la traducción inversa al exportar a Postman, y editar las etiquetas de carpeta en la interfaz.

---

## F10 — Versión 1.0

### Tareas

- [x] Integración con git: estado, diff semántico por request, commit, cambio de rama.
  - [x] Puerto `GitRepository` y adaptador `apifast-git` con gitoxide ([ADR 0014](adr/0014-git-con-gitoxide.md)): la rama, los archivos que cambiaron frente al último commit (añadidos, modificados, borrados; respeta `.gitignore`) y su contenido en él. Una colección puede ser una carpeta de un repositorio más grande. Lo que el puerto promete es una batería genérica (`apifast-git-contract`) que cualquier adaptador pasa.
  - [x] Diff semántico por request: `git_diff` lee el archivo del último commit y el del disco con el mismo lector de OpenCollection (`CollectionRepository::parse_file` y `file_now`) y dice qué campos cambiaron, con su valor antes y después. Las listas con nombre (cabeceras, parámetros, variables, aserciones, capturas, scripts, ejemplos) se comparan elemento a elemento. Vale también para `folder.yml`, `opencollection.yml` y los entornos; otro archivo se compara como texto.
  - [x] Commit de los archivos elegidos, tal como están en disco (uno borrado, como borrado), en la rama actual y con la identidad de la configuración de git. Lo demás que estaba en el índice se queda ahí. El índice se actualiza para que `git status` vea lo mismo, y `git fsck --strict` acepta lo escrito. Sin identidad configurada, el error dice cómo configurarla.
  - [x] Cambio de rama: lista de ramas locales y cambio a una de ellas. gix no lo trae (solo el checkout inicial de un clon); el adaptador recorre los árboles de las dos ramas, borra lo que sobra (y las carpetas que quedan vacías), escribe lo que cambia (con su modo: ejecutable, enlace), rehace el índice y mueve `HEAD`, con su entrada en el reflog. Se rechaza mientras un archivo seguido del repositorio, aunque esté fuera de la colección, tenga cambios, o si pisaría un archivo no seguido; los no seguidos que no toca se quedan. La batería tiene 14 escenarios.
  - [x] En la CLI: `apifast git status`, `diff`, `commit`, `branches` y `switch`. gix añade unos 3,7 MB al binario de la CLI en release (de 16,9 a 20,6 MB).
  - [x] Feature de Cargo `git`, activa por defecto en la CLI y la GUI, como `grpc`: sin ella no se compila gix (la CLI pesa 3,6 MB menos y la GUI 3,5 MB menos). El CI ya compila y prueba los drivers sin features.
  - [x] En la interfaz: la rama y los cambios en una barra de estado nueva, que abre la ventana "Git": la lista de cambios con casillas, el diff por campos del archivo elegido, el commit de los marcados y el cambio de rama (rechazado con pestañas sin guardar). El estado se refresca al abrir la colección, con el watcher y tras cada operación.
- [ ] Empaquetado para Linux (AppImage, Flatpak, paquetes); pulido y empaquetado de macOS y Windows.
  - [x] Instalar en la propia máquina sin root: `packaging/linux/install-local.sh` compila e instala en `~/.local` (binarios, entrada de menú, metainfo e icono), y `--uninstall` lo quita. No se publica nada.
  - [x] Archivos comunes de Linux en `packaging/linux/`: la entrada de menú (`.desktop`), el metainfo de AppStream y el icono, con el id `com.tryxtek.apifast` (el de la ventana también).
  - [x] Paquetes `.deb` (cargo-deb) y `.rpm` (cargo-generate-rpm), un `apifast` con la interfaz y la CLI, con `packaging/linux/build.sh`. Solo enlazan la libc: lo que la interfaz carga al ejecutarse (Vulkan o EGL, Wayland o X11) va como recomendado. Construidos y revisados aquí: contenido, metadatos y dependencias; la CLI extraída funciona.
  - [ ] AppImage y Flatpak, con los mismos archivos comunes.
    - [x] AppImage: `packaging/linux/appimage/build.sh` arma el AppDir (interfaz, CLI, entrada de menú, metainfo, icono; sin bibliotecas del sistema) y lo empaqueta con appimagetool. `AppRun` lanza la interfaz, o la CLI si se invoca con un enlace llamado `apifast`. appimagetool 1.9.1 y el runtime 20251108 (las últimas versiones numeradas) se descargan con su hash fijado (`appimage/tools.sh`), y appimagetool no descarga nada más. El workflow de paquetes lo construye también, en Ubuntu 22.04. Probado: el AppImage arranca la CLI con FUSE y sin él.
    - [ ] Flatpak: no por ahora; sin publicar en Flathub no aporta frente al `.rpm` o el AppImage.
  - [x] Construir los paquetes de release en el sistema más antiguo soportado (Ubuntu 22.04, glibc 2.35), en CI: `.github/workflows/packages.yml` valida la entrada de menú y el metainfo, construye, comprueba la glibc que piden los binarios (`check-glibc.sh`) y guarda los paquetes como artefacto; un tag `v<versión>` debe coincidir con la del workspace. No se publica nada.
  - [x] El icono definitivo: `packaging/linux/icons/hicolor/scalable/apps/com.tryxtek.apifast.svg`.
  - [ ] macOS y Windows.
- [x] Actualizaciones.
  - [x] Versiones: `tools/release.sh <versión>` la pone en el workspace y en el metainfo, hace el commit y el tag `v<versión>`, sin publicar nada. `apifast --version` y la ventana de ajustes dicen la versión y el commit compilado.
  - [x] Actualizar lo instalado: `packaging/linux/update.sh` compila el tag más nuevo (o el pedido) en un worktree aparte e instala con su `install-local.sh`; `install-local.sh --rollback` vuelve a la versión anterior, que guarda. Paquetes y AppImage, en [docs/13](13-compilar-y-empaquetar.md#versiones-y-actualizaciones).
  - [x] Migraciones de datos: `last-version` en la carpeta de datos; al arrancar una versión más nueva, las migraciones intermedias corren una vez, tras copiar lo que tocan; una más vieja avisa y no cambia nada. Todavía no hay ninguna.
- [ ] Traducciones: español e inglés.
  - [x] La interfaz en español y en inglés: un archivo por idioma en `apifast-gui/src/strings/` que el script de compilación encuentra solo, sin registrarlo en ningún sitio. El idioma del sistema por defecto (`LC_ALL`, `LC_MESSAGES`, `LANG`) y uno fijo en los ajustes, que cambia la interfaz en el momento. Los textos con datos son plantillas (`{name}`) y los que llevan un número, plurales con la regla de su idioma. Las notas del núcleo de la interfaz (commits, cambios de rama) son datos que la interfaz pone en palabras. Una prueba comprueba que cada idioma tiene los marcadores del inglés y ningún texto vacío ([docs/09](09-ui-ux.md#idiomas)).
  - [x] Los errores y avisos de las capas que no son la interfaz: las validaciones de la aplicación (`Invalid`), los de git (`GitError`) y los avisos de importar y exportar (`ImportNote`) pasan de frases a variantes con sus datos; con los que ya lo eran (red, plantillas, entornos, bóveda, importadores, filtros), la interfaz los dice en su idioma (`messages.rs`, unas 175 plantillas por idioma). El núcleo de la interfaz sabe el idioma y dice en él sus errores. En inglés, una prueba exige que cada uno diga lo mismo que su `Display`, que siguen usando la CLI y los informes ([docs/09](09-ui-ux.md#errores-y-avisos-de-las-otras-capas)).
  - [ ] Los resultados de las aserciones y los fallos de los scripts, que se guardan como texto enmascarado y van a los informes; y los detalles internos que llegan como texto (almacén de respuestas, archivos de datos del runner, clave de colección).
  - La CLI queda en inglés, como `git` o `cargo`.
- [ ] Guía de usuario.
- [ ] Auditoría de seguridad de la bóveda.
- [ ] Revisión final de presupuestos de rendimiento.
  - [x] RAM en reposo, arranque, apertura del árbol, CPU en reposo y tamaño, medidos ([docs/10](10-rendimiento-y-memoria.md#mediciones-de-f10)). Los presupuestos de RAM pasan a ser de la memoria propia de apifast, sin la pila gráfica del sistema (con RADV, LLVM aporta de 9 a 24 MB según quién más la use): 30–33 MB.
  - [x] Medidos con el ejemplo `bench` de `apifast-gui`: 100 MB de respuesta suman 3,5–4,7 MB (presupuesto, 10 MB); una tecla en un body de 200 KB cuesta 1,6 ms de CPU (10 ms); un envío a un servidor local, 0,29 ms (2 ms). Todos los presupuestos se cumplen en Linux.
  - [x] Los 4 MB que quedan tras una respuesta grande no son una fuga: en 8 vueltas la memoria no crece; son código del visor usado por primera vez (páginas que el kernel suelta), lo que el asignador guarda y memoria de los hilos del envío.
  - [ ] Medir en macOS y Windows.
- [ ] Experiencia de escritorio: lo que traen las demás aplicaciones.
  - [x] Registro con niveles (`error`, `warn`, `info`, `debug`, `trace`), con `tracing` ([docs/14](14-desarrollo.md#el-registro)):
    - El crate adaptador `apifast-log` es el único que usa `tracing-subscriber`; el resto solo emite eventos. Los mensajes del crate `log` (wgpu, winit, reqwest) llegan también.
    - El nivel sale de `APIFAST_LOG` (con la sintaxis de `tracing`, también por crate: `apifast_http=debug`) o del ajuste "Registro" (por defecto `warn`), que cambia en el momento; las bibliotecas quedan en `warn`.
    - La interfaz escribe en la terminal y en `logs/apifast.log` de la carpeta de datos, que rota a `apifast.log.1` a los 5 MB. La CLI escribe en la terminal y solo con `APIFAST_LOG`: sin ella, su salida no cambia.
    - Eventos: arranque y backend gráfico, migraciones, colecciones abiertas, envíos (ruta de la request, estado, tiempo y tamaño; el adaptador HTTP en `debug`, solo con el host), errores que muestra la interfaz, commits y cambios de rama. Nunca la URL completa ni valores secretos.
    - Coste: +1,8 MB en el binario de la GUI ([docs/10](10-rendimiento-y-memoria.md)). La RAM no se midió.
  - [x] Preferencias completas: la ventana de ajustes (`Ctrl+,`) tiene secciones, con la lista a la izquierda ([docs/09](09-ui-ux.md#ajustes-de-la-aplicación)).
    - General: idioma, abrir el último espacio de trabajo (si no, el de por defecto) y preguntar al cerrar con pestañas sin guardar (guardar todo y cerrar, cerrar sin guardar o cancelar).
    - Apariencia: tema, color de acento (seis, cada uno con su tono claro y oscuro AA, comprobado por un test), escala y densidad.
    - Editor: fuente del código (un archivo TTF u OTF) y su tamaño, lo que inserta Tab (tabulador o 2, 4, 8 espacios), ajuste de línea (si no, desplazamiento horizontal) y números de línea; y el límite de respuesta formateada.
    - Red: tiempo de espera por defecto (`SendOptions::default_timeout_ms`, que sustituye solo al valor por defecto de `resolve`), proxy, TLS y cookies.
    - Atajos: abre el editor que ya existía.
    - Datos: historial, registro y las carpetas de datos, configuración y registro, con un botón que las abre; el espacio que ocupan los datos.
    - Todo sigue en `settings.yml`, legible y editable a mano.
  - [x] Menú de la aplicación: un botón de menú dibujado en la barra superior, al estilo de GNOME, con Ajustes, Atajos de teclado, Importar…, Acerca de y Salir (`Ctrl+Q`, que pasa por la confirmación de cambios sin guardar). Acerca de y Salir son también acciones de la paleta ([docs/09](09-ui-ux.md#menú-de-la-aplicación-y-acerca-de)).
    - [ ] La barra de menú nativa de macOS, cuando se pueda probar allí.
  - [x] Ventana "Acerca de":
    - El icono, el nombre, la versión con su commit, la licencia (MIT o Apache-2.0) y las de las 512 dependencias. Las genera el script de compilación con `cargo metadata` para la plataforma de destino.
    - El sitio, el enlace para reportar errores, y las carpetas de datos y de ajustes.
    - Un botón que copia la versión, el sistema y los gráficos (backend, adaptador y driver) para un reporte de error.
  - [x] Barra de título propia, con minimizar, maximizar y cerrar siempre, aunque GNOME no los pida ([docs/09](09-ui-ux.md#temas-y-accesibilidad)):
    - La ventana va sin decoraciones del sistema y la barra la dibuja la interfaz (`chrome`): arrastrar mueve la ventana, el doble clic maximiza, y los bordes y las esquinas la redimensionan.
    - Lleva el buscador ("Search…", que abre la paleta), el entorno y el menú.
    - Un ajuste (`own_title_bar`) vuelve a la barra del sistema en el momento; se lee antes de abrir la ventana.
    - En Windows y macOS se mantiene la nativa (`chrome::SUPPORTED`), hasta probar allí.
    - [x] Esquinas redondeadas y sombra con la barra propia: la ventana es transparente y apifast dibuja su marco, con un margen para la sombra que también redimensiona. Si el driver no admite transparencia, el registro dice cómo volver a la barra del sistema.
  - [x] Mejoras visuales ([docs/09](09-ui-ux.md#temas-y-accesibilidad)):
    - Campos de texto a la altura de los botones (26 puntos con la densidad normal), y la × de pestañas dibujada.
    - Iconos dibujados en lugar de texto en la barra lateral y la del árbol.
    - Espaciado y tipografía desde los tokens del tema; Send como botón principal, con el acento.
    - Estados vacíos con una indicación y botones con su atajo.
    - Transiciones: la barra lateral se desliza; las ventanas usan el fundido de egui.
    - Revisado en los dos temas con capturas sin ventana (`egui_kittest` con `wgpu`), que encontraron el método mal alineado en el árbol y botones cortados en la barra lateral.
    - [ ] La × de las ventanas es la de egui: cambiarla pide una barra de título propia para cada ventana.
    - [x] Tablas clave/valor (params, headers, campos de formulario, path params) al estilo de Bruno ([docs/09](09-ui-ux.md#tablas-clavevalor)):
      - Un solo marco con líneas finas entre filas y columnas, campos sin borde propio, la fila resaltada al pasar el ratón y la celda con el foco marcada con el acento.
      - Fila de encabezado con «Name» y «Value» en texto tenue.
      - Nombre y valor en monoespaciada, con las variables resaltadas como antes.
      - Papelera dibujada en lugar de la ×, visible al pasar por la fila; filas desactivadas atenuadas.
      - Las capturas (`tests/gui/snapshots.rs`) cubren params, headers y una fila con el ratón encima y una celda con el foco.
    - [x] El editor del Body ocupa todo el alto disponible, con números de línea activados por defecto. Quien ya guardó `line_numbers: false` en sus ajustes lo conserva.
  - [ ] Historial de respuestas al estilo del HTTP Client de IntelliJ:
    - Cada entrada guarda también lo enviado: método, URL final, headers y body ya resueltos (el body hasta un límite, en el almacén de respuestas), con los secretos enmascarados como en la timeline. Migración del formato sin perder las entradas existentes.
    - En el panel de respuesta, un botón «History (n)» con las respuestas anteriores de esa request (fecha, estado, tiempo y nombre), para abrirlas, renombrarlas o borrarlas. Sustituye al «Selector de respuestas anteriores» que quedó fuera de F2.
    - Una entrada abierta muestra las pestañas de siempre y una pestaña Request con lo enviado, para copiarlo o reenviarlo tal cual.
    - Renombrar una entrada la fija: la limpieza automática no la borra.
    - En el historial lateral, renombrar y borrar desde el menú contextual, y borrar todo tras confirmar.
  - [ ] Disposiciones editables (layouts). Se llaman así para no confundirlas con los workspaces de `workspace.yml`, que agrupan colecciones. Sustituyen a la disposición lado a lado con `Ctrl+J` que quedó fuera de F2.
    - [ ] Paso 0, decisión (ADR):
      - Elegir entre `egui_tiles`, `egui_dock` o paneles propios, según cuál tenga versión para egui 0.37, su licencia (cargo-deny) y si deja sacar un panel a otra ventana.
      - Medir en un prototipo el coste en memoria y por frame de cada ventana extra, contra los presupuestos de [rendimiento](10-rendimiento-y-memoria.md).
    - [ ] Paso 1, paneles acoplables:
      - Request, respuesta, historial y árbol pasan a ser paneles con pestaña: se arrastran para ponerlos al lado, debajo o como pestaña de otro, y se redimensionan con los divisores.
      - La disposición predeterminada es la de Bruno: request a la izquierda y respuesta a la derecha, del mismo tamaño.
      - `Ctrl+J` alterna entre lado a lado y uno encima del otro.
      - El layout de `layout::workbench` y las esquinas redondeadas de la barra propia siguen funcionando.
    - [ ] Paso 2, guardar y recargar:
      - La disposición (qué panel va dónde, sus tamaños y las ventanas sacadas) se guarda serializada en la configuración personal (`layouts.yml`), no en la colección.
      - Varias disposiciones con nombre: guardar como, cambiar, renombrar y borrar desde la paleta y el menú de la aplicación. Al arrancar se restaura la última.
      - Un archivo de otra versión que no se entiende se ignora con un aviso y se usa la predeterminada.
      - Hay siempre una acción para volver a la predeterminada.
    - [ ] Paso 3, sacar un panel a otra ventana:
      - Un botón «Sacar a ventana» en la pestaña lo abre en una ventana del sistema propia (viewport de egui). Al cerrarla vuelve a su sitio.
      - Se recuerda su tamaño. En Wayland no se puede recordar la posición, porque la elige el compositor.
      - Esas ventanas llevan la barra del sistema; dibujar la propia en ellas queda para después.
      - Los atajos funcionan también con el foco en la ventana sacada.
    - [ ] Pruebas:
      - Mover, partir y cerrar paneles, y guardar y restaurar el estado, con `egui_kittest`.
      - Capturas en los dos temas.
      - Medición de memoria con una y con dos ventanas sacadas.
    - [ ] Documentar en [docs/09](09-ui-ux.md): sección de disposiciones y atajos.

### Criterio de salida

Todos los presupuestos de [rendimiento](10-rendimiento-y-memoria.md) se cumplen en las tres plataformas, y no quedan errores críticos abiertos.

---

## Después de la 1.0

Ideas sin compromiso:

- Extensiones mediante WebAssembly (nunca un proceso auxiliar permanente).
- Integración con gestores de secretos externos (Vault, 1Password, gestores de nube).
- Passphrase como alternativa al archivo `0600` cuando no hay keyring.
- HTTP/3.
- Servidor MCP para que asistentes de IA usen las colecciones.
- Pruebas de carga básicas.
- Servidor de mocks a partir de los ejemplos guardados.

---

## Riesgos principales

| Riesgo | Impacto | Fase | Mitigación |
|---|---|---|---|
| Escribir en bodies grandes es lento en egui (41 ms por tecla con 1 MB) | Medio | F2 | Límite de 200 KB para la edición con resaltado; visor de solo lectura para lo demás |
| Sin GPU, egui usa unos 98 MB y mucha CPU | Bajo | F2, F10 | Aceptado; se revisa si el uso sin GPU pasa a ser requisito ([ADR 0004](adr/0004-egui-como-ui.md)) |
| `oneyaml` es reciente y puede tener errores o quedar sin mantenimiento | Medio | F1 | Aislada en `apifast-storage-fs`; corpus de round-trip; plan alternativo con `serde-saphyr` |
| Bruno renombra entornos o variables y deja secretos compartidos huérfanos | Bajo | F3 | Detección y reasignación en el editor de entornos |
| OpenCollection cambia de forma incompatible | Medio | Todas | Fijar la versión soportada y añadir migraciones |
| El keyring no está disponible en algunos entornos Linux | Medio | F3 | Respaldo en archivo `0600`, señalado en la interfaz |
| OAuth 2.0 tiene demasiadas variantes | Medio | F5 | Empezar por los tres flujos más usados |
| gRPC aumenta mucho el binario | Medio | F8 | Feature opcional |
| El alcance crece antes del MVP | Alto | F1–F3 | El MVP es F1 + F2 + F3; lo demás espera |

## Cómo se actualiza este documento

- Las casillas se marcan al completar cada tarea.
- Un cambio de alcance o de orden se anota aquí con su motivo.
- Los resultados de los spikes se enlazan desde la fase F0.
