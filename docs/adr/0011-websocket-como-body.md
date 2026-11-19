# ADR 0011: Una request WebSocket es una request con mensajes como body

- **Estado:** aceptado
- **Fecha:** 2026-11-19

## Contexto

[docs/03](../03-arquitectura.md) proponía un `Request` con una variante por protocolo (`Http`, `GraphQl`, `WebSocket`, `Grpc`). GraphQL ya se modeló de otra forma: es una `HttpRequest` con `Body::GraphQl`, porque sale como HTTP.

Una request WebSocket tiene URL, headers, auth, variables, ajustes y documentación, igual que una HTTP. El handshake es un `GET` con `Upgrade`, así que su herencia de headers y auth, sus plantillas, su proxy y su TLS son los de HTTP. Lo único propio son los mensajes que deja listos para enviar.

Bruno lo modela así por dentro: su request de WebSocket tiene `body.mode: ws` y `body.ws` con los mensajes. En el archivo, en cambio, tiene su propio bloque (`websocket`), sin `method` ni `params`.

Una variante propia de `Request` obligaría a duplicar en la GUI las pestañas, los borradores, la sesión, el árbol, la paleta y el guardado, que hoy trabajan sobre `HttpRequest`.

## Decisión

- Una request WebSocket es una `HttpRequest` con `Body::WebSocket(Vec<WsMessage>)`. Cada mensaje tiene nombre, tipo (`json`, `xml` o `text`), texto y si está `selected`. `HttpRequest::kind()` dice qué es (`Http`, `GraphQl` o `WebSocket`) y el árbol lo lleva en `TreeItem::Request::kind`.
- En el archivo se lee y se escribe como Bruno: `info.type: websocket` y el bloque `websocket` con `url`, `headers`, `message` y `auth`. Sus parámetros son los de la URL: no se escribe `params`, y tampoco `method`. Un solo mensaje sin nombre y con texto se escribe plano (`type` y `data`); cualquier otra lista, como variantes (`title`, `selected`, `message`). Se edita en su lugar, sin añadir claves que el archivo no tenía.
- Lo que no aplica a una conexión se oculta en vez de modelarse aparte: no tiene aserciones, capturas ni ejemplos en la interfaz. El runner y la exportación a Postman la dejan fuera con su motivo.
- Conectarse irá en `apifast-ws`, detrás de un puerto propio, y partirá de la misma `RenderedRequest`.

## Consecuencias

- La herencia, las plantillas, la auth (OAuth 2.0 incluido), los borradores y el guardado funcionan para WebSocket sin código nuevo.
- Hay que distinguir el tipo donde importa (envío, runner, exportación, barra de URL), con `kind()` o `Body::is_websocket()`.
- Un body no puede cambiar a WebSocket ni dejar de serlo desde el selector de body: es otro tipo de request.
- gRPC, con servicio, método y mensajes en vez de URL y body, probablemente no encaje igual; se decidirá en F8.
