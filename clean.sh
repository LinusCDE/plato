#!/bin/sh

cargo clean

rm -rf dist/ libs/

rm -f "plato-*.zip"

if ls | grep "mupdf-*.tar.xz" >/dev/null 2>&1; then
  # download_headers.sh was likely executed to get headers
  rm -rf thirdparty/mupdf/include
  rm "mupdf-*.tar.xz"
fi
