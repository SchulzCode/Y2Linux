// SPDX-License-Identifier: GPL-2.0-only
/* MT6582 WMT firmware protocol, derived from MediaTek GPL wmt_ic_soc.c.
 * One command owner, exact replies, no Android launcher/private ioctls.
 */
#include <linux/delay.h>
#include <linux/firmware.h>
#include <linux/jiffies.h>
#include <linux/slab.h>
#include "conn.h"

int y2_conn_wmt(struct y2_conn *c, const unsigned char *request, unsigned size,
	       const unsigned char *expected, unsigned expected_size)
{
	static const unsigned char rf_reply[] = {2, 0x14, 2, 0, 0, 1};
	int ret;
	if (size < 5 || request[0] != 1 || y2_conn_le16(request + 2) != size - 4)
		return -EINVAL;
	mutex_lock(&c->command);
	reinit_completion(&c->response); c->response_size = 0;
	WRITE_ONCE(c->wmt_reg_read,request[1]==8 && request[4]==2 && expected_size==16);
	WRITE_ONCE(c->wmt_rf_calibrate, size == 5 && request[1] == 0x14 && request[4] == 1 &&
		expected && expected_size == sizeof(rf_reply) && !memcmp(expected, rf_reply, sizeof(rf_reply)));
	ret = y2_stp_send(c, Y2_CONN_WMT, request, size);
	if (ret) goto out;
	if (!wait_for_completion_timeout(&c->response, msecs_to_jiffies(4000))) {
		dev_err(c->dev, "WMT opcode=%02x timeout in %s STP mode\n",
			request[1], c->full_stp ? "full" : "mandatory");
		y2_btif_report_timeout(c);
		ret = -ETIMEDOUT; goto out;
	}
	if (c->failure) { ret = c->failure; goto out; }
	if (c->wmt_rf_calibrate &&
	    y2_wmt_rf_result(c->chip, c->hvr, c->fvr, c->response_data, c->response_size))
		goto out;
	if (c->response_size != expected_size || c->response_data[1] != request[1] ||
	    c->response_data[4] || (expected && memcmp(c->response_data, expected, expected_size)))
		ret = -EPROTO;
out:
	if (ret) dev_err(c->dev,
		"WMT opcode=%02x parameter=%02x failed: %d mode=%s request=%u response=%u expected=%u prefix=%*ph\n",
		request[1], request[4], ret, c->full_stp ? "full" : "mandatory",
		size, c->response_size, expected_size, min(c->response_size, 6U), c->response_data);
	WRITE_ONCE(c->wmt_reg_read,false);
	WRITE_ONCE(c->wmt_rf_calibrate,false);
	mutex_unlock(&c->command);
	return ret;
}
static int read_version(struct y2_conn *c, unsigned address, unsigned *value)
{
	unsigned char request[] = {1,8,16,0,2,1,0,1,0,0,0,0,0,0,0,0,0xff,0xff,0,0};
	y2_conn_put32(request + 8, address);
	int ret = y2_conn_wmt(c, request, sizeof(request), NULL, 16);
	if (ret) return ret;
	if (y2_conn_le32(c->response_data + 8) != address) return -EPROTO;
	*value = y2_conn_le32(c->response_data + 12) & 0xffff;
	return 0;
}
static int patch(struct y2_conn *c, const char *name, unsigned sequence)
{
	const struct firmware *fw;
	unsigned char request[1005];
	unsigned char address[] = {1,8,16,0,1,1,0,1,0x3c,2,9,2,0,0,0,0,0xff,0xff,0xff,0xff};
	static const unsigned char address_reply[] = {2,8,4,0,0,0,0,1};
	static const unsigned char patch_reply[] = {2,1,1,0,0};
	static const unsigned char reset[] = {1,7,1,0,4}, reset_reply[] = {2,7,1,0,0};
	int ret = y2_conn_request_firmware(c->dev, name, &fw);
	if (ret) return ret;
	if (fw->size < 29 || fw->size > 65536 || memcmp(fw->data + 16, "ALPS", 4) ||
	    fw->data[20] != 0x8a || fw->data[21] || fw->data[22] != (c->fvr >> 8) ||
	    fw->data[23] != (c->fvr & 0xff) || fw->data[24] != (0x20 | sequence)) {
		ret = -ENOEXEC; goto out;
	}
	ret = y2_conn_wmt(c, address, sizeof(address), address_reply, sizeof(address_reply));
	if (ret) goto out;
	address[8] = 0xc4; address[9] = 4;
	memcpy(address + 12, fw->data + 24, 4); address[12] = 0;
	ret = y2_conn_wmt(c, address, sizeof(address), address_reply, sizeof(address_reply));
	for (unsigned at = 28; !ret && at < fw->size;) {
		unsigned count = min_t(unsigned, 1000, fw->size - at);
		request[0] = 1; request[1] = 1; request[2] = count + 1; request[3] = (count + 1) >> 8;
		request[4] = at + count == fw->size ? 3 : at == 28 ? 1 : 2;
		memcpy(request + 5, fw->data + at, count);
		ret = y2_conn_wmt(c, request, count + 5, patch_reply, sizeof(patch_reply));
		at += count;
	}
	if (!ret) ret = y2_conn_wmt(c, reset, sizeof(reset), reset_reply, sizeof(reset_reply));
	if (!ret) dev_info(c->dev, "WMT patch %u applied and reset acknowledged\n", sequence);
out:
	if (ret) dev_err(c->dev, "WMT patch %u failed: %d\n", sequence, ret);
	release_firmware(fw);
	return ret;
}
static int board_config(struct y2_conn *c)
{
	const struct firmware *fw;
	char *text, *cursor, *line; unsigned seen = 0; int ret;
	ret = y2_conn_request_firmware(c->dev, Y2_CONN_FW "WMT_SOC.cfg", &fw);
	if (ret) return ret;
	if (!fw->size || fw->size > 1024 || memchr(fw->data, 0, fw->size)) { ret = -EINVAL; goto out; }
	text = kmemdup_nul(fw->data, fw->size, GFP_KERNEL);
	if (!text) { ret = -ENOMEM; goto out; }
	cursor = text;
	while ((line = strsep(&cursor, "\n"))) {
		unsigned bit;
		line = strim(line);
		if (!*line || *line == '#') continue;
		if (!strcmp(line, "coex_wmt_ant_mode=1")) bit = 1;
		else if (!strcmp(line, "co_clock_flag=1")) bit = 2;
		else if (!strcmp(line, "wmt_gps_lna_pin=0")) bit = 4;
		else if (!strcmp(line, "wmt_gps_lna_enable=0")) bit = 8;
		else { ret = -EINVAL; break; }
		if (seen & bit) { ret = -EINVAL; break; }
		seen |= bit;
	}
	if (!ret && seen != 15) ret = -EINVAL;
	kfree(text);
out:
	release_firmware(fw);
	return ret;
}
int y2_wmt_boot(struct y2_conn *c)
{
	static const unsigned char query[] = {1,4,1,0,4};
	static const unsigned char initial[] = {2,4,6,0,0,4,0x11,0,0,0};
	static const unsigned char full[] = {2,4,6,0,0,4,0xdf,0x0e,0x68,1};
	static const unsigned char set[] = {1,4,5,0,3,0xdf,0x0e,0x68,1};
	static const unsigned char set_reply[] = {2,4,2,0,0,3};
	static const unsigned char calibrate[] = {1,0x14,1,0,1}, calibrated[] = {2,0x14,2,0,0,1};
	static const unsigned char coex[] = {1,0x10,2,0,1,1}, coex_reply[] = {2,0x10,1,0,0};
	static const unsigned char clock[] = {1,0x0a,2,0,8,3}, clock_reply[] = {2,0x0a,1,0,0};
	int ret = board_config(c);
	if (ret) return ret;
	ret = read_version(c, 0x80000008, &c->chip);
	if (!ret) ret = read_version(c, 0x80000000, &c->hvr);
	if (!ret) ret = read_version(c, 0x80000004, &c->fvr);
	if (ret) return ret;
	dev_info(c->dev, "CONSYS chip=%04x HVR=%04x FVR=%04x\n", c->chip, c->hvr, c->fvr);
	if (c->chip != 0x6582 || (c->hvr != 0x8a00 && c->hvr != 0x8a01)) return -ENODEV;
	ret = y2_conn_wmt(c, query, sizeof(query), initial, sizeof(initial));
	if (!ret) ret = y2_conn_wmt(c, set, sizeof(set), set_reply, sizeof(set_reply));
	if (ret) return ret;
	mutex_lock(&c->stp_lock); c->full_stp = true; mutex_unlock(&c->stp_lock);
	msleep(10);
	ret = y2_conn_wmt(c, query, sizeof(query), full, sizeof(full));
	if (!ret) dev_info(c->dev, "WMT full STP mode verified\n");
	if (!ret) ret = patch(c, Y2_CONN_FW "mt6572_82_patch_e1_1_hdr.bin", 1);
	if (!ret) ret = patch(c, Y2_CONN_FW "mt6572_82_patch_e1_0_hdr.bin", 2);
	if (ret) return ret;
	/* RF calibration needs both PA rails. The core unwinds every partial
	 * enable; an unsuccessful calibration event never becomes success. */
	ret = y2_conn_rail(c, 2, true);
	if (!ret) ret = y2_conn_rail(c, 3, true);
	if (!ret) ret = y2_conn_wmt(c, calibrate, sizeof(calibrate), calibrated, sizeof(calibrated));
	if (!ret) dev_info(c->dev, "WMT RF calibration succeeded: event=%u bytes\n", c->response_size);
	int wifi_off = y2_conn_rail(c, 3, false), bt_off = y2_conn_rail(c, 2, false);
	if (!ret) ret = wifi_off ? wifi_off : bt_off;
	if (!ret) ret = y2_conn_wmt(c, coex, sizeof(coex), coex_reply, sizeof(coex_reply));
	if (!ret) ret = y2_conn_wmt(c, clock, sizeof(clock), clock_reply, sizeof(clock_reply));
	return ret;
}
int y2_wmt_function(struct y2_conn *c, unsigned function, bool enable)
{
	unsigned char request[] = {1,6,2,0,function,enable};
	static const unsigned char reply[] = {2,6,1,0,0};
	if (function != Y2_CONN_BT && function != Y2_CONN_WIFI) return -EINVAL;
	return y2_conn_wmt(c, request, sizeof(request), reply, sizeof(reply));
}
