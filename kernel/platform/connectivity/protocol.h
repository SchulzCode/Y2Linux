/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_CONNECTIVITY_PROTOCOL_H
#define Y2_CONNECTIVITY_PROTOCOL_H
/* Pure bounded wire helpers shared with host fault-injection tests.
 * MT6582 STP/WMT wire format: pinned donor common/core/stp_core.c and
 * wmt_ic_soc.c. No raw traffic, radio addresses or calibration is logged. */
#define Y2_STP_MAX_PAYLOAD 2048
#define Y2_STP_WINDOW 7
#define Y2_FS_STRIDE 0x4004
#define Y2_FS_ARGS 16

static inline unsigned y2_conn_le16(const unsigned char *p)
{
	return p[0] | ((unsigned)p[1] << 8);
}
static inline unsigned y2_conn_le32(const unsigned char *p)
{
	return y2_conn_le16(p) | (y2_conn_le16(p + 2) << 16);
}
static inline void y2_conn_put32(unsigned char *p, unsigned value)
{
	p[0] = value; p[1] = value >> 8; p[2] = value >> 16; p[3] = value >> 24;
}
static inline unsigned y2_stp_crc(const unsigned char *data, unsigned size)
{
	unsigned crc = 0, i;
	while (size--) {
		crc ^= *data++;
		for (i = 0; i < 8; i++)
			crc = (crc >> 1) ^ ((crc & 1) ? 0xa001 : 0);
	}
	return crc;
}
static inline int y2_stp_header(const unsigned char *p, unsigned *channel,
			      unsigned *length, unsigned *seq, unsigned *ack)
{
	if (!(p[0] & 0x80) || (p[0] & 0x40) || (p[1] & 0x80) ||
	    (unsigned char)(p[0] + p[1] + p[2]) != p[3])
		return -1;
	*length = ((p[1] & 15) << 8) | p[2];
	*channel = (p[1] >> 4) & 7;
	*seq = (p[0] >> 3) & 7;
	*ack = p[0] & 7;
	return *length <= Y2_STP_MAX_PAYLOAD ? 0 : -1;
}
/* This MT6582 E2 / E1-ROM combination returns 624 bytes of RF results after
 * the six-byte calibration event prefix. The own-unit STP capture has outer
 * length 630, inner length 626, status=0 and operation=1. Inspect only the
 * prefix here; STP must validate and consume the entire frame and CRC first.
 * Do not apply the donor's unconditional opcode-0x14 result-check bypass. */
#define Y2_WMT_RF_RESULT_SIZE 630
static inline int y2_wmt_rf_result(unsigned chip, unsigned hvr, unsigned fvr,
				   const unsigned char *p, unsigned size)
{
	return chip == 0x6582 && hvr == 0x8a01 && fvr == 0x8a00 &&
		size == Y2_WMT_RF_RESULT_SIZE && p[0] == 2 && p[1] == 0x14 &&
		y2_conn_le16(p + 2) == size - 4 && !p[4] && p[5] == 1;
}
struct y2_fs_argument { const unsigned char *data; unsigned size; };
struct y2_fs_packet {
	unsigned op, count;
	struct y2_fs_argument args[Y2_FS_ARGS];
};
static inline int y2_fs_parse_prefix(const unsigned char *data, unsigned size,
				   struct y2_fs_packet *packet)
{
	unsigned at = 8, i, n, padded;
	if (size < 8 || size > Y2_FS_STRIDE) return -1;
	packet->op = y2_conn_le32(data);
	packet->count = y2_conn_le32(data + 4);
	if (packet->count > Y2_FS_ARGS) return -1;
	for (i = 0; i < packet->count; i++) {
		if (size - at < 4) return -1;
		n = y2_conn_le32(data + at); at += 4;
		if (n > size - at) return -1;
		padded = (n + 3) & ~3U;
		if (padded > size - at) return -1;
		packet->args[i].data = data + at;
		packet->args[i].size = n;
		at += padded;
	}
	return at;
}
/* The service ABI and AP replies contain only the counted arguments. */
static inline int y2_fs_parse(const unsigned char *data, unsigned size,
			    struct y2_fs_packet *packet)
{
	int used = y2_fs_parse_prefix(data, size, packet);
	return used >= 0 && (unsigned)used == size ? 0 : -1;
}
/* MT6582 MD requests may include one unused trailing word in the reported
 * stream length. Stock GetPackInfo follows the argument count/alignment;
 * it does not interpret that word. Normalize at the transport boundary so
 * the private service keeps its strict, unchanged ABI. Never accept a
 * partial argument or arbitrary trailing data, or assume padding is zero. */
static inline int y2_fs_request_size(const unsigned char *data, unsigned size,
				     struct y2_fs_packet *packet)
{
	int used = y2_fs_parse_prefix(data, size, packet);
	if (used < 0 || ((unsigned)used != size && (unsigned)used + 4 != size))
		return -1;
	return used;
}
/* Modem filenames are UTF-16LE Z:\NVRAM\... . Bound the ASCII subset used
 * by its record store; reject traversal, alternate drives, NUL suffixes and
 * non-ASCII names before any filesystem operation. */
static inline int y2_fs_path(const unsigned char *data, unsigned size,
			   char *path, unsigned capacity)
{
	unsigned i, out = 0, component = 0;
	if (size < 8 || size > 256 || (size & 1) || capacity < 2 ||
	    data[0] != 'Z' || data[1] || data[2] != ':' || data[3] ||
	    data[4] != '\\' || data[5]) return -1;
	for (i = 6; i < size; i += 2) {
		unsigned c = data[i];
		if (data[i + 1]) return -1;
		if (!c) {
			if (i + 2 != size || !component) return -1;
			path[out] = 0;
			return 0;
		}
		if (out + 1 >= capacity) return -1;
		if (c == '\\') {
			if (!component) return -1;
			path[out++] = '/'; component = 0;
		} else {
			if (!((c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') ||
			      c == '_' || c == '.')) return -1;
			if (c == '.') return -1;
			path[out++] = c; component++;
		}
	}
	return -1;
}
#endif
