# ADR 0005: Bóveda de secretos híbrida

- **Estado:** aceptado
- **Fecha:** 2026-11-14

## Contexto

Bruno guarda en disco solo los nombres de los secretos; los valores son locales y cada miembro del equipo debe cargarlos. Su cifrado de respaldo es débil (IV fijo, clave derivada del identificador de máquina) e indexa los secretos por ruta absoluta.

Yaak cifra los secretos dentro de los archivos con XChaCha20-Poly1305 y una clave por workspace que se comparte fuera de banda. Es cómodo para equipos, pero no permite tener secretos personales que nunca salgan de la máquina, salvo marcando todo un entorno como privado.

## Decisión

Un modelo híbrido, elegido por variable:

1. **Secreto solo local (por defecto):** en el repositorio solo el nombre; el valor en una bóveda local cifrada con la clave maestra.
2. **Secreto compartido (acción explícita):** valor cifrado con la clave de la colección, guardado en `.apifast/secrets/<entorno>.yml`.

Detalles:

- Cifrado XChaCha20-Poly1305 con nonce aleatorio de 24 bytes y formato `enc:v1:`.
- Datos asociados que atan cada valor a su colección, entorno y variable.
- Clave maestra en el keyring del sistema; sin keyring, en un archivo con permisos `0600` (como Yaak).
- Clave de colección envuelta con la maestra, indexada por un identificador estable de la colección.
- *Challenge* en `.apifast/collection.yml` para detectar claves incorrectas.
- Clave compartible en base32 Crockford.
- Los secretos son siempre variables; no hay texto cifrado incrustado en plantillas.

## Consecuencias

**A favor**

- Un equipo comparte secretos por git sin exponerlos.
- Las credenciales personales no salen de la máquina.
- Mover o renombrar la colección no pierde secretos.
- No existe ningún modo de cifrado débil: la clave siempre es aleatoria.
- Un secreto marcado sin pensar no llega al repositorio.
- "Los secretos son variables" simplifica el enmascarado y la auditoría.

**En contra**

- Dos modelos que explicar en la interfaz.
- Sin keyring, quien copie el directorio personal obtiene la clave maestra. Se acepta a cambio de no pedir una contraseña; la interfaz lo indica.
- Renombrar una variable o un entorno obliga a volver a cifrar.
- La clave compartible se distribuye por un canal externo que apifast no controla.
- El historial de git conserva valores cifrados con claves antiguas: rotar la clave no sustituye a rotar la credencial.

## Alternativas descartadas

- **Solo cifrado en archivos:** no cubre los secretos personales.
- **Solo bóveda local:** cada miembro del equipo repite la carga de secretos.
- **Texto cifrado incrustado en plantillas (como Yaak):** más flexible, pero dispersa los secretos por todos los archivos.
- **Passphrase con Argon2id como respaldo sin keyring:** más seguro que el archivo `0600`, pero añade una contraseña que recordar. Queda como mejora posterior a la 1.0.
- **Secretos compartidos por defecto:** más cómodo para equipos, pero un token personal podría acabar en git por descuido.
