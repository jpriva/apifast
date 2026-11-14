# ADR 0004: Interfaz con egui y wgpu

- **Estado:** aceptado, confirmado por los [spikes 2 y 3](../spikes/02-interfaz-egui-vs-iced.md)
- **Fecha:** 2026-11-14

## Contexto

Se busca una interfaz nativa en Rust con bajo consumo de memoria. Bruno usa Electron y Yaak un webview; en ambos casos el motor web domina el consumo.

Antes de decidir se compararon egui e iced con el mismo prototipo (árbol de 5000 filas y editor JSON con resaltado), con cuatro renderers, con y sin GPU.

## Decisión

**egui, mediante eframe, con el renderer wgpu**, en modo reactivo (repintado solo ante eventos).

Reglas derivadas del spike:

- **El presupuesto de memoria se mide en PSS**, no en RSS. RSS cuenta enteras las bibliotecas del driver de GPU que el sistema ya tiene cargadas y exagera el costo en unos 45 MB.
- **La edición con resaltado se limita a unos 200 KB.** Por encima, el body se edita como texto plano o se abre en el visor de solo lectura.
- **Las respuestas grandes se muestran en un visor propio** que dibuja solo las líneas visibles, leyendo el archivo por ventanas.
- **Para componentes donde egui no rinda**, se dibuja con wgpu directamente dentro de egui mediante `PaintCallback` (`egui_wgpu::CallbackTrait`) o se renderiza a una textura. Es la vía prevista para el visor de respuestas si el texto de egui no basta.

## Datos que respaldan la decisión

Memoria proporcional (PSS), árbol de 5000 filas y body de 100 KB:

| Opción | Ventana vacía | Con datos | Primer fotograma | CPU por tecla |
|---|---|---|---|---|
| **egui + wgpu** | 35 MB | **47 MB** | **72 ms** | 4,6 ms |
| egui + glow | 43 MB | 54 MB | 46 ms | 4,9 ms |
| iced + wgpu | 31 MB | 70 MB | 200 ms | 0,9 ms |
| iced + tiny-skia | 16 MB | 55 MB | 144 ms | 4,4 ms |

Con datos reales, egui con wgpu es la opción más ligera y la que carga más rápido. glow (OpenGL directo con winit y glutin) no aporta: usa más memoria que wgpu con las mismas bibliotecas del driver.

## Consecuencias

**A favor**

- Un único binario Rust, sin motor web ni JavaScript.
- Mismo lenguaje y mismos tipos que el núcleo; sin frontera de serialización.
- Dentro del presupuesto de 60 MB con datos realistas, y arranque de unos 70 ms.
- El modelo de modo inmediato simplifica el estado de la interfaz.
- Ampliable a bajo nivel: dibujo wgpu propio dentro de la interfaz y render a textura.
- Multiplataforma; wgpu usa Vulkan, Metal o DirectX 12 según el sistema.

**En contra**

- **Escribir en textos grandes es lento.** egui recalcula y resalta el texto entero en cada tecla: 4,6 ms con 100 KB, 41 ms con 1 MB. Se resuelve con el límite de 200 KB.
- **Sin GPU es pesado.** egui no tiene renderer por software. En máquinas virtuales sin aceleración, escritorio remoto o equipos sin driver, depende del driver por software del sistema (Mesa en Linux, WARP en Windows): unos 98 MB y unas 6 veces más CPU por fotograma. Funciona, pero en equipos de pocos núcleos puede bajar de 60 fps. Se acepta porque no son entornos prioritarios.
- El modo inmediato redibuja todo en cada repintado: hay que virtualizar listas y guardar en caché el layout del texto.
- Aspecto menos "nativo" que el de las herramientas de cada plataforma.
- Accesibilidad menos madura que en la web, aunque integra AccessKit.
- Menos componentes listos para usar: tablas, pestañas arrastrables y paleta de comandos se construyen a medida.
- El código que use `PaintCallback` queda atado a wgpu.

## Alternativas descartadas

- **iced:** fue la alternativa medida. Su editor no se degrada al escribir en textos grandes y, con su renderer por software, es la mejor opción sin GPU. Se descarta porque con datos reales usa más memoria (su editor cuesta unos 40 MB por cada 100 KB de texto), carga más lento, no tiene versión 1.0 y cambia su API entre versiones.
- **Renderer propio con OpenGL, winit y glutin (como Alacritty):** el backend glow de egui ya es esa combinación, y no mejoró a wgpu. Un renderer propio ahorraría como mucho 10–15 MB a cambio de construir un toolkit de interfaz completo.
- **egui con glow:** usa más memoria que wgpu y no admite el dibujo wgpu propio.
- **Tauri o Electron:** contradicen el objetivo de bajo consumo.
- **Slint:** su `TextEdit` no admite resaltado de sintaxis, y su licencia (GPLv3 o gratuita con atribución obligatoria) no encaja con publicar apifast como MIT OR Apache-2.0.
- **Floem:** sigue antes de la 1.0, sin versiones publicadas a la vista y con poca adopción fuera de Lapce.
- **GTK o Qt:** dependencias nativas pesadas y empaquetado más complejo.

## Cuándo revisar esta decisión

- Si el soporte sin GPU pasa a ser un requisito (por ejemplo, uso habitual en escritorio remoto).
- Si el visor o el editor no alcanzan los presupuestos ni siquiera con dibujo wgpu propio.

La arquitectura hexagonal limita el costo de un cambio de toolkit al crate `apifast-gui`.
