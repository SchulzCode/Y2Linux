/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_CONN_H
#define Y2_CONN_H
#include <linux/clk.h>
#include <linux/completion.h>
#include <linux/device.h>
#include <linux/dma-mapping.h>
#include <linux/mutex.h>
#include <linux/platform_device.h>
#include <linux/regmap.h>
#include <linux/regulator/consumer.h>
#include <linux/reset.h>
#include <linux/skbuff.h>
#include <linux/wait.h>
#include <linux/workqueue.h>
#include "protocol.h"

#define Y2_CONN_BT 0
#define Y2_CONN_WIFI 3
#define Y2_CONN_WMT 4
#define Y2_CONN_DMA_SIZE 8192
#define Y2_CONN_FW "mediatek/mt6582/"
struct firmware;
int y2_conn_request_firmware(struct device *dev, const char *name, const struct firmware **fw);
struct hci_dev;
struct y2_md;
struct y2_stp_frame { unsigned size; unsigned char data[Y2_STP_MAX_PAYLOAD + 6]; };
struct y2_conn {
	struct device *dev;
	struct mutex lifecycle, command, stp_lock, dma_tx, dma_rx, recovery_lock;
	struct clk_bulk_data clocks[3];
	struct regulator_bulk_data rails[4];
	struct regmap *pmic;
	struct reset_control *reset;
	bool rail_on[4], clock_on, factory_ready, activated, recovering, pmic_ready;
	unsigned long last_recovery;
	unsigned automatic_recoveries;
	bool hci_recovery_pending, wifi_wanted, md_owned;
	void __iomem *mcu, *top, *emi, *btif, *txdma, *rxdma;
	int irq[4];
	unsigned char *txbuf, *rxbuf;
	dma_addr_t txaddr, rxaddr;
	unsigned txptr, rxptr;
	bool transport_on, dma_active, full_stp, powered, suspended, removing, calibrated;
	unsigned functions, wanted, hvr, fvr, chip;
	unsigned stp_tx, stp_oldest, stp_rx, stp_pending, rx_used, rx_needed;
	unsigned stp_retries, transport_errors, recoveries;
	int failure;
	unsigned char rx_frame[Y2_STP_MAX_PAYLOAD + 6];
	struct y2_stp_frame window[8];
	wait_queue_head_t tx_wait;
	struct delayed_work retry_work;
	struct work_struct recovery_work;
	struct completion response;
	unsigned response_size;
	bool wmt_reg_read;
	unsigned char response_data[256];
	struct hci_dev *hdev;
	struct sk_buff *hci_rx;
	struct sk_buff_head hci_tx;
	struct work_struct hci_work;
	bool hci_open, hci_paused;
	bool bt_identity_set, bt_controller_address;
	unsigned char bt_address[6];
	unsigned hci_needed;
	unsigned char wifi_factory[512], bt_factory[64];
	struct platform_device *wifi;
	struct y2_md *md;
};
int y2_btif_probe(struct platform_device *pdev, struct y2_conn *c);
int y2_btif_start(struct y2_conn *c);
int y2_btif_stop(struct y2_conn *c);
int y2_btif_abort(struct y2_conn *c);
int y2_btif_send(struct y2_conn *c, const unsigned char *data, unsigned size);
void y2_stp_init(struct y2_conn *c);
void y2_stp_reset(struct y2_conn *c);
void y2_stp_receive(struct y2_conn *c, const unsigned char *data, unsigned size);
int y2_stp_send(struct y2_conn *c, unsigned channel, const unsigned char *data, unsigned size);
void y2_conn_failed(struct y2_conn *c, int error);
int y2_conn_get(struct y2_conn *c, unsigned function);
int y2_conn_put(struct y2_conn *c, unsigned function);
int y2_conn_wmt(struct y2_conn *c, const unsigned char *request, unsigned size,
	       const unsigned char *expected, unsigned expected_size);
int y2_wmt_boot(struct y2_conn *c);
int y2_wmt_function(struct y2_conn *c, unsigned function, bool enable);
int y2_conn_rail(struct y2_conn *c, unsigned rail, bool enable);
int y2_hci_register(struct y2_conn *c);
void y2_hci_unregister(struct y2_conn *c);
void y2_hci_receive(struct y2_conn *c, const unsigned char *data, unsigned size);
int y2_md_probe(struct platform_device *pdev, struct y2_conn *c);
int y2_md_calibrate(struct y2_conn *c);
void y2_md_unregister(struct y2_conn *c);
void y2_md_abort(struct y2_conn *c);
void y2_conn_hci_error(struct y2_conn *c);
#endif
