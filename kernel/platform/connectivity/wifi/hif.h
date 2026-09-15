/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_WIFI_HIF_H
#define Y2_WIFI_HIF_H
#include "../conn.h"
struct y2_wifi;
typedef struct _GL_HIF_INFO_T {
    struct device *Dev;
    struct y2_wifi *bus;
    u32 ChipID;
    BOOLEAN fgIntReadClear, fgMbxReadClear, fgDmaEnable;
    void __iomem *HifRegBaseAddr;
} GL_HIF_INFO_T, *P_GL_HIF_INFO_T;
#define CONF_HIF_LOOPBACK_AUTO 0
#define MTK_CHIP_ID_6582 0x6582
VOID HifRegDump(P_ADAPTER_T adapter);
BOOLEAN HifIsFwOwn(P_ADAPTER_T adapter);
WLAN_STATUS glRegisterBus(probe_card probe, remove_card remove);
VOID glUnregisterBus(remove_card remove);
VOID glResetHif(GLUE_INFO_T *glue);
VOID glSetHifInfo(P_GLUE_INFO_T glue, UINT_32 cookie);
VOID glClearHifInfo(P_GLUE_INFO_T glue);
VOID glGetChipInfo(GLUE_INFO_T *glue, UINT_8 *buf);
BOOL glBusInit(PVOID data);
VOID glBusRelease(PVOID data);
INT_32 glBusSetIrq(PVOID dev, PVOID isr, PVOID cookie);
VOID glBusFreeIrq(PVOID dev, PVOID cookie);
VOID glSetPowerState(P_GLUE_INFO_T glue, UINT_32 mode);
const unsigned char *y2_wifi_factory(P_GLUE_INFO_T glue);
void y2_wifi_error(P_GLUE_INFO_T glue);
#endif
