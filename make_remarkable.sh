#!/bin/sh

cd "`dirname \"$0\"`"

if ! which rustup >/dev/null 2>&1; then
  echo 'Please install rustup either from your system package manager or https://rustup.rs/' >&2
  exit 1
fi

if ! rustup show active-toolchain | grep "nightly-" >/dev/null 2>&1; then
  echo 'Please use the nightly build.' >&2
  echo 'Run "rustup install nightly; rustup default nightly" to do so' >&2
  exit 1
fi

if ! rustup target list | grep "armv7-unknown-linux-gnueabihf (installed)" >/dev/null 2>&1; then
  echo 'You need to add the armv7-unknown-linux-gnueabihf target' >&2
  echo 'Run "rustup target add armv7-unknown-linux-gnueabihf" to do so' >&2
  exit 1
fi

if ! which arm-linux-gnueabihf-gcc >/dev/null 2>&1; then
  echo 'You need to install a toolchain for compiling arm programs.' >&2
  echo 'Search for a fitting package that provides commands like "arm-linux-gnueabihf-gcc"' >&2
  exit 1
fi

./clean.sh && \
./build.sh && \
./dist.sh || exit $?

echo
echo 'Congrats! You have compiled the plato port for the reMarkable!'
echo 'The dist/ folder contains everything you need. Put it onto your reMarklable and run ./plato.sh to use it.'
echo 'Tip: After the first launch and proper "Quit", you can make changes to the created file Settings.toml .'
