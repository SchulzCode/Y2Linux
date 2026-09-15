// SPDX-License-Identifier: GPL-2.0-only
/* MT6582 BTIF and its two AP_DMA virtual FIFOs. Register/handshake definitions
 * derived from MediaTek 2011-2014 GPL hal_btif{,_dma}.c. Only channel windows
 * 0x780/0x800 are mapped; I2C's channels and shared DMA clock remain CCF-owned.
 */
#include <linux/delay.h>
#include <linux/interrupt.h>
#include <linux/io.h>
#include <linux/iopoll.h>
#include "conn.h"

#define INT_FLAG 0x00
#define INT_EN 0x04
#define ENABLE 0x08
#define RESET 0x0c
#define STOP 0x10
#define FLUSH 0x14
#define BASE 0x1c
#define LENGTH 0x24
#define THRESHOLD 0x28
#define WRITE_PTR 0x2c
#define READ_PTR 0x30
#define VALID 0x3c
#define LEFT 0x40

static unsigned advance(unsigned ptr, unsigned count)
{
	unsigned offset = (ptr & 0xffff) + count;
	return (offset % Y2_CONN_DMA_SIZE) | ((ptr ^
		(offset >= Y2_CONN_DMA_SIZE ? 0x10000 : 0)) & 0x10000);
}
static irqreturn_t y2_btif_irq(int irq, void *data)
{
	struct y2_conn *c = data;
	unsigned available, first, offset, pointer;
	if (!READ_ONCE(c->transport_on)) return IRQ_NONE;
	if (irq == c->irq[1]) {
		/* TX status is cleared by zero, RX status is write-one-to-clear. */
		mutex_lock(&c->dma_tx);
		writel(0, c->txdma + INT_FLAG);
		available = readl(c->txdma + VALID);
		if (available && available < 8 && !(readl(c->txdma + FLUSH) & 1) &&
		    !(readl(c->txdma + STOP) & 1)) writel(1, c->txdma + FLUSH);
		mutex_unlock(&c->dma_tx);
		wake_up_all(&c->tx_wait);
		return IRQ_HANDLED;
	}
	if (irq == c->irq[3]) {
		writel(1, c->btif + 0x64); /* Host/controller wake handshake. */
		return IRQ_HANDLED;
	}
	mutex_lock(&c->dma_rx);
	pointer = readl(c->rxdma + WRITE_PTR) & 0x1ffff;
	offset = c->rxptr & 0xffff;
	if ((pointer & 0xffff) >= Y2_CONN_DMA_SIZE || offset >= Y2_CONN_DMA_SIZE) goto corrupt;
	available = (pointer & 0xffff) - offset;
	if ((pointer ^ c->rxptr) & 0x10000) available += Y2_CONN_DMA_SIZE;
	if (available > Y2_CONN_DMA_SIZE) goto corrupt;
	dma_rmb();
	first = min(available, Y2_CONN_DMA_SIZE - offset);
	if (first) y2_stp_receive(c, c->rxbuf + offset, first);
	if (available > first) y2_stp_receive(c, c->rxbuf, available - first);
	c->rxptr = advance(c->rxptr, available);
	dma_wmb(); writel(c->rxptr, c->rxdma + READ_PTR);
	writel(3, c->rxdma + INT_FLAG);
	readl(c->btif + 0x4c); /* clear legacy BTIF RX timeout */
	mutex_unlock(&c->dma_rx);
	return IRQ_HANDLED;
corrupt:
	mutex_unlock(&c->dma_rx);
	y2_conn_failed(c, -EOVERFLOW);
	return IRQ_HANDLED;
}
int y2_btif_send(struct y2_conn *c, const unsigned char *data, unsigned size)
{
	unsigned value, offset, first; int ret;
	if (!size || size > Y2_CONN_DMA_SIZE) return -EMSGSIZE;
	mutex_lock(&c->dma_tx);
	if (!READ_ONCE(c->transport_on)) { ret = -ESHUTDOWN; goto out; }
	ret = readl_poll_timeout(c->txdma + FLUSH, value, !(value & 1), 10, 100000);
	if (!ret) ret = readl_poll_timeout(c->txdma + LEFT, value, value >= size, 10, 100000);
	if (ret) goto out;
	if (readl(c->txdma + STOP) & 1) { ret = -ESHUTDOWN; goto out; }
	/* TX interrupt enable is hardware-cleared at the free-space threshold.
	 * Mask while filling, then rearm after EVERY submission (stock sequence).
	 * Enabling it once on an empty FIFO loses the later <8-byte tail IRQ. */
	writel(0, c->txdma + INT_EN);
	offset = c->txptr & 0xffff;
	first = min(size, Y2_CONN_DMA_SIZE - offset);
	memcpy(c->txbuf + offset, data, first);
	if (first != size) memcpy(c->txbuf, data + first, size - first);
	c->txptr = advance(c->txptr, size);
	dma_wmb();
	writel(1, c->txdma + ENABLE);
	writel(c->txptr, c->txdma + WRITE_PTR);
	/* Flush short trailing transfers; never combine STOP and FLUSH. */
	value = readl(c->txdma + VALID);
	if (value && value < 8)
		writel(1, c->txdma + FLUSH);
	writel(1, c->txdma + INT_EN);
out:
	mutex_unlock(&c->dma_tx);
	return ret;
}
static void init_dma(void __iomem *base, dma_addr_t address, bool rx)
{
	writel(0, base + INT_EN);
	writel(2, base + RESET); writel(0, base + RESET);
	writel(lower_32_bits(address), base + BASE);
	writel(Y2_CONN_DMA_SIZE, base + LENGTH);
	writel(0, base + WRITE_PTR); writel(0, base + READ_PTR);
	writel(rx ? Y2_CONN_DMA_SIZE * 3 / 4 : Y2_CONN_DMA_SIZE - 7, base + THRESHOLD);
	writel(rx ? 3 : 0, base + INT_FLAG);
	writel(rx ? 3 : 0, base + INT_EN);
	if (rx) writel(1, base + ENABLE);
}
int y2_btif_start(struct y2_conn *c)
{
	int ret;
	if (c->dma_active) return -EBUSY;
	/* clocks[0] is CONNMCU, owned by the core power lifecycle. */
	ret = clk_bulk_prepare_enable(2, &c->clocks[1]);
	if (ret) return ret;
	c->dma_active = true;
	c->txptr = c->rxptr = 0;
	memset(c->txbuf, 0, Y2_CONN_DMA_SIZE); memset(c->rxbuf, 0, Y2_CONN_DMA_SIZE);
	writel(0, c->btif + 4);
	writel(0, c->btif + 0x0c);
	writel(1, c->btif + 0x6c);
	writel(0x18, c->btif + 0x60);
	writel(6, c->btif + 8);
	writel(0, c->btif + 8); /* Release both FIFO clear bits before enabling DMA. */
	writel(0, c->btif + 0x48);
	init_dma(c->txdma, c->txaddr, false); init_dma(c->rxdma, c->rxaddr, true);
	writel(7, c->btif + 0x4c);
	WRITE_ONCE(c->transport_on, true);
	for (unsigned i = 0; i < ARRAY_SIZE(c->irq); i++) enable_irq(c->irq[i]);
	writel(1, c->btif + 4);
	return 0;
}
void y2_btif_report_timeout(struct y2_conn *c)
{
	/* WMT's lifecycle owner calls this before shutdown, with clocks held.
	 * Log only control/status and FIFO offsets, never DMA base addresses,
	 * traffic, factory data or radio identities. */
	if (!c->dma_active || !READ_ONCE(c->transport_on)) return;
	mutex_lock(&c->dma_tx);
	dev_err(c->dev, "BTIF timeout: IER=%x LSR=%x DMA=%x TX en=%x ien=%x flag=%x wpt=%x rpt=%x valid=%u left=%u flush=%x\n",
		readl(c->btif+4), readl(c->btif+0x14), readl(c->btif+0x4c),
		readl(c->txdma+ENABLE), readl(c->txdma+INT_EN), readl(c->txdma+INT_FLAG),
		readl(c->txdma+WRITE_PTR), readl(c->txdma+READ_PTR),
		readl(c->txdma+VALID), readl(c->txdma+LEFT), readl(c->txdma+FLUSH));
	mutex_unlock(&c->dma_tx);
	mutex_lock(&c->dma_rx);
	dev_err(c->dev, "BTIF timeout: RX en=%x flag=%x wpt=%x rpt=%x valid=%u\n",
		readl(c->rxdma+ENABLE), readl(c->rxdma+INT_FLAG),
		readl(c->rxdma+WRITE_PTR), readl(c->rxdma+READ_PTR), readl(c->rxdma+VALID));
	mutex_unlock(&c->dma_rx);
}
int y2_btif_stop(struct y2_conn *c)
{
	unsigned value; int ret = 0;
	if (!c->dma_active) return 0;
	/* Block submissions, finish threaded RX, then stop the only DMA channels
	 * this driver owns. A failed stop retains buffers and clock references. */
	if (READ_ONCE(c->transport_on)) {
		WRITE_ONCE(c->transport_on, false);
		for (unsigned i = 0; i < ARRAY_SIZE(c->irq); i++) disable_irq(c->irq[i]);
	}
	mutex_lock(&c->dma_tx);
	writel(0, c->btif + 4);
	writel(0, c->txdma + INT_EN); writel(0, c->rxdma + INT_EN);
	ret = readl_poll_timeout(c->txdma + FLUSH, value, !(value & 1), 10, 100000);
	if (ret) goto out;
	writel(1, c->txdma + STOP); writel(1, c->rxdma + STOP);
	ret = readl_poll_timeout(c->txdma + ENABLE, value, !(value & 1), 10, 10000);
	if (!ret) ret = readl_poll_timeout(c->rxdma + ENABLE, value, !(value & 1), 10, 10000);
	if (ret) goto out;
	writel(0, c->txdma + STOP); writel(0, c->rxdma + STOP);
	writel(0, c->btif + 0x4c);
	clk_bulk_disable_unprepare(2, &c->clocks[1]);
	c->dma_active = false;
out:
	mutex_unlock(&c->dma_tx);
	return ret;
}
int y2_btif_probe(struct platform_device *pdev, struct y2_conn *c)
{
	static const char *const names[] = {"btif", "tx-dma", "rx-dma", "wake"};
	int ret = dma_set_mask_and_coherent(c->dev, DMA_BIT_MASK(32));
	if (ret) return ret;
	c->btif = devm_platform_ioremap_resource_byname(pdev, "btif");
	c->txdma = devm_platform_ioremap_resource_byname(pdev, "tx-dma");
	c->rxdma = devm_platform_ioremap_resource_byname(pdev, "rx-dma");
	if (IS_ERR(c->btif)) return PTR_ERR(c->btif);
	if (IS_ERR(c->txdma)) return PTR_ERR(c->txdma);
	if (IS_ERR(c->rxdma)) return PTR_ERR(c->rxdma);
	c->txbuf = dmam_alloc_coherent(c->dev, Y2_CONN_DMA_SIZE, &c->txaddr, GFP_KERNEL);
	c->rxbuf = dmam_alloc_coherent(c->dev, Y2_CONN_DMA_SIZE, &c->rxaddr, GFP_KERNEL);
	if (!c->txbuf || !c->rxbuf) return -ENOMEM;
	for (unsigned i = 0; i < ARRAY_SIZE(c->irq); i++) {
		c->irq[i] = platform_get_irq_byname(pdev, names[i]);
		if (c->irq[i] < 0) return c->irq[i];
		ret = devm_request_threaded_irq(c->dev, c->irq[i], NULL, y2_btif_irq,
			IRQF_ONESHOT | IRQF_NO_AUTOEN, dev_name(c->dev), c);
		if (ret) return ret;
	}
	mutex_init(&c->dma_tx);
	mutex_init(&c->dma_rx);
	return 0;
}
int y2_btif_abort(struct y2_conn *c)
{
	/* Only after CONN bus isolation and both power-off acknowledgments.
	 * A wedged remote can leave TX FLUSH pending forever: reset these two
	 * channels, never the shared AP_DMA controller or its I2C channels. */
	unsigned value; int ret=0;
	if (!c->dma_active) return 0;
	if (c->powered || c->transport_on) return -EBUSY;
	mutex_lock(&c->dma_tx);
	writel(2,c->txdma+RESET); writel(2,c->rxdma+RESET);
	ret=readl_poll_timeout(c->txdma+ENABLE,value,!(value&1),10,10000);
	if (!ret) ret=readl_poll_timeout(c->rxdma+ENABLE,value,!(value&1),10,10000);
	if (!ret) {
		writel(0,c->txdma+RESET); writel(0,c->rxdma+RESET);
		writel(0,c->btif+0x4c); c->dma_active=false;
		clk_bulk_disable_unprepare(2,&c->clocks[1]);
	}
	mutex_unlock(&c->dma_tx);
	return ret;
}
