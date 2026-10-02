/* Host harness for the early splash: includes the real renderer and progress
 * code and prints frames or progress facts. No DRM device is touched.
 * Build: cc -Os tests/splash_picture.c $(pkg-config --cflags --libs libdrm) */
#define main splash_entry
#include "../tools/graphics/reborn-splash.c"
#undef main
#include <assert.h>
static void ppm(const Display *d) {
    printf("P6\n480 360\n255\n");
    for (unsigned i = 0; i < 480 * 360; i++) {
        uint32_t p = ((uint32_t *)d->pixels)[i];
        unsigned char rgb[3] = {p >> 16, p >> 8, p};
        if (fwrite(rgb, 1, 3, stdout) != 3) exit(1);
    }
}
static Progress settle(char **tokens, int count) {
    Progress p = {0};
    for (int i = 0; i < count; i++) progress_report(&p, tokens[i]);
    return p;
}
/* frame TOKEN... | finish TOKEN... | failure | mid UNITS TOKEN... | report TOKEN... | steps TOKEN | table */
int main(int argc, char **argv) {
    if (argc < 2) return 2;
    Display d = {.fd = -1, .width = 480, .height = 360, .pitch = 1920, .size = 1920 * 360};
    d.pixels = malloc(d.size + 64); assert(d.pixels); memset(d.pixels, 0xaa, d.size + 64);
    if (!strcmp(argv[1], "report")) {
        Progress p = {0};
        for (int i = 2; i < argc; i++) {
            bool changed = progress_report(&p, argv[i]);
            printf("%s changed=%d phase=%u target=%u failed=%d\n", argv[i], changed, p.phase, p.target, p.failed);
        }
        return 0;
    }
    if (!strcmp(argv[1], "steps")) {
        Progress p = settle(argv + 2, argc - 2);
        unsigned ticks = 0, last = 0;
        while (progress_step(&p)) { assert(p.shown > last); last = p.shown; ticks++; assert(ticks < 400); }
        printf("ticks=%u shown=%u target=%u\n", ticks, p.shown, p.target);
        return 0;
    }
    if (!strcmp(argv[1], "table")) {
        for (unsigned i = 0; i < RB_PHASE_COUNT; i++) printf("%s %u %u\n", rb_phases[i].token, rb_phases[i].fill, fill_units(i));
        return 0;
    }
    if (!strcmp(argv[1], "finish")) {
        /* The READY path: repaint only the bar and, if it changed, the status. */
        Progress p = settle(argv + 2, argc - 2);
        p.shown = p.target / 2;
        draw(&d, &p, false);
        unsigned before = p.phase;
        progress_finish(&p);
        bar(&d, p.shown);
        if (label_changed(before, p.phase)) status(&d, p.phase);
    } else if (!strcmp(argv[1], "failure")) {
        Progress p = {0};
        draw(&d, &p, true);
    } else if (!strcmp(argv[1], "mid")) {
        Progress p = settle(argv + 3, argc - 3);
        p.shown = (unsigned)atoi(argv[2]);
        draw(&d, &p, false);
    } else if (!strcmp(argv[1], "frame")) {
        /* Settled at the last milestone and rounded to a whole pixel, as Reborn draws it. */
        Progress p = settle(argv + 2, argc - 2);
        p.shown = (p.target + FILL_UNIT / 2) / FILL_UNIT * FILL_UNIT;
        draw(&d, &p, false);
    } else return 2;
    for (size_t i = d.size; i < d.size + 64; i++) assert(d.pixels[i] == 0xaa);
    ppm(&d);
    free(d.pixels);
    return 0;
}
