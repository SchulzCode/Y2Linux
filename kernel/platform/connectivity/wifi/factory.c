// SPDX-License-Identifier: GPL-2.0-only
#include "gl_os.h"
#include "precomp.h"
static_assert(sizeof(WIFI_CFG_PARAM_STRUCT)==512);
static_assert(offsetof(WIFI_CFG_PARAM_STRUCT,aucMacAddress)==4);
static_assert(offsetof(WIFI_CFG_PARAM_STRUCT,rTxPwr)==12);
static_assert(offsetof(WIFI_CFG_PARAM_STRUCT,ucTxPwrValid)==196);
BOOLEAN kalCfgDataRead16(P_GLUE_INFO_T glue, UINT_32 offset, PUINT_16 value)
{
	if (!glue || !value || (offset&1) || offset>510) return FALSE;
	*value=y2_conn_le16(y2_wifi_factory(glue)+offset); return TRUE;
}
BOOLEAN kalCfgDataWrite16(P_GLUE_INFO_T glue, UINT_32 offset, UINT_16 value)
{
	/* Factory storage and handed-off calibration are read-only. */
	return FALSE;
}
