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

if [ ! -d /usr/local/oecore-x86_64/ ]; then
  echo "Couldn't find the oecore toolchain at its default location" >&2
  echo "Please install it from https://remarkable.engineering/" >&2
  exit 1
fi

./clean.sh && \
./build.sh && \
./dist.sh || exit $?

echo
echo 'Congrats! You have compiled the plato port for the reMarkable!'
echo 'The dist/ folder contains everything you need. Put it onto your reMarklable and run ./plato.sh to use it.'
echo 'Tip: After the first launch and proper "Quit", you can make changes to the created file Settings.toml .'