/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_CALIBRATION_ABI_H
#define Y2_CALIBRATION_ABI_H
#include "protocol.h"
/* Root-only calibration service messages; little endian, no kernel pointers
 * or physical addresses. A generation/serial prevents late replies from a
 * previous radio boot. The payload is one validated modem FS packet. */
#define Y2_CAL_HEADER 16
#define Y2_CAL_MESSAGE (Y2_CAL_HEADER + Y2_FS_STRIDE)
#define Y2_CAL_VERSION 1
#endif
