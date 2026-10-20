# reMarkable 2 performance build

Scripts used to build this fork for the reMarkable 2 (AppLoad/Vellum layout),
run inside `ghcr.io/toltec-dev/rust:v4.0` like Vellum's VELBUILD.

- `build.sh full` — first build: applies the VELBUILD edits in a separate
  worktree (`$PLATO_BUILD`, default `~/plato-build`) and builds the third-party libraries
  and Plato.
- `build.sh plato` — rebuilds only Plato from this checkout.
- `build-libs.sh` — rebuilds `libmupdf.so` with Cortex-A7/NEON/-O3 tuning,
  static libjpeg-turbo, and `patch-mupdf.py`.
- `patch-mupdf.py` — MuPDF 1.23.11 changes for scanned (MRC/JPEG 2000)
  PDFs: lazy, reduced-resolution, two-threaded JPX decoding.
- `bench.c` — per-step MuPDF render timing to run on the device.

Set `DOCKER="sudo docker"` if your user is not in the docker group.

Runtime knobs: `PLATO_TIMING=1`, `PLATO_QTFB=0`, `PLATO_QTFB_REFRESH_MODE`,
`PLATO_QTFB_FULL_REFRESH=0`, `PLATO_PAGE_CACHE=0`, `PLATO_PAGE_CACHE_MB` (default 100; only the open book is kept),
`PLATO_INPUT_COOLDOWN_MS` (default 500), `PLATO_PARALLEL_RENDER=0`.

`bench.c` builds together with `mupdf_wrapper/mupdf_wrapper.c`.
