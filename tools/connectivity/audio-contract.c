/* SPDX-License-Identifier: GPL-2.0-only
 * Query direct ALSA hw_params constraints; no hw_params commit or PCM write.
 * 'null' is a host/ARM userspace test sink, never hardware qualification.
 */
#include <alsa/asoundlib.h>
#include <stdio.h>
#include <string.h>

int main(int argc, char **argv)
{
    const char *device = argc == 3 && !strcmp(argv[1], "--device") ? argv[2] : "hw:Y2Audio,0";
    if ((argc != 1 && argc != 3) || (argc == 3 && strcmp(argv[1], "--device")) || (strcmp(device, "hw:Y2Audio,0") && strcmp(device, "null"))) {
        fputs("Use y2-audio-contract [--device hw:Y2Audio,0|null]\n", stderr); return 2;
    }
    snd_pcm_t *pcm = NULL;
    int rc = snd_pcm_open(&pcm, device, SND_PCM_STREAM_PLAYBACK, SND_PCM_NONBLOCK);
    if (rc < 0) {
        printf("{\"schema\":\"org.y2linux.audio-query/v1\",\"state\":\"Unavailable\",\"alsa_error\":%d}\n", rc);
        return 1;
    }
    snd_pcm_hw_params_t *base = NULL, *params = NULL;
    if (snd_pcm_hw_params_malloc(&base) < 0 || snd_pcm_hw_params_malloc(&params) < 0 ||
        snd_pcm_hw_params_any(pcm, base) < 0) {
        snd_pcm_hw_params_free(base); snd_pcm_hw_params_free(params); snd_pcm_close(pcm); return 1;
    }
    const snd_pcm_format_t formats[] = { SND_PCM_FORMAT_S16_LE, SND_PCM_FORMAT_S24_LE, SND_PCM_FORMAT_S32_LE };
    const unsigned rates[] = {44100, 48000, 88200, 96000};
    printf("{\"schema\":\"org.y2linux.audio-query/v1\",\"state\":\"Observed\",\"device\":\"%s\","
           "\"physical_qualification\":false,\"pcm_started\":false,\"combinations\":[", device);
    for (unsigned f=0; f<3; f++) for (unsigned r=0; r<4; r++) {
        snd_pcm_hw_params_copy(params, base);
        int ok = snd_pcm_hw_params_set_access(pcm,params,SND_PCM_ACCESS_RW_INTERLEAVED)==0 &&
                 snd_pcm_hw_params_set_format(pcm,params,formats[f])==0 &&
                 snd_pcm_hw_params_set_channels(pcm,params,2)==0 &&
                 snd_pcm_hw_params_set_rate(pcm,params,rates[r],0)==0;
        printf("%s{\"format\":\"%s\",\"physical_bits\":%d,\"valid_bits\":%d,\"rate_hz\":%u,"
               "\"channels\":2,\"accepted_constraints\":%s}",f||r?",":"",snd_pcm_format_name(formats[f]),
               snd_pcm_format_physical_width(formats[f]),snd_pcm_format_width(formats[f]),rates[r],ok?"true":"false");
    }
    puts("]}");
    snd_pcm_hw_params_free(params); snd_pcm_hw_params_free(base); snd_pcm_close(pcm); return 0;
}
