#include <mupdf/fitz.h>

#define WRAP(name, ret_type, failure_val, call, ...) \
    ret_type mp_##name(fz_context *ctx, ##__VA_ARGS__) { \
        ret_type ret; \
        fz_try (ctx) { ret = call; } \
        fz_catch (ctx) { ret = failure_val; } \
        return ret; \
    }

WRAP(open_document, fz_document*, NULL, fz_open_document(ctx, path), char *path)
WRAP(open_document_with_stream, fz_document*, NULL, fz_open_document_with_stream(ctx, kind, stream), const char *kind, fz_stream *stream)
WRAP(load_page, fz_page*, NULL, fz_load_page(ctx, doc, pageno), fz_document *doc, int pageno)
WRAP(load_outline, fz_outline*, NULL, fz_load_outline(ctx, doc), fz_document *doc)
WRAP(load_links, fz_link*, NULL, fz_load_links(ctx, page), fz_page *page)
WRAP(count_pages, int, -1, fz_count_pages(ctx, doc), fz_document *doc)
WRAP(page_number_from_location, int, -1, fz_page_number_from_location(ctx, doc, loc), fz_document *doc, fz_location loc)
WRAP(new_pixmap_from_page, fz_pixmap*, NULL, fz_new_pixmap_from_page(ctx, page, mat, cs, alpha), fz_page *page, fz_matrix mat, fz_colorspace *cs, int alpha)
WRAP(new_stext_page_from_page, fz_stext_page*, NULL, fz_new_stext_page_from_page(ctx, page, options), fz_page *page, fz_stext_options *options)

/* Rendering a page on two cores.
 *
 * Scanned pages spend their time decoding a few large images, then scaling
 * and compositing them. The images are first decoded into the store that
 * all the clones of a context share, split between two threads. Then the
 * top and bottom halves of the page are drawn at the same time, both
 * finding the decoded images in the store. */

#include <pthread.h>
#include <string.h>

typedef struct {
	fz_device super;
	int *next; /* Shared: the first image neither thread has taken yet. */
	int count;
} predecode_device;

typedef struct {
	fz_context *ctx;
	fz_display_list *list;
	fz_matrix ctm;
	fz_pixmap *dest; /* NULL: decode images, otherwise: draw into this band. */
	int *next;
	int failed;
} render_job;

/* Decodes the images the other thread hasn't taken, the way the draw device
 * will ask for them, so that it finds the results in the store. Both threads
 * see the images in the same order; each takes the next one when it's free,
 * so a long decode on one side doesn't hold up the rest. */
static void predecode(fz_context *ctx, fz_device *dev_, fz_image *image, fz_matrix ctm)
{
	predecode_device *dev = (predecode_device *)dev_;
	fz_pixmap *pix = NULL;
	int index = dev->count++;

	fz_var(pix);

	if (!__atomic_compare_exchange_n(dev->next, &index, index + 1, 0, __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE))
		return;
	if (image->w == 0 || image->h == 0)
		return;

	ctm = fz_gridfit_matrix(0, ctm);
	fz_try(ctx)
		pix = fz_get_pixmap_from_image(ctx, image, NULL, &ctm, NULL, NULL);
	fz_catch(ctx)
		; /* The draw device will run into it and report it. */
	fz_drop_pixmap(ctx, pix);
}

static void predecode_fill_image(fz_context *ctx, fz_device *dev, fz_image *image, fz_matrix ctm, float alpha, fz_color_params color_params)
{
	predecode(ctx, dev, image, ctm);
}

static void predecode_fill_image_mask(fz_context *ctx, fz_device *dev, fz_image *image, fz_matrix ctm, fz_colorspace *cs, const float *color, float alpha, fz_color_params color_params)
{
	predecode(ctx, dev, image, ctm);
}

static void predecode_clip_image_mask(fz_context *ctx, fz_device *dev, fz_image *image, fz_matrix ctm, fz_rect scissor)
{
	predecode(ctx, dev, image, ctm);
}

static void run_job(render_job *job)
{
	fz_context *ctx = job->ctx;
	fz_device *dev = NULL;

	fz_var(dev);

	fz_try(ctx)
	{
		if (job->dest)
		{
			fz_rect band = fz_rect_from_irect(fz_pixmap_bbox(ctx, job->dest));
			dev = fz_new_draw_device(ctx, job->ctm, job->dest);
			fz_run_display_list(ctx, job->list, dev, fz_identity,
				fz_transform_rect(band, fz_invert_matrix(job->ctm)), NULL);
		}
		else
		{
			predecode_device *pd = fz_new_derived_device(ctx, predecode_device);
			pd->super.fill_image = predecode_fill_image;
			pd->super.fill_image_mask = predecode_fill_image_mask;
			pd->super.clip_image_mask = predecode_clip_image_mask;
			pd->next = job->next;
			dev = &pd->super;
			fz_run_display_list(ctx, job->list, dev, job->ctm, fz_infinite_rect, NULL);
		}
		fz_close_device(ctx, dev);
	}
	fz_always(ctx)
		fz_drop_device(ctx, dev);
	fz_catch(ctx)
		job->failed = 1;
}

static void *run_job_thread(void *job)
{
	run_job(job);
	return NULL;
}

/* Runs jobs[1] in a new thread with a clone of ctx, and jobs[0] in this one. */
static int run_jobs(fz_context *ctx, render_job jobs[2])
{
	pthread_t thread;
	int threaded;

	jobs[0].ctx = ctx;
	jobs[1].ctx = fz_clone_context(ctx);
	if (jobs[1].ctx == NULL)
		jobs[1].ctx = ctx;
	threaded = jobs[1].ctx != ctx && pthread_create(&thread, NULL, run_job_thread, &jobs[1]) == 0;

	run_job(&jobs[0]);
	if (threaded)
		pthread_join(thread, NULL);
	else
		run_job(&jobs[1]);

	if (jobs[1].ctx != ctx)
		fz_drop_context(jobs[1].ctx);
	return jobs[0].failed || jobs[1].failed;
}

static fz_pixmap *new_pixmap_from_page_parallel(fz_context *ctx, fz_page *page, fz_matrix ctm, fz_colorspace *cs, int alpha)
{
	fz_display_list *list = NULL;
	fz_pixmap *pix = NULL;
	fz_pixmap *bands[2] = { NULL, NULL };
	render_job jobs[2];
	fz_irect bbox, top, bottom;
	int mid, i, next = 0;

	fz_var(list);
	fz_var(pix);
	fz_var(bands);

	fz_try(ctx)
	{
		bbox = fz_round_rect(fz_transform_rect(fz_bound_page(ctx, page), ctm));
		list = fz_new_display_list_from_page(ctx, page);
		pix = fz_new_pixmap_with_bbox(ctx, cs, bbox, NULL, alpha);
		if (alpha)
			fz_clear_pixmap(ctx, pix);
		else
			fz_clear_pixmap_with_value(ctx, pix, 0xFF);

		memset(jobs, 0, sizeof jobs);
		for (i = 0; i < 2; i++)
		{
			jobs[i].list = list;
			jobs[i].ctm = ctm;
			jobs[i].next = &next;
		}
		/* A failure here only means the bands decode for themselves. */
		run_jobs(ctx, jobs);

		mid = bbox.y0 + (bbox.y1 - bbox.y0) / 2;
		top = bottom = bbox;
		top.y1 = bottom.y0 = mid;
		bands[0] = fz_new_pixmap_with_bbox_and_data(ctx, cs, top, NULL, alpha, pix->samples);
		bands[1] = fz_new_pixmap_with_bbox_and_data(ctx, cs, bottom, NULL, alpha,
			pix->samples + (size_t)(mid - bbox.y0) * pix->stride);
		for (i = 0; i < 2; i++)
		{
			jobs[i].dest = bands[i];
			jobs[i].failed = 0;
		}
		if (run_jobs(ctx, jobs))
			fz_throw(ctx, FZ_ERROR_GENERIC, "cannot draw page");
	}
	fz_always(ctx)
	{
		fz_drop_pixmap(ctx, bands[0]);
		fz_drop_pixmap(ctx, bands[1]);
		fz_drop_display_list(ctx, list);
	}
	fz_catch(ctx)
	{
		fz_drop_pixmap(ctx, pix);
		return NULL;
	}

	return pix;
}

fz_pixmap *mp_new_pixmap_from_page_parallel(fz_context *ctx, fz_page *page, fz_matrix ctm, fz_colorspace *cs, int alpha)
{
	fz_rect rect = fz_empty_rect;

	fz_var(rect);

	fz_try(ctx)
		rect = fz_transform_rect(fz_bound_page(ctx, page), ctm);
	fz_catch(ctx)
		return NULL;

	/* Not worth a thread. */
	if (rect.y1 - rect.y0 < 64)
		return mp_new_pixmap_from_page(ctx, page, ctm, cs, alpha);
	return new_pixmap_from_page_parallel(ctx, page, ctm, cs, alpha);
}
