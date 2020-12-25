#! /bin/sh

WORKDIR=$(dirname "`realpath \"$0\"`")
cd "$WORKDIR" || exit 1

export PRODUCT=remarkable
export LD_LIBRARY_PATH=./libs

./plato
