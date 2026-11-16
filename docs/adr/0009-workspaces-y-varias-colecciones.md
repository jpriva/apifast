# ADR 0009: Workspaces con varias colecciones

- **Estado:** aceptado
- **Fecha:** 2026-11-16

## Contexto

Hasta ahora la interfaz trabaja con una sola colección: sin colección muestra una bienvenida que solo abre una carpeta existente, y abrir otra cierra la anterior. Quien trabaja con varias APIs tiene que cambiar de colección continuamente, y no puede crear una desde la aplicación.

Bruno agrupa colecciones en **workspaces**: una carpeta con un `workspace.yml` que lista sus colecciones, opcionalmente con entornos globales en `environments/`. El archivo se puede versionar en git junto a las colecciones, y la barra lateral muestra todas a la vez.

## Decisión

### El workspace es un archivo compartible, el de Bruno

```yaml
opencollection: 1.0.0
info:
  name: "Mi equipo"
  type: workspace
collections:
  - name: "Shop API"
    path: "collections/shop"
specs:
docs: ''
```

- Las rutas se guardan relativas a la carpeta del workspace cuando la colección está dentro, y absolutas si está fuera.
- Se edita como las colecciones: se cambian solo las líneas afectadas y se conservan las claves que apifast no modela (`specs`, `docs`).
- Una colección puede estar en varios workspaces: el workspace solo guarda referencias.
- Sus entornos (`environments/`) son los globales de Bruno. Se conservan, y su uso como ámbito `Global` es una tarea posterior.

### El estado personal no va en el archivo compartido

| Dato | Dónde |
|---|---|
| Workspaces conocidos y el último activo | `~/.local/share/apifast/workspaces.yml` (estado, no configuración) |
| Colecciones desplegadas, pestañas abiertas y la activa | `~/.local/share/apifast/workspaces/<hash-de-la-ruta>/state.yml` |
| Borradores y entorno activo de cada colección | La sesión de la colección, como hasta ahora |
| Workspace por defecto ("My workspace") | `~/.local/share/apifast/default-workspace/` |

Siempre hay un workspace activo: en el primer arranque se crea el de por defecto, así la aplicación entra directamente al visor, sin bienvenida.

### Identidad, carga y entorno

- Una colección se identifica por su carpeta raíz canónica; una pestaña, por la raíz y la ruta de la request.
- **Carga perezosa:** una colección plegada en el árbol no se carga, no se vigila y no lee su historial. Se carga al desplegarla, al abrir una de sus requests o al restaurar la sesión.
- Cada colección tiene su propio entorno activo. La barra superior muestra el de la colección de la pestaña activa.
- Un solo workspace activo a la vez. Al cambiar, se guarda su estado, se descargan las colecciones que el otro no usa y se restaura el estado del otro.
- `workspace.yml` se vigila sin recursión, así que un `git pull` que lo cambia actualiza la lista.

## Consecuencias

**A favor**

- Varias colecciones a la vez, como en Bruno, y compatibilidad con sus workspaces.
- Un equipo comparte su grupo de colecciones por git.
- La memoria y los watchers crecen solo con las colecciones desplegadas, no con las listadas.
- El estado de la interfaz sigue fuera de lo que se versiona ([ADR 0008](0008-estado-local.md)).

**En contra**

- Las pestañas, los comandos y el árbol de la interfaz necesitan saber a qué colección pertenecen.
- Mover la carpeta de una colección que está fuera del workspace rompe su ruta absoluta; aparece marcada con su error y se puede quitar.
- Dos workspaces pueden listar la misma colección con nombres distintos; el nombre que se muestra es el de la colección.

## Alternativas descartadas

- **Workspaces solo locales** (una lista en el directorio de configuración): más simple, pero no se comparte y no es compatible con Bruno.
- **Una lista global de colecciones sin workspaces:** no permite separar grupos de trabajo.
- **Cargar todas las colecciones listadas:** cada una cuesta árbol, watcher e historial; con muchas, el presupuesto de memoria ([docs/10](../10-rendimiento-y-memoria.md)) no se cumpliría.
- **Identificar la colección por su id de `.apifast/collection.yml`:** no todas lo tienen (solo las que guardaron un secreto) y el id no dice dónde está la carpeta.
