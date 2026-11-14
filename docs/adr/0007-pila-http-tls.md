# ADR 0007: Pila HTTP y TLS

- **Estado:** aceptado
- **Fecha:** 2026-11-14

## Contexto

Bruno usa axios en el proceso principal de Electron. Yaak usa reqwest sobre hyper, con dos particularidades que conviene no repetir: desactiva el pool de conexiones para poder medir el tiempo de DNS en cada request, y mantiene dos pilas TLS (rustls cuando valida certificados, native-tls cuando no).

## Decisión

- **Cliente:** reqwest sobre hyper, con tokio.
- **TLS:** solo rustls, con el verificador de la plataforma. La opción de no validar certificados se implementa con un verificador permisivo de rustls.
- **Pool de conexiones activo.** Los tiempos se obtienen con un conector instrumentado que emite eventos al abrir una conexión nueva; las reutilizadas se señalan como tales.
- **Redirecciones y descompresión manuales**, para registrarlas en el timeline.
- **Autenticación nativa** en Rust con un enum tipado.
- **HTTP/3** fuera de la versión 1.0.

## Consecuencias

**A favor**

- Una sola pila TLS: binario más pequeño y comportamiento uniforme.
- Envíos consecutivos más rápidos gracias al pool.
- Timeline detallado sin penalizar el rendimiento.
- Sin procesos auxiliares para autenticar.

**En contra**

- rustls no admite versiones antiguas de TLS (1.0 y 1.1) ni algunas suites heredadas; ciertos servidores muy antiguos no serán accesibles.
- El conector instrumentado es código propio que mantener.
- Si reqwest no expone algún detalle necesario, habrá que bajar a hyper para esa parte.

## Alternativas descartadas

- **hyper directamente:** máximo control, pero obliga a reimplementar proxy, cookies y multipart.
- **Enlaces a libcurl:** timeline excelente y compatibilidad amplia, a cambio de una dependencia en C y un empaquetado más complejo.
- **native-tls como respaldo:** resolvería los servidores antiguos, pero duplica la pila TLS. Se reconsiderará solo si hay demanda real.
