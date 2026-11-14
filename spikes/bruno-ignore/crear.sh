#!/usr/bin/env bash
# Crea una colección OpenCollection con carpeta .apifast/ para probarla en Bruno.
# Uso: crear.sh [directorio]   (por defecto ~/dev/apitest/bruno-ignore-test)
set -euo pipefail

dir=${1:-$HOME/dev/apitest/bruno-ignore-test}
if [ -e "$dir" ]; then
  echo "Ya existe $dir. Bórralo antes de repetir la prueba." >&2
  exit 1
fi
mkdir -p "$dir"/{environments,usuarios,.apifast/secrets,.control}
cd "$dir"

cat > opencollection.yml <<'YML'
opencollection: 1.0.0

info:
  name: prueba-ignore-apifast

request:
  headers:
    - name: accept
      value: application/json

extensions:
  bruno:
    ignore:
      - node_modules
      - .git
      - .apifast
YML

cat > environments/produccion.yml <<'YML'
name: produccion
variables:
  - name: host
    value: https://httpbin.org
  - secret: true
    name: api_key
YML

cat > usuarios/folder.yml <<'YML'
info:
  name: usuarios
  type: folder
  seq: 1
YML

cat > usuarios/listar.yml <<'YML'
info:
  name: Listar usuarios
  type: http
  seq: 1

http:
  method: GET
  url: "{{host}}/get"
  headers:
    - name: x-api-key
      value: "{{api_key}}"
  auth: inherit
YML

# Datos de apifast, con el formato de docs/04-formato-de-archivos.md.
cat > .apifast/collection.yml <<'YML'
format: 1
id: col_7Hq2mXv9Rk
vault:
  challenge: "enc:v1:Zm9vYmFyYmF6cXV4Zm9vYmFyYmF6cXV4Zm9vYmFyYmF6cXV4"
YML

cat > .apifast/secrets/produccion.yml <<'YML'
environment: produccion
secrets:
  - name: api_key
    value: "enc:v1:c2VjcmV0by1jaWZyYWRvLWRlLWVqZW1wbG8tbm8tcmVhbA"
YML

# Control: carpeta oculta que NO está en ignore. Si Bruno la muestra, queda
# confirmado que no ignora las carpetas ocultas por sí solo.
cat > .control/request-de-control.yml <<'YML'
info:
  name: Request de control
  type: http
  seq: 1

http:
  method: GET
  url: "{{host}}/get"
  auth: inherit
YML

git init -q
git add -A
git -c user.name=prueba -c user.email=prueba@localhost commit -qm "estado inicial"

echo "Colección creada en $dir"
