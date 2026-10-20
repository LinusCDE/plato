#!/usr/bin/env python3
"""Patch MuPDF 1.23.11 (thirdparty/mupdf) for faster JPX on the reMarkable 2.

1. Decode JPEG 2000 images at reduced resolution natively (OpenJPEG's
   resolution factor) when MuPDF only needs a smaller version, instead of
   decoding at full size and subsampling afterwards.
2. Allow up to two extra reduction levels for JPX images that have a mask: in MRC
   scans (Internet Archive, PDFium) the full resolution JPX is only the
   colour of a full resolution 1-bit mask that carries all the detail.
3. Decode JPX with two threads (the rM2 has two cores).
4. Load simple JPX images in PDFs lazily, as compressed images, instead of
   decoding them at full resolution when the image object is loaded, so
   the reduced decoding above applies to them.

5. Read only the codestream header for those lazily loaded images (MuPDF's
   fz_load_jpx_info decodes the whole image just to report its size).
6. Decode unmasked colour JPX one level lower when that still covers 7/8
   of the displayed size (book covers that are just under twice the
   screen size were decoded in full).
7. Let JPX images decode in parallel: OpenJPEG allocates with malloc/free
   instead of through a global context pointer guarded by a process-wide
   lock (Plato uses the default allocator anyway).

Idempotent: running it twice leaves the tree unchanged.
"""
import sys
from pathlib import Path

root = Path(sys.argv[1])
MARK = "/* plato-jpx */"


def patch(rel, pairs, mark=MARK):
    path = root / rel
    text = path.read_text()
    if mark in text:
        print(f"{rel}: already patched")
        return
    for old, new in pairs:
        if text.count(old) != 1:
            sys.exit(f"{rel}: expected exactly one match for:\n{old}")
        text = text.replace(old, new)
    path.write_text(text)
    print(f"{rel}: patched")


patch("source/fitz/load-jpx.c", [
    # Thread the requested reduction through the reader.
    ("""static fz_pixmap *
jpx_read_image(fz_context *ctx, fz_jpxd *state, const unsigned char *data, size_t size, fz_colorspace *defcs, int onlymeta)
{""",
     """static fz_pixmap *
jpx_read_image(fz_context *ctx, fz_jpxd *state, const unsigned char *data, size_t size, fz_colorspace *defcs, int onlymeta, int *reduce) /* plato-jpx */
{"""),
    ("""	if (!opj_setup_decoder(codec, &params))
	{
		opj_destroy_codec(codec);
		fz_throw(ctx, FZ_ERROR_GENERIC, "j2k decode failed");
	}
""",
     """	if (!opj_setup_decoder(codec, &params))
	{
		opj_destroy_codec(codec);
		fz_throw(ctx, FZ_ERROR_GENERIC, "j2k decode failed");
	}
	if (!onlymeta)
		opj_codec_set_threads(codec, 2);
"""),
    ("""	if (!opj_decode(codec, stream, jpx))
	{""",
     """	if (reduce && *reduce > 0)
	{
		/* Only for the simple layout where the reduced components map
		 * one to one onto the pixmap; anything else decodes in full. */
		OPJ_UINT32 c;
		int simple = jpx->x0 == 0 && jpx->y0 == 0;
		for (c = 0; c < jpx->numcomps; c++)
			if (jpx->comps[c].dx != 1 || jpx->comps[c].dy != 1)
				simple = 0;
		if (!simple || !opj_set_decoded_resolution_factor(codec, *reduce))
			*reduce = 0;
	}

	if (!opj_decode(codec, stream, jpx))
	{"""),
    ("""	w = state->width = jpx->x1 - jpx->x0;
	h = state->height = jpx->y1 - jpx->y0;
""",
     """	w = state->width = jpx->x1 - jpx->x0;
	h = state->height = jpx->y1 - jpx->y0;
	if (reduce && *reduce > 0)
	{
		/* The image keeps its full size; the components are reduced. */
		w = jpx->comps[0].w;
		h = jpx->comps[0].h;
	}
"""),
    ("""		pix = jpx_read_image(ctx, &state, data, size, defcs, 0);""",
     """		pix = jpx_read_image(ctx, &state, data, size, defcs, 0, NULL);"""),
    ("""		jpx_read_image(ctx, &state, data, size, NULL, 1);""",
     """		jpx_read_image(ctx, &state, data, size, NULL, 1, NULL);"""),
    ("""void
fz_load_jpx_info(fz_context *ctx, const unsigned char *data, size_t size, int *wp, int *hp, int *xresp, int *yresp, fz_colorspace **cspacep)
{
	fz_jpxd state = { 0 };
""",
     """fz_pixmap *
fz_load_jpx_reduced(fz_context *ctx, const unsigned char *data, size_t size, fz_colorspace *defcs, int *reduce)
{
	fz_jpxd state = { 0 };
	fz_pixmap *pix = NULL;

	fz_try(ctx)
	{
		opj_lock(ctx);
		pix = jpx_read_image(ctx, &state, data, size, defcs, 0, reduce);
	}
	fz_always(ctx)
		opj_unlock(ctx);
	fz_catch(ctx)
		fz_rethrow(ctx);

	return pix;
}

void
fz_load_jpx_info(fz_context *ctx, const unsigned char *data, size_t size, int *wp, int *hp, int *xresp, int *yresp, fz_colorspace **cspacep)
{
	fz_jpxd state = { 0 };
"""),
    # Builds without JPX support still need the symbol.
    ("""#else /* FZ_ENABLE_JPX */
""",
     """#else /* FZ_ENABLE_JPX */

fz_pixmap *
fz_load_jpx_reduced(fz_context *ctx, const unsigned char *data, size_t size, fz_colorspace *defcs, int *reduce)
{
	fz_throw(ctx, FZ_ERROR_GENERIC, "JPX support disabled");
}
"""),
])

patch("source/fitz/image-imp.h", [
    ("""void fz_load_jpx_info(""",
     """/* plato-jpx */ /* Decode at 1/2^*reduce size; *reduce returns what was applied. */
fz_pixmap *fz_load_jpx_reduced(fz_context *ctx, const unsigned char *data, size_t size, fz_colorspace *cs, int *reduce);
void fz_load_jpx_info("""),
])

patch("source/fitz/image.c", [
    ("""	case FZ_IMAGE_JPX:
		tile = fz_load_jpx(ctx, image->buffer->buffer->data, image->buffer->buffer->len, image->super.colorspace);
		break;""",
     """	case FZ_IMAGE_JPX: /* plato-jpx */
		{
			int reduce = l2factor ? *l2factor : 0;
			tile = fz_load_jpx_reduced(ctx, image->buffer->buffer->data, image->buffer->buffer->len, image->super.colorspace, &reduce);
			if (l2factor)
				*l2factor -= reduce;
		}
		break;"""),
    ("""		while (image->w>>(l2factor+1) >= w+2 && image->h>>(l2factor+1) >= h+2 && l2factor < 6)
			l2factor++;
	}
""",
     """		while (image->w>>(l2factor+1) >= w+2 && image->h>>(l2factor+1) >= h+2 && l2factor < 6)
			l2factor++;

		/* JPX colour layers under a mask (MRC scans) only provide colour;
		 * the full resolution mask provides the detail. Decode them up to
		 * two levels lower (down to a quarter of the displayed size),
		 * which JPX does natively at a fraction of the cost. */
		if (image->mask)
		{
			fz_compressed_buffer *cbuf = fz_compressed_image_buffer(ctx, image);
			int extra = 0;
			if (cbuf && cbuf->params.type == FZ_IMAGE_JPX)
				while (extra < 2 && l2factor < 6 &&
					image->w>>(l2factor+1) >= (w+3)/4 && image->h>>(l2factor+1) >= (h+3)/4)
				{
					l2factor++;
					extra++;
				}
		}
	}
"""),
])


patch("source/pdf/pdf-image.c", [
    ("""#include "mupdf/fitz.h"
#include "mupdf/pdf.h"
""",
     """#include "mupdf/fitz.h"
#include "mupdf/pdf.h"
#include "../fitz/image-imp.h" /* plato-jpx */
"""),
    ("""	buf = pdf_load_stream(ctx, dict);

	/* FIXME: We can't handle decode arrays for indexed images currently */
	fz_try(ctx)
	{
		unsigned char *data;
		size_t len;

		obj = pdf_dict_get(ctx, dict, PDF_NAME(ColorSpace));
		if (obj)
			colorspace = pdf_load_colorspace(ctx, obj);

		len = fz_buffer_storage(ctx, buf, &data);
		pix = fz_load_jpx(ctx, data, len, colorspace);
""",
     """	buf = pdf_load_stream(ctx, dict);

	/* FIXME: We can't handle decode arrays for indexed images currently */
	fz_try(ctx)
	{
		unsigned char *data;
		size_t len;

		obj = pdf_dict_get(ctx, dict, PDF_NAME(ColorSpace));
		if (obj)
			colorspace = pdf_load_colorspace(ctx, obj);

		len = fz_buffer_storage(ctx, buf, &data);

		/* Simple JPX images (no decode array, no alpha in the codestream,
		 * not indexed) become compressed images that are decoded when
		 * drawn, at the reduced size the drawing needs. */
		if (!pdf_dict_geta(ctx, dict, PDF_NAME(Decode), PDF_NAME(D)) &&
			!pdf_dict_get_int(ctx, dict, PDF_NAME(SMaskInData)) &&
			!fz_colorspace_is_indexed(ctx, colorspace))
		{
			int w, h, xres, yres;
			fz_colorspace *jpxcs = NULL;
			fz_compressed_buffer *cbuf;

			fz_load_jpx_info(ctx, data, len, &w, &h, &xres, &yres, &jpxcs);
			if (colorspace && jpxcs && fz_colorspace_n(ctx, colorspace) != fz_colorspace_n(ctx, jpxcs))
			{
				fz_drop_colorspace(ctx, colorspace);
				colorspace = NULL;
			}
			if (!colorspace)
				colorspace = jpxcs;
			else
				fz_drop_colorspace(ctx, jpxcs);

			obj = pdf_dict_geta(ctx, dict, PDF_NAME(SMask), PDF_NAME(Mask));
			if (pdf_is_dict(ctx, obj) && !forcemask)
				mask = pdf_load_image_imp(ctx, doc, NULL, obj, NULL, 1);

			cbuf = fz_malloc_struct(ctx, fz_compressed_buffer);
			cbuf->params.type = FZ_IMAGE_JPX;
			cbuf->buffer = fz_keep_buffer(ctx, buf);
			img = fz_new_image_from_compressed_buffer(ctx, w, h, 8, colorspace, xres, yres, 0, 0, NULL, NULL, cbuf, mask);
			break;
		}

		pix = fz_load_jpx(ctx, data, len, colorspace);
"""),
])


MARK2 = "/* plato-jpx-header */"

patch("source/fitz/load-jpx.c", [
    ("""	if (reduce && *reduce > 0)
	{
		/* Only for the simple layout""",
     """	if (onlymeta == 2) /* plato-jpx-header */
	{
		/* Size and number of components from the header alone. */
		state->width = jpx->x1 - jpx->x0;
		state->height = jpx->y1 - jpx->y0;
		state->xres = 72;
		state->yres = 72;
		state->cs = NULL;
		*reduce = jpx->numcomps;
		opj_stream_destroy(stream);
		opj_destroy_codec(codec);
		opj_image_destroy(jpx);
		return NULL;
	}

	if (reduce && *reduce > 0)
	{
		/* Only for the simple layout"""),
    ("""	if (!onlymeta)
		opj_codec_set_threads(codec, 2);
""",
     """	if (!onlymeta)
		opj_codec_set_threads(codec, 2);
	/* plato-jpx-header: onlymeta == 2 stops after the header. */
"""),
    ("""fz_pixmap *
fz_load_jpx_reduced(fz_context *ctx, const unsigned char *data, size_t size, fz_colorspace *defcs, int *reduce)
{
	fz_jpxd state = { 0 };""",
     """void
fz_load_jpx_header(fz_context *ctx, const unsigned char *data, size_t size, int *wp, int *hp, int *np)
{
	fz_jpxd state = { 0 };
	int n = 0;

	fz_try(ctx)
	{
		opj_lock(ctx);
		jpx_read_image(ctx, &state, data, size, NULL, 2, &n);
	}
	fz_always(ctx)
		opj_unlock(ctx);
	fz_catch(ctx)
		fz_rethrow(ctx);

	*wp = state.width;
	*hp = state.height;
	*np = n;
}

fz_pixmap *
fz_load_jpx_reduced(fz_context *ctx, const unsigned char *data, size_t size, fz_colorspace *defcs, int *reduce)
{
	fz_jpxd state = { 0 };"""),
    ("""#else /* FZ_ENABLE_JPX */
""",
     """#else /* FZ_ENABLE_JPX */

void
fz_load_jpx_header(fz_context *ctx, const unsigned char *data, size_t size, int *wp, int *hp, int *np)
{
	fz_throw(ctx, FZ_ERROR_GENERIC, "JPX support disabled");
}
"""),
], MARK2)

patch("source/fitz/image-imp.h", [
    ("""fz_pixmap *fz_load_jpx_reduced(""",
     """/* plato-jpx-header */ /* Width, height and component count from the codestream header. */
void fz_load_jpx_header(fz_context *ctx, const unsigned char *data, size_t size, int *w, int *h, int *n);
fz_pixmap *fz_load_jpx_reduced("""),
], MARK2)

patch("source/pdf/pdf-image.c", [
    ("""		{
			int w, h, xres, yres;
			fz_colorspace *jpxcs = NULL;
			fz_compressed_buffer *cbuf;

			fz_load_jpx_info(ctx, data, len, &w, &h, &xres, &yres, &jpxcs);
			if (colorspace && jpxcs && fz_colorspace_n(ctx, colorspace) != fz_colorspace_n(ctx, jpxcs))
			{
				fz_drop_colorspace(ctx, colorspace);
				colorspace = NULL;
			}
			if (!colorspace)
				colorspace = jpxcs;
			else
				fz_drop_colorspace(ctx, jpxcs);
""",
     """		{
			int w, h, n;
			fz_compressed_buffer *cbuf;

			/* plato-jpx-header */
			fz_load_jpx_header(ctx, data, len, &w, &h, &n);
			if (colorspace && fz_colorspace_n(ctx, colorspace) != n)
			{
				fz_drop_colorspace(ctx, colorspace);
				colorspace = NULL;
			}
			if (!colorspace && n == 1)
				colorspace = fz_keep_colorspace(ctx, fz_device_gray(ctx));
			else if (!colorspace && n == 3)
				colorspace = fz_keep_colorspace(ctx, fz_device_rgb(ctx));
			if (!colorspace)
				goto decode_now; /* alpha, CMYK or unusual: decode as before */
"""),
    ("""			img = fz_new_image_from_compressed_buffer(ctx, w, h, 8, colorspace, xres, yres, 0, 0, NULL, NULL, cbuf, mask);
			break;
		}

		pix = fz_load_jpx(ctx, data, len, colorspace);
""",
     """			img = fz_new_image_from_compressed_buffer(ctx, w, h, 8, colorspace, 72, 72, 0, 0, NULL, NULL, cbuf, mask);
			break;
		}

decode_now:
		pix = fz_load_jpx(ctx, data, len, colorspace);
"""),
], MARK2)


MARK3 = "/* plato-jpx-near */"

patch("source/fitz/image.c", [
    ("""					l2factor++;
					extra++;
				}
		}
	}
""",
     """					l2factor++;
					extra++;
				}
		}
		/* plato-jpx-near */
		/* Unmasked colour JPX (covers, MRC backgrounds) that is only a
		 * little larger than needed, which the search above leaves at full
		 * size: decode it one level lower when that still gives at least
		 * 7/8 of the size. That is 4x less decoding, and the small upscale
		 * doesn't show on e-ink. Grey images, usually text, keep their
		 * full resolution. */
		else if (image->n >= 3 && l2factor < 6 &&
			image->w>>(l2factor+1) >= w*7/8 && image->h>>(l2factor+1) >= h*7/8)
		{
			fz_compressed_buffer *cbuf = fz_compressed_image_buffer(ctx, image);
			if (cbuf && cbuf->params.type == FZ_IMAGE_JPX)
				l2factor++;
		}
	}
"""),
], mark=MARK3)


MARK4 = "/* plato-jpx-unlocked */"

# OpenJPEG's allocations go through opj_malloc & co, which MuPDF routes to
# the context of the decode in progress through one global variable, so it
# only lets one JPX image decode at a time in the whole process. Plato uses
# the default allocator, so plain malloc/free do the same job and two pages
# (or two layers of a page) can decode at once.
patch("source/fitz/load-jpx.c", [
    ("""void opj_lock(fz_context *ctx)
{
	fz_lock(ctx, FZ_LOCK_FREETYPE);

	set_opj_context(ctx);
}

void opj_unlock(fz_context *ctx)
{
	set_opj_context(NULL);

	fz_unlock(ctx, FZ_LOCK_FREETYPE);
}

void *opj_malloc(size_t size)
{
	fz_context *ctx = get_opj_context();

	assert(ctx != NULL);

	return Memento_label(fz_malloc_no_throw(ctx, size), "opj_malloc");
}

void *opj_calloc(size_t n, size_t size)
{
	fz_context *ctx = get_opj_context();

	assert(ctx != NULL);

	return fz_calloc_no_throw(ctx, n, size);
}

void *opj_realloc(void *ptr, size_t size)
{
	fz_context *ctx = get_opj_context();

	assert(ctx != NULL);

	return fz_realloc_no_throw(ctx, ptr, size);
}

void opj_free(void *ptr)
{
	fz_context *ctx = get_opj_context();

	assert(ctx != NULL);

	fz_free(ctx, ptr);
}
""",
     """/* plato-jpx-unlocked */
/* Plato uses MuPDF's default allocator (malloc and free), so OpenJPEG can
 * allocate directly instead of through the context of the decode in
 * progress, and decodes no longer need to exclude each other. */
void opj_lock(fz_context *ctx)
{
	(void)set_opj_context;
	(void)get_opj_context;
}

void opj_unlock(fz_context *ctx)
{
}

void *opj_malloc(size_t size)
{
	return malloc(size);
}

void *opj_calloc(size_t n, size_t size)
{
	return calloc(n, size);
}

void *opj_realloc(void *ptr, size_t size)
{
	return realloc(ptr, size);
}

void opj_free(void *ptr)
{
	free(ptr);
}
"""),
], mark=MARK4)
