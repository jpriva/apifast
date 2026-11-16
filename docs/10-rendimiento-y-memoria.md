# 10. Rendimiento y memoria

El bajo consumo es un objetivo de producto, no una optimización tardía. Este documento fija los presupuestos, las técnicas y la forma de medir.

## Presupuestos

La memoria se mide en **PSS** (memoria proporcional, de `/proc/<pid>/smaps_rollup`), no en RSS. RSS cuenta enteras las bibliotecas del driver de GPU que el sistema ya tiene cargadas y exagera el consumo en unos 45 MB ([spike 3](spikes/02-interfaz-egui-vs-iced.md)).

Referencia medida en el spike, con egui y wgpu: 35 MB con la ventana vacía y 47 MB con un árbol de 5000 filas y un body de 100 KB.

| Métrica | Objetivo | Cómo se mide |
|---|---|---|
| RAM en reposo, colección de 100 requests | < 60 MB PSS | `/proc/<pid>/smaps_rollup` tras 10 s inactivo |
| RAM con colección de 5000 requests | < 120 MB PSS | Igual, con colección sintética |
| RAM extra al recibir 100 MB de respuesta | < 10 MB | Diferencia de PSS durante la descarga |
| CPU por tecla en un body de 200 KB | < 10 ms | Escenario automático como el del spike |
| Arranque en frío hasta primera ventana | < 300 ms | Marca de tiempo interna |
| Abrir colección de 1000 requests | < 200 ms hasta ver el árbol | Marca de tiempo interna |
| Sobrecarga de envío (sin red) | < 2 ms | Benchmark contra servidor local |
| CPU en reposo | ~0 % | Sin repintados sin eventos |
| Tamaño del binario (release) | < 25 MB | Tamaño en disco |

Como referencia, las aplicaciones sobre Electron o webview suelen consumir varios cientos de MB en reposo. Las cifras de Bruno y Yaak se medirán en F0 en la misma máquina para tener una comparación real.

## Técnicas

### Interfaz

- **Repintado por eventos.** egui se usa en modo reactivo: solo se dibuja ante entrada del usuario o cuando una tarea de fondo pide repintar. No hay bucle continuo.
- **Listas virtualizadas.** El árbol, las tablas y el timeline dibujan solo las filas visibles.
- **Caché de layout.** El resultado del resaltado de sintaxis se guarda y se invalida solo cuando cambia el texto.
- **Resaltado ligero.** Tokenizadores propios para JSON, XML y GraphQL en lugar de un motor genérico con gramáticas pesadas.
- **La interfaz no guarda el modelo.** Solo identificadores, borradores y la vista actual.
- **Renderer.** wgpu: usó menos memoria que glow en el spike y permite dibujo propio con `PaintCallback` para los componentes donde el texto de egui no rinda.

### Modelo en memoria

- **Colecciones bajo demanda.** Un workspace puede listar muchas colecciones, pero solo las desplegadas o con pestañas abiertas se cargan: árbol, watcher e historial. Una plegada cuesta una fila en el árbol ([ADR 0009](adr/0009-workspaces-y-varias-colecciones.md)).
- **Carga perezosa.** Al abrir una colección se leen solo los metadatos (`info`) de cada request. El contenido completo se carga al abrir la pestaña y se libera al cerrarla si no hay cambios.
- **Cadenas compartidas.** `Arc<str>` para textos repetidos (nombres de headers, métodos) y cadenas compactas para valores cortos.
- **Instantáneas inmutables.** El núcleo publica el estado como `Arc`; la interfaz lo lee sin copiar.
- **Sin clones de la colección.** Un envío recibe referencias a la request y a su cadena de padres, nunca una copia del árbol.

### Respuestas

- **Streaming a disco** con búfer fijo; la memoria no depende del tamaño de la respuesta.
- **Visor por ventanas.** El visor lee del archivo solo las filas visibles y las dibuja con el texto de egui. Lo guía un índice construido de forma incremental, 8 MB por frame, que guarda dónde empieza una fila de cada 64. Las filas intermedias se vuelven a encontrar en los bytes de la ventana. Las líneas de más de 2 KB se parten en filas, sin cortar caracteres. Medido en release con un body de 100 MB de líneas cortas (7,5 millones de filas):
  - indexado en 9 frames;
  - unos 3 MB de índice;
  - desplazarse cuesta 0,23 ms de CPU por frame en la mediana y 0,4 ms como máximo, sin contar teselación ni GPU.

  Se repite con `cargo test --release -p apifast-gui --test body_viewer -- --ignored --nocapture`. Límite conocido: el desplazamiento usa `f32`, así que por encima de unos 10 millones de filas el salto a una fila puede caer una fila antes o después.
- **Formateo con umbral.** Por encima de un tamaño configurable no se formatea ni se resalta automáticamente.
- **Límites del historial.** Tamaño total y antigüedad máximos; limpieza al arrancar y periódica.

### Red

- **Pool de conexiones activo** y clientes reutilizados por combinación de opciones.
- **Runtime pequeño.** Pocos hilos de trabajo; la mayoría de las tareas son de entrada/salida.
- **Cuerpos de request en streaming** para archivos y multipart.

### Funciones opcionales

- **Inicialización al primer uso** para gRPC y el motor JavaScript.
- **Features de Cargo** para compilar la CLI sin dependencias de la interfaz y, si se desea, sin gRPC.

### Arranque

- Mostrar la ventana antes de cargar la colección; el árbol aparece en cuanto están los metadatos.
- Restaurar pestañas en segundo plano.
- Ningún acceso a red ni al keyring durante el arranque; la bóveda se desbloquea al necesitarla.

### Sistema de archivos

- **Caché de metadatos** opcional (ruta, fecha de modificación, tamaño, metadatos) para colecciones muy grandes. Es desechable: borrarla no pierde nada.
- **Watcher con agrupación** de eventos para no reprocesar ráfagas (por ejemplo, un `git checkout`).
- **Parseo en paralelo** al abrir, con un número acotado de tareas.

## Mediciones de F2

Medido el 2026-11-16 con `tools/bench/` (ver su README), binario release, en un AMD Ryzen 7 8700G (Radeon 780M, Mesa, Vulkan), Fedora 44 con Wayland. La memoria es PSS tras 10 s inactivo; los tiempos van desde el inicio del proceso, con la caché de disco caliente.

| Métrica | Objetivo | Medido |
|---|---|---|
| RAM en reposo, 100 requests | < 60 MB PSS | 50 MB |
| RAM en reposo, 5000 requests | < 120 MB PSS | 55 MB |
| Arranque hasta primera ventana | < 300 ms | 43–64 ms |
| Abrir 1000 requests (árbol visible, contando el arranque) | < 200 ms | 67 ms |
| CPU en reposo | ~0 % | 0,0 % |
| Tamaño del binario | < 25 MB | 22,7 MB |
| Desplazar un body de 100 MB (CPU por frame) | — | 0,23 ms |

Dos decisiones salieron de estas medidas:

- **wgpu usa primero los backends nativos** (Vulkan, Metal, DirectX 12) y OpenGL solo si fallan. Con OpenGL activo también, wgpu arranca el driver OpenGL de Mesa, que carga `libgallium` y LLVM: 66 MB en reposo con 100 requests, por encima del presupuesto. Con `WGPU_BACKEND=gl` se fuerza OpenGL (77 MB). Aun así, el cargador de Vulkan abre todos los drivers instalados, incluido lavapipe, que trae 15 MB de LLVM: es memoria del sistema que la aplicación no controla.
- **`lto = "fat"`** baja el binario de 25,6 MB a 22,7 MB, a cambio de una compilación release de unos 4 minutos en vez de 1.

Sin medir aún: la RAM extra al recibir 100 MB y la CPU por tecla en un body de 200 KB en la ventana real (el spike 2 las midió en un prototipo), y la sobrecarga de envío.

## Mediciones con workspaces

Medido el 2026-11-16 en la misma máquina y de la misma forma que F2, tras F3, las primeras tareas de F4 y los workspaces ([ADR 0009](adr/0009-workspaces-y-varias-colecciones.md)). Tres ejecuciones de cada caso; la primera, con la caché fría, se descarta.

| Caso | PSS en reposo | Primera ventana | CPU en reposo |
|---|---|---|---|
| Una colección de 100 requests (cargada) | 52,9 MB | 44–57 ms | 0,0 % |
| Workspace que lista tres colecciones de 1000 requests, ninguna desplegada | 51,2 MB | 43–44 ms | 0,0 % |

- Las colecciones plegadas no cuestan memoria medible: el workspace con 3000 requests listadas usa lo mismo que la ventana con una colección pequeña.
- Una colección de 100 requests usa 2,9 MB más que en F2 (50 MB). Sigue dentro del presupuesto de 60 MB.
- El binario release pasa de 22,7 MiB a 24,6 MiB: dentro del presupuesto de 25 MB, pero con poco margen. Han entrado la bóveda (cifrado y keyring, que trae el cliente D-Bus) y `regex`. Falta ver qué parte aporta cada uno antes de añadir más dependencias.
- Con JSONPath y XPath (`apifast-query` y `roxmltree`) pasa a 24,8 MiB. Queda muy poco margen: el análisis por crate con `cargo bloat` es necesario antes de la próxima dependencia.

## Perfil de compilación

Configuración prevista para release:

```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true
```

- En uso desde F2: `lto = "fat"`, `codegen-units = 1` y `strip = true`. `panic = "abort"` todavía no: con él, un pánico en el hilo del núcleo cerraría la aplicación sin guardar la sesión.
- En F0 se compara `opt-level = 3` con `"s"` (tamaño frente a velocidad).
- Se evalúa un asignador alternativo (por ejemplo mimalloc) midiendo RSS y fragmentación; solo se adopta si mejora las cifras.
- Las dependencias se revisan con `cargo bloat` y `cargo tree` para evitar duplicados y funciones no usadas.

## Micro-optimizaciones: reglas

Las micro-optimizaciones se aplican **donde un benchmark muestra que importan**:

1. Medir primero. Ninguna optimización sin un benchmark que la justifique.
2. Rutas calientes conocidas: render de plantillas, resolución de variables, parseo de YAML, dibujo del árbol y del visor.
3. En esas rutas: evitar asignaciones por fotograma, reutilizar búferes, preferir préstamos a copias, `Cow` para el caso sin cambios.
4. Fuera de ellas, prima la claridad.

## Cómo se mide

| Herramienta | Uso |
|---|---|
| `criterion` | Benchmarks de plantillas, parseo, resolución de herencia |
| Colecciones sintéticas | Generador de colecciones de 100, 1000 y 5000 requests |
| Script de memoria | Arranca la aplicación con una colección y registra PSS y memoria privada |
| `heaptrack` o similar | Perfil de asignaciones cuando un presupuesto se rompe |
| `cargo bloat` | Tamaño del binario por crate |
| Servidor local de pruebas | Respuestas grandes, lentas y en streaming |

### En CI

- Los benchmarks se ejecutan en cada cambio a la rama principal y se comparan con la base.
- Una regresión superior al 10 % en una métrica presupuestada hace fallar la comprobación.
- El tamaño del binario se registra en cada release.

## Riesgos

| Riesgo | Mitigación |
|---|---|
| El editor de texto de egui no escala con cuerpos grandes (41 ms por tecla con 1 MB) | Límite de 200 KB para la edición con resaltado; visor propio por ventanas |
| Sin GPU, el driver por software consume unos 98 MB y mucha CPU | Aceptado; documentado en el [ADR 0004](adr/0004-egui-como-ui.md) |
| Las fuentes con muchos glifos aumentan la memoria | Cargar fuentes adicionales solo si se necesitan |
| tonic y prost-reflect aumentan el binario | Feature opcional e inicialización diferida |
| El parseo de YAML es lento en colecciones grandes | Carga perezosa, parseo en paralelo y caché de metadatos |
