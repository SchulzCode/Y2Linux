// SPDX-License-Identifier: GPL-2.0-only
/* Production STP mandatory bootstrap, then full mode with sequence/ACK window
 * and bounded retransmission. BTIF ROM commands are framed from the first byte.
 * Wire protocol derived from MediaTek GPL stp_core.c. HCI packet credits stay
 * with the Linux Bluetooth stack; these are separate transport acknowledgments.
 */
#include <linux/delay.h>
#include <linux/jiffies.h>
#include "conn.h"

static void y2_stp_retry(struct work_struct *work)
{
	struct y2_conn *c = container_of(to_delayed_work(work), struct y2_conn, retry_work);
	int ret = 0;
	mutex_lock(&c->stp_lock);
	if (!c->transport_on || !c->stp_pending || c->failure) goto out;
	if (++c->stp_retries > 3) { ret = -ETIMEDOUT; goto out; }
	/* Peer parser resynchronization, followed by every still-unacked frame. */
	static const unsigned char sync[] = {0x7f, 0x7f, 0x7f, 0x7f};
	ret = y2_btif_send(c, sync, sizeof(sync));
	for (unsigned i = 0; !ret && i < c->stp_pending; i++) {
		struct y2_stp_frame *frame = &c->window[(c->stp_oldest + i) & 7];
		ret = y2_btif_send(c, frame->data, frame->size);
	}
	if (!ret) mod_delayed_work(system_wq, &c->retry_work, msecs_to_jiffies(250));
out:
	mutex_unlock(&c->stp_lock);
	if (ret) y2_conn_failed(c, ret);
}
void y2_stp_init(struct y2_conn *c)
{
	mutex_init(&c->stp_lock); init_waitqueue_head(&c->tx_wait);
	INIT_DELAYED_WORK(&c->retry_work, y2_stp_retry);
}
void y2_stp_reset(struct y2_conn *c)
{
	cancel_delayed_work_sync(&c->retry_work);
	mutex_lock(&c->stp_lock);
	c->full_stp = false;
	c->stp_tx = c->stp_oldest = c->stp_rx = c->stp_pending = 0;
	c->rx_used = c->rx_needed = c->stp_retries = 0;
	memzero_explicit(c->window, sizeof(c->window));
	memzero_explicit(c->rx_frame, sizeof(c->rx_frame));
	mutex_unlock(&c->stp_lock);
	wake_up_all(&c->tx_wait);
}
int y2_stp_send(struct y2_conn *c, unsigned channel, const unsigned char *data, unsigned size)
{
	struct y2_stp_frame *f; unsigned crc; int ret;
	if ((channel != Y2_CONN_WMT && channel != Y2_CONN_BT) || !size || size > Y2_STP_MAX_PAYLOAD)
		return -EINVAL;
	if (!wait_event_timeout(c->tx_wait, READ_ONCE(c->stp_pending) < Y2_STP_WINDOW ||
		READ_ONCE(c->failure) || !READ_ONCE(c->transport_on), msecs_to_jiffies(1500))) return -ETIMEDOUT;
	mutex_lock(&c->stp_lock);
	if (c->failure || !c->transport_on) { ret = -ESHUTDOWN; goto out; }
	if (!c->full_stp) {
		if (channel != Y2_CONN_WMT) { ret = -EHOSTDOWN; goto out; }
		/* Stock wmt_core_stp_init enables BTIF mandatory mode before
		 * wmt_core_hw_check. Even the first register read has a four-byte
		 * STP header and two-byte trailer, with checksum/CRC disabled.
		 * No sequence, ACK window or retry timer exists in this mode. */
		f = &c->window[0]; f->size = size + 6;
		f->data[0] = 0x80; f->data[1] = (channel << 4) | (size >> 8);
		f->data[2] = size; f->data[3] = 0;
		memcpy(f->data + 4, data, size);
		f->data[size + 4] = f->data[size + 5] = 0;
		ret = y2_btif_send(c, f->data, f->size);
		goto out;
	}
	if (c->stp_pending >= Y2_STP_WINDOW) { ret = -EAGAIN; goto out; }
	f = &c->window[c->stp_tx]; f->size = size + 6;
	f->data[0] = 0x80 | (c->stp_tx << 3) | ((c->stp_rx + 7) & 7);
	f->data[1] = (channel << 4) | (size >> 8);
	f->data[2] = size; f->data[3] = f->data[0] + f->data[1] + f->data[2];
	memcpy(f->data + 4, data, size);
	crc = y2_stp_crc(data, size); f->data[size + 4] = crc; f->data[size + 5] = crc >> 8;
	ret = y2_btif_send(c, f->data, f->size);
	if (!ret) {
		c->stp_tx = (c->stp_tx + 1) & 7;
		if (!c->stp_pending++) mod_delayed_work(system_wq, &c->retry_work, msecs_to_jiffies(250));
	}
out:
	mutex_unlock(&c->stp_lock);
	return ret;
}
static void y2_stp_acknowledge(struct y2_conn *c, unsigned ack)
{
	/* Accept only acknowledgments inside the current live window. A stale
	 * duplicate must not release HCI data or advance across modulo rollover. */
	unsigned count = ((ack - c->stp_oldest) & 7) + 1;
	if (count > c->stp_pending) return;
	for (unsigned i = 0; i < count; i++) {
		memzero_explicit(&c->window[c->stp_oldest], sizeof(c->window[0]));
		c->stp_oldest = (c->stp_oldest + 1) & 7;
	}
	c->stp_pending -= count; c->stp_retries = 0;
	if (!c->stp_pending) cancel_delayed_work(&c->retry_work);
	else mod_delayed_work(system_wq, &c->retry_work, msecs_to_jiffies(250));
	wake_up_all(&c->tx_wait);
}
static int deliver(struct y2_conn *c, unsigned channel, const unsigned char *p, unsigned size)
{
	if (channel == Y2_CONN_BT) { y2_hci_receive(c, p, size); return 0; }
	if (channel != Y2_CONN_WMT || size > sizeof(c->response_data) || size < 5 ||
	    p[0] != 2 || (y2_conn_le16(p + 2) != size - 4 &&
	    !(READ_ONCE(c->wmt_reg_read) && p[1]==8 && size==16 && y2_conn_le16(p+2)==4))) return -EPROTO;
	if (completion_done(&c->response)) return -EPROTO;
	memcpy(c->response_data, p, size); c->response_size = size;
	complete(&c->response);
	return 0;
}
void y2_stp_receive(struct y2_conn *c, const unsigned char *data, unsigned size)
{
	int ret = 0;
	mutex_lock(&c->stp_lock);
	while (size-- && !c->failure) {
		unsigned channel, length, seq, ack;
		unsigned char byte = *data++;
		if (!c->rx_used && c->full_stp && (byte == 0x55 || byte == 0x7f)) continue;
		if (c->rx_used >= sizeof(c->rx_frame)) { ret = -EOVERFLOW; break; }
		c->rx_frame[c->rx_used++] = byte;
		if (c->rx_used == 4) {
			if (c->full_stp) {
				if (y2_stp_header(c->rx_frame, &channel, &length, &seq, &ack)) { ret = -EBADMSG; break; }
				c->rx_needed = length ? length + 6 : 4;
			} else {
				/* Use the outer STP length, including for the WMT register
				 * event whose inner length field omits address/value. The
				 * mandatory-mode checksum and CRC bytes are not validated. */
				channel = (c->rx_frame[1] >> 4) & 7;
				length = ((c->rx_frame[1] & 15) << 8) | c->rx_frame[2];
				if (!(c->rx_frame[0] & 0x80) || (c->rx_frame[0] & 0x40) ||
				    (c->rx_frame[1] & 0x80) || channel != Y2_CONN_WMT ||
				    !length || length > sizeof(c->response_data)) { ret = -EPROTO; break; }
				c->rx_needed = length + 6;
			}
		}
		if (c->rx_used < 4 || c->rx_used != c->rx_needed) continue;
		if (!c->full_stp) ret = deliver(c, Y2_CONN_WMT, c->rx_frame + 4, c->rx_used - 6);
		else {
			ret = y2_stp_header(c->rx_frame, &channel, &length, &seq, &ack);
			if (ret) break;
			if (length && y2_stp_crc(c->rx_frame + 4, length) != y2_conn_le16(c->rx_frame + 4 + length)) {
				ret = -EBADMSG; break;
			}
			y2_stp_acknowledge(c, ack);
			if (length) {
				if (seq == c->stp_rx) {
					ret = deliver(c, channel, c->rx_frame + 4, length);
					if (ret) break;
					c->stp_rx = (c->stp_rx + 1) & 7;
				}
				/* Duplicate/reordered payload is not delivered twice. Repeat
				 * our last accepted ACK so the peer retransmits correctly. */
				unsigned char response[] = {0x80 | ((c->stp_rx + 7) & 7), 0, 0, 0};
				response[3] = response[0];
				ret = y2_btif_send(c, response, sizeof(response));
			}
		}
		c->rx_used = c->rx_needed = 0;
		if (ret) break;
	}
	mutex_unlock(&c->stp_lock);
	if (ret) y2_conn_failed(c, ret);
}
