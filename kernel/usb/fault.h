/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_USB_FAULT_H
#define Y2_USB_FAULT_H
/* Fix02 USB transport attribution. Fix01 lost USB during a loaded transfer
 * with the UI alive; every y2_usb_fail() is terminal until reboot and the
 * in-RAM observer was lost with USB, so the path was never identified. Each
 * terminal site now carries a reason and the first failure freezes a full
 * controller snapshot, readable over the independent Wi-Fi observer. */
enum y2_usb_reason {
    Y2_USB_REASON_NONE,
    Y2_USB_REASON_FIFO_LAYOUT,
    Y2_USB_REASON_IRQ_OVERFLOW,
    Y2_USB_REASON_DMA_BUS_ERROR,
    Y2_USB_REASON_INIT,
    Y2_USB_REASON_SUPPLY_MONITOR,
    Y2_USB_REASON_REGISTER,
    Y2_USB_REASON_RECONNECT,
    Y2_USB_REASON_PREFLIGHT,
    Y2_USB_REASON_PHY_REGION,
    Y2_USB_REASON_COUNT
};
static const char *const y2_usb_reason_names[Y2_USB_REASON_COUNT] = {
    "none", "fifo_layout", "irq_overflow", "dma_bus_error", "init",
    "supply_monitor", "register", "reconnect", "preflight", "phy_region",
};
struct y2_usb_fault {
    unsigned valid, reason, ms, gadget_state, irqs, dma_irqs, max_burst;
    int rc;
    unsigned l1_status, l1_mask;
    unsigned short tx, tx_mask, rx, rx_mask;
    unsigned char usb, usb_mask, power, devctl, faddr, index, dma_intr, endpoints;
    unsigned short txcsr[5], rxcsr[5], rxcount[5];
    unsigned dma_cntl[8], dma_addr[8], dma_count[8];
};
/* A PMIC/PWRAP read is a supply observation, not proof the supply vanished.
 * Only persistent failure (one second at the 250-ms poll) is terminal. */
#define Y2_USB_MONITOR_TOLERANCE 4
/* A detach/reconnect preflight performs no write; while the cable is present
 * it may be retried for ten seconds (VBUS/PHY settling after a droop). */
#define Y2_USB_RECONNECT_ATTEMPTS 40
struct y2_usb_tolerance {
    unsigned monitor_failures, monitor_transients;
    unsigned reconnect_failures, reconnect_retries;
};
static inline int y2_usb_monitor_terminal(struct y2_usb_tolerance *t, int sample_ok)
{
    if (sample_ok) {
        t->monitor_failures = 0;
        return 0;
    }
    if (++t->monitor_failures >= Y2_USB_MONITOR_TOLERANCE)
        return 1;
    t->monitor_transients++;
    return 0;
}
/* rc from a reconnect attempt; wrote tells whether any PHY/controller write
 * happened. Only a write-free refusal is retried; success resets the budget. */
static inline int y2_usb_reconnect_terminal(struct y2_usb_tolerance *t, int rc, int wrote)
{
    if (!rc) {
        t->reconnect_failures = 0;
        return 0;
    }
    if (wrote || ++t->reconnect_failures >= Y2_USB_RECONNECT_ATTEMPTS)
        return 1;
    t->reconnect_retries++;
    return 0;
}
/* Peak interrupts within one jiffy; the terminal storm limit is unchanged. */
#define Y2_USB_IRQ_BURST_LIMIT 512
#endif
