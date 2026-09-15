/*
* Copyright (C) 2011-2014 MediaTek Inc.
*
* This program is free software: you can redistribute it and/or modify it under the terms of the
* GNU General Public License version 2 as published by the Free Software Foundation.
*
* This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
* without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
* See the GNU General Public License for more details.
*
* You should have received a copy of the GNU General Public License along with this program.
* If not, see <http://www.gnu.org/licenses/>.
*/

/*
** $Id: //Department/DaVinci/BRANCHES/MT6620_WIFI_DRIVER_V2_3/os/linux/gl_init.c#7 $
*/

/*! \file   gl_init.c
    \brief  Main routines of Linux driver

    This file contains the main routines of Linux driver for MediaTek Inc. 802.11
    Wireless LAN Adapters.
*/



/*
** $Log: gl_init.c $
**
** 09 03 2013 cp.wu
** add path for reassociation
 *
 * 07 17 2012 yuche.tsai
 * NULL
 * Fix compile error.
 *
 * 07 17 2012 yuche.tsai
 * NULL
 * Fix compile error for JB.
 *
 * 07 17 2012 yuche.tsai
 * NULL
 * Let netdev bring up.
 *
 * 07 17 2012 yuche.tsai
 * NULL
 * Compile no error before trial run.
 *
 * 06 13 2012 yuche.tsai
 * NULL
 * Update maintrunk driver.
 * Add support for driver compose assoc request frame.
 *
 * 05 25 2012 yuche.tsai
 * NULL
 * Fix reset KE issue.
 *
 * 05 11 2012 cp.wu
 * [WCXRP00001237] [MT6620 Wi-Fi][Driver] Show MAC address and MAC address source for ACS's convenience
 * show MAC address & source while initiliazation
 *
 * 03 02 2012 terry.wu
 * NULL
 * EXPORT_SYMBOL(rsnParseCheckForWFAInfoElem);.
 *
 * 03 02 2012 terry.wu
 * NULL
 * Snc CFG80211 modification for ICS migration from branch 2.2.
 *
 * 03 02 2012 terry.wu
 * NULL
 * Sync CFG80211 modification from branch 2,2.
 *
 * 03 02 2012 terry.wu
 * NULL
 * Enable CFG80211 Support.
 *
 * 12 22 2011 george.huang
 * [WCXRP00000905] [MT6628 Wi-Fi][FW] Code refinement for ROM/ RAM module dependency
 * using global variable instead of stack for setting wlanoidSetNetworkAddress(), due to buffer may be released before TX thread handling
 *
 * 11 18 2011 yuche.tsai
 * NULL
 * CONFIG P2P support RSSI query, default turned off.
 *
 * 11 14 2011 yuche.tsai
 * [WCXRP00001107] [Volunteer Patch][Driver] Large Network Type index assert in FW issue.
 * Fix large network type index assert in FW issue.
 *
 * 11 14 2011 cm.chang
 * NULL
 * Fix compiling warning
 *
 * 11 11 2011 yuche.tsai
 * NULL
 * Fix work thread cancel issue.
 *
 * 11 10 2011 cp.wu
 * [WCXRP00001098] [MT6620 Wi-Fi][Driver] Replace printk by DBG LOG macros in linux porting layer
 * 1. eliminaite direct calls to printk in porting layer.
 * 2. replaced by DBGLOG, which would be XLOG on ALPS platforms.
 *
 * 10 06 2011 eddie.chen
 * [WCXRP00001027] [MT6628 Wi-Fi][Firmware/Driver] Tx fragmentation
 * Add rlmDomainGetChnlList symbol.
 *
 * 09 22 2011 cm.chang
 * NULL
 * Safer writng stype to avoid unitialized regitry structure
 *
 * 09 21 2011 cm.chang
 * [WCXRP00000969] [MT6620 Wi-Fi][Driver][FW] Channel list for 5G band based on country code
 * Avoid possible structure alignment problem
 *
 * 09 20 2011 chinglan.wang
 * [WCXRP00000989] [WiFi Direct] [Driver] Add a new io control API to start the formation for the sigma test.
 * .
 *
 * 09 08 2011 cm.chang
 * [WCXRP00000969] [MT6620 Wi-Fi][Driver][FW] Channel list for 5G band based on country code
 * Use new fields ucChannelListMap and ucChannelListIndex in NVRAM
 *
 * 08 31 2011 cm.chang
 * [WCXRP00000969] [MT6620 Wi-Fi][Driver][FW] Channel list for 5G band based on country code
 * .
 *
 * 08 11 2011 cp.wu
 * [WCXRP00000830] [MT6620 Wi-Fi][Firmware] Use MDRDY counter to detect empty channel for shortening scan time
 * expose scnQuerySparseChannel() for P2P-FSM.
 *
 * 08 11 2011 cp.wu
 * [WCXRP00000830] [MT6620 Wi-Fi][Firmware] Use MDRDY counter to detect empty channel for shortening scan time
 * sparse channel detection:
 * driver: collect sparse channel information with scan-done event
 *
 * 08 02 2011 yuche.tsai
 * [WCXRP00000896] [Volunteer Patch][WiFi Direct][Driver] GO with multiple client, TX deauth to a disconnecting device issue.
 * Fix GO send deauth frame issue.
 *
 * 07 07 2011 wh.su
 * [WCXRP00000839] [MT6620 Wi-Fi][Driver] Add the dumpMemory8 and dumpMemory32 EXPORT_SYMBOL
 * Add the dumpMemory8 symbol export for debug mode.
 *
 * 07 06 2011 terry.wu
 * [WCXRP00000735] [MT6620 Wi-Fi][BoW][FW/Driver] Protect BoW connection establishment
 * Improve BoW connection establishment speed.
 *
 * 07 05 2011 yuche.tsai
 * [WCXRP00000821] [Volunteer Patch][WiFi Direct][Driver] WiFi Direct Connection Speed Issue
 * Export one symbol for enhancement.
 *
 * 06 13 2011 eddie.chen
 * [WCXRP00000779] [MT6620 Wi-Fi][DRV]  Add tx rx statistics in linux and use netif_rx_ni
 * Add tx rx statistics and netif_rx_ni.
 *
 * 05 27 2011 cp.wu
 * [WCXRP00000749] [MT6620 Wi-Fi][Driver] Add band edge tx power control to Wi-Fi NVRAM
 * invoke CMD_ID_SET_EDGE_TXPWR_LIMIT when there is valid data exist in NVRAM content.
 *
 * 05 18 2011 cp.wu
 * [WCXRP00000734] [MT6620 Wi-Fi][Driver] Pass PHY_PARAM in NVRAM to firmware domain
 * pass PHY_PARAM in NVRAM from driver to firmware.
 *
 * 05 09 2011 jeffrey.chang
 * [WCXRP00000710] [MT6620 Wi-Fi] Support pattern filter update function on IP address change
 * support ARP filter through kernel notifier
 *
 * 05 03 2011 chinghwa.yu
 * [WCXRP00000065] Update BoW design and settings
 * Use kalMemAlloc to allocate event buffer for kalIndicateBOWEvent.
 *
 * 04 27 2011 george.huang
 * [WCXRP00000684] [MT6620 Wi-Fi][Driver] Support P2P setting ARP filter
 * Support P2P ARP filter setting on early suspend/ late resume
 *
 * 04 18 2011 terry.wu
 * [WCXRP00000660] [MT6620 Wi-Fi][Driver] Remove flag CFG_WIFI_DIRECT_MOVED
 * Remove flag CFG_WIFI_DIRECT_MOVED.
 *
 * 04 15 2011 chinghwa.yu
 * [WCXRP00000065] Update BoW design and settings
 * Add BOW short range mode.
 *
 * 04 14 2011 yuche.tsai
 * [WCXRP00000646] [Volunteer Patch][MT6620][FW/Driver] Sigma Test Modification for some test case.
 * Modify some driver connection flow or behavior to pass Sigma test more easier..
 *
 * 04 12 2011 cm.chang
 * [WCXRP00000634] [MT6620 Wi-Fi][Driver][FW] 2nd BSS will not support 40MHz bandwidth for concurrency
 * .
 *
 * 04 11 2011 george.huang
 * [WCXRP00000621] [MT6620 Wi-Fi][Driver] Support P2P supplicant to set power mode
 * export wlan functions to p2p
 *
 * 04 08 2011 pat.lu
 * [WCXRP00000623] [MT6620 Wi-Fi][Driver] use ARCH define to distinguish PC Linux driver
 * Use CONFIG_X86 instead of PC_LINUX_DRIVER_USE option to have proper compile settting for PC Linux driver
 *
 * 04 08 2011 cp.wu
 * [WCXRP00000540] [MT5931][Driver] Add eHPI8/eHPI16 support to Linux Glue Layer
 * glBusFreeIrq() should use the same pvCookie as glBusSetIrq() or request_irq()/free_irq() won't work as a pair.
 *
 * 04 08 2011 eddie.chen
 * [WCXRP00000617] [MT6620 Wi-Fi][DRV/FW] Fix for sigma
 * Fix for sigma
 *
 * 04 06 2011 cp.wu
 * [WCXRP00000540] [MT5931][Driver] Add eHPI8/eHPI16 support to Linux Glue Layer
 * 1. do not check for pvData inside wlanNetCreate() due to it is NULL for eHPI  port
 * 2. update perm_addr as well for MAC address
 * 3. not calling check_mem_region() anymore for eHPI
 * 4. correct MSC_CS macro for 0-based notation
 *
 * 03 29 2011 cp.wu
 * [WCXRP00000598] [MT6620 Wi-Fi][Driver] Implementation of interface for communicating with user space process for RESET_START and RESET_END events
 * fix typo.
 *
 * 03 29 2011 cp.wu
 * [WCXRP00000598] [MT6620 Wi-Fi][Driver] Implementation of interface for communicating with user space process for RESET_START and RESET_END events
 * implement kernel-to-userspace communication via generic netlink socket for whole-chip resetting mechanism
 *
 * 03 23 2011 cp.wu
 * [WCXRP00000540] [MT5931][Driver] Add eHPI8/eHPI16 support to Linux Glue Layer
 * apply multi-queue operation only for linux kernel > 2.6.26
 *
 * 03 22 2011 pat.lu
 * [WCXRP00000592] [MT6620 Wi-Fi][Driver] Support PC Linux Environment Driver Build
 * Add a compiler option "PC_LINUX_DRIVER_USE" for building driver in PC Linux environment.
 *
 * 03 21 2011 cp.wu
 * [WCXRP00000540] [MT5931][Driver] Add eHPI8/eHPI16 support to Linux Glue Layer
 * portability for compatible with linux 2.6.12.
 *
 * 03 21 2011 cp.wu
 * [WCXRP00000540] [MT5931][Driver] Add eHPI8/eHPI16 support to Linux Glue Layer
 * improve portability for awareness of early version of linux kernel and wireless extension.
 *
 * 03 21 2011 cp.wu
 * [WCXRP00000540] [MT5931][Driver] Add eHPI8/eHPI16 support to Linux Glue Layer
 * portability improvement
 *
 * 03 18 2011 jeffrey.chang
 * [WCXRP00000512] [MT6620 Wi-Fi][Driver] modify the net device relative functions to support the H/W multiple queue
 * remove early suspend functions
 *
 * 03 17 2011 cp.wu
 * [WCXRP00000562] [MT6620 Wi-Fi][Driver] I/O buffer pre-allocation to avoid physically continuous memory shortage after system running for a long period
 * reverse order to prevent probing racing.
 *
 * 03 16 2011 cp.wu
 * [WCXRP00000562] [MT6620 Wi-Fi][Driver] I/O buffer pre-allocation to avoid physically continuous memory shortage after system running for a long period
 * 1. pre-allocate physical continuous buffer while module is being loaded
 * 2. use pre-allocated physical continuous buffer for TX/RX DMA transfer
 *
 * The windows part remained the same as before, but added similiar APIs to hide the difference.
 *
 * 03 15 2011 jeffrey.chang
 * [WCXRP00000558] [MT6620 Wi-Fi][MT6620 Wi-Fi][Driver] refine the queue selection algorithm for WMM
 * refine the queue_select function
 *
 * 03 10 2011 cp.wu
 * [WCXRP00000532] [MT6620 Wi-Fi][Driver] Migrate NVRAM configuration procedures from MT6620 E2 to MT6620 E3
 * deprecate configuration used by MT6620 E2
 *
 * 03 10 2011 terry.wu
 * [WCXRP00000505] [MT6620 Wi-Fi][Driver/FW] WiFi Direct Integration
 * Remove unnecessary assert and message.
 *
 * 03 08 2011 terry.wu
 * [WCXRP00000505] [MT6620 Wi-Fi][Driver/FW] WiFi Direct Integration
 * Export nicQmUpdateWmmParms.
 *
 * 03 03 2011 jeffrey.chang
 * [WCXRP00000512] [MT6620 Wi-Fi][Driver] modify the net device relative functions to support the H/W multiple queue
 * support concurrent network
 *
 * 03 03 2011 jeffrey.chang
 * [WCXRP00000512] [MT6620 Wi-Fi][Driver] modify the net device relative functions to support the H/W multiple queue
 * modify net device relative functions to support multiple H/W queues
 *
 * 02 24 2011 george.huang
 * [WCXRP00000495] [MT6620 Wi-Fi][FW] Support pattern filter for unwanted ARP frames
 * Support ARP filter during suspended
 *
 * 02 21 2011 cp.wu
 * [WCXRP00000482] [MT6620 Wi-Fi][Driver] Simplify logic for checking NVRAM existence in driver domain
 * simplify logic for checking NVRAM existence only once.
 *
 * 02 17 2011 terry.wu
 * [WCXRP00000459] [MT6620 Wi-Fi][Driver] Fix deference null pointer problem in wlanRemove
 * Fix deference a null pointer problem in wlanRemove.
 *
 * 02 16 2011 jeffrey.chang
 * NULL
 * fix compilig error
 *
 * 02 16 2011 jeffrey.chang
 * NULL
 * Add query ipv4 and ipv6 address during early suspend and late resume
 *
 * 02 15 2011 jeffrey.chang
 * NULL
 * to support early suspend in android
 *
 * 02 11 2011 yuche.tsai
 * [WCXRP00000431] [Volunteer Patch][MT6620][Driver] Add MLME support for deauthentication under AP(Hot-Spot) mode.
 * Add one more export symbol.
 *
 * 02 10 2011 yuche.tsai
 * [WCXRP00000431] [Volunteer Patch][MT6620][Driver] Add MLME support for deauthentication under AP(Hot-Spot) mode.
 * Add RX deauthentication & disassociation process under Hot-Spot mode.
 *
 * 02 09 2011 terry.wu
 * [WCXRP00000383] [MT6620 Wi-Fi][Driver] Separate WiFi and P2P driver into two modules
 * Halt p2p module init and exit until TxThread finished p2p register and unregister.
 *
 * 02 08 2011 george.huang
 * [WCXRP00000422] [MT6620 Wi-Fi][Driver] support query power mode OID handler
 * Support querying power mode OID.
 *
 * 02 08 2011 yuche.tsai
 * [WCXRP00000421] [Volunteer Patch][MT6620][Driver] Fix incorrect SSID length Issue
 * Export Deactivation Network.
 *
 * 02 01 2011 jeffrey.chang
 * [WCXRP00000414] KAL Timer is not unregistered when driver not loaded
 * Unregister the KAL timer during driver unloading
 *
 * 01 26 2011 cm.chang
 * [WCXRP00000395] [MT6620 Wi-Fi][Driver][FW] Search STA_REC with additional net type index argument
 * Allocate system RAM if fixed message or mgmt buffer is not available
 *
 * 01 19 2011 cp.wu
 * [WCXRP00000371] [MT6620 Wi-Fi][Driver] make linux glue layer portable for Android 2.3.1 with Linux 2.6.35.7
 * add compile option to check linux version 2.6.35 for different usage of system API to improve portability
 *
 * 01 12 2011 cp.wu
 * [WCXRP00000357] [MT6620 Wi-Fi][Driver][Bluetooth over Wi-Fi] add another net device interface for BT AMP
 * implementation of separate BT_OVER_WIFI data path.
 *
 * 01 10 2011 cp.wu
 * [WCXRP00000349] [MT6620 Wi-Fi][Driver] make kalIoctl() of linux port as a thread safe API to avoid potential issues due to multiple access
 * use mutex to protect kalIoctl() for thread safe.
 *
 * 01 04 2011 cp.wu
 * [WCXRP00000338] [MT6620 Wi-Fi][Driver] Separate kalMemAlloc into kmalloc and vmalloc implementations to ease physically continous memory demands
 * separate kalMemAlloc() into virtually-continous and physically-continous type to ease slab system pressure
 *
 * 12 15 2010 cp.wu
 * [WCXRP00000265] [MT6620 Wi-Fi][Driver] Remove set_mac_address routine from legacy Wi-Fi Android driver
 * remove set MAC address. MAC address is always loaded from NVRAM instead.
 *
 * 12 10 2010 kevin.huang
 * [WCXRP00000128] [MT6620 Wi-Fi][Driver] Add proc support to Android Driver for debug and driver status check
 * Add Linux Proc Support
 *
 * 11 01 2010 yarco.yang
 * [WCXRP00000149] [MT6620 WI-Fi][Driver]Fine tune performance on MT6516 platform
 * Add GPIO debug function
 *
 * 11 01 2010 cp.wu
 * [WCXRP00000056] [MT6620 Wi-Fi][Driver] NVRAM implementation with Version Check[WCXRP00000150] [MT6620 Wi-Fi][Driver] Add implementation for querying current TX rate from firmware auto rate module
 * 1) Query link speed (TX rate) from firmware directly with buffering mechanism to reduce overhead
 * 2) Remove CNM CH-RECOVER event handling
 * 3) cfg read/write API renamed with kal prefix for unified naming rules.
 *
 * 10 26 2010 cp.wu
 * [WCXRP00000056] [MT6620 Wi-Fi][Driver] NVRAM implementation with Version Check[WCXRP00000137] [MT6620 Wi-Fi] [FW] Support NIC capability query command
 * 1) update NVRAM content template to ver 1.02
 * 2) add compile option for querying NIC capability (default: off)
 * 3) modify AIS 5GHz support to run-time option, which could be turned on by registry or NVRAM setting
 * 4) correct auto-rate compiler error under linux (treat warning as error)
 * 5) simplify usage of NVRAM and REG_INFO_T
 * 6) add version checking between driver and firmware
 *
 * 10 21 2010 chinghwa.yu
 * [WCXRP00000065] Update BoW design and settings
 * .
 *
 * 10 19 2010 jeffrey.chang
 * [WCXRP00000120] [MT6620 Wi-Fi][Driver] Refine linux kernel module to the license of MTK propietary and enable MTK HIF by default
 * Refine linux kernel module to the license of MTK and enable MTK HIF
 *
 * 10 18 2010 jeffrey.chang
 * [WCXRP00000106] [MT6620 Wi-Fi][Driver] Enable setting multicast  callback in Android
 * .
 *
 * 10 18 2010 cp.wu
 * [WCXRP00000056] [MT6620 Wi-Fi][Driver] NVRAM implementation with Version Check[WCXRP00000086] [MT6620 Wi-Fi][Driver] The mac address is all zero at android
 * complete implementation of Android NVRAM access
 *
 * 09 27 2010 chinghwa.yu
 * [WCXRP00000063] Update BCM CoEx design and settings[WCXRP00000065] Update BoW design and settings
 * Update BCM/BoW design and settings.
 *
 * 09 23 2010 cp.wu
 * [WCXRP00000051] [MT6620 Wi-Fi][Driver] WHQL test fail in MAC address changed item
 * use firmware reported mac address right after wlanAdapterStart() as permanent address
 *
 * 09 21 2010 kevin.huang
 * [WCXRP00000052] [MT6620 Wi-Fi][Driver] Eliminate Linux Compile Warning
 * Eliminate Linux Compile Warning
 *
 * 09 03 2010 kevin.huang
 * NULL
 * Refine #include sequence and solve recursive/nested #include issue
 *
 * 09 01 2010 wh.su
 * NULL
 * adding the wapi support for integration test.
 *
 * 08 18 2010 yarco.yang
 * NULL
 * 1. Fixed HW checksum offload function not work under Linux issue.
 * 2. Add debug message.
 *
 * 08 16 2010 yarco.yang
 * NULL
 * Support Linux x86
 *
 * 08 02 2010 jeffrey.chang
 * NULL
 * 1) modify tx service thread to avoid busy looping
 * 2) add spin lock declartion for linux build
 *
 * 07 29 2010 jeffrey.chang
 * NULL
 * fix memory leak for module unloading
 *
 * 07 28 2010 jeffrey.chang
 * NULL
 * 1) remove unused spinlocks
 * 2) enable encyption ioctls
 * 3) fix scan ioctl which may cause supplicant to hang
 *
 * 07 23 2010 jeffrey.chang
 *
 * bug fix: allocate regInfo when disabling firmware download
 *
 * 07 23 2010 jeffrey.chang
 *
 * use glue layer api to decrease or increase counter atomically
 *
 * 07 22 2010 jeffrey.chang
 *
 * add new spinlock
 *
 * 07 19 2010 jeffrey.chang
 *
 * modify cmd/data path for new design
 *
 * 07 08 2010 cp.wu
 *
 * [WPD00003833] [MT6620 and MT5931] Driver migration - move to new repository.
 *
 * 06 06 2010 kevin.huang
 * [WPD00003832][MT6620 5931] Create driver base
 * [MT6620 5931] Create driver base
 *
 * 05 26 2010 jeffrey.chang
 * [WPD00003826]Initial import for Linux port
 * 1) Modify set mac address code
 * 2) remove power managment macro
 *
 * 05 10 2010 cp.wu
 * [WPD00003831][MT6620 Wi-Fi] Add framework for Wi-Fi Direct support
 * implement basic wi-fi direct framework
 *
 * 05 07 2010 jeffrey.chang
 * [WPD00003826]Initial import for Linux port
 * prevent supplicant accessing driver during resume
 *
 * 05 07 2010 cp.wu
 * [WPD00003831][MT6620 Wi-Fi] Add framework for Wi-Fi Direct support
 * add basic framework for implementating P2P driver hook.
 *
 * 04 27 2010 jeffrey.chang
 * [WPD00003826]Initial import for Linux port
 * 1) fix firmware download bug
 * 2) remove query statistics for acelerating firmware download
 *
 * 04 27 2010 jeffrey.chang
 * [WPD00003826]Initial import for Linux port
 * follow Linux's firmware framework, and remove unused kal API
 *
 * 04 21 2010 jeffrey.chang
 * [WPD00003826]Initial import for Linux port
 * add for private ioctl support
 *
 * 04 19 2010 jeffrey.chang
 * [WPD00003826]Initial import for Linux port
 * Query statistics from firmware
 *
 * 04 19 2010 jeffrey.chang
 * [WPD00003826]Initial import for Linux port
 * modify tcp/ip checksum offload flags
 *
 * 04 16 2010 jeffrey.chang
 * [WPD00003826]Initial import for Linux port
 * fix tcp/ip checksum offload bug
 *
 * 04 13 2010 cp.wu
 * [WPD00003823][MT6620 Wi-Fi] Add Bluetooth-over-Wi-Fi support
 * add framework for BT-over-Wi-Fi support.
 *  *  *  *  *  *  *  *  *  *  *  *  *  *  *  *  * 1) prPendingCmdInfo is replaced by queue for multiple handler capability
 *  *  *  *  *  *  *  *  *  *  *  *  *  *  *  *  * 2) command sequence number is now increased atomically
 *  *  *  *  *  *  *  *  *  *  *  *  *  *  *  *  * 3) private data could be hold and taken use for other purpose
 *
 * 04 09 2010 jeffrey.chang
 * [WPD00003826]Initial import for Linux port
 * fix spinlock usage
 *
 * 04 07 2010 jeffrey.chang
 * [WPD00003826]Initial import for Linux port
 * Set MAC address from firmware
 *
 * 04 07 2010 cp.wu
 * [WPD00001943]Create WiFi test driver framework on WinXP
 * rWlanInfo should be placed at adapter rather than glue due to most operations
 *  *  *  *  *  * are done in adapter layer.
 *
 * 04 07 2010 jeffrey.chang
 * [WPD00003826]Initial import for Linux port
 * (1)improve none-glue code portability
 *  * (2) disable set Multicast address during atomic context
 *
 * 04 06 2010 jeffrey.chang
 * [WPD00003826]Initial import for Linux port
 * adding debug module
 *
 * 03 31 2010 wh.su
 * [WPD00003816][MT6620 Wi-Fi] Adding the security support
 * modify the wapi related code for new driver's design.
 *
 * 03 30 2010 jeffrey.chang
 * [WPD00003826]Initial import for Linux port
 * emulate NDIS Pending OID facility
 *
 * 03 26 2010 jeffrey.chang
 * [WPD00003826]Initial import for Linux port
 * fix f/w download start and load address by using config.h
 *
 * 03 26 2010 jeffrey.chang
 * [WPD00003826]Initial import for Linux port
 * [WPD00003826] Initial import for Linux port
 * adding firmware download support
 *
 * 03 24 2010 jeffrey.chang
 * [WPD00003826]Initial import for Linux port
 * initial import for Linux port
**  \main\maintrunk.MT5921\52 2009-10-27 22:49:59 GMT mtk01090
**  Fix compile error for Linux EHPI driver
**  \main\maintrunk.MT5921\51 2009-10-20 17:38:22 GMT mtk01090
**  Refine driver unloading and clean up procedure. Block requests, stop main thread and clean up queued requests, and then stop hw.
**  \main\maintrunk.MT5921\50 2009-10-08 10:33:11 GMT mtk01090
**  Avoid accessing private data of net_device directly. Replace with netdev_priv(). Add more checking for input parameters and pointers.
**  \main\maintrunk.MT5921\49 2009-09-28 20:19:05 GMT mtk01090
**  Add private ioctl to carry OID structures. Restructure public/private ioctl interfaces to Linux kernel.
**  \main\maintrunk.MT5921\48 2009-09-03 13:58:46 GMT mtk01088
**  remove non-used code
**  \main\maintrunk.MT5921\47 2009-09-03 11:40:25 GMT mtk01088
**  adding the module parameter for wapi
**  \main\maintrunk.MT5921\46 2009-08-18 22:56:41 GMT mtk01090
**  Add Linux SDIO (with mmc core) support.
**  Add Linux 2.6.21, 2.6.25, 2.6.26.
**  Fix compile warning in Linux.
**  \main\maintrunk.MT5921\45 2009-07-06 20:53:00 GMT mtk01088
**  adding the code to check the wapi 1x frame
**  \main\maintrunk.MT5921\44 2009-06-23 23:18:55 GMT mtk01090
**  Add build option BUILD_USE_EEPROM and compile option CFG_SUPPORT_EXT_CONFIG for NVRAM support
**  \main\maintrunk.MT5921\43 2009-02-16 23:46:51 GMT mtk01461
**  Revise the order of increasing u4TxPendingFrameNum because of  CFG_TX_RET_TX_CTRL_EARLY
**  \main\maintrunk.MT5921\42 2009-01-22 13:11:59 GMT mtk01088
**  set the tid and 1x value at same packet reserved field
**  \main\maintrunk.MT5921\41 2008-10-20 22:43:53 GMT mtk01104
**  Fix wrong variable name "prDev" in wlanStop()
**  \main\maintrunk.MT5921\40 2008-10-16 15:37:10 GMT mtk01461
**  add handle WLAN_STATUS_SUCCESS in wlanHardStartXmit() for CFG_TX_RET_TX_CTRL_EARLY
**  \main\maintrunk.MT5921\39 2008-09-25 15:56:21 GMT mtk01461
**  Update driver for Code review
**  \main\maintrunk.MT5921\38 2008-09-05 17:25:07 GMT mtk01461
**  Update Driver for Code Review
**  \main\maintrunk.MT5921\37 2008-09-02 10:57:06 GMT mtk01461
**  Update driver for code review
**  \main\maintrunk.MT5921\36 2008-08-05 01:53:28 GMT mtk01461
**  Add support for linux statistics
**  \main\maintrunk.MT5921\35 2008-08-04 16:52:58 GMT mtk01461
**  Fix ASSERT if removing module in BG_SSID_SCAN state
**  \main\maintrunk.MT5921\34 2008-06-13 22:52:24 GMT mtk01461
**  Revise status code handling in wlanHardStartXmit() for WLAN_STATUS_SUCCESS
**  \main\maintrunk.MT5921\33 2008-05-30 18:56:53 GMT mtk01461
**  Not use wlanoidSetCurrentAddrForLinux()
**  \main\maintrunk.MT5921\32 2008-05-30 14:39:40 GMT mtk01461
**  Remove WMM Assoc Flag
**  \main\maintrunk.MT5921\31 2008-05-23 10:26:40 GMT mtk01084
**  modify wlanISR interface
**  \main\maintrunk.MT5921\30 2008-05-03 18:52:36 GMT mtk01461
**  Fix Unset Broadcast filter when setMulticast
**  \main\maintrunk.MT5921\29 2008-05-03 15:17:26 GMT mtk01461
**  Move Query Media Status to GLUE
**  \main\maintrunk.MT5921\28 2008-04-24 22:48:21 GMT mtk01461
**  Revise set multicast function by using windows oid style for LP own back
**  \main\maintrunk.MT5921\27 2008-04-24 12:00:08 GMT mtk01461
**  Fix multicast setting in Linux and add comment
**  \main\maintrunk.MT5921\26 2008-03-28 10:40:22 GMT mtk01461
**  Fix set mac address func in Linux
**  \main\maintrunk.MT5921\25 2008-03-26 15:37:26 GMT mtk01461
**  Add set MAC Address
**  \main\maintrunk.MT5921\24 2008-03-26 14:24:53 GMT mtk01461
**  For Linux, set net_device has feature with checksum offload by default
**  \main\maintrunk.MT5921\23 2008-03-11 14:50:52 GMT mtk01461
**  Fix typo
**  \main\maintrunk.MT5921\22 2008-02-29 15:35:20 GMT mtk01088
**  add 1x decide code for sw port control
**  \main\maintrunk.MT5921\21 2008-02-21 15:01:54 GMT mtk01461
**  Rearrange the set off place of GLUE spin lock in HardStartXmit
**  \main\maintrunk.MT5921\20 2008-02-12 23:26:50 GMT mtk01461
**  Add debug option - Packet Order for Linux and add debug level - Event
**  \main\maintrunk.MT5921\19 2007-12-11 00:11:12 GMT mtk01461
**  Fix SPIN_LOCK protection
**  \main\maintrunk.MT5921\18 2007-11-30 17:02:25 GMT mtk01425
**  1. Set Rx multicast packets mode before setting the address list
**  \main\maintrunk.MT5921\17 2007-11-26 19:44:24 GMT mtk01461
**  Add OS_TIMESTAMP to packet
**  \main\maintrunk.MT5921\16 2007-11-21 15:47:20 GMT mtk01088
**  fixed the unload module issue
**  \main\maintrunk.MT5921\15 2007-11-07 18:37:38 GMT mtk01461
**  Fix compile warnning
**  \main\maintrunk.MT5921\14 2007-11-02 01:03:19 GMT mtk01461
**  Unify TX Path for Normal and IBSS Power Save + IBSS neighbor learning
**  \main\maintrunk.MT5921\13 2007-10-30 10:42:33 GMT mtk01425
**  1. Refine for multicast list
**  \main\maintrunk.MT5921\12 2007-10-25 18:08:13 GMT mtk01461
**  Add VOIP SCAN Support  & Refine Roaming
** Revision 1.4  2007/07/05 07:25:33  MTK01461
** Add Linux initial code, modify doc, add 11BB, RF init code
**
** Revision 1.3  2007/06/27 02:18:50  MTK01461
** Update SCAN_FSM, Initial(Can Load Module), Proc(Can do Reg R/W), TX API
**
** Revision 1.2  2007/06/25 06:16:24  MTK01461
** Update illustrations, gl_init.c, gl_kal.c, gl_kal.h, gl_os.h and RX API
**
*/

/*******************************************************************************
*                         C O M P I L E R   F L A G S
********************************************************************************
*/

/*******************************************************************************
*                    E X T E R N A L   R E F E R E N C E S
********************************************************************************
*/
#include "gl_os.h"
#include "os_debug.h"
#include "wlan_lib.h"
#include "gl_wext.h"
#include "gl_cfg80211.h"
#include "precomp.h"
#if defined(CONFIG_MTK_TC1_FEATURE)
#include <tc1_partition.h>
#endif
/*******************************************************************************
*                              C O N S T A N T S
********************************************************************************
*/
//#define MAX_IOREQ_NUM   10

BOOLEAN fgIsUnderEarlierSuspend = false;

struct semaphore g_halt_sem;
int g_u4HaltFlag = 0;

#if CFG_ENABLE_WIFI_DIRECT
spinlock_t g_p2p_lock;
int g_u4P2PEnding = 0;
int g_u4P2POnOffing = 0;
#endif


/*******************************************************************************
*                             D A T A   T Y P E S
********************************************************************************
*/
/* Tasklet mechanism is like buttom-half in Linux. We just want to
 * send a signal to OS for interrupt defer processing. All resources
 * are NOT allowed reentry, so txPacket, ISR-DPC and ioctl must avoid preempty.
 */
typedef struct _WLANDEV_INFO_T {
    struct net_device *prDev;
} WLANDEV_INFO_T, *P_WLANDEV_INFO_T;

/*******************************************************************************
*                            P U B L I C   D A T A
********************************************************************************
*/

MODULE_AUTHOR(NIC_AUTHOR);
MODULE_DESCRIPTION(NIC_DESC);
MODULE_LICENSE("GPL");

#define NIC_INF_NAME    "wlan%d" /* interface name */
#define NIC_INF_NAME_FOR_AP_MODE "legacy%d"


/* support to change debug module info dynamically */
UINT_8  aucDebugModule[DBG_MODULE_NUM];
UINT_32 u4DebugModule = 0;


//4 2007/06/26, mikewu, now we don't use this, we just fix the number of wlan device to 1
static WLANDEV_INFO_T arWlanDevInfo[CFG_MAX_WLAN_DEVICES] = {{0}};
static UINT_32 u4WlanDevNum = 0; /* How many NICs coexist now */

/*******************************************************************************
*                           P R I V A T E   D A T A
********************************************************************************
*/
#if CFG_ENABLE_WIFI_DIRECT
static SUB_MODULE_HANDLER rSubModHandler[SUB_MODULE_NUM] = {{NULL}};
#endif

#define CHAN2G(_channel, _freq, _flags)         \
    {                                           \
    .band               = NL80211_BAND_2GHZ,  \
    .center_freq        = (_freq),              \
    .hw_value           = (_channel),           \
    .flags              = (_flags),             \
    .max_antenna_gain   = 0,                    \
    .max_power          = 30,                   \
    }
static struct ieee80211_channel mtk_2ghz_channels[] = {
    CHAN2G(1, 2412, 0),
    CHAN2G(2, 2417, 0),
    CHAN2G(3, 2422, 0),
    CHAN2G(4, 2427, 0),
    CHAN2G(5, 2432, 0),
    CHAN2G(6, 2437, 0),
    CHAN2G(7, 2442, 0),
    CHAN2G(8, 2447, 0),
    CHAN2G(9, 2452, 0),
    CHAN2G(10, 2457, 0),
    CHAN2G(11, 2462, 0),
    CHAN2G(12, 2467, 0),
    CHAN2G(13, 2472, 0),
    CHAN2G(14, 2484, 0),
};

#define CHAN5G(_channel, _flags)                    \
    {                                               \
    .band               = NL80211_BAND_5GHZ,      \
    .center_freq        = 5000 + (5 * (_channel)),  \
    .hw_value           = (_channel),               \
    .flags              = (_flags),                 \
    .max_antenna_gain   = 0,                        \
    .max_power          = 30,                       \
    }
static struct ieee80211_channel mtk_5ghz_channels[] = {
    CHAN5G(34, 0),      CHAN5G(36, 0),
    CHAN5G(38, 0),      CHAN5G(40, 0),
    CHAN5G(42, 0),      CHAN5G(44, 0),
    CHAN5G(46, 0),      CHAN5G(48, 0),
    CHAN5G(52, 0),      CHAN5G(56, 0),
    CHAN5G(60, 0),      CHAN5G(64, 0),
    CHAN5G(100, 0),     CHAN5G(104, 0),
    CHAN5G(108, 0),     CHAN5G(112, 0),
    CHAN5G(116, 0),     CHAN5G(120, 0),
    CHAN5G(124, 0),     CHAN5G(128, 0),
    CHAN5G(132, 0),     CHAN5G(136, 0),
    CHAN5G(140, 0),     CHAN5G(149, 0),
    CHAN5G(153, 0),     CHAN5G(157, 0),
    CHAN5G(161, 0),     CHAN5G(165, 0),
    CHAN5G(169, 0),     CHAN5G(173, 0),
    CHAN5G(184, 0),     CHAN5G(188, 0),
    CHAN5G(192, 0),     CHAN5G(196, 0),
    CHAN5G(200, 0),     CHAN5G(204, 0),
    CHAN5G(208, 0),     CHAN5G(212, 0),
    CHAN5G(216, 0),
};

/* for cfg80211 - rate table */
#define RATETAB_ENT(_rate, _rateid, _flags) { \
    .bitrate = (_rate), .hw_value = (_rateid), .flags = (_flags) }
static struct ieee80211_rate mtk_rates[] = {
    RATETAB_ENT(10,   0x1000,   0),
    RATETAB_ENT(20,   0x1001,   0),
    RATETAB_ENT(55,   0x1002,   0),
    RATETAB_ENT(110,  0x1003,   0), /* 802.11b */
    RATETAB_ENT(60,   0x2000,   0),
    RATETAB_ENT(90,   0x2001,   0),
    RATETAB_ENT(120,  0x2002,   0),
    RATETAB_ENT(180,  0x2003,   0),
    RATETAB_ENT(240,  0x2004,   0),
    RATETAB_ENT(360,  0x2005,   0),
    RATETAB_ENT(480,  0x2006,   0),
    RATETAB_ENT(540,  0x2007,   0), /* 802.11a/g */
};

#define mtk_a_rates         (mtk_rates + 4)
#define mtk_a_rates_size    (sizeof(mtk_rates) / sizeof(mtk_rates[0]) - 4)
#define mtk_g_rates         (mtk_rates + 0)
#define mtk_g_rates_size    (sizeof(mtk_rates) / sizeof(mtk_rates[0]) - 0)

#define MT6620_MCS_INFO                                 \
{                                                       \
    .rx_mask        = {0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0},\
    .rx_highest     = 0,                                \
    .tx_params      = IEEE80211_HT_MCS_TX_DEFINED,      \
}

#define MT6620_HT_CAP                                   \
{                                                       \
    .ht_supported   = true,                             \
    .cap            = 0  \
                    | IEEE80211_HT_CAP_SM_PS            \
                    | IEEE80211_HT_CAP_GRN_FLD          \
                    | IEEE80211_HT_CAP_SGI_20           \
                    ,          \
    .ampdu_factor   = IEEE80211_HT_MAX_AMPDU_64K,       \
    .ampdu_density  = IEEE80211_HT_MPDU_DENSITY_NONE,   \
    .mcs            = MT6620_MCS_INFO,                  \
}

/* public for both Legacy Wi-Fi / P2P access */
struct ieee80211_supported_band mtk_band_2ghz = {
    .band       = NL80211_BAND_2GHZ,
    .channels   = mtk_2ghz_channels,
    .n_channels = ARRAY_SIZE(mtk_2ghz_channels),
    .bitrates   = mtk_g_rates,
    .n_bitrates = mtk_g_rates_size,
    .ht_cap     = MT6620_HT_CAP,
};

/* public for both Legacy Wi-Fi / P2P access */
struct ieee80211_supported_band mtk_band_5ghz = {
    .band       = NL80211_BAND_5GHZ,
    .channels   = mtk_5ghz_channels,
    .n_channels = ARRAY_SIZE(mtk_5ghz_channels),
    .bitrates   = mtk_a_rates,
    .n_bitrates = mtk_a_rates_size,
    .ht_cap     = MT6620_HT_CAP,
};

static const UINT_32 mtk_cipher_suites[] = {
    /* keep WEP first, it may be removed below */
    WLAN_CIPHER_SUITE_WEP40,
    WLAN_CIPHER_SUITE_WEP104,
    WLAN_CIPHER_SUITE_TKIP,
    WLAN_CIPHER_SUITE_CCMP,

    /* keep last -- depends on hw flags! */
    WLAN_CIPHER_SUITE_AES_CMAC
};

static int mtk_change_iface_compat(struct wiphy *wiphy,
        struct net_device *ndev, enum nl80211_iftype type,
        struct vif_params *params)
{
    return mtk_cfg80211_change_iface(wiphy, ndev, type, NULL, params);
}

static int mtk_add_key_compat(struct wiphy *wiphy, struct net_device *ndev,
        int link_id, u8 key_index, bool pairwise, const u8 *mac_addr,
        struct key_params *params)
{
    return mtk_cfg80211_add_key(wiphy, ndev, key_index, pairwise,
            mac_addr, params);
}

static int mtk_del_key_compat(struct wiphy *wiphy, struct net_device *ndev,
        int link_id, u8 key_index, bool pairwise, const u8 *mac_addr)
{
    return mtk_cfg80211_del_key(wiphy, ndev, key_index, pairwise, mac_addr);
}

static int mtk_set_default_key_compat(struct wiphy *wiphy,
        struct net_device *ndev, int link_id, u8 key_index,
        bool unicast, bool multicast)
{
    return mtk_cfg80211_set_default_key(wiphy, ndev, key_index,
            unicast, multicast);
}

static int mtk_get_station_compat(struct wiphy *wiphy,
        struct net_device *ndev, const u8 *mac, struct station_info *sinfo)
{
    return mtk_cfg80211_get_station(wiphy, ndev, (u8 *)mac, sinfo);
}

static struct cfg80211_ops mtk_wlan_ops = {
    .change_virtual_intf    = mtk_change_iface_compat,
    .add_key                = mtk_add_key_compat,
    .del_key                = mtk_del_key_compat,
    .set_default_key        = mtk_set_default_key_compat,
    .get_station            = mtk_get_station_compat,
    .scan                   = mtk_cfg80211_scan,
    .connect                = mtk_cfg80211_connect,
    .disconnect             = mtk_cfg80211_disconnect,
    .set_power_mgmt         = mtk_cfg80211_set_power_mgmt,
    .set_pmksa              = mtk_cfg80211_set_pmksa,
    .del_pmksa              = mtk_cfg80211_del_pmksa,
    .flush_pmksa            = mtk_cfg80211_flush_pmksa,

    #ifdef CONFIG_NL80211_TESTMODE
    .testmode_cmd               = mtk_cfg80211_testmode_cmd,
    #endif
};


/* There isn't a lot of sense in it, but you can transmit anything you like */
static const struct ieee80211_txrx_stypes
mtk_cfg80211_ais_default_mgmt_stypes[NUM_NL80211_IFTYPES] = {
    [NL80211_IFTYPE_ADHOC] = {
        .tx = 0xffff,
        .rx = BIT(IEEE80211_STYPE_ACTION >> 4)
    },
    [NL80211_IFTYPE_STATION] = {
        .tx = 0xffff,
        .rx = BIT(IEEE80211_STYPE_ACTION >> 4) |
        BIT(IEEE80211_STYPE_PROBE_REQ >> 4)
    },
    [NL80211_IFTYPE_AP] = {
        .tx = 0xffff,
        .rx = BIT(IEEE80211_STYPE_PROBE_REQ >> 4) |
        BIT(IEEE80211_STYPE_ACTION >> 4)
    },
    [NL80211_IFTYPE_AP_VLAN] = {
        /* copy AP */
        .tx = 0xffff,
        .rx = BIT(IEEE80211_STYPE_ASSOC_REQ >> 4) |
        BIT(IEEE80211_STYPE_REASSOC_REQ >> 4) |
        BIT(IEEE80211_STYPE_PROBE_REQ >> 4) |
        BIT(IEEE80211_STYPE_DISASSOC >> 4) |
        BIT(IEEE80211_STYPE_AUTH >> 4) |
        BIT(IEEE80211_STYPE_DEAUTH >> 4) |
        BIT(IEEE80211_STYPE_ACTION >> 4)
    },
    [NL80211_IFTYPE_P2P_CLIENT] = {
        .tx = 0xffff,
        .rx = BIT(IEEE80211_STYPE_ACTION >> 4) |
        BIT(IEEE80211_STYPE_PROBE_REQ >> 4)
    },
    [NL80211_IFTYPE_P2P_GO] = {
        .tx = 0xffff,
        .rx = BIT(IEEE80211_STYPE_PROBE_REQ >> 4) |
        BIT(IEEE80211_STYPE_ACTION >> 4)
    }
};


/*******************************************************************************
*                                 M A C R O S
********************************************************************************
*/

/*******************************************************************************
*                   F U N C T I O N   D E C L A R A T I O N S
********************************************************************************
*/

#if defined(CONFIG_HAS_EARLYSUSPEND)
extern int glRegisterEarlySuspend(
    struct early_suspend        *prDesc,
    early_suspend_callback      wlanSuspend,
    late_resume_callback        wlanResume);

extern int glUnregisterEarlySuspend(struct early_suspend *prDesc);
#endif

/*******************************************************************************
*                              F U N C T I O N S
********************************************************************************
*/

#if LINUX_VERSION_CODE >= KERNEL_VERSION(2, 6, 31)
/*----------------------------------------------------------------------------*/
/*!
* \brief Override the implementation of select queue
*
* \param[in] dev Pointer to struct net_device
* \param[in] skb Pointer to struct skb_buff
*
* \return (none)
*/
/*----------------------------------------------------------------------------*/
unsigned int _cfg80211_classify8021d(struct sk_buff *skb)
{
    unsigned int dscp = 0;

    /* skb->priority values from 256->263 are magic values
     * directly indicate a specific 802.1d priority.  This is
     * to allow 802.1d priority to be passed directly in from
     * tags
     */

    if (skb->priority >= 256 && skb->priority <= 263) {
        return skb->priority - 256;
    }
    switch (skb->protocol) {
        case htons(ETH_P_IP):
            dscp = ip_hdr(skb)->tos & 0xfc;
            break;
    }
    return dscp >> 5;
}


static const UINT_16 au16Wlan1dToQueueIdx[8] = { 1, 0, 0, 1, 2, 2, 3, 3 };

static UINT_16
wlanSelectQueue(
    struct net_device *dev,
    struct sk_buff *skb,
    struct net_device *sb_dev)
{
    skb->priority = _cfg80211_classify8021d(skb);

    return au16Wlan1dToQueueIdx[skb->priority];
}
#endif

/*----------------------------------------------------------------------------*/
/*!
* \brief Load NVRAM data and translate it into REG_INFO_T
*
* \param[in]  prGlueInfo Pointer to struct GLUE_INFO_T
* \param[out] prRegInfo  Pointer to struct REG_INFO_T
*
* \return (none)
*/
/*----------------------------------------------------------------------------*/
static void
glLoadNvram (
    IN  P_GLUE_INFO_T   prGlueInfo,
    OUT P_REG_INFO_T    prRegInfo
    )
{
    UINT_32 i, j;
    UINT_8 aucTmp[2];
    PUINT_8 pucDest;

    ASSERT(prGlueInfo);
    ASSERT(prRegInfo);

    if((!prGlueInfo) || (!prRegInfo)) {
        return;
    }

    if(kalCfgDataRead16(prGlueInfo,
            sizeof(WIFI_CFG_PARAM_STRUCT) - sizeof(UINT_16),
            (PUINT_16)aucTmp) == TRUE) {
        prGlueInfo->fgNvramAvailable = TRUE;

        // load MAC Address
#if !defined(CONFIG_MTK_TC1_FEATURE)
        for (i = 0 ; i < PARAM_MAC_ADDR_LEN ; i += sizeof(UINT_16)) {
            kalCfgDataRead16(prGlueInfo,
                    OFFSET_OF(WIFI_CFG_PARAM_STRUCT, aucMacAddress) + i,
                    (PUINT_16) (((PUINT_8)prRegInfo->aucMacAddr) + i));
        }
#else
	TC1_FAC_NAME(FacReadWifiMacAddr)((unsigned char *)prRegInfo->aucMacAddr);
#endif
        // load country code
        kalCfgDataRead16(prGlueInfo,
                OFFSET_OF(WIFI_CFG_PARAM_STRUCT, aucCountryCode[0]),
                    (PUINT_16)aucTmp);

        // cast to wide characters
        prRegInfo->au2CountryCode[0] = (UINT_16) aucTmp[0];
        prRegInfo->au2CountryCode[1] = (UINT_16) aucTmp[1];

        // load default normal TX power
        for (i = 0 ; i < sizeof(TX_PWR_PARAM_T) ; i += sizeof(UINT_16)) {
            kalCfgDataRead16(prGlueInfo,
                    OFFSET_OF(WIFI_CFG_PARAM_STRUCT, rTxPwr) + i,
                    (PUINT_16) (((PUINT_8)&(prRegInfo->rTxPwr)) + i));
        }

        // load feature flags
        kalCfgDataRead16(prGlueInfo,
                OFFSET_OF(WIFI_CFG_PARAM_STRUCT, ucTxPwrValid),
                (PUINT_16)aucTmp);
        prRegInfo->ucTxPwrValid     = aucTmp[0];
        prRegInfo->ucSupport5GBand  = aucTmp[1];

        kalCfgDataRead16(prGlueInfo,
                OFFSET_OF(WIFI_CFG_PARAM_STRUCT, uc2G4BwFixed20M),
                (PUINT_16)aucTmp);
        prRegInfo->uc2G4BwFixed20M  = aucTmp[0];
        prRegInfo->uc5GBwFixed20M   = aucTmp[1];

        kalCfgDataRead16(prGlueInfo,
                OFFSET_OF(WIFI_CFG_PARAM_STRUCT, ucEnable5GBand),
                (PUINT_16)aucTmp);
        prRegInfo->ucEnable5GBand   = aucTmp[0];

        /* load EFUSE overriding part */
        for (i = 0 ; i < sizeof(prRegInfo->aucEFUSE) ; i += sizeof(UINT_16)) {
            kalCfgDataRead16(prGlueInfo,
                    OFFSET_OF(WIFI_CFG_PARAM_STRUCT, aucEFUSE) + i,
                    (PUINT_16) (((PUINT_8)&(prRegInfo->aucEFUSE)) + i));
        }

        /* load band edge tx power control */
        kalCfgDataRead16(prGlueInfo,
                OFFSET_OF(WIFI_CFG_PARAM_STRUCT, fg2G4BandEdgePwrUsed),
                (PUINT_16)aucTmp);
        prRegInfo->fg2G4BandEdgePwrUsed = (BOOLEAN)aucTmp[0];
        if (aucTmp[0]) {
            prRegInfo->cBandEdgeMaxPwrCCK   = (INT_8)aucTmp[1];

            kalCfgDataRead16(prGlueInfo,
                    OFFSET_OF(WIFI_CFG_PARAM_STRUCT, cBandEdgeMaxPwrOFDM20),
                    (PUINT_16)aucTmp);
            prRegInfo->cBandEdgeMaxPwrOFDM20    = (INT_8)aucTmp[0];
            prRegInfo->cBandEdgeMaxPwrOFDM40    = (INT_8)aucTmp[1];
        }

        /* load regulation subbands */
        kalCfgDataRead16(prGlueInfo,
                OFFSET_OF(WIFI_CFG_PARAM_STRUCT, ucRegChannelListMap),
                (PUINT_16)aucTmp);
        prRegInfo->eRegChannelListMap = (ENUM_REG_CH_MAP_T) aucTmp[0];
        prRegInfo->ucRegChannelListIndex = aucTmp[1];

        if (prRegInfo->eRegChannelListMap == REG_CH_MAP_CUSTOMIZED) {
            for (i = 0 ; i < MAX_SUBBAND_NUM; i++) {
                pucDest = (PUINT_8) &prRegInfo->rDomainInfo.rSubBand[i];
                for (j = 0; j < 6; j += sizeof(UINT_16)) {
                    kalCfgDataRead16(prGlueInfo,
                            OFFSET_OF(WIFI_CFG_PARAM_STRUCT, aucRegSubbandInfo)
                            + (i * 6 + j),
                            (PUINT_16)aucTmp);

                    *pucDest++ = aucTmp[0];
                    *pucDest++ = aucTmp[1];
                }
            }
        }
    }
    else {
        prGlueInfo->fgNvramAvailable = FALSE;
    }

    return;
}


#if CFG_ENABLE_WIFI_DIRECT
/*----------------------------------------------------------------------------*/
/*!
* \brief called by txthread, run sub module init function
*
* \param[in]  prGlueInfo Pointer to struct GLUE_INFO_T
*
* \return (none)
*/
/*----------------------------------------------------------------------------*/
VOID
wlanSubModRunInit(
    P_GLUE_INFO_T prGlueInfo
    )
{
    /*now, we only have p2p module*/
    if(rSubModHandler[P2P_MODULE].fgIsInited == FALSE) {
        rSubModHandler[P2P_MODULE].subModInit(prGlueInfo);
        rSubModHandler[P2P_MODULE].fgIsInited = TRUE;
    }

}

/*----------------------------------------------------------------------------*/
/*!
* \brief called by txthread, run sub module exit function
*
* \param[in]  prGlueInfo Pointer to struct GLUE_INFO_T
*
* \return (none)
*/
/*----------------------------------------------------------------------------*/
VOID
wlanSubModRunExit(
    P_GLUE_INFO_T prGlueInfo
    )
{
    /*now, we only have p2p module*/
    if(rSubModHandler[P2P_MODULE].fgIsInited == TRUE) {
        rSubModHandler[P2P_MODULE].subModExit(prGlueInfo);
        rSubModHandler[P2P_MODULE].fgIsInited = FALSE;
    }
}

/*----------------------------------------------------------------------------*/
/*!
* \brief set sub module init flag, force TxThread to run sub modle init
*
* \param[in]  prGlueInfo Pointer to struct GLUE_INFO_T
*
* \return (none)
*/
/*----------------------------------------------------------------------------*/
BOOLEAN
wlanSubModInit(
    P_GLUE_INFO_T prGlueInfo
    )
{
    //4  Mark HALT, notify main thread to finish current job
    prGlueInfo->u4Flag |= GLUE_FLAG_SUB_MOD_INIT;
    /* wake up main thread */
    wake_up_interruptible(&prGlueInfo->waitq);
    /* wait main thread  finish sub module INIT*/
    wait_for_completion_interruptible(&prGlueInfo->rSubModComp);

#if 0
    if(prGlueInfo->prAdapter->fgIsP2PRegistered) {
        p2pNetRegister(prGlueInfo);
    }
#endif

    return TRUE;
}

/*----------------------------------------------------------------------------*/
/*!
* \brief set sub module exit flag, force TxThread to run sub modle exit
*
* \param[in]  prGlueInfo Pointer to struct GLUE_INFO_T
*
* \return (none)
*/
/*----------------------------------------------------------------------------*/
BOOLEAN
wlanSubModExit(
    P_GLUE_INFO_T prGlueInfo
    )
{
#if 0
    if(prGlueInfo->prAdapter->fgIsP2PRegistered) {
        p2pNetUnregister(prGlueInfo);
    }
#endif

    //4  Mark HALT, notify main thread to finish current job
    prGlueInfo->u4Flag |= GLUE_FLAG_SUB_MOD_EXIT;
    /* wake up main thread */
    wake_up_interruptible(&prGlueInfo->waitq);
    /* wait main thread finish sub module EXIT */
    wait_for_completion_interruptible(&prGlueInfo->rSubModComp);

    return TRUE;
}


/*----------------------------------------------------------------------------*/
/*!
* \brief set by sub module, indicate sub module is already inserted
*
* \param[in]  rSubModInit, function pointer point to sub module init function
* \param[in]  rSubModExit,  function pointer point to sub module exit function
* \param[in]  eSubModIdx,  sub module index
*
* \return (none)
*/
/*----------------------------------------------------------------------------*/
VOID
wlanSubModRegisterInitExit(
    SUB_MODULE_INIT rSubModInit,
    SUB_MODULE_EXIT rSubModExit,
    ENUM_SUB_MODULE_IDX_T eSubModIdx
    )
{
    rSubModHandler[eSubModIdx].subModInit = rSubModInit;
    rSubModHandler[eSubModIdx].subModExit = rSubModExit;
    rSubModHandler[eSubModIdx].fgIsInited = FALSE;
}

#if 0
/*----------------------------------------------------------------------------*/
/*!
* \brief check wlan is launched or not
*
* \param[in]  (none)
*
* \return TRUE, wlan is already started
*             FALSE, wlan is not started yet
*/
/*----------------------------------------------------------------------------*/
BOOLEAN
wlanIsLaunched(
    VOID
    )
{
    struct net_device *prDev = NULL;
    P_GLUE_INFO_T prGlueInfo = NULL;

    //4 <0> Sanity check
    ASSERT(u4WlanDevNum <= CFG_MAX_WLAN_DEVICES);
    if (0 == u4WlanDevNum) {
        return FALSE;
    }

    prDev = arWlanDevInfo[u4WlanDevNum-1].prDev;

    ASSERT(prDev);
    if (NULL == prDev) {
        return FALSE;
    }

    prGlueInfo = *((P_GLUE_INFO_T *) netdev_priv(prDev));
    ASSERT(prGlueInfo);
    if (NULL == prGlueInfo) {
        return FALSE;
    }

    return prGlueInfo->prAdapter->fgIsWlanLaunched;
}

#endif

/*----------------------------------------------------------------------------*/
/*!
* \brief Export wlan GLUE_INFO_T pointer to p2p module
*
* \param[in]  prGlueInfo Pointer to struct GLUE_INFO_T
*
* \return TRUE: get GlueInfo pointer successfully
*            FALSE: wlan is not started yet
*/
/*---------------------------------------------------------------------------*/
BOOLEAN
wlanExportGlueInfo(
    P_GLUE_INFO_T *prGlueInfoExpAddr
    )
{
    struct net_device *prDev = NULL;
    P_GLUE_INFO_T prGlueInfo = NULL;

    if (0 == u4WlanDevNum) {
        return FALSE;
    }

    prDev = arWlanDevInfo[u4WlanDevNum-1].prDev;
    if (NULL == prDev) {
        return FALSE;
    }

    prGlueInfo = *((P_GLUE_INFO_T *) netdev_priv(prDev));
    if (NULL == prGlueInfo) {
        return FALSE;
    }

    if(FALSE == prGlueInfo->prAdapter->fgIsWlanLaunched) {
        return FALSE;
    }

    *prGlueInfoExpAddr = prGlueInfo;
    return TRUE;
}

#endif


/*----------------------------------------------------------------------------*/
/*!
* \brief Release prDev from wlandev_array and free tasklet object related to it.
*
* \param[in] prDev  Pointer to struct net_device
*
* \return (none)
*/
/*----------------------------------------------------------------------------*/
static void
wlanClearDevIdx (
    struct net_device *prDev
    )
{
    int i;

    ASSERT(prDev);

    for (i = 0; i < CFG_MAX_WLAN_DEVICES; i++) {
        if (arWlanDevInfo[i].prDev == prDev) {
            arWlanDevInfo[i].prDev = NULL;
            u4WlanDevNum--;
        }
    }

    return;
} /* end of wlanClearDevIdx() */


/*----------------------------------------------------------------------------*/
/*!
* \brief Allocate an unique interface index, net_device::ifindex member for this
*        wlan device. Store the net_device in wlandev_array, and initialize
*        tasklet object related to it.
*
* \param[in] prDev  Pointer to struct net_device
*
* \retval >= 0      The device number.
* \retval -1        Fail to get index.
*/
/*----------------------------------------------------------------------------*/
static int
wlanGetDevIdx (
    struct net_device *prDev
    )
{
    int i;

    ASSERT(prDev);

    for (i = 0; i < CFG_MAX_WLAN_DEVICES; i++) {
        if (arWlanDevInfo[i].prDev == (struct net_device *) NULL) {
            /* Reserve 2 bytes space to store one digit of
             * device number and NULL terminator.
             */
            arWlanDevInfo[i].prDev = prDev;
            u4WlanDevNum++;
            return i;
        }
    }

    return -1;
} /* end of wlanGetDevIdx() */

/*----------------------------------------------------------------------------*/
/*!
* \brief A method of struct net_device, a primary SOCKET interface to configure
*        the interface lively. Handle an ioctl call on one of our devices.
*        Everything Linux ioctl specific is done here. Then we pass the contents
*        of the ifr->data to the request message handler.
*
* \param[in] prDev      Linux kernel netdevice
*
* \param[in] prIFReq    Our private ioctl request structure, typed for the generic
*                       struct ifreq so we can use ptr to function
*
* \param[in] cmd        Command ID
*
* \retval WLAN_STATUS_SUCCESS The IOCTL command is executed successfully.
* \retval OTHER The execution of IOCTL command is failed.
*/
/*----------------------------------------------------------------------------*/
int
wlanDoIOCTL(
    struct net_device *prDev,
    struct ifreq *prIFReq,
    int i4Cmd
    )
{
    return -EOPNOTSUPP;
} /* end of wlanDoIOCTL() */

/*----------------------------------------------------------------------------*/
/*!
* \brief This function is to set multicast list and set rx mode.
*
* \param[in] prDev  Pointer to struct net_device
*
* \return (none)
*/
/*----------------------------------------------------------------------------*/

static struct delayed_work workq;
static struct net_device *gPrDev;
static BOOLEAN fgIsWorkMcStart = FALSE;
static BOOLEAN fgIsWorkMcEverInit = FALSE;

static void
wlanSetMulticastList (struct net_device *prDev)
{
    gPrDev = prDev;
    schedule_delayed_work(&workq, 0);
}

/* FIXME: Since we cannot sleep in the wlanSetMulticastList, we arrange
 * another workqueue for sleeping. We don't want to block
 * tx_thread, so we can't let tx_thread to do this */

static void
wlanSetMulticastListWorkQueue (struct work_struct *work) {

    P_GLUE_INFO_T prGlueInfo = NULL;
    UINT_32 u4PacketFilter = 0;
    UINT_32 u4SetInfoLen;
    struct net_device *prDev = gPrDev;

	fgIsWorkMcStart = TRUE;

    DBGLOG(INIT, INFO, ("wlanSetMulticastListWorkQueue start...\n"));

    down(&g_halt_sem);
    if (g_u4HaltFlag) {
		fgIsWorkMcStart = FALSE;
        up(&g_halt_sem);
        return;
    }

    prGlueInfo = (NULL != prDev) ? *((P_GLUE_INFO_T *) netdev_priv(prDev)) : NULL;
    ASSERT(prDev);
    ASSERT(prGlueInfo);
    if (!prDev || !prGlueInfo) {
        DBGLOG(INIT, WARN, ("abnormal dev or skb: prDev(0x%p), prGlueInfo(0x%p)\n",
            prDev, prGlueInfo));
		fgIsWorkMcStart = FALSE;
        up(&g_halt_sem);
        return;
    }

    if (prDev->flags & IFF_PROMISC) {
        u4PacketFilter |= PARAM_PACKET_FILTER_PROMISCUOUS;
    }

    if (prDev->flags & IFF_BROADCAST) {
        u4PacketFilter |= PARAM_PACKET_FILTER_BROADCAST;
    }

    if (prDev->flags & IFF_MULTICAST) {
        if ((prDev->flags & IFF_ALLMULTI) ||
#if LINUX_VERSION_CODE >= KERNEL_VERSION(2, 6, 35)
            (netdev_mc_count(prDev) > MAX_NUM_GROUP_ADDR)) {
#else
            (prDev->mc_count > MAX_NUM_GROUP_ADDR)) {
#endif

            u4PacketFilter |= PARAM_PACKET_FILTER_ALL_MULTICAST;
        }
        else {
            u4PacketFilter |= PARAM_PACKET_FILTER_MULTICAST;
        }
    }

    up(&g_halt_sem);

    if (kalIoctl(prGlueInfo,
            wlanoidSetCurrentPacketFilter,
            &u4PacketFilter,
            sizeof(u4PacketFilter),
            FALSE,
            FALSE,
            TRUE,
            FALSE,
            &u4SetInfoLen) != WLAN_STATUS_SUCCESS) {
		fgIsWorkMcStart = FALSE;
        return;
    }


    if (u4PacketFilter & PARAM_PACKET_FILTER_MULTICAST) {
        /* Prepare multicast address list */
#if LINUX_VERSION_CODE >= KERNEL_VERSION(2, 6, 35)
        struct netdev_hw_addr *ha;
#else
        struct dev_mc_list *prMcList;
#endif
        PUINT_8 prMCAddrList = NULL;
        UINT_32 i = 0;

        down(&g_halt_sem);
        if (g_u4HaltFlag) {
			fgIsWorkMcStart = FALSE;
            up(&g_halt_sem);
            return;
        }

        prMCAddrList = kalMemAlloc(MAX_NUM_GROUP_ADDR * ETH_ALEN, VIR_MEM_TYPE);

#if LINUX_VERSION_CODE >= KERNEL_VERSION(2, 6, 35)
        netdev_for_each_mc_addr(ha, prDev) {
            if(i < MAX_NUM_GROUP_ADDR) {
                memcpy((prMCAddrList + i * ETH_ALEN), ha->addr, ETH_ALEN);
                i++;
            }
        }
#else
        for (i = 0, prMcList = prDev->mc_list;
             (prMcList) && (i < prDev->mc_count) && (i < MAX_NUM_GROUP_ADDR);
             i++, prMcList = prMcList->next) {
            memcpy((prMCAddrList + i * ETH_ALEN), prMcList->dmi_addr, ETH_ALEN);
        }
#endif

        up(&g_halt_sem);

        kalIoctl(prGlueInfo,
            wlanoidSetMulticastList,
            prMCAddrList,
            (i * ETH_ALEN),
            FALSE,
            FALSE,
            TRUE,
            FALSE,
            &u4SetInfoLen);

        kalMemFree(prMCAddrList, VIR_MEM_TYPE, MAX_NUM_GROUP_ADDR * ETH_ALEN);
    }

	fgIsWorkMcStart = FALSE;
	DBGLOG(INIT, INFO, ("wlanSetMulticastListWorkQueue end\n"));
    return;
} /* end of wlanSetMulticastList() */


/* FIXME: Since we cannot sleep in the wlanSetMulticastList, we arrange
 * another workqueue for sleeping. We don't want to block
 * tx_thread, so we can't let tx_thread to do this */

void
p2pSetMulticastListWorkQueueWrapper (P_GLUE_INFO_T prGlueInfo) {

    ASSERT(prGlueInfo);

    if (!prGlueInfo) {
        DBGLOG(INIT, WARN, ("abnormal dev or skb: prGlueInfo(0x%p)\n",
            prGlueInfo));
        return;
    }

#if CFG_ENABLE_WIFI_DIRECT
    if(prGlueInfo->prAdapter->fgIsP2PRegistered) {
        mtk_p2p_wext_set_Multicastlist(prGlueInfo);
    }
#endif

    return;
} /* end of p2pSetMulticastListWorkQueueWrapper() */



/*----------------------------------------------------------------------------*/
/*!
* \brief This function is TX entry point of NET DEVICE.
*
* \param[in] prSkb  Pointer of the sk_buff to be sent
* \param[in] prDev  Pointer to struct net_device
*
* \retval NETDEV_TX_OK - on success.
* \retval NETDEV_TX_BUSY - on failure, packet will be discarded by upper layer.
*/
/*----------------------------------------------------------------------------*/
int
wlanHardStartXmit(
    struct sk_buff *prSkb,
    struct net_device *prDev
    )
{
    P_GLUE_INFO_T prGlueInfo = *((P_GLUE_INFO_T *) netdev_priv(prDev));

    P_QUE_ENTRY_T prQueueEntry = NULL;
    P_QUE_T prTxQueue = NULL;
    UINT_16 u2QueueIdx = 0;

#if CFG_BOW_TEST
    UINT_32 i;
#endif

    GLUE_SPIN_LOCK_DECLARATION();

    ASSERT(prSkb);
    ASSERT(prDev);
    ASSERT(prGlueInfo);

    /* check if WiFi is halt */
    if (prGlueInfo->u4Flag & GLUE_FLAG_HALT) {
        DBGLOG(INIT, INFO, ("GLUE_FLAG_HALT skip tx\n"));
        dev_kfree_skb(prSkb);
        return NETDEV_TX_OK;
    }

#if CFG_SUPPORT_HOTSPOT_2_0
	if (prGlueInfo->fgIsDad) {
		//kalPrint("[Passpoint R2] Due to ipv4_dad...TX is forbidden\n");
		dev_kfree_skb(prSkb);
		return NETDEV_TX_OK;
	}
	if (prGlueInfo->fgIs6Dad) {
		//kalPrint("[Passpoint R2] Due to ipv6_dad...TX is forbidden\n");
		dev_kfree_skb(prSkb);
		return NETDEV_TX_OK;
	}
#endif

    prQueueEntry = (P_QUE_ENTRY_T) GLUE_GET_PKT_QUEUE_ENTRY(prSkb);
    prTxQueue = &prGlueInfo->rTxQueue;

#if CFG_BOW_TEST
    DBGLOG(BOW, TRACE, ("sk_buff->len: %d\n", prSkb->len));
    DBGLOG(BOW, TRACE, ("sk_buff->data_len: %d\n", prSkb->data_len));
    DBGLOG(BOW, TRACE, ("sk_buff->data:\n"));

    for(i = 0; i < prSkb->len; i++)
    {
        DBGLOG(BOW, TRACE, ("%4x", prSkb->data[i]));

        if((i+1)%16 ==0)
        {
            DBGLOG(BOW, TRACE, ("\n"));
        }
    }

    DBGLOG(BOW, TRACE, ("\n"));
#endif

    if (wlanProcessSecurityFrame(prGlueInfo->prAdapter, (P_NATIVE_PACKET) prSkb) == FALSE) {

        /* non-1x packets */

    #if CFG_DBG_GPIO_PINS
        {
            /* TX request from OS */
            mtk_wcn_stp_debug_gpio_assert(IDX_TX_REQ, DBG_TIE_LOW);
            kalUdelay(1);
            mtk_wcn_stp_debug_gpio_assert(IDX_TX_REQ, DBG_TIE_HIGH);
        }
    #endif

#if LINUX_VERSION_CODE > KERNEL_VERSION(2, 6, 26)
        u2QueueIdx = skb_get_queue_mapping(prSkb);
        ASSERT(u2QueueIdx < CFG_MAX_TXQ_NUM);
#endif

#if CFG_ENABLE_PKT_LIFETIME_PROFILE
        GLUE_SET_PKT_ARRIVAL_TIME(prSkb, kalGetTimeTick());
#endif
        GLUE_INC_REF_CNT(prGlueInfo->i4TxPendingFrameNum);
        GLUE_INC_REF_CNT(prGlueInfo->ai4TxPendingFrameNumPerQueue[NETWORK_TYPE_AIS_INDEX][u2QueueIdx]);

        GLUE_ACQUIRE_SPIN_LOCK(prGlueInfo, SPIN_LOCK_TX_QUE);
        QUEUE_INSERT_TAIL(prTxQueue, prQueueEntry);
        GLUE_RELEASE_SPIN_LOCK(prGlueInfo, SPIN_LOCK_TX_QUE);

//        GLUE_INC_REF_CNT(prGlueInfo->i4TxPendingFrameNum);
//        GLUE_INC_REF_CNT(prGlueInfo->ai4TxPendingFrameNumPerQueue[NETWORK_TYPE_AIS_INDEX][u2QueueIdx]);

#if LINUX_VERSION_CODE > KERNEL_VERSION(2, 6, 26)
        if (prGlueInfo->ai4TxPendingFrameNumPerQueue[NETWORK_TYPE_AIS_INDEX][u2QueueIdx] >= CFG_TX_STOP_NETIF_PER_QUEUE_THRESHOLD) {
            netif_stop_subqueue(prDev, u2QueueIdx);

#if (CONF_HIF_LOOPBACK_AUTO == 1)
            prGlueInfo->rHifInfo.HifLoopbkFlg |= 0x01;
#endif /* CONF_HIF_LOOPBACK_AUTO */
        }
#else
        if (prGlueInfo->i4TxPendingFrameNum >= CFG_TX_STOP_NETIF_QUEUE_THRESHOLD) {
            netif_stop_queue(prDev);

#if (CONF_HIF_LOOPBACK_AUTO == 1)
            prGlueInfo->rHifInfo.HifLoopbkFlg |= 0x01;
#endif /* CONF_HIF_LOOPBACK_AUTO */
        }
#endif
    } else {
        //printk("is security frame\n");

        GLUE_INC_REF_CNT(prGlueInfo->i4TxPendingSecurityFrameNum);
    }

    DBGLOG(TX, EVENT, ("\n+++++ pending frame %d len = %d +++++\n", (INT_32)prGlueInfo->i4TxPendingFrameNum, prSkb->len));
    prGlueInfo->rNetDevStats.tx_bytes += prSkb->len;
    prGlueInfo->rNetDevStats.tx_packets++;

    /* set GLUE_FLAG_TXREQ_BIT */

    //pr->u4Flag |= GLUE_FLAG_TXREQ;
    //wake_up_interruptible(&prGlueInfo->waitq);
    kalSetEvent(prGlueInfo);


    /* For Linux, we'll always return OK FLAG, because we'll free this skb by ourself */
    return NETDEV_TX_OK;
} /* end of wlanHardStartXmit() */


/*----------------------------------------------------------------------------*/
/*!
* \brief A method of struct net_device, to get the network interface statistical
*        information.
*
* Whenever an application needs to get statistics for the interface, this method
* is called. This happens, for example, when ifconfig or netstat -i is run.
*
* \param[in] prDev      Pointer to struct net_device.
*
* \return net_device_stats buffer pointer.
*/
/*----------------------------------------------------------------------------*/
struct net_device_stats *
wlanGetStats (
    IN struct net_device *prDev
    )
{
    P_GLUE_INFO_T prGlueInfo = *((P_GLUE_INFO_T *) netdev_priv(prDev));

    #if 0
    WLAN_STATUS rStatus;
    UINT_32 u4XmitError = 0;
    UINT_32 u4XmitOk = 0;
    UINT_32 u4RecvError = 0;
    UINT_32 u4RecvOk = 0;
    UINT_32 u4BufLen;

    ASSERT(prDev);

    /* @FIX ME: need a more clear way to do this */


    rStatus = kalIoctl(prGlueInfo,
            wlanoidQueryXmitError,
            &u4XmitError,
            sizeof(UINT_32),
            TRUE,
            TRUE,
            TRUE,
                         &u4BufLen);

    rStatus = kalIoctl(prGlueInfo,
            wlanoidQueryXmitOk,
            &u4XmitOk,
            sizeof(UINT_32),
            TRUE,
            TRUE,
            TRUE,
            &u4BufLen);
    rStatus = kalIoctl(prGlueInfo,
            wlanoidQueryRcvOk,
            &u4RecvOk,
            sizeof(UINT_32),
            TRUE,
            TRUE,
            TRUE,
            &u4BufLen);
    rStatus = kalIoctl(prGlueInfo,
            wlanoidQueryRcvError,
            &u4RecvError,
            sizeof(UINT_32),
            TRUE,
            TRUE,
            TRUE,
            &u4BufLen);
    prGlueInfo->rNetDevStats.rx_packets = u4RecvOk;
    prGlueInfo->rNetDevStats.tx_packets = u4XmitOk;
    prGlueInfo->rNetDevStats.tx_errors  = u4XmitError;
    prGlueInfo->rNetDevStats.rx_errors  = u4RecvError;
    //prGlueInfo->rNetDevStats.rx_bytes   = rCustomNetDevStats.u4RxBytes;
    //prGlueInfo->rNetDevStats.tx_bytes   = rCustomNetDevStats.u4TxBytes;
    //prGlueInfo->rNetDevStats.rx_errors  = rCustomNetDevStats.u4RxErrors;
    //prGlueInfo->rNetDevStats.multicast  = rCustomNetDevStats.u4Multicast;
    #endif
    //prGlueInfo->rNetDevStats.rx_packets = 0;
    //prGlueInfo->rNetDevStats.tx_packets = 0;
    prGlueInfo->rNetDevStats.tx_errors  = 0;
    prGlueInfo->rNetDevStats.rx_errors  = 0;
    //prGlueInfo->rNetDevStats.rx_bytes   = 0;
    //prGlueInfo->rNetDevStats.tx_bytes   = 0;
    prGlueInfo->rNetDevStats.rx_errors  = 0;
    prGlueInfo->rNetDevStats.multicast  = 0;

    return &prGlueInfo->rNetDevStats;

} /* end of wlanGetStats() */

/*----------------------------------------------------------------------------*/
/*!
* \brief A function for prDev->init
*
* \param[in] prDev      Pointer to struct net_device.
*
* \retval 0         The execution of wlanInit succeeds.
* \retval -ENXIO    No such device.
*/
/*----------------------------------------------------------------------------*/
static int
wlanInit(
    struct net_device *prDev
    )
{
    P_GLUE_INFO_T prGlueInfo = NULL;

	if (fgIsWorkMcEverInit == FALSE)
	{
    if (!prDev) {
        return -ENXIO;
    }

    prGlueInfo = *((P_GLUE_INFO_T *) netdev_priv(prDev));
#if LINUX_VERSION_CODE > KERNEL_VERSION(2, 6, 12)
    INIT_DELAYED_WORK(&workq, wlanSetMulticastListWorkQueue);
#else
    INIT_DELAYED_WORK(&workq, wlanSetMulticastListWorkQueue, NULL);
#endif

		fgIsWorkMcEverInit = TRUE;
	}

    return 0; /* success */
} /* end of wlanInit() */


/*----------------------------------------------------------------------------*/
/*!
* \brief A function for prDev->uninit
*
* \param[in] prDev      Pointer to struct net_device.
*
* \return (none)
*/
/*----------------------------------------------------------------------------*/
static void
wlanUninit(
    struct net_device *prDev
    )
{
    return;
} /* end of wlanUninit() */


/*----------------------------------------------------------------------------*/
/*!
* \brief A function for prDev->open
*
* \param[in] prDev      Pointer to struct net_device.
*
* \retval 0     The execution of wlanOpen succeeds.
* \retval < 0   The execution of wlanOpen failed.
*/
/*----------------------------------------------------------------------------*/
static int
wlanOpen(
    struct net_device *prDev
    )
{
    ASSERT(prDev);

#if LINUX_VERSION_CODE > KERNEL_VERSION(2, 6, 26)
    netif_tx_start_all_queues(prDev);
#else
    netif_start_queue(prDev);
#endif

    return 0; /* success */
} /* end of wlanOpen() */


/*----------------------------------------------------------------------------*/
/*!
* \brief A function for prDev->stop
*
* \param[in] prDev      Pointer to struct net_device.
*
* \retval 0     The execution of wlanStop succeeds.
* \retval < 0   The execution of wlanStop failed.
*/
/*----------------------------------------------------------------------------*/
static int
wlanStop(
    struct net_device *prDev
    )
{
    P_GLUE_INFO_T prGlueInfo = NULL;
    struct cfg80211_scan_request *prScanRequest = NULL;
    GLUE_SPIN_LOCK_DECLARATION();

    ASSERT(prDev);

    prGlueInfo = *((P_GLUE_INFO_T *) netdev_priv(prDev));

    /* CFG80211 down */
    GLUE_ACQUIRE_SPIN_LOCK(prGlueInfo, SPIN_LOCK_NET_DEV);
    if(prGlueInfo->prScanRequest != NULL) {
        prScanRequest = prGlueInfo->prScanRequest;
        prGlueInfo->prScanRequest = NULL;
    }
    GLUE_RELEASE_SPIN_LOCK(prGlueInfo, SPIN_LOCK_NET_DEV);

    if(prScanRequest) {
        struct cfg80211_scan_info info = {
            .aborted = true,
        };

        cfg80211_scan_done(prScanRequest, &info);
    }

#if LINUX_VERSION_CODE > KERNEL_VERSION(2, 6, 26)
    netif_tx_stop_all_queues(prDev);
#else
    netif_stop_queue(prDev);
#endif

    return 0; /* success */
} /* end of wlanStop() */


/*----------------------------------------------------------------------------*/
/*!
 * \brief Update Channel table for cfg80211 for Wi-Fi Direct based on current country code
 *
 * \param[in] prGlueInfo      Pointer to glue info
 *
 * \return   none
 */
/*----------------------------------------------------------------------------*/
VOID
wlanUpdateChannelTable(
    P_GLUE_INFO_T prGlueInfo
    )
{
    UINT_8 i, j;
    UINT_8 ucNumOfChannel;
    RF_CHANNEL_INFO_T aucChannelList[ARRAY_SIZE(mtk_2ghz_channels) + ARRAY_SIZE(mtk_5ghz_channels)];

    // 1. Disable all channel
    for(i = 0; i < ARRAY_SIZE(mtk_2ghz_channels); i++) {
        mtk_2ghz_channels[i].flags |= IEEE80211_CHAN_DISABLED;
    }

    for(i = 0; i < ARRAY_SIZE(mtk_5ghz_channels); i++) {
        mtk_5ghz_channels[i].flags |= IEEE80211_CHAN_DISABLED;
    }

    // 2. Get current domain channel list
    rlmDomainGetChnlList(prGlueInfo->prAdapter,
            BAND_NULL,
            ARRAY_SIZE(mtk_2ghz_channels) + ARRAY_SIZE(mtk_5ghz_channels),
            &ucNumOfChannel,
            aucChannelList);

    // 3. Enable specific channel based on domain channel list
    for(i = 0; i < ucNumOfChannel; i++) {
        switch(aucChannelList[i].eBand) {
        case BAND_2G4:
            for(j = 0 ; j < ARRAY_SIZE(mtk_2ghz_channels) ; j++) {
                if(mtk_2ghz_channels[j].hw_value == aucChannelList[i].ucChannelNum) {
                    mtk_2ghz_channels[j].flags &= ~IEEE80211_CHAN_DISABLED;
                    break;
                }
            }
            break;

        case BAND_5G:
            for(j = 0 ; j < ARRAY_SIZE(mtk_5ghz_channels) ; j++) {
                if(mtk_5ghz_channels[j].hw_value == aucChannelList[i].ucChannelNum) {
                    mtk_5ghz_channels[j].flags &= ~IEEE80211_CHAN_DISABLED;
                    break;
                }
            }
            break;

        default:
            break;
        }
    }

    return;
}


/*----------------------------------------------------------------------------*/
/*!
* \brief Register the device to the kernel and return the index.
*
* \param[in] prDev      Pointer to struct net_device.
*
* \retval 0     The execution of wlanNetRegister succeeds.
* \retval < 0   The execution of wlanNetRegister failed.
*/
/*----------------------------------------------------------------------------*/
static INT_32
wlanNetRegister(
    struct wireless_dev *prWdev
    )
{
    P_GLUE_INFO_T prGlueInfo;
    INT_32 i4DevIdx = -1;

    ASSERT(prWdev);


    do {
        if (!prWdev) {
            break;
        }

        prGlueInfo = (P_GLUE_INFO_T) wiphy_priv(prWdev->wiphy);

        if ((i4DevIdx = wlanGetDevIdx(prWdev->netdev)) < 0) {
            DBGLOG(INIT, ERROR, ("wlanNetRegister: net_device number exceeds.\n"));
            break;
        }

        /* adjust channel support status */


        if (wiphy_register(prWdev->wiphy) < 0) {
            DBGLOG(INIT, ERROR, ("wlanNetRegister: wiphy context is not registered.\n"));
            wlanClearDevIdx(prWdev->netdev);
            i4DevIdx = -1;
            break;
        }

        if(register_netdev(prWdev->netdev) < 0) {
            DBGLOG(INIT, ERROR, ("wlanNetRegister: net_device context is not registered.\n"));

            wiphy_unregister(prWdev->wiphy);
            wlanClearDevIdx(prWdev->netdev);
            i4DevIdx = -1;
        }

        if(i4DevIdx != -1) {
            prGlueInfo->fgIsRegistered = TRUE;
        }
    }
    while(FALSE);

    return i4DevIdx; /* success */
} /* end of wlanNetRegister() */


/*----------------------------------------------------------------------------*/
/*!
* \brief Unregister the device from the kernel
*
* \param[in] prWdev      Pointer to struct net_device.
*
* \return (none)
*/
/*----------------------------------------------------------------------------*/
static VOID
wlanNetUnregister(
    struct wireless_dev *prWdev
    )
{
    P_GLUE_INFO_T prGlueInfo;

    if (!prWdev) {
        DBGLOG(INIT, ERROR, ("wlanNetUnregister: The device context is NULL\n"));
        return;
    }

    prGlueInfo = (P_GLUE_INFO_T) wiphy_priv(prWdev->wiphy);

    wlanClearDevIdx(prWdev->netdev);
    unregister_netdev(prWdev->netdev);
    wiphy_unregister(prWdev->wiphy);

    prGlueInfo->fgIsRegistered = FALSE;

    DBGLOG(INIT, INFO, ("unregister wireless_dev(0x%p)\n", prWdev));

    return;
} /* end of wlanNetUnregister() */


#if LINUX_VERSION_CODE >= KERNEL_VERSION(2, 6, 31)
static const struct net_device_ops wlan_netdev_ops = {
	.ndo_open		= wlanOpen,
	.ndo_stop		= wlanStop,
#if LINUX_VERSION_CODE >= KERNEL_VERSION(3, 2, 0)
    .ndo_set_rx_mode = wlanSetMulticastList,
#else
	.ndo_set_multicast_list	= wlanSetMulticastList,
#endif
	.ndo_get_stats		= wlanGetStats,

	.ndo_start_xmit		= wlanHardStartXmit,
	.ndo_init		= wlanInit,
	.ndo_uninit		= wlanUninit,
    .ndo_select_queue = wlanSelectQueue,
};
#endif

/*----------------------------------------------------------------------------*/
/*!
* \brief A method for creating Linux NET4 struct net_device object and the
*        private data(prGlueInfo and prAdapter). Setup the IO address to the HIF.
*        Assign the function pointer to the net_device object
*
* \param[in] pvData     Memory address for the device
*
* \retval Not null      The wireless_dev object.
* \retval NULL          Fail to create wireless_dev object
*/
/*----------------------------------------------------------------------------*/
static struct lock_class_key rSpinKey[SPIN_LOCK_NUM];
static struct wireless_dev *
wlanNetCreate(
    PVOID pvData
    )
{
    struct wireless_dev *prWdev = NULL;
    P_GLUE_INFO_T prGlueInfo =  NULL;
    P_ADAPTER_T prAdapter = NULL;
    UINT_32 i;
    struct device *prDev;

    //4 <1.1> Create wireless_dev
    prWdev = kzalloc(sizeof(struct wireless_dev), GFP_KERNEL);
    DBGLOG(INIT, INFO, ("wireless_dev prWdev(0x%p) allocated\n", prWdev));
    if (!prWdev) {
        DBGLOG(INIT, ERROR, ("Allocating memory to wireless_dev context failed\n"));
        return NULL;
    }

    //4 <1.2> Create wiphy
    prWdev->wiphy = wiphy_new(&mtk_wlan_ops, sizeof(GLUE_INFO_T));
    DBGLOG(INIT, INFO, ("wiphy (0x%p) allocated\n", prWdev->wiphy));
    if (!prWdev->wiphy) {
        DBGLOG(INIT, ERROR, ("Allocating memory to wiphy device failed\n"));
        kfree(prWdev);
        return NULL;
    }

    //4 <1.3> co-relate wiphy & prDev
#if MTK_WCN_HIF_SDIO
    mtk_wcn_hif_sdio_get_dev(*((MTK_WCN_HIF_SDIO_CLTCTX *)pvData), &prDev);
#else
//    prDev = &((struct sdio_func *) pvData)->dev; //samp
    prDev = pvData; //samp
#endif
    if (!prDev) {
        printk(KERN_ALERT DRV_NAME "unable to get struct dev for wlan\n");
    }
    set_wiphy_dev(prWdev->wiphy, prDev);

    //4 <1.4> configure wireless_dev & wiphy
    prWdev->iftype = NL80211_IFTYPE_STATION;
    prWdev->wiphy->max_scan_ssids   = 1;    /* FIXME: for combo scan */
    prWdev->wiphy->max_scan_ie_len = 512;
    prWdev->wiphy->interface_modes  = BIT(NL80211_IFTYPE_STATION);
    prWdev->wiphy->mgmt_stypes      = mtk_cfg80211_ais_default_mgmt_stypes;
    prWdev->wiphy->bands[NL80211_BAND_2GHZ] = &mtk_band_2ghz;
    //for the WPS probe request suband issue
    //prWdev->wiphy->bands[NL80211_BAND_5GHZ] = &mtk_band_5ghz;
    prWdev->wiphy->bands[NL80211_BAND_5GHZ] = NULL;
    prWdev->wiphy->signal_type      = CFG80211_SIGNAL_TYPE_MBM;
    prWdev->wiphy->cipher_suites    = (const u32 *)mtk_cipher_suites;
    prWdev->wiphy->n_cipher_suites  = ARRAY_SIZE(mtk_cipher_suites);
    prWdev->wiphy->flags            = WIPHY_FLAG_SUPPORTS_FW_ROAM;
    prWdev->wiphy->regulatory_flags = REGULATORY_DISABLE_BEACON_HINTS;
    prWdev->wiphy->reg_notifier = y2_wifi_reg_notifier;

    //4 <2> Create Glue structure
    prGlueInfo = (P_GLUE_INFO_T) wiphy_priv(prWdev->wiphy);
    if (!prGlueInfo) {
        DBGLOG(INIT, ERROR, ("Allocating memory to glue layer failed\n"));
        goto netcreate_err;
    }

    //4 <3> Initial Glue structure
    //4 <3.1> Create net device
    prGlueInfo->prDevHandler = alloc_netdev_mq(sizeof(P_GLUE_INFO_T), NIC_INF_NAME, NET_NAME_ENUM, ether_setup, CFG_MAX_TXQ_NUM);

    DBGLOG(INIT, INFO, ("net_device prDev(0x%p) allocated\n", prGlueInfo->prDevHandler));
    if (!prGlueInfo->prDevHandler) {
        DBGLOG(INIT, ERROR, ("Allocating memory to net_device context failed\n"));
        goto netcreate_err;
    }

    //4 <3.1.1> initialize net device varaiables
    *((P_GLUE_INFO_T *) netdev_priv(prGlueInfo->prDevHandler)) = prGlueInfo;

    prGlueInfo->prDevHandler->netdev_ops = &wlan_netdev_ops;
    netif_carrier_off(prGlueInfo->prDevHandler);
    netif_tx_stop_all_queues(prGlueInfo->prDevHandler);

    //4 <3.1.2> co-relate with wiphy bi-directionally
    prGlueInfo->prDevHandler->ieee80211_ptr = prWdev;
#if CFG_TCP_IP_CHKSUM_OFFLOAD
    prGlueInfo->prDevHandler->features = NETIF_F_HW_CSUM;
#endif
    prWdev->netdev = prGlueInfo->prDevHandler;

    //4 <3.1.3> co-relate net device & prDev
    SET_NETDEV_DEV(prGlueInfo->prDevHandler, wiphy_dev(prWdev->wiphy));

    //4 <3.2> initiali glue variables
    prGlueInfo->eParamMediaStateIndicated = PARAM_MEDIA_STATE_DISCONNECTED;
    prGlueInfo->ePowerState = ParamDeviceStateD0;
    prGlueInfo->fgIsMacAddrOverride = FALSE;
    prGlueInfo->fgIsRegistered = FALSE;
    prGlueInfo->prScanRequest = NULL;

#if CFG_SUPPORT_HOTSPOT_2_0
	/* Init DAD */
	prGlueInfo->fgIsDad  = FALSE;
	prGlueInfo->fgIs6Dad = FALSE;
	kalMemZero(prGlueInfo->aucDADipv4,4);
	kalMemZero(prGlueInfo->aucDADipv6,16);
#endif

    init_completion(&prGlueInfo->rScanComp);
    init_completion(&prGlueInfo->rHaltComp);
    init_completion(&prGlueInfo->rPendComp);
#if CFG_ENABLE_WIFI_DIRECT
    init_completion(&prGlueInfo->rSubModComp);
#endif

    /* initialize timer for OID timeout checker */
    kalOsTimerInitialize(prGlueInfo, kalTimeoutHandler);

    for (i = 0; i < SPIN_LOCK_NUM; i++) {
        spin_lock_init(&prGlueInfo->rSpinLock[i]);
        lockdep_set_class(&prGlueInfo->rSpinLock[i], &rSpinKey[i]);
    }

    /* initialize semaphore for ioctl */
    sema_init(&prGlueInfo->ioctl_sem, 1);

    /* initialize semaphore for ioctl */
    sema_init(&g_halt_sem, 1);
    g_u4HaltFlag = 0;

    //4 <4> Create Adapter structure
    prAdapter = (P_ADAPTER_T) wlanAdapterCreate(prGlueInfo);

    if (!prAdapter) {
        DBGLOG(INIT, ERROR, ("Allocating memory to adapter failed\n"));
        goto netcreate_err;
    }

    prGlueInfo->prAdapter = prAdapter;

#ifdef CONFIG_CFG80211_WEXT
    //4 <5> Use wireless extension to replace IOCTL
    prWdev->wiphy->wext = &wext_handler_def;
#endif

    goto netcreate_done;

netcreate_err:
    if (NULL != prAdapter) {
        wlanAdapterDestroy(prAdapter);
        prAdapter = NULL;
    }

    if (prGlueInfo && prGlueInfo->prDevHandler) {
        free_netdev(prGlueInfo->prDevHandler);
        prGlueInfo->prDevHandler = NULL;
    }

    if (NULL != prWdev->wiphy) {
        wiphy_free(prWdev->wiphy);
        prWdev->wiphy = NULL;
    }

    if (NULL != prWdev) {
        /* Free net_device and private data, which are allocated by
         * alloc_netdev().
         */
        kfree(prWdev);
        prWdev = NULL;
    }

netcreate_done:

    return prWdev;
} /* end of wlanNetCreate() */


/*----------------------------------------------------------------------------*/
/*!
* \brief Destroying the struct net_device object and the private data.
*
* \param[in] prWdev      Pointer to struct wireless_dev.
*
* \return (none)
*/
/*----------------------------------------------------------------------------*/
static VOID
wlanNetDestroy(
    struct wireless_dev *prWdev
    )
{
    P_GLUE_INFO_T prGlueInfo = NULL;

    ASSERT(prWdev);

    if (!prWdev) {
        DBGLOG(INIT, ERROR, ("wlanNetDestroy: The device context is NULL\n"));
        return;
    }

    /* prGlueInfo is allocated with net_device */
    prGlueInfo = (P_GLUE_INFO_T) wiphy_priv(prWdev->wiphy);
    ASSERT(prGlueInfo);

    /* destroy kal OS timer */
    kalCancelTimer(prGlueInfo);

    glClearHifInfo(prGlueInfo);

    wlanAdapterDestroy(prGlueInfo->prAdapter);
    prGlueInfo->prAdapter = NULL;

    /* A timed-out OID may still have been referenced by a firmware event.
     * IRQ, worker and command/timer queues are now drained. */
    kfree_sensitive(prGlueInfo->OidEntry.pvInfoBuf);
    prGlueInfo->OidEntry.pvInfoBuf = NULL;

    /* Free net_device and private data prGlueInfo, which are allocated by alloc_netdev().
     */
    free_netdev(prWdev->netdev);
    wiphy_free(prWdev->wiphy);

    kfree(prWdev);

    return;
} /* end of wlanNetDestroy() */



extern void wlanRegisterNotifier(void);
extern void wlanUnregisterNotifier(void);

/*mtk80707 rollback to aosp hal*/
typedef void (*set_dbg_level)(unsigned char modules[DBG_MODULE_NUM]);
extern void register_set_dbg_level_handler(set_dbg_level handler);
#if CFG_ENABLE_WIFI_DIRECT
typedef int (*set_p2p_mode)(struct net_device *netdev, PARAM_CUSTOM_P2P_SET_STRUC_T p2pmode);

extern void register_set_p2p_mode_handler(set_p2p_mode handler);

int
set_p2p_mode_handler(
    struct net_device *netdev,
    PARAM_CUSTOM_P2P_SET_STRUC_T p2pmode
    )
{
#if 0
    P_GLUE_INFO_T prGlueInfo  = *((P_GLUE_INFO_T *)netdev_priv(netdev));
    PARAM_CUSTOM_P2P_SET_STRUC_T rSetP2P;
    WLAN_STATUS rWlanStatus = WLAN_STATUS_SUCCESS;
    UINT_32 u4BufLen = 0;

    rSetP2P.u4Enable = p2pmode.u4Enable;
    rSetP2P.u4Mode = p2pmode.u4Mode;

    if(!rSetP2P.u4Enable) {
        p2pNetUnregister(prGlueInfo, TRUE);
    }

    rWlanStatus = kalIoctl(prGlueInfo,
                        wlanoidSetP2pMode,
                        (PVOID)&rSetP2P,
                        sizeof(PARAM_CUSTOM_P2P_SET_STRUC_T),
                        FALSE,
                        FALSE,
                        TRUE,
                        FALSE,
                        &u4BufLen);
    printk("set_p2p_mode_handler ret = %d\n", rWlanStatus);
    if(rSetP2P.u4Enable) {
        p2pNetRegister(prGlueInfo, TRUE);
    }

    return 0;

#else

    P_GLUE_INFO_T prGlueInfo  = *((P_GLUE_INFO_T *)netdev_priv(netdev));
	extern int g_u4HaltFlag;
	extern spinlock_t g_p2p_lock;
	extern int g_u4P2PEnding;
	extern int g_u4P2POnOffing;
	PARAM_CUSTOM_P2P_SET_STRUC_T rSetP2P;
	WLAN_STATUS rWlanStatus = WLAN_STATUS_SUCCESS;
	BOOLEAN fgIsP2PEnding;
	UINT_32 u4BufLen = 0;
	GLUE_SPIN_LOCK_DECLARATION();


	printk("PRIV_CMD_P2P_MODE=%u %u\n", (UINT32)p2pmode.u4Enable, (UINT32)p2pmode.u4Mode);

	/* avoid remove & p2p off command simultaneously */
	GLUE_ACQUIRE_THE_SPIN_LOCK(&g_p2p_lock);
	fgIsP2PEnding = g_u4P2PEnding;
	g_u4P2POnOffing = 1;
	GLUE_RELEASE_THE_SPIN_LOCK(&g_p2p_lock);

	if (fgIsP2PEnding == 1)
	{
		/* skip the command if we are removing */
		GLUE_ACQUIRE_THE_SPIN_LOCK(&g_p2p_lock);
		g_u4P2POnOffing = 0;
		GLUE_RELEASE_THE_SPIN_LOCK(&g_p2p_lock);
		return 0;
	}
	rSetP2P.u4Enable = p2pmode.u4Enable;
	rSetP2P.u4Mode = p2pmode.u4Mode;

	if(!rSetP2P.u4Enable) {
		p2pNetUnregister(prGlueInfo, TRUE);
	}


	/* move out to caller to avoid kalIoctrl & suspend/resume deadlock problem ALPS00844864 */
	/*
		Scenario:
			1. System enters suspend/resume but not yet enter wlanearlysuspend()
				or wlanlateresume();

			2. System switches to do PRIV_CMD_P2P_MODE and execute kalIoctl()
				and get g_halt_sem then do glRegisterEarlySuspend() or
				glUnregisterEarlySuspend();

				But system suspend/resume procedure is not yet finished so we
				suspend;

			3. System switches back to do suspend/resume procedure and execute
				kalIoctl(). But driver does not yet release g_halt_sem so system
				suspend in wlanearlysuspend() or wlanlateresume();

				==> deadlock occurs.
	*/
	if ((!rSetP2P.u4Enable) && (g_u4HaltFlag == 0) &&
		(kalIsResetting() == FALSE))
	{
		/* fgIsP2PRegistered == TRUE means P2P is enabled */
		p2pEalySuspendReg(prGlueInfo, rSetP2P.u4Enable); /* p2p remove */
	}

	rWlanStatus = kalIoctl(prGlueInfo,
						wlanoidSetP2pMode,
						(PVOID)&rSetP2P, /* pu4IntBuf[0] is used as input SubCmd */
						sizeof(PARAM_CUSTOM_P2P_SET_STRUC_T),
						FALSE,
						FALSE,
						TRUE,
						FALSE,
						&u4BufLen);

	/* move out to caller to avoid kalIoctrl & suspend/resume deadlock problem ALPS00844864 */
	if ((rSetP2P.u4Enable) && (g_u4HaltFlag == 0) &&
		(kalIsResetting() == FALSE))
	{
		/* fgIsP2PRegistered == TRUE means P2P on successfully */
		p2pEalySuspendReg(prGlueInfo, rSetP2P.u4Enable); /* p2p on */
	}

	/*it need to check fgIsP2PRegistered, in case of whole chip reset.
	 * in this case, kalIOCTL return success always,
	 * and prGlueInfo->prP2pInfo maay ben NULL*/
	if(rSetP2P.u4Enable &&
		prGlueInfo->prAdapter->fgIsP2PRegistered) {
		p2pNetRegister(prGlueInfo, TRUE);
	}

	GLUE_ACQUIRE_THE_SPIN_LOCK(&g_p2p_lock);
	g_u4P2POnOffing = 0;
	GLUE_RELEASE_THE_SPIN_LOCK(&g_p2p_lock);
	return 0;
#endif
}
#endif

static void
set_dbg_level_handler(
    unsigned char dbg_lvl[DBG_MODULE_NUM]
    )
{
    kalMemCopy(aucDebugModule, dbg_lvl, sizeof(aucDebugModule));
    kalPrint("[wlan] change debug level");
}
/*endof mtk80707*/

/*----------------------------------------------------------------------------*/
/*!
* \brief Wlan probe function. This function probes and initializes the device.
*
* \param[in] pvData     data passed by bus driver init function
*                           _HIF_EHPI: NULL
*                           _HIF_SDIO: sdio bus driver handle
*
* \retval 0 Success
* \retval negative value Failed
*/
/*----------------------------------------------------------------------------*/
/* Linux lifetime wrapper around the fullmac protocol engine. Every failed
 * stage unwinds through the same path as a radio toggle; no orphan netdev,
 * IRQ, firmware allocation, or worker survives an unsuccessful enable. */
static struct wireless_dev *y2_wdev;
static bool y2_adapter_started;
static VOID wlanRemove(VOID)
{
    struct wireless_dev *wdev=y2_wdev;
    P_GLUE_INFO_T glue;
    if (!wdev) return;
    glue=wiphy_priv(wdev->wiphy);
    if (glue->fgIsRegistered) wlanNetUnregister(wdev);
    WRITE_ONCE(g_u4HaltFlag,1);
    cancel_delayed_work_sync(&workq);
    glBusFreeIrq(wdev->netdev,glue);
    set_bit(GLUE_FLAG_HALT_BIT,&glue->u4Flag);
    wake_up_interruptible(&glue->waitq);
    if (!IS_ERR_OR_NULL(glue->main_thread)) {
        kthread_stop(glue->main_thread);
        glue->main_thread=NULL;
    }
    if (y2_adapter_started) wlanAdapterStop(glue->prAdapter);
    y2_adapter_started=false;
    gPrDev=NULL; y2_wdev=NULL;
    wlanNetDestroy(wdev);
}
static INT_32 wlanProbe(PVOID data)
{
    P_GLUE_INFO_T glue;
    P_REG_INFO_T reg;
    PVOID firmware=NULL;
    UINT_32 size=0, length=0;
    unsigned char address[ETH_ALEN];
    int ret;
    if (y2_wdev || !glBusInit(data)) return -EBUSY;
    y2_wdev=wlanNetCreate(data);
    if (!y2_wdev) return -ENOMEM;
    glue=wiphy_priv(y2_wdev->wiphy); gPrDev=y2_wdev->netdev;
    glSetHifInfo(glue,(UINT_32)data);
    INIT_DELAYED_WORK(&workq,wlanSetMulticastListWorkQueue);
    ret=glBusSetIrq(y2_wdev->netdev,NULL,glue);
    if (ret) goto fail;
    reg=&glue->rRegInfo;
    memset(reg,0,sizeof(*reg));
    reg->u4StartAddress=CFG_FW_START_ADDRESS; reg->u4LoadAddress=CFG_FW_LOAD_ADDRESS;
    glLoadNvram(glue,reg);
    if (!glue->fgNvramAvailable) { ret=-ENODATA; goto fail; }
    reg->u4PowerMode=CFG_INIT_POWER_SAVE_PROF;
    reg->fgEnArpFilter=TRUE;
    reg->uc2G4BwFixed20M=1; reg->uc5GBwFixed20M=1;
    reg->ucSupport5GBand=0; reg->ucEnable5GBand=0;
    reg->eRegChannelListMap=REG_CH_MAP_CUSTOMIZED;
    memset(&reg->rDomainInfo,0,sizeof(reg->rDomainInfo));
    reg->rDomainInfo.rSubBand[0]=(DOMAIN_SUBBAND_INFO){81,BAND_2G4,1,1,11,0};
    reg->au2CountryCode[0]='0'; reg->au2CountryCode[1]='0';
    if (!kalFirmwareImageMapping(glue,&firmware,&size)) { ret=-ENOENT; goto fail; }
    if (size!=160368) ret=-ENOEXEC;
    else ret=wlanAdapterStart(glue->prAdapter,reg,firmware,size)==WLAN_STATUS_SUCCESS ? 0 : -EIO;
    kalFirmwareImageUnmapping(glue,NULL,firmware);
    if (ret) goto fail;
    y2_adapter_started=true;
    glue->main_thread=kthread_run(tx_thread,y2_wdev->netdev,"y2-wifi");
    if (IS_ERR(glue->main_thread)) { ret=PTR_ERR(glue->main_thread); goto fail; }
    ret=kalIoctl(glue,wlanoidQueryCurrentAddr,address,ETH_ALEN,TRUE,TRUE,TRUE,FALSE,&length);
    if (ret!=WLAN_STATUS_SUCCESS || !is_valid_ether_addr(address)) { ret=-EIO; goto fail; }
    eth_hw_addr_set(y2_wdev->netdev,address);
    memcpy(y2_wdev->netdev->perm_addr,address,ETH_ALEN);
    glue->u4ReadyFlag=1;
    ret=wlanNetRegister(y2_wdev);
    if (ret<0) goto fail;
    glue->i4DevIdx=ret;
    return 0;
fail:
    wlanRemove();
    return ret;
}


/*----------------------------------------------------------------------------*/
/*!
* \brief Driver entry point when the driver is configured as a Linux Module, and
*        is called once at module load time, by the user-level modutils
*        application: insmod or modprobe.
*
* \retval 0     Success
*/
/*----------------------------------------------------------------------------*/
//1 Module Entry Point
static int initWlan(void)
{
    int ret = 0, i;

    DBGLOG(INIT, INFO, ("initWlan\n"));

#if CFG_ENABLE_WIFI_DIRECT
    spin_lock_init(&g_p2p_lock);
#endif

    /* memory pre-allocation */
    kalInitIOBuffer();

    ret = ((glRegisterBus(wlanProbe, wlanRemove) == WLAN_STATUS_SUCCESS) ? 0: -EIO);

    if (ret == -EIO) {
        kalUninitIOBuffer();
        return ret;
    }

#if (CFG_CHIP_RESET_SUPPORT)
    glResetInit();
#endif

    /* register set_dbg_level handler to mtk_wmt_wifi */


    for (i=0;i<DBG_MODULE_NUM;i++) aucDebugModule[i]=0;

    return ret;
} /* end of initWlan() */


/*----------------------------------------------------------------------------*/
/*!
* \brief Driver exit point when the driver as a Linux Module is removed. Called
*        at module unload time, by the user level modutils application: rmmod.
*        This is our last chance to clean up after ourselves.
*
* \return (none)
*/
/*----------------------------------------------------------------------------*/
//1 Module Leave Point
static VOID exitWlan(void)
{
    DBGLOG(INIT, INFO, ("exitWlan\n"));

    /* unregister set_dbg_level handler to mtk_wmt_wifi */


#if CFG_CHIP_RESET_SUPPORT
    glResetUninit();
#endif

    glUnregisterBus(wlanRemove);

    /* free pre-allocated memory */
    kalUninitIOBuffer();

    DBGLOG(INIT, INFO, ("exitWlan\n"));

    return;
} /* end of exitWlan() */


#ifdef MTK_WCN_REMOVE_KERNEL_MODULE
int mtk_wcn_wlan_soc_init(void)
{
    return initWlan();
}

void mtk_wcn_wlan_soc_exit(void)
{
    return exitWlan();
}

EXPORT_SYMBOL(mtk_wcn_wlan_soc_init);
EXPORT_SYMBOL(mtk_wcn_wlan_soc_exit);
#else
module_init(initWlan);
module_exit(exitWlan);
#endif
