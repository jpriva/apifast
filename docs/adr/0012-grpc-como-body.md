# ADR 0012: Una request gRPC es una request con la llamada como body

- **Estado:** aceptado
- **Fecha:** 2026-11-19

## Contexto

El [ADR 0011](0011-websocket-como-body.md) dejó abierto si gRPC encajaba en el mismo modelo que WebSocket, porque tiene servicio, método y mensajes en lugar de URL y body.

Un archivo gRPC de Bruno (`stringifyGrpcRequest.ts`) tiene `url` (el servidor), `method` (`/paquete.Servicio/Metodo`), `methodType`, `protoFilePath`, `metadata` (nombre, valor, `disabled`), `message` (variantes con `title` y el JSON del mensaje), `auth`, y en `runtime` sus variables, aserciones y scripts. Fuera del método y los mensajes, es lo mismo que una request HTTP: un servidor, pares clave/valor que viajan como headers HTTP/2 (la metadata), la misma auth heredada, variables y documentación.

## Decisión

- Una request gRPC es una `HttpRequest` con `Body::Grpc(GrpcCall)`. `GrpcCall` tiene el método, su tipo (`unary`, `client-streaming`, `server-streaming` o `bidi-streaming`, o ninguno mientras no se sepa), la ruta del `.proto`, relativa a la carpeta de la colección como en Bruno (ninguna: se pregunta al servidor por reflection) y los mensajes (`GrpcMessage`: nombre y JSON). `HttpRequest::kind()` da `RequestKind::Grpc`.
- Los headers de la request son la metadata de la llamada, y su auth va en ella: heredan de la carpeta y la colección como los de una HTTP. El método HTTP y los parámetros no se usan y no se escriben.
- En el archivo se lee y se escribe como Bruno: `info.type: grpc` y el bloque `grpc` con `url`, `method`, `methodType`, `protoFilePath`, `metadata`, `message` y `auth`. Los mensajes se escriben como variantes (`title`, `message`), como Bruno; un mensaje plano, que Bruno también lee, se conserva plano al editarlo. Se edita en su lugar, sin añadir claves que el archivo no tenía.
- Lo que una llamada no usa se oculta: en la interfaz no tiene Params, Tests ni Examples, y la pestaña Headers se llama Metadata. Sus aserciones se conservan en el archivo.

## Consecuencias

- La herencia de metadata y auth, las plantillas, los borradores, la sesión, el árbol y el guardado funcionan para gRPC sin código nuevo.
- Las llamadas van en `apifast-grpc`, detrás de un puerto propio, y parten de la misma `RenderedRequest` (URL, metadata y auth ya renderizadas). Hasta entonces, enviar una request gRPC da un error que lo dice, y el runner y la exportación la dejan fuera con aviso.
- Los scripts de los ciclos de gRPC (`grpc:before-call-start`…) se conservan sin ejecutarse, como el resto de los scripts (F9).
