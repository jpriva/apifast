# ADR 0004: Interfaz con egui

- **Estado:** aceptado, pendiente de confirmar con los spikes 2 y 3 de F0
- **Fecha:** 2026-11-14

## Contexto

Se busca una interfaz nativa en Rust con bajo consumo de memoria. Bruno usa Electron y Yaak un webview; en ambos casos el motor web domina el consumo.

## Decisión

egui, mediante eframe, en modo reactivo (repintado solo ante eventos).

## Consecuencias

**A favor**

- Un único binario Rust, sin motor web ni JavaScript.
- Mismo lenguaje y mismos tipos que el núcleo; sin frontera de serialización.
- Arranque rápido y consumo bajo.
- El modelo de modo inmediato simplifica el estado de la interfaz.
- Multiplataforma.

**En contra**

- **Edición de texto limitada.** El campo de texto de egui no está pensado para documentos grandes ni ofrece funciones de un editor de código. Es el mayor riesgo técnico del proyecto.
- El modo inmediato redibuja todo en cada repintado: hay que virtualizar listas y guardar en caché el layout del texto.
- Aspecto menos "nativo" que el de las herramientas de cada plataforma.
- Accesibilidad menos madura que en la web.
- **Solo dibuja con GPU.** egui no tiene renderer por CPU, y el contexto gráfico (driver, atlas de texturas) fija un piso de memoria que no depende de nuestro código. Un caso publicado midió unos 135 MB en una aplicación egui frente a unos 30 MB tras pasar a un renderer por software en otro toolkit. Pone en riesgo el presupuesto de 60 MB; se mide en el spike 3.
- Menos componentes listos para usar: tablas, pestañas arrastrables y paleta de comandos se construyen a medida.

## Mitigaciones

- Spike 2: prototipo del editor con resaltado y texto grande antes de comprometerse.
- Visor de respuestas propio, por ventanas sobre el archivo, independiente del campo de texto.
- Límite de tamaño para la edición con resaltado; por encima, edición en texto plano.
- Spike 3: elegir renderer (glow o wgpu) midiendo memoria.

## Alternativas descartadas

- **Tauri o Electron:** contradicen el objetivo de bajo consumo.
- **iced:** es la alternativa principal. Incluye un widget `text_editor` con resaltado de sintaxis, renderizado reactivo desde la versión 0.14 y soporte de métodos de entrada. En contra: sin versión 1.0, cambios incompatibles entre versiones, más código por pantalla y sin soporte de accesibilidad confirmado.
- **Slint:** descartado. Su `TextEdit` no admite resaltado de sintaxis (la petición de un editor de texto enriquecido sigue abierta desde 2023), y su licencia (GPLv3 o gratuita con atribución obligatoria) no encaja con publicar apifast como MIT OR Apache-2.0. A favor tenía el renderer por software y el menor consumo.
- **Floem:** descartado por madurez. Sigue antes de la 1.0, sin versiones publicadas a la vista y con cambios incompatibles anunciados. Tiene lista virtual y el núcleo de editor de Lapce, pero poca documentación y poca adopción fuera de ese proyecto.
- **GTK o Qt:** dependencias nativas pesadas y empaquetado más complejo.

## Plan alternativo

Decisión del responsable del proyecto: **si el spike 2 no cumple, se reconsidera el toolkit** antes de empezar F2, en lugar de aceptar un editor limitado o escribir uno propio.

Criterios de aprobación del spike 2, con un body JSON de 1 MB:

- Escribir y desplazarse sin retraso perceptible (menos de 16 ms por repintado).
- Resaltado de sintaxis y de variables activo.
- Memoria adicional acotada (objetivo: menos de 20 MB sobre la ventana vacía).

El mismo prototipo se construye también en iced, para comparar con cifras y no solo decidir si egui aprueba. La arquitectura hexagonal limita el costo del cambio al crate `apifast-gui`; F1 no depende de esta decisión y puede avanzar en paralelo.
