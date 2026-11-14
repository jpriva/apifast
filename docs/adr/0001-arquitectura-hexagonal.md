# ADR 0001: Arquitectura hexagonal en un workspace de crates

- **Estado:** aceptado
- **Fecha:** 2026-11-14

## Contexto

Bruno y Yaak muestran dos problemas de estructura. Bruno duplica su tubería de ejecución entre la aplicación y la CLI, y concentra lógica en archivos de miles de líneas. Yaak separa bien sus crates, pero reparte la orquestación entre Rust y JavaScript.

apifast necesita que la GUI y la CLI compartan un único motor, que el núcleo se pueda probar sin red ni disco y que tecnologías como la librería YAML o el cliente HTTP se puedan cambiar sin tocar las reglas de negocio.

## Decisión

Arquitectura hexagonal sobre un workspace de Cargo:

- `apifast-domain` y `apifast-template`: dominio puro, sin entrada/salida.
- `apifast-app`: casos de uso y puertos (traits).
- Un crate por adaptador, que implementa uno o varios puertos.
- `apifast-gui` y `apifast-cli`: drivers y raíz de composición.

Las dependencias apuntan siempre hacia el dominio. Un test automático lo verifica.

## Consecuencias

**A favor**

- Un solo motor para GUI, CLI y runner.
- El núcleo se prueba con implementaciones en memoria, rápido y sin red.
- El compilador impide atajos: un adaptador no puede usar a otro.
- La compilación incremental mejora al tener crates pequeños.
- gRPC y JavaScript se aíslan y se pueden desactivar.

**En contra**

- Más código de conversión entre DTO y dominio.
- Más crates que mantener.
- Los traits asíncronos añaden algo de complejidad.

## Alternativas descartadas

- **Un solo crate con módulos:** más simple al principio, pero las reglas de dependencia no se pueden imponer con el compilador.
- **Capas tradicionales (UI → servicio → datos):** el servicio acaba dependiendo de la tecnología de datos.
