/*
 * blobhost.c — run an HC01 blob natively on Linux to test hcc output
 * before booting the real kernel.
 *
 *   gcc -O0 -no-pie -o blobhost blobhost.c
 *   ./blobhost app.bin build/kernel_api.nm
 *
 * 1. mmaps the blob RWX at 0x200000 (the kernel PROG_BASE),
 * 2. mmaps a scratch page RWX at 0x68000 and plants a `jmp real_impl`
 *    stub at every HC_* address from kernel_api.nm,
 * 3. fills the HC_FB* statics with a small fake framebuffer,
 * 4. calls the blob entry and returns its value.
 */
#define _GNU_SOURCE
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
#include <time.h>

#define PROG_BASE 0x200000UL
#define API_BASE 0x68000UL
#define API_LEN 0x10000UL

typedef int64_t (*entry_t)(void);

static int n_keypolls = 0;
static uint8_t *fake_fb;
static long fb_writes = 0;
static long fill_colors[16]; // index histogram of HC_Fill colors
static long fill_bad = 0;

static int64_t h_print(int64_t fmt, int64_t a0, int64_t a1, int64_t a2, int64_t a3, int64_t a4) {
    const char *f = (const char *)fmt;
    void *args[5] = { &a0, &a1, &a2, &a3, &a4 };
    int nx = 0;
    for (const char *p = f; *p; p++) {
        if (*p == '%' && p[1]) {
            p++;
            int64_t v = *(int64_t *)args[nx < 5 ? nx : 4];
            nx++;
            switch (*p) {
            case 'd': case 'i': printf("%ld", (long)v); break;
            case 'x': printf("%lx", (unsigned long)v); break;
            case 'X': printf("%lX", (unsigned long)v); break;
            case 'c': printf("%c", (char)v); break;
            case 's': printf("%s", (char *)v); break;
            case '%': printf("%%"); break;
            default: printf("%%%c", *p);
            }
        } else {
            putchar(*p);
        }
    }
    fflush(stdout);
    return nx;
}

static int64_t h_putc(int64_t c) { putchar((int)c); fflush(stdout); return 0; }
static int64_t h_cls(void) { printf("[CLS]\n"); return 0; }
static int64_t h_beep(void) { printf("[BEEP]\n"); return 0; }
static int64_t h_snd(int64_t f) { printf("[SND %ld]\n", (long)f); return 0; }
static int64_t h_sleep(int64_t ms) { struct timespec ts = { 0, ms * 1000000 }; nanosleep(&ts, 0); return 0; }
static int64_t h_key(void) {
    // Q after the 200th poll so busy loops exit
    return ++n_keypolls > 200 ? 'Q' : -1;
}
static int64_t h_waitkey(void) {
    int64_t k;
    do { k = h_key(); if (k == -1) nanosleep(&(struct timespec){0, 1000000}, 0); } while (k == -1);
    return k;
}
static int64_t h_dir(int64_t p) { printf("[DIR %s]\n", (char *)p); return 0; }
static int64_t h_type(int64_t p) { printf("[TYPE %s]\n", (char *)p); return 0; }
static int64_t h_snake(void) { printf("[SNAKE]\n"); return 0; }
static int64_t h_life(void) { printf("[LIFE]\n"); return 0; }
static int64_t h_lines(void) { printf("[LINES]\n"); return 0; }
static int64_t h_pixel(int64_t x, int64_t y, int64_t c) {
    long w = 64, h = 16;
    if (x < 0 || y < 0 || x >= w || y >= h) return -1;
    fake_fb[y * w * 4 + x * 4] = (uint8_t)c;
    fb_writes++;
    return 0;
}
static int64_t h_fill(int64_t x, int64_t y, int64_t wd, int64_t ht, int64_t c) {
    if (c < 0 || c > 15) fill_bad++;
    else fill_colors[c & 15]++;
    for (int64_t j = 0; j < ht; j++)
        for (int64_t i = 0; i < wd; i++)
            h_pixel(x + i, y + j, c);
    return 0;
}
static int64_t h_glyph(int64_t c, int64_t r, int64_t ch, int64_t col) {
    printf("[GLYPH %c at %ld,%ld col %ld]", (char)ch, (long)c, (long)r, (long)col);
    return 0;
}

static const struct { const char *name; void *fn; } impls[] = {
    { "HC_Print", (void *)h_print },     { "HC_Putc", (void *)h_putc },
    { "HC_Cls", (void *)h_cls },         { "HC_Beep", (void *)h_beep },
    { "HC_Snd", (void *)h_snd },         { "HC_Sleep", (void *)h_sleep },
    { "HC_KEY", (void *)h_key },         { "HC_WaitKey", (void *)h_waitkey },
    { "HC_Dir", (void *)h_dir },         { "HC_Type", (void *)h_type },
    { "HC_Snake", (void *)h_snake },     { "HC_Life", (void *)h_life },
    { "HC_Lines", (void *)h_lines },     { "HC_Pixel", (void *)h_pixel },
    { "HC_Fill", (void *)h_fill },       { "HC_Glyph", (void *)h_glyph },
};

static void plant(const char *nm, unsigned long addr) {
    void *real = 0;
    for (unsigned i = 0; i < sizeof impls / sizeof impls[0]; i++) {
        if (strcmp(impls[i].name, nm) == 0) { real = impls[i].fn; break; }
    }
    if (!real) return; // statics (HC_FB*) are not calls
    unsigned char *site = (unsigned char *)addr;
    site[0] = 0xE9;
    *(int32_t *)(site + 1) = (int32_t)((unsigned char *)real - (site + 5));
}

int main(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "usage: blobhost <blob.bin> <kernel_api.nm>\n");
        return 1;
    }
    FILE *fp = fopen(argv[1], "rb");
    if (!fp) { perror(argv[1]); return 1; }
    fseek(fp, 0, SEEK_END);
    long len = ftell(fp);
    fseek(fp, 0, SEEK_SET);
    uint8_t *buf = malloc(len);
    if (fread(buf, 1, len, fp) != (size_t)len) { perror("read"); return 1; }
    fclose(fp);

    if (buf[0] != 0x48 || buf[1] != 0x43 || buf[2] != 0x30 || buf[3] != 0x31) {
        fprintf(stderr, "bad magic\n");
        return 1;
    }
    uint32_t entry_off = *(uint32_t *)(buf + 4);
    uint32_t total = *(uint32_t *)(buf + 8);
    uint32_t data_off = *(uint32_t *)(buf + 12);
    printf("blob: entry=%u total=%u data=%u code=%u\n", entry_off, total, data_off, data_off - 16);

    void *p = mmap((void *)PROG_BASE, 0x100000, PROT_READ | PROT_WRITE | PROT_EXEC,
                   MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0);
    if (p != (void *)PROG_BASE) { perror("mmap prog"); return 1; }
    memcpy((void *)PROG_BASE, buf, len);

    fake_fb = mmap((void *)0x300000, 0x80000, PROT_READ | PROT_WRITE,
                   MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0);
    if (fake_fb != (void *)0x300000) { perror("mmap fb"); return 1; }

    void *api = mmap((void *)API_BASE, API_LEN, PROT_READ | PROT_WRITE | PROT_EXEC,
                     MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0);
    if (api != (void *)API_BASE) { perror("mmap api"); return 1; }

    FILE *nm = fopen(argv[2], "r");
    if (!nm) {
        fprintf(stderr, "warning: no %s; statics FBW/FBH unset\n", argv[2]);
        // still fill defaults so apps reading FB* see something sane
        *(uint64_t *)0x68000 = 0x300000UL; // FBADDR
        *(uint32_t *)0x68008 = 16;    // FBH
        *(uint32_t *)0x6800C = 64;    // FBPITCH
        *(uint32_t *)0x68010 = 64;    // FBW
    } else {
        char line[256];
        while (fgets(line, sizeof line, nm)) {
            char n[64];
            unsigned long a;
            if (sscanf(line, "%63s %lx", n, &a) == 2) {
                if (strcmp(n, "HC_FBADDR") == 0) *(uint64_t *)a = 0x300000UL;
                else if (strcmp(n, "HC_FBW") == 0) *(uint32_t *)a = 64;
                else if (strcmp(n, "HC_FBH") == 0) *(uint32_t *)a = 16;
                else if (strcmp(n, "HC_FBPITCH") == 0) *(uint32_t *)a = 256;
                else if (strcmp(n, "HC_Print") == 0) plant(n, a);
            }
        }
        // second pass for functions (jmp stubs)
        rewind(nm);
        while (fgets(line, sizeof line, nm)) {
            char n[64];
            unsigned long a;
            if (sscanf(line, "%63s %lx", n, &a) == 2) {
                if (strcmp(n, "HC_FBADDR") != 0 && strcmp(n, "HC_FBW") != 0 &&
                    strcmp(n, "HC_FBH") != 0 && strcmp(n, "HC_FBPITCH") != 0)
                    plant(n, a);
            }
        }
        fclose(nm);
    }

    printf("--- running entry ---\n");
    entry_t e = (entry_t)(PROG_BASE + entry_off);
    int64_t r = e();
    printf("--- exit rax=%ld, fb writes=%ld ---\n", (long)r, fb_writes);
    printf("fill rainbow: ");
    for (int i = 0; i < 16; i++) printf("%ld ", fill_colors[i]);
    printf(" bad:%ld\n", fill_bad);
    return (int)(r & 0xFF);
}
