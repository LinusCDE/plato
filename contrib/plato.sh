#!/bin/sh

WORKDIR=$(dirname "`realpath \"$0\"`")
cd "$WORKDIR" || exit 1

export PRODUCT=remarkable
export LD_LIBRARY_PATH=./libs

# Default to disabling built-in swtfb client (still seems not quite rock solid currently :| )
[ -z "$PLATO_DISABLE_BUILTIN_SWTFB_CLIENT" ] && export PLATO_DISABLE_BUILTIN_SWTFB_CLIENT=yes

exec ./plato
