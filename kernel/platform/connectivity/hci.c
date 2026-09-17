// SPDX-License-Identifier: GPL-2.0-only
/* Standard HCI over MT6582 BTIF/STP. Controller setup is reconstructed from
 * this product's libbluetooth_mtk GORM_Init script (64-byte BTAddr record).
 * No EDR restriction or invented LE feature/command bits are installed. */
#include <crypto/sha2.h>
#include <linux/etherdevice.h>
#include <net/bluetooth/bluetooth.h>
#include <net/bluetooth/hci_core.h>
#include "conn.h"

static void controller_quirks(struct y2_conn *c)
{
	if (c->chip != 0x6582 || c->hvr != 0x8a01 || c->fvr != 0x8a00) return;
	/* Own E2 trace: page 1 succeeds with max_page=2, but reading page 2
	 * returns status 0x30. Use the kernel quirk, as the donor does, without
	 * masking EDR/LE features or supported commands. Also retire a cached
	 * false maximum if an earlier setup attempt already read it. */
	hci_set_quirk(c->hdev, HCI_QUIRK_BROKEN_LOCAL_EXT_FEATURES_PAGE_2);
	c->hdev->max_page = 1;
}
static int command(struct hci_dev *hdev, u16 opcode, unsigned n, const void *data)
{
	struct sk_buff *reply = __hci_cmd_sync(hdev, opcode, n, data, HCI_CMD_TIMEOUT);
	int ret;
	if (IS_ERR(reply)) return PTR_ERR(reply);
	ret = reply->len && !reply->data[0] ? 0 : -EPROTO;
	kfree_skb(reply); return ret;
}
static int setup(struct hci_dev *hdev)
{
	struct y2_conn *c = hci_get_drvdata(hdev);
	struct sk_buff *reply; int ret;
	unsigned char wire[6], candidate[6], digest[32];
	/* Hash of the generic, non-unique MT6582 address in BOTH this Y2's stock
	 * libcustom_nvram and libbluetooth_mtk. It is a reject value, never an
	 * assigned identity. A valid controller identity takes precedence. */
	static const unsigned char default_hash[32] = {
		0x8b,0xf9,0xd1,0xe7,0x4e,0xea,0x89,0xe5,0x1c,0xfb,0x4d,0x83,0x22,0x59,0x02,0x12,
		0xd2,0x7b,0xde,0x6e,0xe8,0x4f,0x49,0x7f,0xb7,0xaf,0x57,0xdf,0xf1,0xa9,0xd9,0xf6,
	};
	if (!c->bt_identity_set) {
		reply = __hci_cmd_sync(hdev, HCI_OP_READ_BD_ADDR, 0, NULL, HCI_CMD_TIMEOUT);
		if (IS_ERR(reply)) return PTR_ERR(reply);
		if (reply->len != 7 || reply->data[0]) { kfree_skb(reply); return -EPROTO; }
		for (unsigned i = 0; i < 6; i++) candidate[i] = reply->data[6-i];
		kfree_skb(reply); sha256(candidate, 6, digest);
		c->bt_controller_address = is_valid_ether_addr(candidate) && memcmp(digest, default_hash, 32);
		memcpy(c->bt_address, c->bt_controller_address ? candidate : c->bt_factory, 6);
		c->bt_identity_set = true;
		dev_info(c->dev, "Bluetooth identity: %s\n", c->bt_controller_address ?
			 "valid controller identity" : "persistent per-device Y2DATA fallback");
	}
	for (unsigned i = 0; i < 6; i++) wire[i] = c->bt_address[5-i];
	ret = command(hdev, 0xfc1a, 6, wire);
	/* Stock order: address, radio, codec, sleep, Reset. Global GORM state
	 * stores the record at +4: its +0x10/+0x1f/+0x16 accesses map below. */
	if (!ret) ret = command(hdev, 0xfc79, 6, c->bt_factory + 12);
	if (!ret) ret = command(hdev, 0xfc93, 3, c->bt_factory + 27);
	if (!ret) ret = command(hdev, 0xfc7a, 7, c->bt_factory + 18);
	if (!ret) ret = command(hdev, HCI_OP_RESET, 0, NULL);
	if (!ret) controller_quirks(c);
	memzero_explicit(candidate, sizeof(candidate)); memzero_explicit(wire, sizeof(wire));
	return ret;
}
static void tx_work(struct work_struct *work)
{
	struct y2_conn *c = container_of(work, struct y2_conn, hci_work);
	struct sk_buff *skb;
	while ((skb = skb_dequeue(&c->hci_tx))) {
		int ret = READ_ONCE(c->hci_open) && !READ_ONCE(c->hci_paused) ?
			y2_stp_send(c, Y2_CONN_BT, skb->data, skb->len) : -ESHUTDOWN;
		if (ret) c->hdev->stat.err_tx++;
		else c->hdev->stat.byte_tx += skb->len;
		kfree_skb(skb);
		if (ret && ret != -ESHUTDOWN) { y2_conn_failed(c, ret); break; }
	}
}
static int open(struct hci_dev *hdev)
{
	struct y2_conn *c = hci_get_drvdata(hdev);
	int ret = y2_conn_get(c, Y2_CONN_BT);
	if (ret) return ret;
	WRITE_ONCE(c->hci_paused,false);
	WRITE_ONCE(c->hci_open, true);
	return 0;
}
static int flush(struct hci_dev *hdev)
{
	struct y2_conn *c = hci_get_drvdata(hdev);
	cancel_work_sync(&c->hci_work); skb_queue_purge(&c->hci_tx);
	return 0;
}
static int close(struct hci_dev *hdev)
{
	struct y2_conn *c = hci_get_drvdata(hdev);
	WRITE_ONCE(c->hci_open, false); flush(hdev);
	mutex_lock(&c->stp_lock); kfree_skb(c->hci_rx); c->hci_rx = NULL; mutex_unlock(&c->stp_lock);
	return y2_conn_put(c, Y2_CONN_BT);
}
static int send(struct hci_dev *hdev, struct sk_buff *skb)
{
	struct y2_conn *c = hci_get_drvdata(hdev); u8 type = hci_skb_pkt_type(skb);
	if (!READ_ONCE(c->hci_open) || READ_ONCE(c->hci_paused)) return -ESHUTDOWN;
	if (skb->len + 1 > Y2_STP_MAX_PAYLOAD || skb_queue_len(&c->hci_tx) >= 128) return -ENOBUFS;
	if (type != HCI_COMMAND_PKT && type != HCI_ACLDATA_PKT && type != HCI_SCODATA_PKT) return -EINVAL;
	if (skb_cow_head(skb, 1)) return -ENOMEM;
	*(u8 *)skb_push(skb, 1) = type;
	if (type == HCI_COMMAND_PKT) hdev->stat.cmd_tx++;
	else if (type == HCI_ACLDATA_PKT) hdev->stat.acl_tx++;
	else hdev->stat.sco_tx++;
	skb_queue_tail(&c->hci_tx, skb); schedule_work(&c->hci_work);
	return 0;
}
void y2_hci_receive(struct y2_conn *c, const unsigned char *data, unsigned size)
{
	/* STP holds its receive mutex. H4 headers and bodies may cross any STP
	 * or DMA boundary; never treat a transport packet as a complete HCI ACL. */
	if (!READ_ONCE(c->hci_open) || READ_ONCE(c->hci_paused) || !c->hdev) return;
	c->hdev->stat.byte_rx += size;
	while (size) {
		struct sk_buff *skb = c->hci_rx;
		if (!skb) {
			u8 type = *data++; size--;
			unsigned header = type == HCI_EVENT_PKT ? 2 : type == HCI_ACLDATA_PKT ? 4 : type == HCI_SCODATA_PKT ? 3 : 0;
			if (!header) goto bad;
			skb = bt_skb_alloc(HCI_MAX_FRAME_SIZE, GFP_ATOMIC);
			if (!skb) goto bad;
			hci_skb_pkt_type(skb) = type; c->hci_needed = header; c->hci_rx = skb;
		}
		unsigned count = min(size, c->hci_needed - skb->len);
		skb_put_data(skb, data, count); data += count; size -= count;
		if (skb->len != c->hci_needed) continue;
		u8 type = hci_skb_pkt_type(skb);
		unsigned header = type == HCI_EVENT_PKT ? 2 : type == HCI_ACLDATA_PKT ? 4 : 3;
		if (c->hci_needed == header) {
			unsigned payload = type == HCI_EVENT_PKT ? skb->data[1] :
				type == HCI_ACLDATA_PKT ? y2_conn_le16(skb->data + 2) : skb->data[2];
			if (header + payload > HCI_MAX_FRAME_SIZE) goto bad;
			c->hci_needed += payload;
			if (payload) continue;
		}
		c->hci_rx = NULL;
		if (hci_recv_frame(c->hdev, skb)) c->hdev->stat.err_rx++;
	}
	return;
bad:
	kfree_skb(c->hci_rx); c->hci_rx = NULL; c->hdev->stat.err_rx++;
	y2_conn_failed(c, -EPROTO);
}
static void hw_error(struct hci_dev *hdev, u8 code)
{
	y2_conn_hci_error(hci_get_drvdata(hdev));
}
int y2_hci_register(struct y2_conn *c)
{
	struct hci_dev *hdev = hci_alloc_dev(); int ret;
	if (!hdev) return -ENOMEM;
	c->hdev = hdev; hci_set_drvdata(hdev, c); SET_HCIDEV_DEV(hdev, c->dev);
	hdev->bus = HCI_VIRTUAL; /* Integrated BTIF has no separate HCI bus enum. */
	hdev->open=open; hdev->close=close; hdev->flush=flush; hdev->send=send;
	hdev->setup=setup; hdev->hw_error=hw_error;
	/* Function power-off loses volatile controller setup; repeat before init. */
	hci_set_quirk(hdev, HCI_QUIRK_NON_PERSISTENT_SETUP);
	skb_queue_head_init(&c->hci_tx); INIT_WORK(&c->hci_work, tx_work);
	ret = hci_register_dev(hdev);
	if (ret) { hci_free_dev(hdev); c->hdev=NULL; }
	return ret;
}
void y2_hci_unregister(struct y2_conn *c)
{
	if (!c->hdev) return;
	hci_unregister_dev(c->hdev); flush(c->hdev); hci_free_dev(c->hdev); c->hdev=NULL;
}
