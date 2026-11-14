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

- **Carga perezosa.** Al abrir una colección se leen solo los metadatos (`info`) de cada request. El contenido completo se carga al abrir la pestaña y se libera al cerrarla si no hay cambios.
- **Cadenas compartidas.** `Arc<str>` para textos repetidos (nombres de headers, métodos) y cadenas compactas para valores cortos.
- **Instantáneas inmutables.** El núcleo publica el estado como `Arc`; la interfaz lo lee sin copiar.
- **Sin clones de la colección.** Un envío recibe referencias a la request y a su cadena de padres, nunca una copia del árbol.

### Respuestas

- **Streaming a disco** con búfer fijo; la memoria no depende del tamaño de la respuesta.
- **Visor por ventanas.** El visor mapea el archivo y dibuja solo las líneas visibles, con un índice de saltos de línea construido de forma incremental.
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
