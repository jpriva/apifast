# 10. Rendimiento y memoria

El bajo consumo es un objetivo de producto, no una optimización tardía. Este documento fija los presupuestos, las técnicas y la forma de medir.

## Presupuestos

La memoria se mide en **PSS** (memoria proporcional, de `/proc/<pid>/smaps_rollup`), no en RSS. RSS cuenta enteras las bibliotecas del driver de GPU que el sistema ya tiene cargadas y exagera el consumo en unos 45 MB ([spike 3](spikes/02-interfaz-egui-vs-iced.md)).

Desde F10, los presupuestos de RAM son de la **memoria propia de apifast**: su binario, su heap, su memoria anónima y su pila. Se deja fuera la pila gráfica del sistema (Mesa y su LLVM, los drivers y cargadores de Vulkan y EGL), que depende de la máquina y no de la aplicación: con una Radeon, RADV carga `libLLVM` (23,6 MB de PSS cuando ningún otro proceso la usa, 8,8 MB cuando otra aplicación también la tiene abierta), y el cargador de Vulkan abre además los drivers por software (lavapipe y Dozen, unos 4 MB). `tools/bench/breakdown.py` da las dos cifras; el total también se registra.

Referencia medida en el spike, con egui y wgpu: 35 MB con la ventana vacía y 47 MB con un árbol de 5000 filas y un body de 100 KB.

| Métrica | Objetivo | Cómo se mide |
|---|---|---|
| RAM propia en reposo, colección de 100 requests | < 60 MB PSS | `tools/bench/breakdown.py` tras 10 s inactivo, sin la pila gráfica |
| RAM propia con colección de 5000 requests | < 120 MB PSS | Igual, con colección sintética |
| RAM extra al recibir 100 MB de respuesta | < 10 MB | Diferencia de PSS durante la descarga |
| CPU por tecla en un body de 200 KB | < 10 ms | Escenario automático como el del spike |
| Arranque en frío hasta primera ventana | < 300 ms | Marca de tiempo interna |
| Abrir colección de 1000 requests | < 200 ms hasta ver el árbol | Marca de tiempo interna |
| Sobrecarga de envío (sin red) | < 2 ms | Benchmark contra servidor local |
| CPU en reposo | ~0 % | Sin repintados sin eventos |
| Tamaño del binario (release) | < 60 MB | Tamaño en disco |

La memoria manda: si hay que elegir, se gasta disco para ahorrar RAM, nunca al revés. Por eso el presupuesto del binario subió en F10 de 25 MB a 60 MB (la GUI estaba en 36,2 MB con git y gRPC), y los de RAM no se tocan.

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

- **Inicialización al primer uso** para gRPC y el motor JavaScript. gRPC no tiene estado global: el transporte se crea en cada llamada o descripción, y nada de tonic, prost-reflect ni protox corre hasta entonces. Medido con la CLI en release: un envío HTTP ocupa lo mismo con gRPC compilado que sin él (unos 9,5 MB de memoria residual máxima, dentro del ruido entre ejecuciones). El motor JavaScript tampoco tiene estado global: cada script crea su runtime de QuickJS al empezar y lo descarta al terminar. Medido en release: el runtime vacío cuesta de 0,1 a 0,3 ms y 105 KB de heap; con la API (`req`, `res`, `bru`, `test`, `expect`) cargada, un script corto tarda alrededor de 1 ms. Una request sin scripts no llama al motor (lo comprueba una prueba de `apifast-app`); con la CLI en release, un envío sin scripts tuvo unos 13,3 MB de memoria residual máxima y el mismo envío con un script de tests unos 15,5 MB: el motor cuesta unos 2 MB, solo cuando un script corre.
- **Features de Cargo** para compilar la CLI sin dependencias de la interfaz y, si se desea, sin gRPC: la feature `grpc` (activa por defecto) de `apifast-cli` y `apifast-gui` trae `apifast-grpc`. Sin ella (`--no-default-features`) no se compilan tonic, prost ni protox, y el binario de la CLI baja de 15,2 MB a 12,9 MB; las requests gRPC se abren y se guardan igual, y llamarlas dice que la build no tiene gRPC (`NoGrpc`, de `apifast-app`). El CI compila y prueba los dos drivers sin la feature.
- **Feature `git`** (activa por defecto) de `apifast-cli` y `apifast-gui`: trae `apifast-git` y con él gix ([ADR 0014](adr/0014-git-con-gitoxide.md)). Sin ella no se compila gix: `apifast git` dice que la build no lo tiene, y la interfaz no muestra la barra de estado ni la ventana Git. Medido en release: la CLI pasa de 20,6 a 17,0 MB y la GUI de 36,2 a 32,7 MB. Para quitar solo git: `--no-default-features --features grpc`. Las dos quedan dentro del presupuesto de 60 MB.

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
- Con F5 hasta los certificados (Digest, NTLM, AWS SigV4, OAuth 2.0 con su servidor de *callback*, proxy con SOCKS, certificados de cliente y las pantallas de ajustes) llega a **25,6 MiB: por encima del presupuesto de 25 MB**. La CLI está en 11,2 MiB. Las dependencias nuevas son pequeñas (`md-5`, `md4`, `hmac` y `sha2` ya estaban en el árbol, y SOCKS va dentro de reqwest), así que el crecimiento viene sobre todo del código propio y de su monomorfización. Antes de seguir añadiendo funciones hay que medir con `cargo bloat` y recortar: candidatos son las funciones genéricas de envío (`send_request` se instancia por cada combinación de puertos) y los `Debug` y formatos que solo usan las pruebas.
- En F10 el presupuesto del binario sube a 60 MB, porque importa más la memoria que el disco. Con F9 y la integración con git, la GUI está en 36,2 MB y la CLI en 20,6 MB.

## Mediciones de F10

Medido el 2026-11-21 en la misma máquina (Ryzen 7 8700G, Radeon 780M, Mesa con RADV, Vulkan), ahora con Fedora 45, binario release con todo (git, gRPC, scripts). Tres ejecuciones de cada caso; la primera, con la caché fría, se descarta.

| Métrica | Objetivo | Medido |
|---|---|---|
| RAM propia en reposo, 100 requests | < 60 MB PSS | 30–31 MB |
| RAM propia en reposo, 5000 requests | < 120 MB PSS | 32–33 MB |
| RAM total en reposo, 100 requests (con la pila gráfica) | — | 48–71 MB, según quién más tenga `libLLVM` abierta |
| Arranque hasta primera ventana | < 300 ms | 42–58 ms |
| Abrir 1000 requests (árbol visible, contando el arranque) | < 200 ms | 66–96 ms |
| Abrir 5000 requests | — | 120–139 ms |
| CPU en reposo | ~0 % | 0,0 % |
| RAM extra al recibir y mostrar 100 MB | < 10 MB | 3,5–4,7 MB (llega en unos 50 ms por loopback) |
| CPU por tecla en un body de 200 KB | < 10 ms | 1,6 ms |
| Sobrecarga de envío (servidor local) | < 2 ms | 0,29 ms de mediana, 0,34 ms en el percentil 95 |
| Tamaño del binario | < 60 MB | 36,2 MB (GUI), 20,6 MB (CLI) |

- Después de esas mediciones, la barra de título de GNOME (+0,44 MB) y el registro con niveles (+1,8 MB) llevan la GUI a 38,6 MB; la CLI está en 21,8 MB. Del registro, 1 MB es el puente con el crate `log`, por el que llegan los mensajes de wgpu, winit y reqwest; cambiar `EnvFilter` por `Targets` ahorró 0,25 MB. Con el nivel por defecto, ese código no se ejecuta.
- De la memoria propia, unos 21 MB son el código del binario residente: páginas limpias del archivo, que el kernel puede soltar. El heap es de 4,5 MB y la memoria anónima de 3,8 MB.
- Se probaron dos recortes que no se aplicaron (se prefirió no cambiar el comportamiento ni la compilación): compilar con `opt-level = "s"` bajaba el código residente de 20,9 a 15,9 MB y el binario de 36,2 a 26,6 MB; y que el cargador de Vulkan no abriera lavapipe ni Dozen (`VK_LOADER_DRIVERS_DISABLE`) quitaba unos 4 MB de la pila gráfica. Quedan como opciones si la memoria propia se acerca al presupuesto.
- La respuesta, el body y el envío se miden con `cargo run --release -p apifast-gui --example bench -- response|typing|send` (ver `tools/bench/README.md`). La respuesta de 100 MB se recibe con la aplicación real (núcleo, almacén de respuestas, visor) en su ventana; la tecla, con el editor de código de los bodies en una ventana real, una tecla por fotograma; el envío, con el núcleo, del comando a la respuesta publicada, historial incluido.
- Los 4 MB de la respuesta de 100 MB no se devuelven al cerrar su pestaña, pero **no crecen**: en 8 vueltas seguidas (abrir, enviar 100 MB, mostrar, cerrar) la memoria propia queda entre 43,4 y 45,1 MB y el heap oscila entre 6,6 y 8,4 MB, sin tendencia. Son el código del visor que se usa por primera vez (unos 1,5 MB de páginas limpias del binario, que el kernel suelta cuando hace falta: en una de las pruebas la memoria propia bajó así a 25 MB), lo que el asignador guarda para reutilizar (1–2 MB de heap) y algo de memoria anónima de los hilos del envío (~1 MB). Devolverlos al sistema con `malloc_trim` exigiría `unsafe`; no compensa.
- Sacar un panel a una ventana del sistema propia (un viewport de egui) cuesta unos 2,4 MB de memoria propia por ventana. Mientras se redibuja, cada ventana extra añade unos 0,27 ms de CPU por frame; en reposo, nada. Se mide con `bench -- viewports` ([ADR 0015](adr/0015-disposiciones-con-egui-tiles.md)).
- En el ejemplo `bench`, `response` dibuja sin vsync: en Wayland, una ventana oculta no recibe fotogramas con vsync y el escenario, que avanza en cada uno, se detenía. `APIFAST_BENCH_ROUNDS` dice cuántas vueltas da (3 por defecto).

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
