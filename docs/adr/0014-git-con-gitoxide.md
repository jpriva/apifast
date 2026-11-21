# ADR 0014: La integración con git usa gitoxide, sin el binario `git`

- **Estado:** aceptado
- **Fecha:** 2026-11-21

## Contexto

F10 pide una integración básica con git: la rama, qué cambió desde el último commit, un diff por request que diga qué cambió en ella (no líneas de YAML), hacer commit y cambiar de rama. Las colecciones de apifast están pensadas para versionarse en git ([docs/01](../01-vision-y-alcance.md), [docs/04](../04-formato-de-archivos.md)).

Hay tres formas de hablar con git:

- **El binario `git` del sistema.** Cubre todo, y respeta la configuración, los hooks y las credenciales del usuario. Pero obliga a tenerlo instalado y a leer su salida de texto. Es lo que hace Yaak ([docs/02](../02-analisis-bruno-y-yaak.md)).
- **git2 (libgit2).** Completo y maduro, pero es C (compila libgit2, y a veces OpenSSL o libssh2), y su licencia (GPL-2.0 con excepción de enlace) no está en la lista de `cargo deny`.
- **gitoxide (`gix`).** Git en Rust puro, MIT o Apache-2.0. Lee el estado, el índice y los objetos con soltura. Lo que escribe en el árbol de trabajo (cambiar de rama) está menos maduro que en `git`, y no ejecuta hooks.

## Decisión

- La integración usa gix, en un adaptador propio, `apifast-git`. Solo él depende de gix: lo comprueba la prueba de arquitectura.
- La aplicación habla con un puerto, `GitRepository`. Las rutas son relativas a la raíz de la colección, que puede ser una carpeta dentro de un repositorio más grande; los archivos de fuera no se ven.
- gix va sin red (ni `fetch` ni `push`) y sin sus funciones por defecto: solo `status`, la escritura del árbol de trabajo y la lectura de revisiones.
- Un cambio se dice frente al último commit (`HEAD`), esté o no en el índice: es lo que una persona que no usa git a diario espera. gix solo encuentra los archivos que pueden haber cambiado; cada uno se clasifica comparando el commit con el disco.
- El diff por request lo calcula apifast: lee el archivo del commit y el del disco con el mismo lector de OpenCollection, y compara las requests campo a campo (`apifast-domain`, `request_changes` y sus hermanas). Un cambio de formato que no cambia nada (orden de claves, comillas) no aparece. Las variables secretas nunca muestran su valor.

## Consecuencias

- Ningún binario que encontrar en el `PATH`, y el mismo comportamiento en Linux, macOS y Windows. Sin `git` instalado, la integración funciona igual.
- La compilación crece: gix es grande, aunque sin sus funciones por defecto. Añade una versión más de `hashbrown` al árbol.
- Los commits no ejecutan los hooks de `.git/hooks`, a diferencia de `git commit`, ni firman (`commit.gpgsign`). Si un equipo depende de ellos, tendrá que hacer esos commits con `git`.
- Un commit edita el árbol del último commit con los archivos elegidos y deja el resto del índice como estaba: es "commit de estos archivos", no "commit de lo preparado".
- Cambiar de rama con cambios locales se rechaza en vez de intentar mezclarlos, como `git switch` sin `--merge`. gix no trae el cambio de rama: lo implementa el adaptador sobre sus piezas (recorrido de árboles, índice desde un árbol, edición de referencias). No aplica los filtros de `.gitattributes` (fin de línea, LFS) al escribir los archivos.
- Si gix no basta para algo, la decisión se revisa: el puerto deja cambiar de adaptador sin tocar la aplicación. Lo que el puerto promete está en `apifast-git-contract`, una batería de escenarios que cualquier adaptador corre con `git_repository_contract!`; un adaptador nuevo es válido cuando la pasa.
