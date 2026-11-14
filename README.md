# apifast

Cliente de escritorio para probar APIs, escrito en Rust con interfaz en egui. Toma las mejores ideas de [Bruno](https://github.com/usebruno/bruno) y de [Yaak](https://github.com/mountain-loop/yaak) y las combina en una aplicación nativa, liviana y basada en archivos.

> **Estado: fase F0 (diseño).** Este repositorio contiene solo documentación y roadmap. Todavía no hay código.

## Qué es

- **Colecciones como archivos.** Un archivo YAML por request, carpetas como directorios. Se versiona con git sin sorpresas.
- **Nativo y liviano.** Un único binario Rust, sin Electron, sin webview y sin procesos auxiliares.
- **Secretos cifrados.** Bóveda con XChaCha20-Poly1305: secretos compartibles dentro del repositorio o guardados solo en la máquina.
- **Un solo motor.** La interfaz gráfica y la línea de comandos ejecutan exactamente el mismo núcleo.
- **Arquitectura hexagonal.** Dominio puro, puertos explícitos y adaptadores intercambiables.

## Decisiones principales

| Tema | Decisión | Detalle |
|---|---|---|
| Lenguaje y UI | Rust + egui (eframe) | [ADR 0004](docs/adr/0004-egui-como-ui.md) |
| Arquitectura | Hexagonal, workspace de crates | [ADR 0001](docs/adr/0001-arquitectura-hexagonal.md) |
| Formato en disco | YAML OpenCollection puro; datos propios en `.apifast/` | [ADR 0002](docs/adr/0002-formato-yaml-opencollection.md) |
| Persistencia | Los archivos son la fuente de verdad | [ADR 0003](docs/adr/0003-archivos-como-fuente-de-verdad.md) |
| Secretos | Bóveda híbrida: solo locales por defecto, compartidos cifrados si se elige | [ADR 0005](docs/adr/0005-boveda-hibrida.md) |
| Tests | Aserciones declarativas primero, JavaScript después | [ADR 0006](docs/adr/0006-scripting-diferido.md) |
| Red | reqwest/hyper + rustls, una sola pila TLS | [ADR 0007](docs/adr/0007-pila-http-tls.md) |
| Estado local | Archivos planos, sin base de datos | [ADR 0008](docs/adr/0008-estado-local.md) |
| Plataformas | Linux primero; macOS y Windows compilados en CI | [Visión](docs/01-vision-y-alcance.md) |
| Idioma | Código en inglés, interfaz en español e inglés | [Calidad](docs/11-calidad-y-clean-code.md) |

## Documentación

1. [Visión y alcance](docs/01-vision-y-alcance.md)
2. [Análisis de Bruno y Yaak](docs/02-analisis-bruno-y-yaak.md)
3. [Arquitectura](docs/03-arquitectura.md)
4. [Formato de archivos](docs/04-formato-de-archivos.md)
5. [Variables y plantillas](docs/05-variables-y-plantillas.md)
6. [Bóveda de secretos](docs/06-boveda-de-secretos.md)
7. [Motor de red](docs/07-motor-de-red.md)
8. [Tests y runner](docs/08-tests-y-runner.md)
9. [UI y UX](docs/09-ui-ux.md)
10. [Rendimiento y memoria](docs/10-rendimiento-y-memoria.md)
11. [Calidad y clean code](docs/11-calidad-y-clean-code.md)
12. [Roadmap](docs/12-roadmap.md)

Las decisiones de arquitectura están en [docs/adr/](docs/adr/).

## Roadmap en una tabla

| Fase | Resultado |
|---|---|
| F0 | Documentación, ADRs y spikes técnicos |
| F1 | Núcleo sin interfaz: enviar una request desde un archivo por CLI |
| F2 | Interfaz gráfica mínima usable a diario |
| F3 | Bóveda de secretos |
| F4 | Aserciones, capturas, runner y reporters |
| F5 | Autenticación avanzada, proxy, certificados, cookies, timeline |
| F6 | Importación y exportación |
| F7 | GraphQL, SSE y WebSocket |
| F8 | gRPC |
| F9 | Scripting en JavaScript |
| F10 | Versión 1.0 |

El detalle de cada fase está en [docs/12-roadmap.md](docs/12-roadmap.md).

## Licencia

Licencia dual **MIT OR Apache-2.0**, a elección de quien lo use. Ver [LICENSE-MIT](LICENSE-MIT) y [LICENSE-APACHE](LICENSE-APACHE).

## Referencias

El análisis se hizo sobre copias locales de ambos proyectos, ubicadas junto a este repositorio (`../bruno` y `../yaak`). Los dos tienen licencia MIT; si se adapta código de alguno, hay que conservar su aviso de copyright.
