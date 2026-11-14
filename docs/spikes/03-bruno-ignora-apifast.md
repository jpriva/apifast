# Comprobación del spike 4: Bruno ignora `.apifast`

- **Fecha:** 2026-11-14
- **Resultado:** **confirmado en Bruno 4.3.0** (flatpak, Linux). Con `.apifast` en `extensions.bruno.ignore`, Bruno no muestra la carpeta, no la modifica y conserva la entrada al reescribir `opencollection.yml`. El diseño del [ADR 0002](../adr/0002-formato-yaml-opencollection.md) se mantiene.
- **Código:** `spikes/bruno-ignore/`. Se reproduce con:

```
spikes/bruno-ignore/crear.sh      # crea ~/dev/apitest/bruno-ignore-test
# abrir la colección en Bruno, editar y guardar (ver "Procedimiento")
spikes/bruno-ignore/comprobar.sh
```

## Por qué

Bruno descarta las claves desconocidas al guardar, así que los datos de apifast (identificador de la colección, *challenge* y secretos cifrados) van en `.apifast/`. Si Bruno mostrara esa carpeta, la trataría como una carpeta de requests, y bastaría con abrir y guardar uno de esos archivos para destruir los secretos del equipo. Hasta ahora, que Bruno respetara la lista `ignore` y la conservara al guardar solo se había deducido leyendo su código.

## Procedimiento

1. `crear.sh` genera una colección con `.apifast/collection.yml` y `.apifast/secrets/produccion.yml` (formato de [formato de archivos](../04-formato-de-archivos.md)), `.apifast` en `extensions.bruno.ignore` y una carpeta de control `.control/` con un request, que **no** está en la lista. Guarda el estado inicial con git.
2. En Bruno: abrir la colección y mirar la barra lateral.
3. Editar y guardar: la URL del request, un header en la configuración de la colección (reescribe `opencollection.yml`) y el valor de una variable del entorno.
4. `comprobar.sh` verifica con git que `.apifast/` no cambió y que `.apifast` sigue en la lista `ignore`, y muestra el diff del resto.

## Resultados

| Comprobación | Resultado |
|---|---|
| `.apifast` aparece en la barra lateral | No |
| `.control` (oculta, fuera de la lista) aparece | Sí: Bruno no ignora las carpetas ocultas por sí solo, la entrada en `ignore` es necesaria |
| Archivos de `.apifast/` cambiados, borrados o añadidos | Ninguno |
| `.apifast` en `extensions.bruno.ignore` tras reescribir `opencollection.yml` | Sí, junto a `node_modules` y `.git` |

## Qué reescribe Bruno al guardar

Diff completo en `spikes/bruno-ignore/resultado-bruno-4.3.0.diff`. Además del cambio hecho, Bruno añade valores por defecto que no estaban en el archivo:

| Archivo | Añadido por Bruno |
|---|---|
| `opencollection.yml` | Bloque `config.proxy` vacío con `inherit: true`; `bundled: false`; quita la línea en blanco antes de `extensions` |
| Request | Bloque `settings` completo (`encodeUrl`, `timeout`, `followRedirects`, `maxRedirects`, `forwardAuthorizationHeader: true`); copia los query params de la URL a `http.params` |
| Entorno | Nada: solo el valor cambiado |

## Consecuencias para F1

- `apifast-storage-fs` debe leer sin problemas los bloques por defecto que añade Bruno (`config.proxy`, `bundled`, `settings`) y conservarlos al guardar.
- Los params de query están duplicados en la URL y en `http.params`; apifast debe tratar la URL y la lista como una sola fuente al editar, igual que Bruno.
- Los archivos reescritos por Bruno entran en el corpus de round-trip.
- La prueba se repite al cambiar de versión mayor de Bruno.
