"""Failing runtime retune must reconcile actual ios before any payload resumes."""
import unittest
from tests.test_hardware_ceiling import source
from tests.test_storage_ceiling import STUBS, between, run_c


class RuntimeTuning(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.text = source('drivers/mmc/host/mtk-sd.c')

    def code(self):
        fixture = STUBS.replace('u32 caps, caps2, f_max, actual_clock;',
                                'unsigned doing_retune; u32 caps, caps2, f_max, actual_clock;')
        fixture = fixture.replace('struct mmc_host mmc; void *dev;', '''
 unsigned y2_tuning_failures, y2_tune_iocon, y2_tune_pad, y2_tune_rddly0;
 int y2_tuning_result, y2_pending_event, y2_removing, y2_clock_recovery, hsq_en;
 unsigned char *base; struct { unsigned pad_tune_reg; } *dev_comp;
 struct mmc_host mmc; void *dev;''')
        return fixture + r'''
#include <stddef.h>
#define MSDC_IOCON 4
static struct msdc_host *mmc_priv(struct mmc_host *m) {
 return (void *)((char *)m - offsetof(struct msdc_host, mmc));
}
static unsigned readl(void *p) { return *(unsigned *)p; }
static int tune_error, queued;
static int y2_sd_execute_tuning(struct mmc_host *m, unsigned op) { return tune_error; }
static int msdc_execute_tuning(struct mmc_host *m, unsigned op) { return tune_error; }
static void atomic_set(int *p, int v) { *p = v; }
static void schedule_delayed_work(int *w, unsigned delay) { assert(!delay); queued++; }
''' + (between(self.text, '#define Y2_MANAGED_CAPS', 'static int y2_msdc_clock_ready(')
             + between(self.text, 'static void y2_downgrade(', '/* Classify a completed data request.')) + between(
            self.text, 'static void y2_reconcile_tuning(', '/* Faults are handled') + between(
            self.text, 'static int y2_msdc_execute_tuning(', 'static int msdc_prepare_hs400_tuning(')

    def test_runtime_failure_queues_actual_mode_reconciliation_without_double_downgrade(self):
        run_c(self.code() + r'''
int main(void) {
 unsigned regs[256] = {0}; struct mmc_card card = {0};
 struct msdc_host h = { .y2_emmc = true, .base = (void *)regs };
 typeof(*h.dev_comp) comp = { 0xec }; h.dev_comp = &comp;
 h.mmc.card = &card; h.mmc.ios.power_mode = MMC_POWER_ON;
 h.mmc.f_max = 200000000; h.y2_dt_caps = MMC_CAP_MMC_HIGHSPEED | MMC_CAP_1_8V_DDR;
 h.y2_dt_caps2 = MMC_CAP2_HS200_1_8V_SDR; y2_apply_level(&h, 0);
 h.mmc.ios.timing = MMC_TIMING_MMC_HS200; h.mmc.doing_retune = 1;
 tune_error = -EIO;
 assert(y2_msdc_execute_tuning(&h.mmc, 21) == -EIO);
 assert(h.y2_level == 1 && queued == 1 && !resets);
 assert(h.y2_pending_event == Y2_EVENT_TUNING + 1);
 assert(!y2_timing_admitted(&h.mmc, h.mmc.ios.timing));
 y2_reconcile_tuning(&h);
 assert(h.y2_level == 1 && resets == 1 && h.y2_resets == 1 && !h.y2_reset_error);
 /* Core error recovery wins the race: the worker must not drop another mode. */
 h.mmc.ios.timing = MMC_TIMING_MMC_DDR52; y2_reconcile_tuning(&h);
 assert(resets == 1 && h.y2_level == 1);
 /* Failed reset continues down the existing safe ladder. */
 h.mmc.ios.timing = MMC_TIMING_MMC_HS200; reset_failures = 1;
 y2_reconcile_tuning(&h);
 assert(h.y2_level == 2 && resets == 3 && !h.y2_reset_error);
 /* Discovery already has its own negotiation retry; no asynchronous reset. */
 y2_apply_level(&h, 0); h.mmc.doing_retune = 0; queued = 0;
 assert(y2_msdc_execute_tuning(&h.mmc, 21) == -EIO);
 assert(h.y2_level == 1 && !queued && resets == 3);
 /* Successful retune does not change capabilities or schedule recovery. */
 tune_error = 0; y2_apply_level(&h, 0); h.mmc.doing_retune = 1;
 assert(!y2_msdc_execute_tuning(&h.mmc, 21));
 assert(h.y2_level == 0 && !queued && resets == 3);
}
''')

    def test_payload_cannot_race_worker_in_rejected_mode_but_reinitialization_can(self):
        guard = between(self.text, 'static void msdc_ops_request(',
                        '\t/* One internal-eMMC transport.')
        run_c(self.code() + r'''
struct mmc_command { unsigned opcode; int error; };
struct mmc_request { struct mmc_command *cmd; void *data; };
static int completed, allowed;
#define mmc_op_tuning(op) ((op) == 19 || (op) == 21)
static int mmc_hsq_finalize_request(struct mmc_host *m, struct mmc_request *r) { return 0; }
static void mmc_request_done(struct mmc_host *m, struct mmc_request *r) { completed++; }
''' + guard + '\tallowed++;\n}\n' + r'''
int main(void) {
 struct msdc_host h = { .y2_sd = true };
 h.mmc.f_max = 200000000; h.y2_dt_caps = MMC_CAP_SD_HIGHSPEED | MMC_CAP_UHS;
 y2_apply_level(&h, 1); h.mmc.ios.timing = MMC_TIMING_UHS_SDR104;
 struct mmc_command cmd = { .opcode = 18 }; struct mmc_request req = { &cmd, &cmd };
 msdc_ops_request(&h.mmc, &req); assert(completed == 1 && !allowed && cmd.error == -EIO);
 cmd.opcode = 25; msdc_ops_request(&h.mmc, &req); assert(completed == 2 && !allowed);
 cmd.opcode = 0; req.data = 0; msdc_ops_request(&h.mmc, &req); assert(allowed == 1);
 cmd.opcode = 19; req.data = &cmd; msdc_ops_request(&h.mmc, &req); assert(allowed == 2);
 h.mmc.ios.timing = MMC_TIMING_UHS_DDR50; cmd.opcode = 18;
 msdc_ops_request(&h.mmc, &req); assert(allowed == 3 && completed == 2);
 h.y2_clock_error = -EIO; msdc_ops_request(&h.mmc, &req); assert(allowed == 3 && completed == 3);
}
''')


if __name__ == '__main__':
    unittest.main()
