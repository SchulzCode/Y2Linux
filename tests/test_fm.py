"""MT6627 FM receive path: the actual fm.c sequences against the stock Y2 kernel.

tests/fixtures/fm-stock-receive.json was recorded by running the stock v3.2.0
FM kernel's own mt6627_* functions in an emulator and capturing every packet
handed to fm_cmd_tx (firmware segments reduced to their framing). Here the
real functions from kernel/platform/connectivity/fm.c run against a recording
transport and must produce the same packets with the same wait masks.
"""
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_audio import function

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = json.loads((ROOT / 'tests/fixtures/fm-stock-receive.json').read_text())
SEQUENCES = ('fm_read', 'fm_write', 'fm_set_bits', 'fm_top_write', 'fm_host_write', 'fm_host_read',
             'fm_rampdown', 'fm_download', 'fm_rom_version', 'fm_power_up_seq', 'fm_power_down_seq',
             'fm_set_freq', 'fm_tune', 'fm_mute', 'fm_volume', 'fm_pre_search', 'fm_restore_search',
             'fm_soft_mute_tune')

HARNESS = r'''
#include <stdbool.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include "connectivity/protocol.h"
#include "connectivity/fm-protocol.h"
#define EINVAL 22
#define ENODEV 19
#define ENOEXEC 8
#define ETIMEDOUT 110
#define min(a,b) ((a)<(b)?(a):(b))
#define min_t(t,a,b) ((t)(a)<(t)(b)?(t)(a):(t)(b))
#define READ_ONCE(x) (x)
#define Y2_FM_WAIT_S 3
#define Y2_FM_SM_TUNE_S 6
#define spin_lock_irqsave(l,f) ((void)(f))
#define spin_unlock_irqrestore(l,f) ((void)(f))
static void usleep_range(unsigned a, unsigned b) { (void)a; (void)b; }
struct firmware { unsigned size; const unsigned char *data; };
struct y2_fm {
	unsigned events, fspi, cspi;
	unsigned char cqi[Y2_FM_CQI_MAX];
	struct y2_fm_pkt tx;
	unsigned khz, chip, rom, patch;
	int lock, events_lock;
};
static unsigned regs[256];
static unsigned char next_cqi[Y2_FM_CQI_MAX];
/* Recording transport: print the packet and its wait mask, answer reads. */
static int fm_cmd(struct y2_fm *fm, unsigned mask, unsigned seconds)
{
	unsigned char *p = fm->tx.data;
	unsigned body = p[2] | p[3] << 8;
	if (body + 4 != fm->tx.size) abort();
	if ((p[1] == Y2_FM_OP_SOFT_MUTE_TUNE) != (seconds == Y2_FM_SM_TUNE_S)) abort();
	printf("%u ", mask);
	if (p[1] == Y2_FM_OP_PATCH || p[1] == Y2_FM_OP_COEFF) {
		for (unsigned i = 0; i < 6; i++) printf("%02x", p[i]);
		printf(" +%u\n", fm->tx.size - 6);
	} else {
		for (unsigned i = 0; i < fm->tx.size; i++) printf("%02x", p[i]);
		printf("\n");
	}
	if (p[1] == Y2_FM_OP_FSPI_READ) fm->fspi = regs[p[4]];
	if (p[1] == Y2_FM_OP_HOST_READ) fm->cspi = 0;
	if (p[1] == Y2_FM_OP_SOFT_MUTE_TUNE) memcpy(fm->cqi, next_cqi, sizeof(next_cqi));
	return 0;
}
'''

MAIN = r'''
static unsigned char patch_bytes[1028], coeff_bytes[1092];
int main(int argc, char **argv)
{
	struct y2_fm fm = {0};
	struct firmware patch = {sizeof(patch_bytes), patch_bytes}, coeff = {sizeof(coeff_bytes), coeff_bytes};
	int raw; bool valid;
	regs[0x62] = 0x6627; regs[0x84] = 1;
	printf("= powerup\n"); if (fm_power_up_seq(&fm, &patch, &coeff)) return 1;
	unsigned freqs[] = {8750, 9810, 10400, 10800};
	for (unsigned i = 0; i < 4; i++) { printf("= setfreq_%u\n", freqs[i]); if (fm_set_freq(&fm, freqs[i])) return 1; }
	printf("= mute1\n"); fm_mute(&fm, true);
	printf("= mute0\n"); fm_mute(&fm, false);
	printf("= setvol15\n"); fm_volume(&fm, 15);
	printf("= pre_search\n"); fm_pre_search(&fm);
	printf("= softmute_tune_9810\n"); fm_soft_mute_tune(&fm, 9810, &raw, &valid);
	printf("= restore_search\n"); fm_restore_search(&fm);
	printf("= powerdown\n"); fm_power_down_seq(&fm);
	printf("= sweep\n");
	for (unsigned f = 8750; f <= 10800; f += 5) { printf("@%u\n", f); fm_set_freq(&fm, f); }
	printf("= volume\n");
	for (unsigned v = 0; v < 20; v++) { printf("@%u\n", v); fm_volume(&fm, v); }
	printf("= end\n");
	return 0;
}
'''


def build_and_run(source):
    with tempfile.TemporaryDirectory() as directory:
        p = Path(directory)
        (p / 'test.c').write_text(source)
        subprocess.run(['clang', '-O2', '-Wall', '-Wextra', '-Werror', '-Wno-unused-parameter',
                        '-Wno-unused-function', '-fsanitize=undefined', '-fsanitize-trap=undefined',
                        '-I' + str(ROOT / 'kernel/platform'), str(p / 'test.c'), '-o', str(p / 'test')], check=True)
        return subprocess.run([str(p / 'test')], check=True, capture_output=True, text=True).stdout


def sections(output):
    out, name = {}, None
    for line in output.splitlines():
        if line.startswith('= '):
            name = line[2:]
            out[name] = []
        else:
            out[name].append(line)
    return out


class FM(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        source = (ROOT / 'kernel/platform/connectivity/fm.c').read_text()
        body = '\n'.join(function(source, n) for n in SEQUENCES)
        cls.out = sections(build_and_run(HARNESS + body + MAIN))

    def test_sequences_match_the_stock_kernel_packet_for_packet(self):
        for name, expected in FIXTURE['sequences'].items():
            if name == 'rssi':
                continue
            got = [line.split(' ', 1) for line in self.out[name]]
            want = [[str(m), p] for m, p in expected]
            self.assertEqual(got, want, name)

    def test_every_channel_tunes_like_the_stock_kernel(self):
        lines, current = [], None
        for line in self.out['sweep']:
            if line.startswith('@'):
                current = [line[1:], []]
                lines.append(current)
            else:
                current[1].append(line.split(' ', 1)[1])
        sweep = '\n'.join('%s:%s' % (f, '|'.join(p)) for f, p in lines)
        self.assertEqual(len(lines), 411)
        self.assertEqual(hashlib.sha256(sweep.encode()).hexdigest(), FIXTURE['setfreq_sha256'])

    def test_volume_table_and_clamp(self):
        got, current = {}, None
        for line in self.out['volume']:
            if line.startswith('@'):
                current = line[1:]
                got[current] = []
            else:
                got[current].append(line.split(' ', 1)[1])
        self.assertEqual(got, FIXTURE['volume'])

    def test_cqi_decision_rssi_conversion_and_desense(self):
        vectors = FIXTURE['softmute']
        table = ',\n'.join('{%d,"%s",%d,%d}' % tuple(v) for v in vectors)
        rejected = ',\n'.join('{%d,%d}' % tuple(v) for v in FIXTURE['desense_rejected'])
        out = build_and_run(r'''
#include <stdio.h>
#include <stdlib.h>
#include "connectivity/fm-protocol.h"
struct v { unsigned freq; const char *cqi; int rssi, valid; } vs[] = {''' + table + r'''};
struct d { unsigned freq; int rssi; } ds[] = {''' + rejected + r'''};
int main(void) {
	for (unsigned i = 0; i < sizeof(vs) / sizeof(vs[0]); i++) {
		unsigned char cqi[Y2_FM_CQI_MAX] = {0};
		for (unsigned b = 0; b < 24; b++) { unsigned x; sscanf(vs[i].cqi + 2 * b, "%2x", &x); cqi[b] = x; }
		struct y2_fm_cqi r = y2_fm_cqi_decide(cqi);
		if (r.rssi != vs[i].rssi || r.valid != vs[i].valid) { printf("cqi %u\n", i); return 1; }
	}
	unsigned rejects = 0;
	for (unsigned f = 8750; f <= 10800; f += 5) {
		int rs[] = {-300, -241, -240, -239, 0};
		for (unsigned i = 0; i < 5; i++) if (y2_fm_desense(f, rs[i])) {
			unsigned k;
			for (k = 0; k < sizeof(ds) / sizeof(ds[0]); k++) if (ds[k].freq == f && ds[k].rssi == rs[i]) break;
			if (k == sizeof(ds) / sizeof(ds[0])) return 2;
			rejects++;
		}
	}
	if (rejects != sizeof(ds) / sizeof(ds[0])) return 3;
	for (unsigned raw = 0; raw < 65536; raw += 7) printf("%u %d %d\n", raw, y2_fm_rssi_dbm(raw), y2_fm_stereo(raw));
	return 0;
}
''')
        self.assertEqual(hashlib.sha256(out.rstrip('\n').encode()).hexdigest(),
                         FIXTURE['rssi_stereo_step7_sha256'])

    def test_events_and_frequency_bounds(self):
        build_and_run(r'''
#include <assert.h>
#include "connectivity/fm-protocol.h"
int main(void) {
	struct y2_fm_event e;
	const unsigned char ok[] = {4, 0x03, 2, 0, 0x27, 0x66, 4, 0x20, 0, 0, 4, 0x09, 1, 0, 1, 4, 0x09, 0, 0};
	unsigned at = 0, used, bits = 0;
	while ((used = y2_fm_event_next(ok + at, sizeof(ok) - at, &e))) { bits |= e.bits; at += used; }
	assert(at == sizeof(ok));
	assert(bits == (Y2_FM_EV(3) | Y2_FM_EV_CSPI_WRITE | Y2_FM_EV_TUNE_DONE | Y2_FM_EV(9)));
	const unsigned char shortlen[] = {4, 0x03, 5, 0, 1};
	assert(!y2_fm_event_next(shortlen, sizeof(shortlen), &e));
	const unsigned char wrongtype[] = {1, 0x03, 0, 0};
	assert(!y2_fm_event_next(wrongtype, sizeof(wrongtype), &e));
	const unsigned char high[] = {4, 0x40, 0, 0};
	assert(y2_fm_event_next(high, 4, &e) == 4 && !e.bits);
	const unsigned char smt[] = {4, 0x17, 1, 0, 0};
	assert(y2_fm_event_next(smt, 5, &e) == 5 && !e.bits);
	assert(y2_fm_khz_valid(87500) && y2_fm_khz_valid(108000) && y2_fm_khz_valid(98150));
	assert(!y2_fm_khz_valid(87450) && !y2_fm_khz_valid(108050) && !y2_fm_khz_valid(98120));
	assert(y2_fm_segments(1028) == 3 && y2_fm_segment_size(1028, 2) == 4);
	assert(y2_fm_segments(1024) == 3 && y2_fm_segment_size(1024, 2) == 0);
	struct y2_fm_pkt p;
	unsigned char big[Y2_FM_TX_MAX];
	assert(!y2_fm_segment_pkt(&p, Y2_FM_OP_PATCH, 1, 0, big, Y2_FM_TX_MAX - 5));
	return 0;
}
''')

    def test_firmware_identity_and_no_firmware_in_fixture(self):
        firmware = (ROOT / 'kernel/platform/connectivity/firmware.c').read_text()
        for name, size, digest in (
                ('mt6627_fm_v1_patch.bin', 1028, 'ba35c74d26660729caf83b41a567e700f633b6c99384dfa6cbb3c17ceb767e26'),
                ('mt6627_fm_v1_coeff.bin', 1092, 'd32f4a16b35f950ed1a57f2ae88f77ac4b5674d5c4210b65f6fe7912901fd8db')):
            line = next(l for l in firmware.splitlines() if name in l)
            self.assertIn(', %d, {' % size, line)
            self.assertEqual(''.join(b[2:] for b in line.split('{', 2)[2].split('}')[0].split(',')), digest)
        for name, packets in FIXTURE['sequences'].items():
            for _, packet in packets:
                self.assertLessEqual(len(packet.split(' ')[0]), 2 * 160, name)


if __name__ == '__main__':
    unittest.main()
