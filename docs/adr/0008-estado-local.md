# ADR 0008: Estado local en archivos planos

- **Estado:** aceptado
- **Fecha:** 2026-11-14

## Contexto

Además de la colección, la aplicación guarda datos que no deben versionarse: historial de ejecuciones, cuerpos de respuesta, cookies, secretos solo locales, claves envueltas, esquemas GraphQL obtenidos por introspección y estado de la interfaz. Yaak usa SQLite para todo esto (más una segunda base para cuerpos); Bruno combina archivos JSON con SQLite.

## Decisión

El estado local vive en el directorio de datos del usuario, fuera de la colección, en archivos planos y sin base de datos:

| Dato | Formato |
|---|---|
| Cuerpos de respuesta | Un archivo por respuesta |
| Historial | JSON Lines, un archivo por colección, solo se añade al final |
| Timeline | Junto a los metadatos de cada respuesta |
| Cookies | Un archivo cifrado por colección |
| Bóveda local | Un archivo cifrado por colección |
| Claves de colección | Un archivo por colección, envuelto con la clave maestra |
| Estado de la interfaz y ajustes | YAML |

Todo se indexa por el identificador estable de la colección, no por su ruta.

Los cuerpos de respuesta y el historial **no se cifran**: se limitan por tamaño y antigüedad, y cada request puede marcarse como "no guardar respuesta". Es el mismo nivel de protección que ofrecen Bruno y Yaak.

Desde el formato 2 de sus líneas (F10), cada entrada guarda también un id, un nombre opcional y lo que se envió:
- Lo enviado se guarda ya resuelto: método, URL, headers y hasta 64 KB del body de texto.
- Como no se cifra, los secretos se enmascaran. Si la request llevaba alguno, también se ocultan los valores de la query, igual que en la timeline.
- Los valores de `Authorization`, `Proxy-Authorization` y `Cookie` no se guardan nunca, aunque no vengan de un secreto: pueden llevar tokens o cookies que en apifast se guardan cifrados.
- Una entrada con nombre, y su respuesta, sobreviven a la limpieza por límites.
- Las líneas del formato 1 se siguen leyendo, y la siguiente reescritura las guarda en el formato 2.
- Renombrar o borrar una entrada reescribe el archivo, aparte y luego renombrado, como ya hacía la limpieza. Un candado compartido por el proceso evita que una entrada añadida mientras tanto se pierda.

## Consecuencias

**A favor**

- Sin dependencia de SQLite: binario más pequeño y menos memoria.
- Los cuerpos de respuesta se leen por ventanas directamente del archivo.
- Fácil de inspeccionar, respaldar y borrar.
- Coherente con "los archivos mandan".

**En contra**

- Las consultas sobre el historial (filtrar, buscar) se hacen recorriendo el archivo; se limita con topes de tamaño y antigüedad.
- La limpieza de respuestas huérfanas es responsabilidad de la aplicación.
- Las escrituras concurrentes desde dos instancias requieren un bloqueo de archivo.

## Alternativas descartadas

- **SQLite:** consultas potentes, pero es justo la dependencia que se quiere evitar; el historial no necesita consultas complejas.
- **Base clave-valor embebida:** intermedia en complejidad. Se reconsiderará si el historial en JSON Lines resulta lento con datos reales.
- **Cifrar todo el historial:** protege frente a la copia del directorio de datos, pero añade CPU, complica el visor por ventanas y obliga a desbloquear la bóveda para ver cualquier respuesta.
- **No persistir respuestas por defecto:** máxima privacidad, pero se pierden las respuestas al reiniciar y `response()` tendría que volver a ejecutar.
- **Guardar el historial dentro de la colección:** ensuciaría el repositorio y podría filtrar datos sensibles.
