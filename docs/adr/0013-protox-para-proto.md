# ADR 0013: Los `.proto` se compilan con protox, sin `protoc`

- **Estado:** aceptado
- **Fecha:** 2026-11-19

## Contexto

Para llamar a un método gRPC sin código generado hacen falta los descriptores de sus servicios y mensajes. Un servidor puede darlos por reflection, pero muchos no la ofrecen: entonces hay que leer los archivos `.proto`. [docs/07](../07-motor-de-red.md#grpc-f8) dejaba abierto cómo compilarlos sin depender de un binario `protoc` externo.

Yaak invoca `protoc`, que distribuye junto a la aplicación. Bruno usa `@grpc/proto-loader`, que parsea en JavaScript.

## Decisión

- Los `.proto` se compilan con [protox](https://crates.io/crates/protox), un compilador de protobuf en Rust que produce un `DescriptorPool` de prost-reflect, el mismo tipo que da la reflection. Los imports se buscan junto al archivo y desde la carpeta de la colección; los tipos de Google (`google/protobuf/*.proto`) vienen incluidos.
- La ruta del `.proto` es relativa a la carpeta de la colección, como en Bruno.

## Consecuencias

- Ningún binario externo que distribuir ni encontrar en el `PATH`; funciona igual en Linux, macOS y Windows.
- Los errores de compilación dicen el archivo, la línea y la causa.
- protox podría no aceptar alguna sintaxis rara que `protoc` acepta; si pasa, se reporta al proyecto o se evalúa de nuevo.
- La misma pieza sirve para los tipos de Google que un servidor con reflection no sirve: se toman de protox.
