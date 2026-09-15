# SPDX-License-Identifier: GPL-2.0-only
ccflags-y += -DLINUX -DMT6628 -D_HIF_SDIO=1 -DDBG=0
# Legacy _HIF_SDIO selects the common fullmac register protocol, not an SDIO bus.
ccflags-y += -DCFG_SUPPORT_WAPI=0 -DCFG_SUPPORT_XLOG=0 -DCFG_SUPPORT_AEE=0
ccflags-y += -DCFG_SUPPORT_HOTSPOT_2_0=0 -DCFG_HS20_DEBUG=0 -DCFG_ENABLE_GTK_FRAME_FILTER=0
ccflags-y += -I$(src)/os -I$(src)/os/linux/include -I$(src)/include -I$(src)/include/nic -I$(src)/include/mgmt -I$(src)
obj-y += y2-wifi.o
y2-wifi-y := ahb.o factory.o recovery.o regulatory.o common/dump.o common/wlan_lib.o common/wlan_oid.o mgmt/aaa_fsm.o mgmt/ais_fsm.o mgmt/assoc.o mgmt/auth.o mgmt/bss.o mgmt/cnm.o mgmt/cnm_mem.o mgmt/cnm_timer.o mgmt/hem_mbox.o mgmt/hs20.o mgmt/mib.o mgmt/privacy.o mgmt/rate.o mgmt/rlm.o mgmt/rlm_domain.o mgmt/rlm_obss.o mgmt/rlm_protection.o mgmt/roaming_fsm.o mgmt/rsn.o mgmt/saa_fsm.o mgmt/scan.o mgmt/scan_fsm.o mgmt/sec_fsm.o mgmt/swcr.o nic/cmd_buf.o nic/nic.o nic/nic_cmd_event.o nic/nic_pwr_mgt.o nic/nic_rx.o nic/nic_tx.o nic/que_mgt.o os/linux/gl_cfg80211.o os/linux/gl_init.o os/linux/gl_kal.o
