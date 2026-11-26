# ADR 0015: Las disposiciones editables usan egui_tiles, y las ventanas sacadas son viewports de egui

- **Estado:** aceptado
- **Fecha:** 2026-11-25

## Contexto

F10 pide **disposiciones editables** ([docs/12](../12-roadmap.md), "Experiencia de escritorio"). Se llaman así para no confundirlas con los workspaces de `workspace.yml`, que agrupan colecciones. Hoy la request va arriba y la respuesta abajo, en un orden fijo (`screens::main_screen::editor_area`). Lo que se quiere:

- Paneles con pestaña (request, respuesta, historial, árbol) que se arrastran para ponerlos al lado, debajo o como pestaña de otro, y se redimensionan con los divisores.
- Por defecto, la disposición de Bruno: request a la izquierda y respuesta a la derecha, del mismo tamaño.
- Guardar varias disposiciones con nombre y recargarlas.
- Sacar un panel a su propia ventana del sistema y devolverlo a su sitio al cerrarla.

Hay tres caminos:

- **`egui_dock` 0.22** (MIT, para egui 0.37). Pestañas con barra propia, arrastrar y soltar, divisiones en dos y estado serializable con la feature `serde`.
  - Puede arrastrar una pestaña fuera, pero a una ventana de egui dentro de la principal, no a una ventana del sistema.
  - Depende de `paste`, archivado y marcado como sin mantenimiento (RUSTSEC-2024-0436), y de `duplicate`.
- **`egui_tiles` 0.18** (MIT OR Apache-2.0, la licencia de apifast; para egui 0.37).
  - Es de Rerun, la empresa que mantiene egui, y sale al ritmo de egui.
  - Ofrece mosaicos con pestañas, filas, columnas y rejilla, y divide en más de dos.
  - El árbol (`Tree<Pane>`) es serializable con `serde`, activo por defecto.
  - Todo el dibujo y el comportamiento pasan por un trait, `Behavior`: la barra de pestañas, la × y los colores, y qué se puede arrastrar o cerrar.
  - `Tree::set_visible` oculta un mosaico sin quitarlo del árbol.
  - Depende de `ahash`, `itertools`, `log` y `serde`, todas MIT o Apache.
- **Paneles propios**, con los `Panel` de egui que ya se usan: sin dependencias nuevas, pero con divisiones fijas. Arrastrar pestañas entre zonas, partir en cualquier dirección y serializar el resultado habría que escribirlo entero.

Ninguna de las dos librerías saca un panel a una **ventana del sistema**. Eso lo hace egui con sus viewports: `Context::show_viewport_immediate` abre una ventana nativa que se dibuja en el mismo frame y con el mismo estado que la principal. winit la crea en Wayland y en X11.

### Lo medido

`cargo run --release -p apifast-gui --example bench -- viewports` mide lo que cuesta cada ventana extra. Abre una y luego dos ventanas del sistema junto a la principal, cada una con un panel de texto, y redibuja sin parar. Tras 6 s, mide la memoria propia de apifast y la CPU por frame. Los resultados son de tres ejecuciones, en la máquina de [docs/10](../10-rendimiento-y-memoria.md#mediciones-de-f10) (Ryzen 7 8700G, Radeon 780M, Mesa, Wayland):

| Ventanas extra | Memoria propia (PSS) | CPU por frame, redibujando sin parar |
|---|---|---|
| 0 | 32,7–34,3 MB | 0,66 ms |
| 1 | +2,4 MB | 0,94 ms |
| 2 | +3,5 a +4,8 MB | 1,2 ms |

Una ventana sacada cuesta unos 2,4 MB: su superficie gráfica y sus búferes. Cada ventana extra añade unos 0,27 ms de CPU, pero solo en los frames que se dibujan. apifast redibuja cuando algo cambia, así que en reposo no cuesta nada. Con la medida en reposo de docs/10 en F10 (30–31 MB con 100 requests), dos ventanas sacadas dejan apifast en unos 35 MB, lejos del presupuesto de 60 MB.

## Decisión

1. **`egui_tiles`** organiza el área de trabajo:
   - Cada parte es un `Pane` del árbol: la request, la respuesta, el historial y el árbol de la colección.
   - Un `Behavior` propio dibuja la barra de pestañas con los átomos y el tema de apifast: la × dibujada, el acento y las alturas de la densidad.
   - Decide también qué se puede cerrar. Ningún panel se pierde: cerrar uno lo oculta, y "Restablecer disposición" lo trae de vuelta.
2. **Las disposiciones se guardan serializando el árbol** de `egui_tiles` con `serde`, en `layouts.yml`, en la carpeta de configuración, junto a `settings.yml`. Es una preferencia de cada usuario, no de la colección. Un archivo que no se entiende, por ejemplo de otra versión, se ignora con un aviso y se usa la disposición predeterminada.
3. **Sacar un panel a su propia ventana** es cosa de apifast, no de la librería:
   - El panel se oculta del árbol con `Tree::set_visible(id, false)`, así conserva su sitio.
   - Se dibuja en un viewport inmediato (`show_viewport_immediate`), con el mismo estado y en el mismo frame que la ventana principal.
   - Al cerrar esa ventana, el panel vuelve a verse en su sitio.
   - La disposición guardada recuerda qué paneles estaban fuera y el tamaño de sus ventanas. La posición no, porque en Wayland la elige el compositor.
4. Los viewports son **inmediatos**, no diferidos: un viewport diferido necesita su propio estado compartido entre hilos (`Arc<Mutex>`), y la pantalla de apifast es una sola estructura. El coste es que cada ventana se redibuja cuando se redibuja la principal; está medido arriba.

## Consecuencias

**A favor**

- Arrastrar, partir en cualquier dirección y serializar vienen hechos y probados en Rerun.
- La licencia es la de apifast, y no entra ninguna dependencia sin mantener.
- El aspecto sigue siendo el de apifast, porque el `Behavior` dibuja las pestañas.
- Sacar paneles no depende de la librería: si un día se cambia, esa parte se queda.

**En contra**

- Una dependencia más, con `ahash` e `itertools`. Su peso en el binario se mide al añadirla en el paso 1; el presupuesto es de 60 MB y la GUI está en 36,2.
- El formato de `layouts.yml` es el de `serde` para el árbol de `egui_tiles`. Si un cambio de versión de la librería lo rompe, se pierden las disposiciones guardadas, pero no los datos.
- Una ventana sacada cuesta unos 2,4 MB y algo de CPU en cada frame que se dibuja.
- Las ventanas sacadas llevan la barra de título del sistema. Dibujar la propia de apifast en ellas queda para después.

## Alternativas descartadas

- **`egui_dock`:** su "sacar" es a una ventana de egui, no del sistema, así que no ahorra esa parte. Divide solo en dos. Arrastra `paste`, que no tiene mantenimiento.
- **Paneles propios:** habría que escribir el arrastre entre zonas, las divisiones libres y la serialización. Es justo lo que traen las librerías.
- **Viewports diferidos:** el estado de la pantalla tendría que pasar a estructuras compartidas entre hilos solo para ahorrar dibujo, y el dibujo de una ventana es barato (medido arriba).
