# ADR 0010: Reimportación que conserva las ediciones

- **Estado:** aceptado
- **Fecha:** 2026-11-19

## Contexto

Una colección importada de una especificación OpenAPI, o de una exportación de Postman, Insomnia o Bruno, se queda vieja cuando la fuente cambia. Importar otra vez en una carpeta nueva duplica la colección y pierde lo que el usuario añadió (headers, aserciones, ejemplos). Sobrescribir la colección pierde lo mismo.

Para actualizar sin pisar nada hace falta saber, por cada request, tres cosas: cómo vino la última vez, cómo está ahora en disco y cómo la trae la fuente.

## Decisión

### Cada request tiene una clave estable en su fuente

| Fuente | Clave |
|---|---|
| OpenAPI | `operationId`, o el método y la ruta (`GET /pets/{id}`) |
| Postman | el `id` del elemento, o su lugar en el árbol |
| Insomnia | el `_id` (v4) o `meta.id` (v5), o su lugar |
| Bruno | la ruta del archivo `.bru` en la colección |

La clave no depende del nombre del archivo en apifast: renombrar la request en la fuente la actualiza, no la duplica.

### La importación deja un registro, versionado

`.apifast/import.yml` guarda la fuente (`openapi pets.yaml`) y, por cada request escrita, su clave, su archivo y una huella de la request tal como vino (sin su posición). Va en git con la colección, así cualquiera del equipo puede reimportar.

```yaml
source: openapi pets.yaml
requests:
  - key: listPets
    path: pets/list-pets.yml
    hash: 81ff5d9b29cae323
```

La huella es FNV-1a de la request del dominio. Si una versión de apifast añade un campo al dominio, todas las huellas cambian una vez: cada request se ve como editada y se pregunta en lugar de sobrescribir.

### Tres versiones, una acción por request

| En la fuente desde la última importación | En disco | Acción | Por defecto |
|---|---|---|---|
| nueva | — | crear (con un nombre libre si el archivo ya existe) | sí |
| igual | cualquiera | nada | — |
| cambió | igual | actualizar, conservando su posición | sí |
| cambió | cambió | conflicto | no |
| está | borrada | volver a crear | no |
| ya no está | cualquiera | borrar | no |

Lo que no se elige conserva su registro anterior: se vuelve a ofrecer en la siguiente reimportación, nunca se olvida en silencio. Los entornos solo se añaden; los que existen pueden tener ediciones.

No se mezclan campos: un conflicto se resuelve tomando la versión de la fuente o conservando la propia. Mezclar por campo (headers, params) se puede añadir después sin cambiar el registro.

## Consecuencias

- `apifast import` sobre una carpeta que ya importó esa fuente la actualiza; `--overwrite` toma la fuente en los conflictos y en lo borrado, `--prune` borra lo que la fuente ya no tiene. Sobre una colección que no se importó, se niega.
- Una colección importada antes de este cambio no tiene registro: no se puede reimportar.
- La interfaz importa con registro; revisar una reimportación en la interfaz queda pendiente.
