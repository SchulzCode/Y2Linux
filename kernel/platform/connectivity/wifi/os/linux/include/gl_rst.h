/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_WIFI_RESET_H
#define Y2_WIFI_RESET_H
VOID glResetInit(VOID);
VOID glResetUninit(VOID);
VOID glSendResetRequest(VOID);
BOOLEAN kalIsResetting(VOID);
#endif
