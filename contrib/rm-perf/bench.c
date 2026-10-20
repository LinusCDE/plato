// Per-step timing of MuPDF page rendering on the device.
// usage: bench file.pdf page
#include <mupdf/fitz.h>
#include <mupdf/pdf.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
#include <pthread.h>

/* Same as Plato: real locks, so contexts can be cloned for threads. */
static pthread_mutex_t mutexes[FZ_LOCK_MAX];
static void lock(void *user, int i) { pthread_mutex_lock(&mutexes[i]); }
static void unlock(void *user, int i) { pthread_mutex_unlock(&mutexes[i]); }
static fz_locks_context locks = { NULL, lock, unlock };

fz_pixmap *mp_new_pixmap_from_page_parallel(fz_context *ctx, fz_page *page, fz_matrix ctm, fz_colorspace *cs, int alpha);

static double now_ms(void)
{
	struct timespec ts;
	clock_gettime(CLOCK_MONOTONIC, &ts);
	return ts.tv_sec * 1e3 + ts.tv_nsec / 1e6;
}

static void time_image(fz_context *ctx, fz_image *img, const char *label, int w, int h)
{
	double t;
	fz_pixmap *pix;
	fz_compressed_buffer *cbuf = fz_compressed_image_buffer(ctx, img);
	int type = cbuf ? cbuf->params.type : -1;
	fz_matrix ctm = fz_scale(w, h);
	int dw, dh;

	/* full size */
	fz_empty_store(ctx);
	t = now_ms();
	pix = fz_get_pixmap_from_image(ctx, img, NULL, NULL, &dw, &dh);
	printf("  %-6s %4dx%-4d type=%d full decode: %7.0f ms -> %dx%d\n", label, img->w, img->h, type, now_ms() - t, pix->w, pix->h);
	fz_drop_pixmap(ctx, pix);

	/* at display size (lets MuPDF pick a reduction) */
	fz_empty_store(ctx);
	t = now_ms();
	pix = fz_get_pixmap_from_image(ctx, img, NULL, &ctm, &dw, &dh);
	printf("  %-6s at %dx%d: %7.0f ms -> %dx%d\n", label, w, h, now_ms() - t, pix->w, pix->h);
	fz_drop_pixmap(ctx, pix);
}

int main(int argc, char **argv)
{
	for (int m = 0; m < FZ_LOCK_MAX; m++)
		pthread_mutex_init(&mutexes[m], NULL);
	fz_context *ctx = fz_new_context(NULL, &locks, 32 << 20);
	fz_register_document_handlers(ctx);
	fz_document *doc = fz_open_document(ctx, argv[1]);
	int pno = atoi(argv[2]);
	pdf_document *pdoc = pdf_specifics(ctx, doc);
	fz_page *page = fz_load_page(ctx, doc, pno);
	fz_rect bounds = fz_bound_page(ctx, page);
	float zoom = fz_min(1404 / (bounds.x1 - bounds.x0), 1872 / (bounds.y1 - bounds.y0));
	int W = (int)((bounds.x1 - bounds.x0) * zoom), H = (int)((bounds.y1 - bounds.y0) * zoom);
	double t;

	/* every image XObject of the page */
	pdf_obj *xobjs = pdf_dict_getp(ctx, pdf_page_resources(ctx, (pdf_page *)page), "XObject");
	int i, n = pdf_dict_len(ctx, xobjs);
	for (i = 0; i < n; i++)
	{
		pdf_obj *x = pdf_dict_get_val(ctx, xobjs, i);
		if (!pdf_name_eq(ctx, pdf_dict_get(ctx, x, PDF_NAME(Subtype)), PDF_NAME(Image)))
			continue;
		fz_image *img = pdf_load_image(ctx, pdoc, x);
		printf("image %s:\n", pdf_to_name(ctx, pdf_dict_get_key(ctx, xobjs, i)));
		time_image(ctx, img, "color", W, H);
		if (img->mask)
			time_image(ctx, img->mask, "mask", W, H);
		fz_drop_image(ctx, img);
	}

	/* whole page: cold, warm, cold again; then display list phases */
	for (i = 0; i < 3; i++)
	{
		if (i != 1)
			fz_empty_store(ctx);
		t = now_ms();
		fz_pixmap *pix = fz_new_pixmap_from_page(ctx, page, fz_scale(zoom, zoom), fz_device_gray(ctx), 0);
		printf("page %s: %7.0f ms (%dx%d)\n", i == 1 ? "warm" : "cold", now_ms() - t, pix->w, pix->h);
		fz_drop_pixmap(ctx, pix);
	}
	/* Two cores (mupdf_wrapper.c), cold, compared with a one core render. */
	{
		fz_pixmap *ref, *par;
		size_t k, diff = 0;
		int maxd = 0;
		fz_empty_store(ctx);
		ref = fz_new_pixmap_from_page(ctx, page, fz_scale(zoom, zoom), fz_device_gray(ctx), 0);
		for (i = 0; i < 2; i++)
		{
			fz_empty_store(ctx);
			t = now_ms();
			par = mp_new_pixmap_from_page_parallel(ctx, page, fz_scale(zoom, zoom), fz_device_gray(ctx), 0);
			printf("page cold, two cores: %7.0f ms\n", now_ms() - t);
			if (i == 0)
			{
				for (k = 0; k < (size_t)ref->stride * ref->h; k++)
				{
					int d = abs(ref->samples[k] - par->samples[k]);
					if (d) diff++;
					if (d > maxd) maxd = d;
				}
				printf("two cores vs one: %zu of %zu pixels differ, by at most %d\n", diff, (size_t)ref->stride * ref->h, maxd);
			}
			fz_drop_pixmap(ctx, par);
		}
		fz_drop_pixmap(ctx, ref);
	}

	fz_empty_store(ctx);
	t = now_ms();
	fz_display_list *list = fz_new_display_list_from_page(ctx, page);
	printf("display list (interpret, load images lazily): %7.0f ms\n", now_ms() - t);
	t = now_ms();
	fz_pixmap *pix = fz_new_pixmap_from_display_list(ctx, list, fz_scale(zoom, zoom), fz_device_gray(ctx), 0);
	printf("draw display list (decode + composite): %7.0f ms\n", now_ms() - t);
	fz_drop_pixmap(ctx, pix);
	fz_drop_display_list(ctx, list);

	fz_drop_page(ctx, page);
	fz_drop_document(ctx, doc);
	fz_drop_context(ctx);
	return 0;
}
