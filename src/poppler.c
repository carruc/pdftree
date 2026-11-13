#include <poppler.h>
#include <cairo.h>
#include <math.h>
#include <stdint.h>

PopplerDocument *pdf_open(const uint8_t *data, size_t length) {
    GBytes *bytes = g_bytes_new_static(data, length);
    PopplerDocument *doc = poppler_document_new_from_bytes(bytes, NULL, NULL);
    g_bytes_unref(bytes);
    return doc;
}

int pdf_count(PopplerDocument *doc) { return poppler_document_get_n_pages(doc); }
void pdf_release(void *object) { g_object_unref(object); }
void pdf_free(void *data) { g_free(data); }

PopplerPage *pdf_page(PopplerDocument *doc, int index, double *size) {
    PopplerPage *page = poppler_document_get_page(doc, index);
    if (page) poppler_page_get_size(page, size, size + 1);
    return page;
}

PopplerRectangle *pdf_text(PopplerPage *page, unsigned *count) {
    PopplerRectangle *rects = NULL;
    *count = 0;
    poppler_page_get_text_layout(page, &rects, count);
    return rects;
}

int pdf_render(PopplerPage *page, int rotation, int width, int height, uint8_t *rgba) {
    double w, h;
    poppler_page_get_size(page, &w, &h);
    cairo_surface_t *surface = cairo_image_surface_create(CAIRO_FORMAT_RGB24, width, height);
    cairo_t *cr = cairo_create(surface);
    cairo_set_source_rgb(cr, 1, 1, 1);
    cairo_paint(cr);
    cairo_scale(cr, width / (rotation % 2 ? h : w), height / (rotation % 2 ? w : h));
    switch (rotation) {
        case 1: cairo_translate(cr, h, 0); break;
        case 2: cairo_translate(cr, w, h); break;
        case 3: cairo_translate(cr, 0, w); break;
    }
    cairo_rotate(cr, rotation * G_PI_2);
    poppler_page_render(page, cr);
    cairo_surface_flush(surface);
    int ok = cairo_status(cr) == CAIRO_STATUS_SUCCESS;
    if (ok) {
        const uint8_t *data = cairo_image_surface_get_data(surface);
        int stride = cairo_image_surface_get_stride(surface);
        for (int y = 0; y < height; y++) {
            const uint32_t *row = (const uint32_t *)(data + y * stride);
            for (int x = 0; x < width; x++) {
                uint32_t pixel = row[x];
                *rgba++ = (uint8_t)(pixel >> 16);
                *rgba++ = (uint8_t)(pixel >> 8);
                *rgba++ = (uint8_t)pixel;
                *rgba++ = 255;
            }
        }
    }
    cairo_destroy(cr);
    cairo_surface_destroy(surface);
    return ok;
}
