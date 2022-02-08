[![rm1](https://img.shields.io/badge/rM1-supported-green)](https://remarkable.com/store/remarkable)
[![rm2](https://img.shields.io/badge/rM2-supported-green)](https://remarkable.com/store/remarkable-2)
[![opkg](https://img.shields.io/badge/OPKG-plato-blue)](https://github.com/matteodelabre/toltec)
[![launchers](https://img.shields.io/badge/Launchers-supported-green)](https://github.com/reHackable/awesome-reMarkable#launchers)

![Logo](artworks/plato-logo.svg)

*Plato* is a document reader for *Kobo*'s e-readers.

**This is a fork for the reMarkable.**

Also see [darvin](https://github.com/darvin)'s [work](https://github.com/darvin/plato) of
porting plato to the reMarkable 2 years ago. ( I read nice mangas with his port. :) )

## Install on reMarkable

### Using an existing build

- Go the [releases](https://github.com/LinusCDE/plato/releases) and get the latest build (attached file with "dist" in the name).
- Copy the unpacked contents that file to your device to a folder of your choice (e.g. using scp or an sftp client like Filezilla or WinSCP)
- SSH into your reMarkable and run the file `plato.sh`. E.g. `/home/root/plato/plato.sh`
- Add it to [draft](https://github.com/dixonary/draft-reMarkable) for easier launching without SSH-ing.

By default files are used from the empty media folder you got in the release. You can change this in the Settings.toml.

Notes: The software automatically recognizes the running UI (xochitl) and kills it. If it did so, you'll only have the option to "Quit to Xochitl", which will automatically start xochitl again when quitting. Should the software crash (had one case with a bad czb file) the screen will seem frozen. Either launch draft again, or hold the power button for about 10 seconds (= poweroff) and then hold it again to start the device again.

### Compiling yourself

You need rust (nightly) the oecore toolchain and the armv7-unknown-linux-gnueabihf target.

After that, you can build the software using build.sh and create the same directory as attached using `dist.sh` (folder dist/).

Or just look at the file `make_remarkable.sh` which checks the above condition and runs a full clean build for you.
Documentation: [GUIDE](doc/GUIDE.md), [MANUAL](doc/MANUAL.md) and [BUILD](doc/BUILD.md).

<img width="45%" src="https://transfer.cosmos-ink.net/4fvGf/screen1.jpg"> &nbsp; <img width="45%" src="https://transfer.cosmos-ink.net/skyR8/screen2.jpg">
<img width="45%" src="https://transfer.cosmos-ink.net/Ma5a9/screen3.jpg">

## reMarkable 2 support

The reMarkable 2 is supported as well. Some features were initially added with the rM 1 in mind and may therefore have deminishing functionaility on the rM 2. An example are the refresh modes, which will not be that granular due to the reduced access to the framebuffer.

Otherwise everything should work fine.

## Supported firmwares

Any 4.*X*.*Y* firmware, with *X* ≥ 6, will do.

## Supported devices

- *Libra 2*.
- *Sage*.
- *Elipsa*.
- *Libra H₂O*.
- *Forma*.
- *Clara HD*.
- *Aura H₂O Edition 2*.
- *Aura Edition 2*.
- *Aura ONE*.
- *Glo HD*.
- *Aura H₂O*.
- *Aura*.
- *Glo*.
- *Touch C*.

## Supported formats

- PDF, CBZ, FB2 and XPS via [MuPDF](https://mupdf.com/index.html).
- ePUB through a built-in renderer.
- DJVU via [DjVuLibre](http://djvu.sourceforge.net/index.html).

## Features

- Crop the margins.
- Continuous fit-to-width zoom mode with line preserving cuts.
- Rotate the screen (portrait ↔ landscape).
- Adjust the contrast.
- Define words using *dictd* dictionaries.
- Annotations, highlights and bookmarks.
- Retrieve articles from online sources through [hooks](doc/HOOKS.md) (an example *wallabag* [article fetcher](doc/ARTICLE_FETCHER.md) is provided).

[![Tn01](artworks/thumbnail01.png)](artworks/screenshot01.png) [![Tn02](artworks/thumbnail02.png)](artworks/screenshot02.png) [![Tn03](artworks/thumbnail03.png)](artworks/screenshot03.png) [![Tn04](artworks/thumbnail04.png)](artworks/screenshot04.png)

## Donations

[![Donate](https://img.shields.io/badge/Donate-PayPal-green.svg)](https://www.paypal.com/cgi-bin/webscr?cmd=_s-xclick&hosted_button_id=KNAR2VKYRYUV6)
