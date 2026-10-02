/* SPDX-License-Identifier: GPL-2.0-only
 * Offline encoders only. Never opens a radio, ALSA PCM, or hardware device.
 * This tests the exact optional libraries independently of peer qualification.
 */
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sbc/sbc.h>
#include <freeaptx.h>
#include <fdk-aac/aacenc_lib.h>
#include <ldac/ldacBT.h>
#include <ldac/ldacBT_abr.h>

static void sbc_contract(void)
{
    sbc_t codec;
    assert(sbc_init(&codec, 0) == 0);
    codec.frequency = SBC_FREQ_44100;
    codec.blocks = SBC_BLK_16;
    codec.subbands = SBC_SB_8;
    codec.mode = SBC_MODE_DUAL_CHANNEL;
    codec.allocation = SBC_AM_LOUDNESS;
    codec.endian = SBC_LE;
    int16_t pcm[256] = {1, -1, 2, -2};
    unsigned char encoded[1024];
    for (unsigned bitpool = 38; bitpool <= 47; bitpool += 9) {
        codec.bitpool = bitpool;
        ssize_t written = 0;
        assert(sbc_encode(&codec, pcm, sizeof(pcm), encoded, sizeof(encoded), &written) == sizeof(pcm));
        assert(written > 0 && encoded[2] == bitpool);
        printf("SBC %s: observed_bitpool=%u payload_kbps=%zu\n", bitpool == 38 ? "XQ" : "XQ+", bitpool,
               8 * sbc_get_frame_length(&codec) * 44100 / 128 / 1000);
    }
    sbc_finish(&codec);
}

static void aptx_contract(int hd)
{
    struct aptx_context *encoder = aptx_init(hd);
    assert(encoder);
    unsigned char pcm[24 * 128] = {1, 2, 3, 4, 5, 6}, output[1024];
    size_t written = 0;
    assert(aptx_encode(encoder, pcm, sizeof(pcm), output, sizeof(output), &written) == sizeof(pcm));
    assert(written == (size_t)(hd ? 6 : 4) * 128);
    printf("aptX%s: PCM24_frames=512 encoded_bytes=%zu\n", hd ? "-HD" : "", written);
    aptx_finish(encoder);
}

static void aac_contract(unsigned rate)
{
    HANDLE_AACENCODER encoder = NULL;
    assert(aacEncOpen(&encoder, 0, 2) == AACENC_OK);
    assert(aacEncoder_SetParam(encoder, AACENC_AOT, AOT_AAC_LC) == AACENC_OK);
    assert(aacEncoder_SetParam(encoder, AACENC_SAMPLERATE, rate) == AACENC_OK);
    assert(aacEncoder_SetParam(encoder, AACENC_CHANNELMODE, MODE_2) == AACENC_OK);
    assert(aacEncoder_SetParam(encoder, AACENC_BITRATE, 256000) == AACENC_OK);
    assert(aacEncoder_SetParam(encoder, AACENC_TRANSMUX, TT_MP4_LATM_MCP1) == AACENC_OK);
    assert(aacEncEncode(encoder, NULL, NULL, NULL, NULL) == AACENC_OK);
    INT_PCM pcm[2048] = {1, -1, 2, -2};
    unsigned char encoded[4096];
    void *in = pcm, *out = encoded;
    int in_id = IN_AUDIO_DATA, out_id = OUT_BITSTREAM_DATA;
    int in_size = sizeof(pcm), out_size = sizeof(encoded);
    int in_element = sizeof(INT_PCM), out_element = 1;
    AACENC_BufDesc input = {1, &in, &in_id, &in_size, &in_element};
    AACENC_BufDesc output = {1, &out, &out_id, &out_size, &out_element};
    AACENC_InArgs args = {.numInSamples = 2048};
    AACENC_OutArgs result = {0};
    unsigned bytes = 0;
    for (unsigned i = 0; i < 8; i++) {
        assert(aacEncEncode(encoder, &input, &output, &args, &result) == AACENC_OK);
        assert(result.numInSamples == 2048);
        bytes += result.numOutBytes;
    }
    assert(bytes > 0);
    printf("AAC: rate=%u encoded_bytes=%u\n", rate, bytes);
    assert(aacEncClose(&encoder) == AACENC_OK);
}

static void ldac_contract(unsigned rate, unsigned eqmid)
{
    HANDLE_LDAC_BT encoder = ldacBT_get_handle();
    HANDLE_LDAC_ABR abr = ldac_ABR_get_handle();
    assert(encoder && abr);
    assert(ldacBT_init_handle_encode(encoder, 1008, eqmid, LDACBT_CHANNEL_MODE_STEREO, LDACBT_SMPL_FMT_S32, rate) == 0);
    assert(ldac_ABR_Init(abr, 1000 * LDACBT_ENC_LSU / rate) == 0);
    assert(ldac_ABR_set_thresholds(abr, 6, 4, 2) == 0);
    int32_t pcm[LDACBT_ENC_LSU * 2] = {256, -256, 512, -512};
    unsigned char output[4096];
    unsigned bytes = 0, changes = 0;
    int first = 0;
    for (unsigned i = 0; i < 256; i++) {
        int used, written, frames;
        assert(ldacBT_encode(encoder, pcm, &used, output, &written, &frames) == 0);
        bytes += written;
        if (written > 0) {
            if (!first) first = ldacBT_get_bitrate(encoder);
            int before = ldacBT_get_eqmid(encoder);
            assert(ldac_ABR_Proc(encoder, abr, 32, 1) >= 0);
            changes += before != ldacBT_get_eqmid(encoder);
        }
    }
    const int expected[][3] = {{909, 606, 303}, {990, 660, 330}};
    assert(first == expected[rate == 48000][eqmid]);
    assert(bytes > 0);
    if (eqmid == 0) assert(changes > 0 && ldacBT_get_bitrate(encoder) < first);
    printf("LDAC: rate=%u initial_kbps=%d congested_kbps=%d abr_changes=%u\n",
           rate, first, ldacBT_get_bitrate(encoder), changes);
    ldac_ABR_free_handle(abr);
    ldacBT_free_handle(encoder);
}

int main(void)
{
    sbc_contract();
    aptx_contract(0);
    aptx_contract(1);
    for (unsigned rate = 44100; rate <= 48000; rate += 3900) {
        aac_contract(rate);
        for (unsigned quality = 0; quality < 3; quality++) ldac_contract(rate, quality);
    }
    puts("PASS: offline software only; physical peer, audible, RF and CPU qualification remain separate");
    return 0;
}
