# Spikes 2 y 3: interfaz, egui frente a iced

- **Fecha:** 2026-11-14
- **Resultado:** ninguno de los dos cumple los criterios del [ADR 0004](../adr/0004-egui-como-ui.md) con un body de 1 MB, y fallan por motivos distintos. Con 100 KB los dos son fluidos.
- **Corrección importante:** la RAM (RSS) exagera el costo de la GPU, porque cuenta unos 45 MB de bibliotecas del driver de Mesa compartidas con el resto del sistema. Medida como memoria proporcional (PSS), **egui con wgpu usa 47 MB con datos de 100 KB, menos que iced con renderer por software (55 MB)**, y queda dentro del presupuesto de 60 MB. Ver [Memoria real](#memoria-real-rss-frente-a-pss).
- **Código:** `spikes/ui-spike/` (desechable). Se reproduce con:

```
cd spikes/ui-spike
cargo build --release
N=3 ./run.sh                                          # body de 1 MB
SPIKE_JSON_KB=100 N=2 OUT=results-100kb.jsonl ./run.sh  # body de 100 KB
python3 -I summarize.py results.jsonl
```

## Qué se midió

Se construyó el mismo prototipo en los dos toolkits:

- un árbol de 5000 filas, virtualizado (solo se dibujan las visibles);
- un editor con un JSON generado, resaltado de sintaxis y resaltado propio de `{{variables}}` (el mismo tokenizador en ambos);
- una ventana de 1280 × 800.

Cada prototipo ejecuta solo un guion idéntico y luego se cierra:

1. 1,5 s de espera tras arrancar.
2. 200 fotogramas escribiendo un carácter por fotograma en mitad del texto.
3. 200 fotogramas desplazando el árbol.
4. 200 fotogramas desplazando el editor.
5. 5 s sin hacer nada.

Mide RAM (RSS), primer fotograma, intervalos entre fotogramas y CPU consumida por el proceso.

| | Versión | Renderers probados |
|---|---|---|
| egui (eframe) | 0.37.0 | wgpu, glow (OpenGL) |
| iced | 0.14.0 | wgpu, tiny-skia (por software, sin GPU) |

**Máquina:** Fedora 44, sesión Wayland, AMD Radeon 780M integrada, 16 hilos, 11 GB de RAM. Compilación release con LTO. El compositor limita a 60 fps, así que 16,7 ms por fotograma es lo máximo medible; la CPU por fotograma muestra el costo real.

Valores: mediana de 3 ejecuciones (1 MB) y de 2 (100 KB). La variación entre ejecuciones fue menor al 5 %.

## Memoria y arranque

| Toolkit | Renderer | Ventana vacía | Con datos (100 KB) | Con datos (1 MB) | Primer fotograma (100 KB / 1 MB) |
|---|---|---|---|---|---|
| egui | wgpu | 82 MB | 94 MB | 182 MB | 72 / 74 ms |
| egui | glow | 88 MB | 99 MB | 188 MB | 46 / 48 ms |
| iced | wgpu | 77 MB | 117 MB | 443 MB | 200 / 1153 ms |
| iced | tiny-skia | **24 MB** | **64 MB** | 389 MB | 144 / 1089 ms |

- **Con GPU, ningún toolkit baja de unos 80 MB, ni con la ventana vacía.** El contexto gráfico fija ese piso. Coincide con el caso publicado que motivó este spike.
- **iced con tiny-skia es la única opción cerca del presupuesto:** 24 MB vacía y 64 MB con datos.
- **El editor de iced es caro en memoria:** unos 40 MB por cada 100 KB de texto, y unos 360 MB con 1 MB. El de egui cuesta unos 10 MB por cada 100 KB.
- **egui carga más rápido:** el editor de iced prepara todo el texto al abrirlo y tarda más de 1 s con 1 MB.

## Memoria real: RSS frente a PSS

RSS cuenta enteras las bibliotecas compartidas, aunque otros procesos (el compositor, el navegador) ya las tengan cargadas. Se repitió la medición leyendo `/proc/<pid>/smaps`:

- **PSS:** reparte cada página compartida entre los procesos que la usan.
- **Privada:** memoria que solo existe por este proceso; es lo que se liberaría al cerrarlo.

Ventana vacía, desglosada:

| Toolkit | Renderer | RSS | PSS | Privada | Driver de GPU (RSS / privada) |
|---|---|---|---|---|---|
| egui | wgpu | 82 MB | 35 MB | 29 MB | 44 / 5 MB |
| egui | glow (OpenGL) | 88 MB | 43 MB | 38 MB | 50 / 9 MB |
| iced | wgpu | 78 MB | 31 MB | 26 MB | 44 / 5 MB |
| iced | tiny-skia | 25 MB | 16 MB | 12 MB | — |
| foot (terminal nativa, referencia) | Software | 15 MB | 5 MB | 4 MB | — |

Con datos:

| Toolkit | Renderer | PSS con 100 KB | Privada con 100 KB | PSS con 1 MB | Privada con 1 MB |
|---|---|---|---|---|---|
| egui | wgpu | **47 MB** | **41 MB** | **136 MB** | **129 MB** |
| egui | glow | 54 MB | 49 MB | 143 MB | 138 MB |
| iced | wgpu | 70 MB | 64 MB | 395 MB | 389 MB |
| iced | tiny-skia | 55 MB | 50 MB | 380 MB | 376 MB |

- **La GPU cuesta unos 13–17 MB privados**, no los ~55 MB que sugería RSS.
- **OpenGL directo (glow) no ahorra memoria:** usa unos 8 MB privados más que wgpu. Las bibliotecas del driver son las mismas.
- **La ventaja de tiny-skia solo se ve con la ventana vacía.** En cuanto hay un body cargado, el editor de iced (unos 40 MB por cada 100 KB) pesa más que el contexto gráfico de egui.

### Sobre Alacritty y OpenGL

Alacritty consume poco porque tiene un renderer mínimo hecho para un solo propósito: un atlas de glifos y rectángulos. No es por usar OpenGL. Las bibliotecas del driver serían las mismas que carga egui con glow, que aquí no mejoró a wgpu. Escribir un renderer OpenGL propio sería escribir un toolkit de interfaz, con poca ganancia de memoria. (Alacritty no está instalado en esta máquina; no se midió.)

## Sin GPU

Para simular un PC sin GPU (máquina virtual sin aceleración 3D, escritorio remoto, driver ausente) se forzaron los drivers por software de Mesa: llvmpipe para OpenGL (`LIBGL_ALWAYS_SOFTWARE=1`) y lavapipe para Vulkan. Se comprobó en `/proc/<pid>/maps` que se cargaba el driver forzado. Body de 100 KB, una ejecución por caso:

| Toolkit | Driver | PSS | Privada | Primer fotograma | CPU por fotograma al escribir | CPU por fotograma al desplazar |
|---|---|---|---|---|---|---|
| egui + glow | llvmpipe | 93 MB | 86 MB | 47 ms | 30 ms | 27 ms |
| egui + wgpu | lavapipe | 98 MB | 87 MB | 56 ms | 32 ms | 28–29 ms |
| iced + wgpu | lavapipe | 113 MB | 103 MB | 190 ms | 14 ms | 13–14 ms |
| iced + tiny-skia | (no usa GPU) | **54 MB** | **50 MB** | 150 ms | **4,5 ms** | **4–6 ms** |

- **Las cuatro funcionan y mantienen 60 fps en esta máquina**, porque llvmpipe y lavapipe reparten el trabajo entre 16 hilos. En un equipo de 2 a 4 núcleos, egui sin GPU bajaría de 60 fps al desplazarse.
- **Sin GPU, egui usa casi el doble de memoria y unas 6 veces más CPU** que iced con tiny-skia. Los drivers por software emulan la GPU completa (incluido un compilador LLVM), lo que cuesta más que un renderer 2D por software.
- **egui no tiene alternativa:** depende de que el sistema ofrezca un driver por software (Mesa en Linux, WARP en Windows). iced puede elegir tiny-skia, pero su selección automática prueba wgpu primero y lavapipe "funciona", así que habría que detectar un adaptador por software y forzar tiny-skia.

## Fluidez

| Toolkit | Renderer | Escribir, 100 KB: CPU por tecla | Escribir, 1 MB: CPU por tecla | Escribir, 1 MB: p95 | Desplazar: CPU por fotograma |
|---|---|---|---|---|---|
| egui | wgpu | 4,6 ms | 41 ms | 44 ms (~23 fps) | 0,8–2 ms |
| egui | glow | 4,9 ms | 41 ms | 44 ms (~23 fps) | 0,8–1,9 ms |
| iced | wgpu | 0,9 ms | 1,0 ms | 17,7 ms (60 fps) | 0,9–1,3 ms |
| iced | tiny-skia | 4,4 ms | 4,5 ms | 17,5 ms (60 fps) | 4,4–6,2 ms |

- **egui recalcula y resalta el texto entero en cada tecla**, así que el costo crece con el tamaño: fluido con 100 KB, unos 23 fps con 1 MB.
- **iced solo vuelve a resaltar la línea que cambia**: el costo de escribir no depende del tamaño.
- **Desplazar el árbol y el editor va a 60 fps en todos los casos.**
- **Con tiny-skia cada fotograma cuesta unos 4–6 ms de CPU** mientras hay movimiento. A 1280 × 800 sobra margen; en pantallas HiDPI grandes subirá, y no se midió.
- **En reposo, los dos quedan a 0 % de CPU** y no dibujan nada.

## Tamaño del binario

Release con LTO y `strip`, cada uno con sus dos renderers: egui 13,8 MB, iced 10,8 MB (incluye tokio).

## Criterios del ADR 0004 con 1 MB

| Criterio | egui | iced |
|---|---|---|
| Escribir sin retraso (< 16 ms por fotograma) | **No** (41 ms) | Sí (1 ms) |
| Resaltado de sintaxis y de variables | Sí | Sí |
| Memoria extra < 20 MB | **No** (+100 MB) | **No** (+365 MB) |

Ninguno cumple con 1 MB. Con 100 KB, los dos cumplen fluidez y resaltado; en memoria, egui suma 12 MB e iced unos 40 MB.

## Conclusiones

1. **El presupuesto de memoria debe medirse en PSS, no en RSS.** RSS penaliza a cualquier programa con GPU por bibliotecas que el sistema ya tiene cargadas.
2. **Medido en PSS, egui con wgpu cumple el presupuesto** con datos realistas (47 MB con 100 KB) y es la opción más ligera en cuanto hay contenido.
3. **El tamaño del body editable decide más que el toolkit.** Ninguno edita 1 MB con resaltado de forma aceptable. El diseño ya preveía un límite de edición con resaltado y un visor propio por ventanas para respuestas grandes ([rendimiento](../10-rendimiento-y-memoria.md)); este spike lo confirma como necesario con cualquiera de los dos.
4. **El punto débil de egui es escribir en textos grandes** (41 ms por tecla con 1 MB, porque recalcula todo el texto). Con un límite de unos 200 KB para la edición con resaltado se mantiene fluido.
5. **El punto débil de iced es la memoria de su editor** (unos 40 MB por cada 100 KB) y la carga inicial (más de 1 s con 1 MB).
6. **egui es más rápido de desarrollar y carga antes** (72 ms frente a 144–200 ms con 100 KB).

## Decisión

El responsable del proyecto eligió **egui con wgpu**, por ser el más ligero con datos reales, el que carga más rápido y el más ampliable: permite dibujar con wgpu propio dentro de la interfaz (`PaintCallback`) y renderizar a textura. Se acepta el mayor costo sin GPU. Detalle y reglas derivadas en el [ADR 0004](../adr/0004-egui-como-ui.md).

## Lo que no se midió

- Pantallas HiDPI o 4K con tiny-skia.
- Otras GPU, X11, macOS o Windows.
- Accesibilidad y métodos de entrada.
- Un renderer por software de terceros para egui.
- El costo de desarrollo real de las pantallas completas (tablas, pestañas, paleta de comandos) en cada toolkit.
