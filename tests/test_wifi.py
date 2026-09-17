"""Exercise the production fullmac command completion path without hardware."""
from pathlib import Path
import re
import unittest

from test_audio import function
from test_power import run_c

ROOT = Path(__file__).resolve().parents[1]
WIFI = ROOT / 'kernel/platform/connectivity/wifi'


def legacy_function(path, name):
    # Normalize only the legacy declaration spacing for the shared extractor.
    source = path.read_text()
    source = re.sub(r'(?m)^(WLAN_STATUS|VOID)\n' + name + r' \(',
                    r'static \1 ' + name + '(', source)
    return function(source, name)


class Wifi(unittest.TestCase):
    def test_tx_power_oid_completes_on_actual_command_queue_transmit(self):
        callbacks = '\n'.join(legacy_function(WIFI/'nic/nic_cmd_event.c', n)
                              for n in ('nicCmdEventSetCommon', 'nicOidCmdTimeoutCommon'))
        producer = legacy_function(WIFI/'common/wlan_oid.c', 'wlanoidSetTxPower')
        consumer = legacy_function(WIFI/'common/wlan_lib.c', 'wlanProcessCommandQueue')
        queue = (WIFI/'include/queue.h').read_text().replace('#include "gl_typedef.h"', '')
        run_c(r'''
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#define IN
#define OUT
#define VOID void
#define DBG 0
#define ASSERT assert
#define DEBUGFUNC(...)
#define DBGLOG(...)
#define TRUE true
#define FALSE false
typedef uint8_t UINT_8, *PUINT_8;
typedef uint32_t UINT_32, *PUINT_32;
typedef bool BOOLEAN;
typedef void *PVOID;
typedef unsigned WLAN_STATUS;
#define WLAN_STATUS_SUCCESS 0
#define WLAN_STATUS_PENDING 0x103
#define WLAN_STATUS_FAILURE 0xc0000001
#define WLAN_STATUS_RESOURCES 0xc000009a
#define CMD_ID_SET_TXPWR_CTRL 0x38
''' + queue + r'''
enum { COMMAND_TYPE_GENERAL_IOCTL, COMMAND_TYPE_NETWORK_IOCTL,
       COMMAND_TYPE_SECURITY_FRAME, COMMAND_TYPE_MANAGEMENT_FRAME };
enum { FRAME_ACTION_DROP_PKT, FRAME_ACTION_QUEUE_PKT, FRAME_ACTION_TX_PKT };
enum { FRAME_TYPE_802_1X, FRAME_TYPE_MMPDU };
typedef unsigned ENUM_FRAME_ACTION_T;
typedef struct adapter *P_ADAPTER_T;
typedef struct command *P_CMD_INFO_T;
typedef void (*PFN_CMD_DONE_HANDLER)(P_ADAPTER_T, P_CMD_INFO_T, PUINT_8);
typedef void (*PFN_CMD_TIMEOUT_HANDLER)(P_ADAPTER_T, P_CMD_INFO_T);
struct glue { unsigned completions, status, length; };
struct adapter { struct glue *prGlueInfo; QUE_T rPendingCmdQueue; };
struct command {
 QUE_ENTRY_T queue;
 unsigned eCmdType, eNetworkType, ucStaRecIndex, u4SetInfoLen, u4InformationBufferLength;
 bool fgIsOid, fgSetQuery;
 PVOID prPacket;
 PUINT_8 pucInfoBuffer;
 PFN_CMD_DONE_HANDLER pfCmdDoneHandler;
 PFN_CMD_TIMEOUT_HANDLER pfCmdTimeoutHandler;
};
typedef struct { unsigned ucNetworkType, ucStaRecIndex; } *P_MSDU_INFO_T;
#define KAL_SPIN_LOCK_DECLARATION()
#define KAL_ACQUIRE_SPIN_LOCK(...)
#define KAL_RELEASE_SPIN_LOCK(...)
static QUE_T commands;
static struct command command;
static unsigned send_result, sent, freed;
static unsigned char wire[64];
static void kalOidComplete(struct glue *g, bool set, unsigned n, unsigned status) {
 assert(set); g->completions++; g->status=status; g->length=n;
}
''' + callbacks + r'''
static WLAN_STATUS wlanSendSetQueryCmd(P_ADAPTER_T a, UINT_8 cid,
 BOOLEAN set, BOOLEAN response, BOOLEAN oid, PFN_CMD_DONE_HANDLER done,
 PFN_CMD_TIMEOUT_HANDLER timeout, UINT_32 n, PUINT_8 buf, PVOID result, UINT_32 length) {
 assert(cid==0x38 && set && !response && oid && n==28 && !result && !length);
 assert(!commands.u4NumElem);
 memcpy(wire,buf,n); memset(&command,0,sizeof(command));
 command.eCmdType=COMMAND_TYPE_NETWORK_IOCTL; command.fgIsOid=oid;
 command.fgSetQuery=set; command.u4SetInfoLen=n; command.pucInfoBuffer=wire;
 command.pfCmdDoneHandler=done; command.pfCmdTimeoutHandler=timeout;
 QUEUE_INSERT_TAIL(&commands,&command.queue);
 return WLAN_STATUS_PENDING;
}
static WLAN_STATUS wlanSendCommand(P_ADAPTER_T a, P_CMD_INFO_T c) {
 sent++; assert(c==&command); return send_result;
}
static void cmdBufFreeCmdInfo(P_ADAPTER_T a, P_CMD_INFO_T c) { assert(c==&command); freed++; }
static void wlanReleaseCommand(P_ADAPTER_T a, P_CMD_INFO_T c) { assert(0); }
static ENUM_FRAME_ACTION_T qmGetFrameAction(P_ADAPTER_T a, unsigned n,
 unsigned s, P_MSDU_INFO_T m, unsigned type) { assert(0); return 0; }
''' + producer + consumer + r'''
static struct glue glue;
static struct adapter adapter={.prGlueInfo=&glue};
static unsigned char power[28];
static void start(void) {
 unsigned used=0;
 memset(&glue,0,sizeof(glue)); sent=freed=0; send_result=WLAN_STATUS_SUCCESS;
 QUEUE_INITIALIZE(&commands); QUEUE_INITIALIZE(&adapter.rPendingCmdQueue);
 assert(wlanoidSetTxPower(&adapter,power,sizeof(power),&used)==WLAN_STATUS_PENDING);
 assert(!memcmp(wire,power,sizeof(power))); /* channel/power bytes unchanged */
}
int main(void) {
 for(unsigned i=0;i<sizeof(power);i++) power[i]=(unsigned char)(i*7);
 /* Actual old failure: successful transmission frees the no-response OID
  * without waking its waiter when the completion callback is absent. */
 start(); command.pfCmdDoneHandler=NULL;
 wlanProcessCommandQueue(&adapter,&commands);
 assert(sent==1 && freed==1 && !glue.completions && !commands.u4NumElem);
 /* Corrected producer + actual consumer must complete exactly once. */
 start(); wlanProcessCommandQueue(&adapter,&commands);
 assert(sent==1 && freed==1 && glue.completions==1 && glue.status==WLAN_STATUS_SUCCESS);
 assert(!commands.u4NumElem && !adapter.rPendingCmdQueue.u4NumElem);
 /* TX failure remains failure, and exhausted credits do not imply success. */
 start(); send_result=WLAN_STATUS_FAILURE; wlanProcessCommandQueue(&adapter,&commands);
 assert(glue.completions==1 && glue.status==WLAN_STATUS_FAILURE && freed==1);
 start(); send_result=WLAN_STATUS_RESOURCES; wlanProcessCommandQueue(&adapter,&commands);
 assert(commands.u4NumElem==1 && !glue.completions && !freed);
 send_result=WLAN_STATUS_SUCCESS; wlanProcessCommandQueue(&adapter,&commands);
 assert(glue.completions==1 && glue.status==WLAN_STATUS_SUCCESS && freed==1);
 /* A queued timeout still fails; unrelated non-OID commands don't complete an OID. */
 start(); assert(command.pfCmdTimeoutHandler);
 command.pfCmdTimeoutHandler(&adapter,&command);
 assert(glue.completions==1 && glue.status==WLAN_STATUS_FAILURE);
 start(); command.fgIsOid=FALSE; wlanProcessCommandQueue(&adapter,&commands);
 assert(!glue.completions && freed==1);
}
''')
