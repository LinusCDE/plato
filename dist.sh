#! /bin/sh

[ -d dist ] && rm -Rf dist

[ -d bin ] || ./download.sh 'bin/*'
[ -d resources ] || ./download.sh 'resources/*'
[ -d hyphenation-patterns ] || ./download.sh 'hyphenation-patterns/*'
[ -e target/armv7-unknown-linux-gnueabihf/release/plato ] || ./build.sh

mkdir -p dist/libs
mkdir dist/dictionaries
mkdir dist/media # Used for default library

cp libs/libz.so dist/libs/libz.so.1
cp libs/libbz2.so dist/libs/libbz2.so.1.0

cp libs/libpng16.so dist/libs/libpng16.so.16
cp libs/libjpeg.so dist/libs/libjpeg.so.9
cp libs/libopenjp2.so dist/libs/libopenjp2.so.7
cp libs/libjbig2dec.so dist/libs/libjbig2dec.so.0

cp libs/libfreetype.so dist/libs/libfreetype.so.6
cp libs/libharfbuzz.so dist/libs/libharfbuzz.so.0

cp libs/libgumbo.so dist/libs/libgumbo.so.1
cp libs/libdjvulibre.so dist/libs/libdjvulibre.so.21
cp libs/libmupdf.so dist/libs

cp -R hyphenation-patterns dist
cp -R keyboard-layouts dist
cp -R bin dist
cp -R scripts dist
cp -R icons dist
cp -R resources dist
cp -R fonts dist
cp -R css dist
find dist/css -name '*-user.css' -delete
find dist/keyboard-layouts -name '*-user.json' -delete
find dist/hyphenation-patterns -name '*.bounds' -delete
find dist/scripts -name 'wifi-*-*.sh' -delete
[ -d target/arm-unknown-linux-gnueabihf ] && cp target/arm-unknown-linux-gnueabihf/release/plato dist/
[ -d target/armv7-unknown-linux-gnueabihf ] && cp target/armv7-unknown-linux-gnueabihf/release/plato dist/
cp contrib/*.sh dist
cp contrib/Settings-sample.toml dist
cp LICENSE-AGPLv3 dist

patchelf --remove-rpath dist/libs/*

# If strip is missing, first try to find command of default arm toolchain on system
if [ -z $STRIP ]; then
  STRIP_ARM_DEFAULT='arm-linux-gnueabihf-strip'
  if command -v $STRIP_ARM_DEFAULT >/dev/null; then
    STRIP="$STRIP_ARM_DEFAULT"
  fi
fi

# If strip is still missing, try to find command of oecore toolchain at default location
if [ -z $STRIP ]; then
  STRIP_OECORE='/usr/local/oecore-x86_64/sysroots/x86_64-oesdk-linux/usr/bin/arm-oe-linux-gnueabi/arm-oe-linux-gnueabi-strip'
  if [ -f "$STRIP_OECORE" ]; then
    STRIP="$STRIP_OECORE"
  fi
fi
$STRIP dist/plato dist/libs/*
