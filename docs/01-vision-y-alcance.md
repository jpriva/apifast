# 1. Visión y alcance

## Problema

Los clientes de API actuales obligan a elegir:

- **Postman** guarda todo en la nube y es pesado.
- **Bruno** guarda colecciones como archivos legibles, pero corre sobre Electron y mantiene colecciones y respuestas completas en el estado de la interfaz.
- **Yaak** tiene un núcleo Rust excelente y buena criptografía, pero su fuente de verdad es una base SQLite (los archivos son un espejo), usa un webview y necesita un proceso Node permanente para sus plugins.

## Visión

Un cliente de API **nativo, rápido y basado en archivos**, donde la colección vive en el repositorio del proyecto, los secretos están cifrados y la aplicación consume pocos recursos.

## Objetivos

1. **Archivos primero.** La colección en disco es la única fuente de verdad. Es legible, se revisa en un pull request y funciona sin la aplicación abierta.
2. **Bajo consumo.** Objetivos iniciales, a validar con mediciones: menos de 60 MB de RAM en reposo (memoria proporcional, PSS), arranque en frío en menos de 300 ms y binario de menos de 25 MB.
3. **Secretos seguros por defecto.** Ningún secreto en claro en disco, en el historial ni en los logs.
4. **Interfaz intuitiva.** Quien venga de Postman, Bruno o Yaak debe poder enviar su primera request sin leer documentación.
5. **Un solo motor.** La GUI y la CLI comparten los mismos casos de uso, para que una colección se comporte igual en el escritorio y en CI.
6. **Código mantenible.** Arquitectura hexagonal, tipos fuertes y módulos pequeños.

## No-objetivos

- Sincronización en la nube propia, cuentas de usuario o colaboración en tiempo real. La colaboración se hace con git.
- Sistema de plugins en la versión 1.0. Se evaluará después (ver [roadmap](12-roadmap.md)).
- Servidor de mocks, proxy de depuración o generación de documentación publicada.
- Versión web o móvil.
- Compatibilidad total con scripts de Postman. Se importan colecciones; los scripts se traducen solo cuando llegue el motor JavaScript.

## Plataformas

**Linux primero.** El desarrollo y las pruebas se hacen en Linux. macOS y Windows se compilan en CI desde el inicio para no romperlos, y su pulido y empaquetado se completan en la fase F10.

## Usuarios

- **Desarrollador backend** que prueba sus endpoints mientras programa y quiere las requests junto al código.
- **Equipo** que comparte colecciones y secretos por git sin exponer credenciales.
- **Pipeline de CI** que ejecuta una colección como suite de pruebas y necesita un reporte JUnit.

## Principios de diseño

| Principio | Consecuencia práctica |
|---|---|
| Los archivos mandan | No hay base de datos; cualquier caché se puede borrar sin perder nada |
| Pagar solo por lo que se usa | gRPC y el motor JavaScript se cargan al primer uso |
| Tipos antes que mapas | `Auth` y `Body` son enums, no `Map<String, Value>` |
| La UI no es dueña de los datos | La interfaz guarda identificadores y vistas; el núcleo guarda el modelo |
| Nada sensible sale por accidente | Los secretos son un tipo propio que no se puede imprimir |
| Todo se mide | Los presupuestos de memoria y arranque se comprueban en CI |

## Alcance funcional de la versión 1.0

- Protocolos: HTTP/REST, GraphQL, Server-Sent Events, WebSocket y gRPC.
- Colecciones, carpetas, requests, entornos y variables con herencia.
- Bóveda de secretos híbrida.
- Autenticación: Basic, Bearer, API key, Digest, OAuth 2.0, AWS SigV4 y NTLM.
- Aserciones declarativas, capturas de variables, encadenamiento de requests y runner de colección.
- CLI con reporters JSON, JUnit y HTML.
- Importación desde Bruno, Postman, Insomnia, OpenAPI y cURL.
- Scripting en JavaScript, cargado bajo demanda.
- Integración básica con git.
