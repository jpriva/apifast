#!/usr/bin/env sh
# Fails if a binary needs a glibc newer than the one given (2.35 by
# default, Ubuntu 22.04's): the oldest the Linux packages promise.
#
#   packaging/linux/check-glibc.sh [max] [binary...]
set -eu

max=${1:-2.35}
[ $# -gt 0 ] && shift
[ $# -gt 0 ] || set -- target/release/apifast target/release/apifast-gui

status=0
for binary in "$@"; do
    needed=$(objdump -T "$binary" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -t. -k1,1n -k2,2n -u | tail -1)
    newest=$(printf '%s\n%s\n' "$needed" "$max" | sort -t. -k1,1n -k2,2n | tail -1)
    if [ "$newest" != "$max" ] && [ "$needed" != "$max" ]; then
        echo "$binary needs glibc $needed, newer than $max" >&2
        status=1
    else
        echo "$binary needs glibc $needed (at most $max)"
    fi
done
exit $status
