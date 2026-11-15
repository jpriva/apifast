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
│ ☰  Mi API ▾                    [ Buscar / comandos  Ctrl+K ]    ● produccion ▾│
├───────────────────┬──────────────────────────────────────────────────────────┤
│ COLECCIONES       │  login ×   crear usuario ●   listar ×          +         │
│ ▾ Mi API          ├──────────────────────────────────────────────────────────┤
│   ▾ auth          │ [POST ▾] {{host}}/v1/users/:orgId              [ Enviar ] │
│      POST login   ├──────────────────────────────────────────────────────────┤
│      POST refresh │ Params  Headers(2)  Body  Auth  Vars  Tests  Docs  Ajustes│
│   ▾ usuarios      │ ┌──────────────────────────────────────────────────────┐ │
│      GET  listar  │ │ {                                                    │ │
│      POST crear   │ │   "name": "{{$randomFullName}}",                     │ │
│                   │ │   "email": "{{user_email}}"                          │ │
│ ENTORNOS          │ │ }                                                    │ │
│   local           │ └──────────────────────────────────────────────────────┘ │
│ ● produccion      ├──────────────────────────────────────────────────────────┤
│                   │ 201 Created    182 ms    1.2 kB        Tests 2/2 ✓       │
│ HISTORIAL         │ Body  Headers(8)  Cookies  Timeline  Tests               │
│                   │ ┌──────────────────────────────────────────────────────┐ │
│                   │ │ { "id": 981, "name": "Ana Ruiz" }                    │ │
│                   │ └──────────────────────────────────────────────────────┘ │
├───────────────────┴──────────────────────────────────────────────────────────┤
│ main ↑1  ·  2 sin guardar  ·  bóveda desbloqueada                            │
└──────────────────────────────────────────────────────────────────────────────┘
```

| Zona | Contenido |
|---|---|
| Barra superior | Selector de colección, búsqueda y paleta de comandos, selector de entorno con su color |
| Barra lateral | Árbol de la colección, entornos, historial. Se puede ocultar y redimensionar |
| Pestañas | Una por request abierta; un punto indica cambios sin guardar |
| Barra de URL | Método, URL con resaltado de variables, botón de enviar o cancelar |
| Panel de request | Pestañas de edición |
| Panel de respuesta | Estado, tiempo, tamaño y pestañas de visualización |
| Barra de estado | Rama de git, cambios sin guardar, estado de la bóveda |

Los paneles de request y respuesta se pueden disponer uno encima del otro o lado a lado (`Ctrl+J`).

## Barra lateral

- Árbol con carpetas plegables. Solo se dibujan las filas visibles.
- Arrastrar y soltar para reordenar (actualiza `seq`) y mover entre carpetas (mueve el archivo).
- Menú contextual: nueva request, nueva carpeta, duplicar, renombrar, borrar, ejecutar carpeta, mostrar en el explorador de archivos, copiar como cURL.
- Filtro rápido por nombre, método o tag (`Ctrl+F`).
- Los archivos con error de parseo aparecen marcados y se pueden abrir como texto.

## Panel de request

| Pestaña | Contenido |
|---|---|
| Params | Tabla de parámetros de query y de ruta, sincronizada con la URL |
| Headers | Tabla clave/valor; los heredados aparecen atenuados con su origen |
| Body | Selector de tipo y editor: código, tabla de formulario o selector de archivo |
| Auth | Selector de tipo y sus campos; "heredada" muestra de quién |
| Vars | Variables de la request y capturas |
| Tests | Tabla de aserciones: expresión, operador, valor |
| Docs | Markdown con vista previa |
| Ajustes | Timeout, redirecciones, TLS; cada uno con "heredar" |

### Tablas clave/valor

- Casilla para habilitar o deshabilitar cada fila.
- Siempre hay una fila vacía al final para añadir.
- Modo de edición masiva: la tabla como texto, una línea por entrada con el formato `nombre: valor`; las que empiezan por `//` se conservan sin enviarse.
- Autocompletado de nombres de header y de variables.

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
| Cookies | Cookies fijadas por la respuesta |
| Timeline | Eventos del envío con sus tiempos |
| Tests | Aserciones con valor esperado y real |

- Los cuerpos grandes se muestran por ventanas; el formateo se desactiva por encima de un umbral (2 MB) y se ofrece verlos en crudo o guardarlos. La vista "Pretty" formatea y colorea JSON y XML; si no se pueden formatear (HTML con `<br>`, JSON roto), se muestran tal cual, coloreados y con un aviso. Los bodies binarios no se dibujan: se indica su tamaño.
- El Timeline básico lista, con su tiempo desde el inicio, cada request que sale, cada redirección seguida, la llegada de los headers y el final del body. Las fases de DNS, conexión y TLS llegarán cuando el transporte las exponga.
- Acciones: copiar, guardar a archivo, guardar como ejemplo, "guardar como variable" desde un valor seleccionado.
- Selector de respuestas anteriores de la misma request.

## Entornos y variables

- El selector de entorno está siempre visible y muestra el color del entorno. Un entorno marcado como "de cuidado" (por ejemplo producción) tiñe la barra superior (pendiente: OpenCollection no tiene esa marca; se decidirá dónde guardarla).
- Los cambios del editor de entornos se acumulan en una copia y se escriben con Save. Así una tabla a medio editar no cambia lo que usa un envío en curso.
- Editor de entornos (`Ctrl+E`): lista de entornos a la izquierda y tabla de variables a la derecha.
- Columnas de la tabla: habilitada, nombre, valor, tipo y clase de secreto (normal, solo local, compartido).
- Marcar una variable como secreta la hace **solo local**. Compartirla con el equipo es una acción aparte, con una explicación breve de lo que implica.
- Vista "todas las variables": cada variable efectiva con su valor y el ámbito del que procede.

## Bóveda

- Al abrir una colección con secretos compartidos y sin clave local, un aviso no bloqueante ofrece introducir la clave.
- Diálogo "Compartir clave": muestra la clave legible, con botón de copiar y advertencia sobre cómo compartirla.
- Los campos secretos tienen un botón de revelar que se vuelve a ocultar solo.
- Indicador del estado de la bóveda en la barra de estado, incluyendo si la clave maestra está en el keyring o en el archivo de respaldo.

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

En macOS, `Ctrl` se sustituye por `Cmd`. Todos los atajos se pueden reasignar; se guardan en `keymap.yml`.

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

- Tema claro y oscuro, siguiendo el del sistema por defecto.
- Escala de interfaz y tamaño de fuente configurables.
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
