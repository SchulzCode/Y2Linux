/* SPDX-License-Identifier: GPL-3.0-or-later */
/* Music formats and Reborn library tags as umtprd reports them (patch 0005).
 * argv: media root, library.db (WAL, writer connection held open by the test),
 * a second library whose writer holds an exclusive lock. */
#include "buildconf.h"
#include <assert.h>
#include <inttypes.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include "mtp.h"
#include "mtp_helpers.h"
#include "mtp_constant.h"
#include "mtp_datasets.h"
#include "mtp_properties.h"
#include "mtp-safe.h"
#include "y2_tags.h"
extern mtp_ctx *mtp_context;

static mtp_ctx ctx;
static uint32_t store;
static unsigned char buffer[16384];

static unsigned u16(const unsigned char *p) { return p[0] | p[1] << 8; }
static uint32_t u32(const unsigned char *p) { return p[0] | p[1] << 8 | p[2] << 16 | (uint32_t)p[3] << 24; }

/* MTP string -> UTF-8 (ASCII/Latin-1 fixtures only); returns bytes consumed. */
static int string_at(const unsigned char *p, char *out, size_t size)
{
    unsigned n = p[0], i, o = 0;
    for (i = 0; i < n; i++) {
        unsigned c = u16(p + 1 + 2 * i);
        if (!c) break;
        if (c < 0x80) { if (o + 1 < size) out[o++] = c; }
        else if (o + 2 < size) { out[o++] = 0xC0 | c >> 6; out[o++] = 0x80 | (c & 0x3F); }
    }
    out[o] = 0;
    return 1 + 2 * n;
}

static fs_entry *child(uint32_t parent, const char *name)
{
    for (fs_entry *e = ctx.fs_db->entry_list; e; e = e->next)
        if (e->parent == parent && e->storage_id == store && e->name && !strcmp(e->name, name) && !(e->flags & ENTRY_IS_DELETED)) return e;
    return NULL;
}

static unsigned info_format(fs_entry *e)
{
    int n = build_objectinfo_dataset(&ctx, buffer, sizeof buffer, e);
    assert(n > 6);
    return u16(buffer + 4);
}

/* Data type the property description declares for a format. */
static unsigned desc_type(uint16_t prop, uint16_t format, unsigned *getset)
{
    int n = build_properties_dataset(&ctx, buffer, sizeof buffer, prop, format);
    if (n <= 0) return 0;
    assert(u16(buffer) == prop);
    if (getset) *getset = buffer[4];
    return u16(buffer + 2);
}

static int value_size(unsigned type, const unsigned char *p)
{
    switch (type) {
    case MTP_TYPE_UINT8: return 1;
    case MTP_TYPE_UINT16: return 2;
    case MTP_TYPE_UINT32: return 4;
    case MTP_TYPE_UINT64: return 8;
    case MTP_TYPE_UINT128: return 16;
    case MTP_TYPE_STR: return 1 + 2 * p[0];
    }
    fprintf(stderr, "unexpected type 0x%04x\n", type);
    abort();
}

/* One property of an object through GetObjectPropValue: text or number. */
static void prop_value(fs_entry *e, uint16_t prop, char *text, uint32_t *number)
{
    uint16_t format = y2_object_format(e->name, e->flags & ENTRY_IS_DIR);
    unsigned type = desc_type(prop, format, NULL);
    assert(type);
    int n = build_ObjectPropValue_dataset(&ctx, buffer, sizeof buffer, e->handle, prop);
    assert(n > 0 && n == value_size(type, buffer));
    if (type == MTP_TYPE_STR) { assert(text); string_at(buffer, text, 1024); }
    else { assert(number); *number = type == MTP_TYPE_UINT16 ? u16(buffer) : u32(buffer); }
}

typedef struct { uint16_t prop; unsigned type; char text[1024]; uint32_t number; } element;

/* GetObjectPropList(handle, all properties); each element's type must match its description. */
static int prop_list(fs_entry *e, element *out, int max)
{
    static unsigned char list[16384];
    int n = build_objectproplist_dataset(&ctx, list, sizeof list, e, e->handle, 0, 0xFFFFFFFF, 0, 0);
    assert(n > 4);
    uint32_t count = u32(list); int ofs = 4;
    uint16_t format = y2_object_format(e->name, e->flags & ENTRY_IS_DIR);
    assert((int)count <= max);
    for (uint32_t i = 0; i < count; i++) {
        element *el = &out[i];
        assert(u32(list + ofs) == e->handle);
        el->prop = u16(list + ofs + 4); el->type = u16(list + ofs + 6); ofs += 8;
        assert(desc_type(el->prop, format, NULL) == el->type);
        el->text[0] = 0; el->number = 0;
        if (el->type == MTP_TYPE_STR) string_at(list + ofs, el->text, sizeof el->text);
        else if (el->type == MTP_TYPE_UINT16) el->number = u16(list + ofs);
        else if (el->type == MTP_TYPE_UINT32) el->number = u32(list + ofs);
        ofs += value_size(el->type, list + ofs);
        assert(ofs <= n);
    }
    assert(ofs == n);
    return count;
}

static element *find(element *list, int count, uint16_t prop)
{
    for (int i = 0; i < count; i++) if (list[i].prop == prop) return &list[i];
    return NULL;
}

static void expect_name(fs_entry *e, const char *name)
{
    char text[1024]; element list[64];
    prop_value(e, MTP_PROPERTY_NAME, text, NULL);
    if (strcmp(text, name)) { fprintf(stderr, "%s: Name '%s', expected '%s'\n", e->name, text, name); abort(); }
    int count = prop_list(e, list, 64);
    assert(!strcmp(find(list, count, MTP_PROPERTY_NAME)->text, name));
    /* The file name stays the real one. */
    assert(!strcmp(find(list, count, MTP_PROPERTY_OBJECT_FILE_NAME)->text, e->name));
    prop_value(e, MTP_PROPERTY_OBJECT_FILE_NAME, text, NULL);
    assert(!strcmp(text, e->name));
}

static void rename_to(fs_entry *e, const char *name)
{
    unsigned char packet[1024] = {0};
    MTP_PACKET_HEADER *h = (void *)packet;
    unsigned char *p = packet + sizeof *h;
    size_t len = strlen(name);
    p[0] = len + 1;
    for (size_t i = 0; i < len; i++) p[1 + 2 * i] = name[i];
    h->length = sizeof *h + 1 + 2 * (len + 1);
    assert(setObjectPropValue(&ctx, h, e->handle, MTP_PROPERTY_OBJECT_FILE_NAME) == MTP_RESPONSE_OK);
}

int main(int argc, char **argv)
{
    assert(argc == 4);
    const char *media = argv[1];
    char path[4096], text[1024];
    uint32_t number;

    mtp_context = &ctx;
    ctx.uid = getuid(); ctx.euid = geteuid(); ctx.gid = getgid(); ctx.egid = getegid();
    ctx.default_uid = ctx.default_gid = -1;
    pthread_mutex_init(&ctx.inotify_mutex, NULL);
    assert(!y2_fs_init(media, NULL, NULL));
    store = mtp_add_storage(&ctx, (char *)media, "Internal Storage", -1, -1, 0);
    assert(store && store != UINT32_MAX);
    ctx.fs_db = init_fs_db(&ctx); assert(ctx.fs_db);
    assert(alloc_root_entry(ctx.fs_db, store));
    assert(!scan_and_add_folder(ctx.fs_db, (char *)media, 0, store));
    fs_entry *music = child(0, "Music"); assert(music);
    snprintf(path, sizeof path, "%s/Music", media);
    assert(!scan_and_add_folder(ctx.fs_db, path, music->handle, store));

    /* Formats by extension, case-insensitive; folders stay associations. */
    static const struct { const char *name; uint16_t format; } formats[] = {
        {"song.mp3", MTP_FORMAT_MP3}, {"OLD.FLAC", MTP_FORMAT_FLAC}, {"both.m4a", 0xB215},
        {"gone.ogg", MTP_FORMAT_OGG}, {"untitled.opus", MTP_FORMAT_OGG}, {"voice.oga", MTP_FORMAT_OGG},
        {"take.wav", MTP_FORMAT_WAV}, {"old.wma", MTP_FORMAT_WMA}, {"raw.aac", MTP_FORMAT_AAC},
        {"notes.txt", MTP_FORMAT_UNDEFINED}, {"noext", MTP_FORMAT_UNDEFINED},
        {"cover.mp3.jpg", MTP_FORMAT_UNDEFINED},
    };
    for (unsigned i = 0; i < sizeof formats / sizeof formats[0]; i++) {
        fs_entry *e = child(music->handle, formats[i].name);
        if (!e) { fprintf(stderr, "missing fixture %s\n", formats[i].name); abort(); }
        assert(y2_object_format(e->name, 0) == formats[i].format);
        assert(info_format(e) == formats[i].format);
        uint32_t format;
        prop_value(e, MTP_PROPERTY_OBJECT_FORMAT, NULL, &format);
        assert(format == formats[i].format);
        element list[64]; int count = prop_list(e, list, 64);
        assert(find(list, count, MTP_PROPERTY_OBJECT_FORMAT)->number == formats[i].format);
        /* Only music carries tag properties. */
        assert(!find(list, count, MTP_PROPERTY_ARTIST) == !y2_format_is_audio(formats[i].format));
    }
    assert(info_format(music) == MTP_FORMAT_ASSOCIATION);
    assert(y2_object_format("x.mp3", 1) == MTP_FORMAT_ASSOCIATION);
    assert(y2_object_format(NULL, 0) == MTP_FORMAT_UNDEFINED);
    assert(y2_object_format(".mp3", 0) == MTP_FORMAT_UNDEFINED);   /* hidden, never listed */
    assert(y2_object_format("._Song.mp3", 0) == MTP_FORMAT_UNDEFINED);  /* macOS AppleDouble */
    assert(y2_object_format(".hidden.flac", 0) == MTP_FORMAT_UNDEFINED);
    assert(y2_object_format("a.", 0) == MTP_FORMAT_UNDEFINED && y2_object_format("A.Mp3", 0) == MTP_FORMAT_MP3);

    /* DeviceInfo: no capture formats; playback lists the music formats. */
    int n = build_deviceinfo_dataset(&ctx, buffer, sizeof buffer); assert(n > 0);
    int ofs = 2 + 4 + 2; ofs += 1 + 2 * buffer[ofs]; ofs += 2;
    for (int array = 0; array < 3; array++) ofs += 4 + 2 * u32(buffer + ofs);
    assert(u32(buffer + ofs) == 0); ofs += 4;
    uint32_t playback = u32(buffer + ofs); ofs += 4;
    static const uint16_t wanted[] = {MTP_FORMAT_UNDEFINED, MTP_FORMAT_ASSOCIATION, MTP_FORMAT_MP3, MTP_FORMAT_WAV,
                                      MTP_FORMAT_WMA, MTP_FORMAT_OGG, MTP_FORMAT_AAC, 0xB215, MTP_FORMAT_FLAC};
    assert(playback == sizeof wanted / sizeof wanted[0]);
    for (unsigned w = 0; w < playback; w++) {
        int seen = 0;
        for (uint32_t i = 0; i < playback; i++) seen |= u16(buffer + ofs + 2 * i) == wanted[w];
        assert(seen);
        /* Every property a format lists has a description for that format. */
        int m = build_properties_supported_dataset(&ctx, buffer + 8192, 8192, wanted[w]); assert(m > 4);
        for (uint32_t i = 0; i < u32(buffer + 8192); i++) {
            uint16_t prop = u16(buffer + 8192 + 4 + 2 * i);
            unsigned char saved[8192]; memcpy(saved, buffer, sizeof saved);
            if (!desc_type(prop, wanted[w], NULL)) { fprintf(stderr, "no desc 0x%04x for 0x%04x\n", prop, wanted[w]); abort(); }
            memcpy(buffer, saved, sizeof saved);
        }
    }
    unsigned getset = 0;
    assert(desc_type(MTP_PROPERTY_OBJECT_FILE_NAME, MTP_FORMAT_MP3, &getset) == MTP_TYPE_STR && getset == 1);
    assert(desc_type(MTP_PROPERTY_NAME, MTP_FORMAT_FLAC, &getset) == MTP_TYPE_STR && getset == 0);
    assert(desc_type(MTP_PROPERTY_OBJECT_SIZE, MTP_FORMAT_FLAC, NULL) == MTP_TYPE_UINT64);
    assert(desc_type(MTP_PROPERTY_TRACK, MTP_FORMAT_MP3, &getset) == MTP_TYPE_UINT16 && getset == 0);
    assert(desc_type(MTP_PROPERTY_DURATION, MTP_FORMAT_MP3, NULL) == MTP_TYPE_UINT32);
    assert(desc_type(MTP_PROPERTY_SAMPLE_RATE, MTP_FORMAT_MP3, NULL) == MTP_TYPE_UINT32);
    assert(desc_type(MTP_PROPERTY_NUMBER_OF_CHANNELS, MTP_FORMAT_MP3, NULL) == MTP_TYPE_UINT16);
    assert(desc_type(MTP_PROPERTY_AUDIO_BITRATE, MTP_FORMAT_MP3, NULL) == MTP_TYPE_UINT32);
    assert(desc_type(MTP_PROPERTY_ARTIST, MTP_FORMAT_MP3, NULL) == MTP_TYPE_STR);

    /* Tags of the online source. */
    y2_tags_set_database(argv[2]);
    fs_entry *song = child(music->handle, "song.mp3");
    char *full = build_full_path(ctx.fs_db, mtp_get_storage_root(&ctx, store), song);
    snprintf(path, sizeof path, "%s/Music/song.mp3", media);
    assert(!strcmp(full, path)); free(full);
    expect_name(song, "Real Title \xc3\xa9");
    prop_value(song, MTP_PROPERTY_ARTIST, text, NULL); assert(!strcmp(text, "Artist A"));
    prop_value(song, MTP_PROPERTY_ALBUM_NAME, text, NULL); assert(!strcmp(text, "Album A"));
    prop_value(song, MTP_PROPERTY_ALBUM_ARTIST, text, NULL); assert(!strcmp(text, "Album Artist"));
    prop_value(song, MTP_PROPERTY_GENRE, text, NULL); assert(!strcmp(text, "Jazz"));
    prop_value(song, MTP_PROPERTY_COMPOSER, text, NULL); assert(!text[0]);
    prop_value(song, MTP_PROPERTY_ORIGINAL_RELEASE_DATE, text, NULL); assert(!text[0]);
    prop_value(song, MTP_PROPERTY_TRACK, NULL, &number); assert(number == 7);
    prop_value(song, MTP_PROPERTY_DURATION, NULL, &number); assert(number == 245000);
    prop_value(song, MTP_PROPERTY_SAMPLE_RATE, NULL, &number); assert(number == 44100);
    prop_value(song, MTP_PROPERTY_NUMBER_OF_CHANNELS, NULL, &number); assert(number == 2);
    prop_value(song, MTP_PROPERTY_AUDIO_BITRATE, NULL, &number); assert(number == 320000);
    element list[64]; int count = prop_list(song, list, 64);
    assert(!strcmp(find(list, count, MTP_PROPERTY_ARTIST)->text, "Artist A"));
    assert(find(list, count, MTP_PROPERTY_TRACK)->number == 7);
    assert(find(list, count, MTP_PROPERTY_DURATION)->number == 245000);
    assert(find(list, count, MTP_PROPERTY_SAMPLE_RATE)->number == 44100);
    assert(find(list, count, MTP_PROPERTY_NUMBER_OF_CHANNELS)->number == 2);
    assert(find(list, count, MTP_PROPERTY_AUDIO_BITRATE)->number == 320000);
    assert(!strcmp(find(list, count, MTP_PROPERTY_GENRE)->text, "Jazz"));

    /* Offline source with the same root is ignored, also when both have the path. */
    expect_name(child(music->handle, "OLD.FLAC"), "OLD");
    prop_value(child(music->handle, "OLD.FLAC"), MTP_PROPERTY_ARTIST, text, NULL); assert(!text[0]);
    expect_name(child(music->handle, "both.m4a"), "Fresh Both");
    /* Deleted rows are ignored; an empty title falls back but other tags stay. */
    expect_name(child(music->handle, "gone.ogg"), "gone");
    expect_name(child(music->handle, "untitled.opus"), "untitled");
    prop_value(child(music->handle, "untitled.opus"), MTP_PROPERTY_ARTIST, text, NULL); assert(!strcmp(text, "Someone"));
    /* Music the library does not know yet; other files keep their file name. */
    expect_name(child(music->handle, "take.wav"), "take");
    expect_name(child(music->handle, "notes.txt"), "notes.txt");
    expect_name(child(music->handle, "noext"), "noext");
    expect_name(music, "Music");

    /* Rename through ObjectFileName: the file moves, Name follows the new file name. */
    rename_to(song, "renamed.mp3");
    snprintf(path, sizeof path, "%s/Music/renamed.mp3", media); assert(!access(path, F_OK));
    song = child(music->handle, "renamed.mp3"); assert(song);
    expect_name(song, "renamed");
    rename_to(song, "song.mp3");
    song = child(music->handle, "song.mp3"); assert(song);
    expect_name(song, "Real Title \xc3\xa9");

    /* A locked library answers within the busy timeout, silently falling back. */
    y2_tags_set_database(argv[3]);
    struct timespec t0, t1; clock_gettime(CLOCK_MONOTONIC, &t0);
    expect_name(song, "song");
    clock_gettime(CLOCK_MONOTONIC, &t1);
    assert((t1.tv_sec - t0.tv_sec) * 1000 + (t1.tv_nsec - t0.tv_nsec) / 1000000 < 1500);

    /* No library: file names, and nothing is created. */
    snprintf(path, sizeof path, "%s.missing/library.db", argv[2]);
    y2_tags_set_database(path);
    expect_name(song, "song");
    prop_value(song, MTP_PROPERTY_DURATION, NULL, &number); assert(number == 0);
    assert(access(path, F_OK));
    snprintf(path, sizeof path, "%s.absent", argv[2]);
    y2_tags_set_database(path);
    expect_name(song, "song");
    assert(access(path, F_OK));

    /* The library comes back. */
    y2_tags_set_database(argv[2]);
    expect_name(song, "Real Title \xc3\xa9");
    puts("music tags ok");
    return 0;
}
