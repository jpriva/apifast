# ADR 0006: Tests declarativos primero, JavaScript después

- **Estado:** aceptado
- **Fecha:** 2026-11-14

## Contexto

Bruno ejecuta scripts JavaScript antes y después de cada request, con QuickJS o con la máquina virtual de Node. Yaak no tiene scripting: resuelve el encadenamiento con funciones de plantilla. Un motor de scripts añade memoria, superficie de seguridad y mucho trabajo de API.

La mayoría de las pruebas habituales (estado, campos del cuerpo, tiempos) y el encadenamiento de requests se pueden expresar sin código.

## Decisión

- **Fases F1 a F8:** aserciones declarativas, capturas de variables y funciones de plantilla (`response`, `header`).
- **Fase F9:** motor JavaScript con QuickJS, con una API compatible con la de Bruno, cargado solo cuando una request tiene scripts.
- Los bloques `runtime.scripts` de los archivos se conservan desde F1 aunque no se ejecuten.

## Consecuencias

**A favor**

- El MVP llega antes y consume menos.
- Las pruebas declarativas se leen y se revisan mejor en un diff.
- Una colección sin scripts nunca carga el motor.
- La compatibilidad con Bruno queda preparada desde el formato.

**En contra**

- Hasta F9, las colecciones importadas con scripts funcionan parcialmente.
- La lógica compleja (bucles, firmas a medida) no se puede expresar antes de F9; se cubre en parte con funciones de plantilla como `hmac` y `jwt`.
- Mantener una API compatible con Bruno obliga a seguir sus cambios.

## Alternativas descartadas

- **JavaScript desde el MVP:** más alcance inicial y más memoria.
- **Solo declarativo para siempre:** impide migrar colecciones con scripts.
- **Lua o Rhai:** más ligeros, pero incompatibles con los scripts existentes de Bruno y Postman.
