#!/usr/bin/env bash
# Comprueba qué cambió Bruno en la colección creada por crear.sh.
# Uso: comprobar.sh [directorio]
set -euo pipefail

dir=${1:-$HOME/dev/apitest/bruno-ignore-test}
cd "$dir"
fallos=0

echo "== 1. .apifast/ sin cambios"
cambios=$(git status --porcelain --untracked-files=all -- .apifast)
if [ -z "$cambios" ]; then
  echo "OK: ningún archivo de .apifast/ cambió, se borró o se añadió"
else
  echo "FALLO:"; echo "$cambios"; fallos=1
fi

echo; echo "== 2. .apifast sigue en extensions.bruno.ignore"
if python3 -I -c '
import sys, yaml
d = yaml.safe_load(open("opencollection.yml"))
ignore = ((d.get("extensions") or {}).get("bruno") or {}).get("ignore") or []
print("ignore =", ignore)
sys.exit(0 if ".apifast" in ignore else 1)
'; then
  echo "OK"
else
  echo "FALLO: .apifast ya no está en la lista"; fallos=1
fi

echo; echo "== 3. Archivos que Bruno reescribió (informativo)"
git status --short --untracked-files=all
echo
git --no-pager diff --stat
echo
git --no-pager diff

echo
if [ "$fallos" -eq 0 ]; then echo "RESULTADO: OK"; else echo "RESULTADO: FALLO"; fi
exit "$fallos"
