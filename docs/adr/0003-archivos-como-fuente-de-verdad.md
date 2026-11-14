# ADR 0003: Los archivos son la única fuente de verdad

- **Estado:** aceptado
- **Fecha:** 2026-11-14

## Contexto

Yaak guarda todo en SQLite y ofrece un espejo en archivos. Para mantener ambos sincronizados necesita una tabla de estado, checksums, un planificador de operaciones y una regla de "gana el último que escribió" basada en el reloj. Bruno usa los archivos como fuente de verdad y añade una caché SQLite solo para acelerar el arranque.

## Decisión

Los archivos de la colección son la única fuente de verdad. No hay base de datos. Cualquier caché es derivada y se puede borrar sin perder información.

- Al abrir, se lee el disco.
- Al guardar, se escribe el archivo de forma atómica.
- Un watcher detecta cambios externos y los aplica al modelo en memoria.
- Los identificadores de request no se guardan en disco: se asignan en memoria por ruta y se trasladan al renombrar.

## Consecuencias

**A favor**

- No existe sincronización en dos sentidos ni conflictos entre base de datos y archivos.
- git, un editor de texto o un script pueden modificar la colección con la aplicación abierta.
- Menos código y menos memoria.
- Hacer una copia de seguridad es copiar una carpeta.

**En contra**

- Las búsquedas globales recorren archivos; para colecciones muy grandes hará falta un índice en memoria.
- Los conflictos entre un cambio externo y un borrador sin guardar se resuelven en la interfaz.
- Renombrar una request referenciada por `response(...)` obliga a actualizar esas referencias.

## Alternativas descartadas

- **SQLite como fuente de verdad con espejo en archivos:** es la principal fuente de complejidad de Yaak.
- **SQLite como caché obligatoria:** se pospone; si las mediciones lo piden, se añadirá una caché de metadatos opcional y desechable.
