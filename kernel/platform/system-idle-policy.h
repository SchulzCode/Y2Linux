/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_SYSTEM_IDLE_POLICY_H
#define Y2_SYSTEM_IDLE_POLICY_H
/* Source SLIDLE needs exactly one online CPU. Core parking is a slow system
 * inactivity decision, never made in the cpuidle IRQ-off entry callback.
 *
 * Fix01 reset its 30-s quiet window on any single 250-ms sample above 10%
 * busy and on every cpufreq PRECHANGE above 598 MHz. Physical screen-off
 * windows showed short bursts (0..31% samples, transient 1040 MHz) often
 * enough that no window ever completed. Fix02 measures sustained idle
 * instead: a time-weighted load average (milli-cores, tau 30 s), the
 * fraction of time above 598 MHz, and sustained saturation. Hard gates
 * (screen, workload lease, timer admission, explicit input/display/workload
 * demand) still reset immediately; real sustained demand still restores. */
#define Y2_SYSTEM_QUIET_MS 30000
#define Y2_SYSTEM_PARK_STEP_MS 5000
#define Y2_SYSTEM_RESTORE_HOLD_MS 60000
#define Y2_SYSTEM_HOLD_MAX_MS 960000
#define Y2_SYSTEM_STABLE_PARK_MS 600000
#define Y2_SYSTEM_TAU_MS 30000
/* 10% of four cores, as a sustained average rather than every sample. */
#define Y2_SYSTEM_QUIET_MCORES 400
#define Y2_SYSTEM_HIGH_FREQ_PERMILLE 250
/* Per online core: saturation for one second is real demand. */
#define Y2_SYSTEM_BURST_PERMILLE 850
#define Y2_SYSTEM_BURST_MS 1000
#define Y2_SYSTEM_LOW_KHZ 598000

enum y2_idle_reason {
	Y2_IDLE_NONE,
	Y2_IDLE_SCREEN,
	Y2_IDLE_LEASE,
	Y2_IDLE_TIMER,
	Y2_IDLE_DISABLED,
	Y2_IDLE_DEMAND,
	Y2_IDLE_BURST,
	Y2_IDLE_REASONS,
};
static const char *const y2_idle_reason_names[Y2_IDLE_REASONS] = {
	"none", "screen", "lease", "timer", "disabled", "demand", "burst",
};
enum y2_idle_action { Y2_IDLE_WAIT, Y2_IDLE_PARK, Y2_IDLE_RESTORE };

struct y2_idle_sample {
	unsigned now_ms, dt_ms;
	unsigned busy_permille;	/* of the online capacity */
	unsigned online;
	unsigned khz;
	int dark, lease, timer, allowed, demand;
};
struct y2_idle_state {
	unsigned load_mc;	/* time-weighted busy milli-cores */
	unsigned high_permille;	/* time-weighted fraction above 598 MHz */
	unsigned burst_ms;
	int quiet;
	unsigned quiet_start_ms, hold_until_ms, hold_ms, last_park_ms;
	unsigned parked_since_ms, parked, longest_quiet_ms;
	enum y2_idle_reason last_reset;
	unsigned resets[Y2_IDLE_REASONS];
	unsigned pressure_restores;
};
static inline int y2_idle_after(unsigned now, unsigned then)
{
	return (int)(now - then) >= 0;
}
static inline void y2_idle_init(struct y2_idle_state *s, unsigned now_ms)
{
	*s = (struct y2_idle_state){ 0 };
	s->load_mc = 4000;	/* pessimistic until measured */
	s->high_permille = 1000;
	s->hold_ms = Y2_SYSTEM_RESTORE_HOLD_MS;
	s->hold_until_ms = now_ms + Y2_SYSTEM_RESTORE_HOLD_MS;
}
/* Operands are bounded (<= 4000 milli-cores, <= 30000 ms): 32-bit signed
 * arithmetic cannot overflow and avoids a 64-bit division on ARMv7. */
static inline unsigned y2_idle_ewma(unsigned avg, unsigned value, unsigned dt_ms)
{
	int w = dt_ms < Y2_SYSTEM_TAU_MS ? (int)dt_ms : Y2_SYSTEM_TAU_MS;
	int delta = (int)value - (int)avg;
	return (unsigned)((int)avg + delta * w / Y2_SYSTEM_TAU_MS);
}
static inline void y2_idle_reset(struct y2_idle_state *s, enum y2_idle_reason why)
{
	s->quiet = 0;
	s->last_reset = why;
	s->resets[why]++;
}
/* Called by the owner after it restored every coordinator-owned CPU. A
 * pressure restore soon after parking doubles the hold (no oscillation);
 * a long stable parked period returns the hold to its base value. */
static inline void y2_idle_restored(struct y2_idle_state *s, unsigned now_ms, int pressure)
{
	if (s->parked) {
		int stable = !y2_idle_after(s->parked_since_ms + Y2_SYSTEM_STABLE_PARK_MS, now_ms);
		if (pressure && !stable) {
			s->hold_ms = s->hold_ms >= Y2_SYSTEM_HOLD_MAX_MS / 2 ?
				Y2_SYSTEM_HOLD_MAX_MS : s->hold_ms * 2;
			s->pressure_restores++;
		} else if (stable) {
			s->hold_ms = Y2_SYSTEM_RESTORE_HOLD_MS;
		}
	}
	s->parked = 0;
	s->quiet = 0;
	s->hold_until_ms = now_ms + s->hold_ms;
}
static inline void y2_idle_parked(struct y2_idle_state *s, unsigned now_ms)
{
	if (!s->parked++)
		s->parked_since_ms = now_ms;
	s->last_park_ms = now_ms;
}
static inline enum y2_idle_action y2_idle_decide(struct y2_idle_state *s,
						 const struct y2_idle_sample *in,
						 enum y2_idle_reason *why)
{
	unsigned load = in->busy_permille * in->online;
	*why = Y2_IDLE_NONE;
	s->load_mc = y2_idle_ewma(s->load_mc, load, in->dt_ms);
	s->high_permille = y2_idle_ewma(s->high_permille,
		in->khz > Y2_SYSTEM_LOW_KHZ ? 1000 : 0, in->dt_ms);
	s->burst_ms = in->busy_permille >= Y2_SYSTEM_BURST_PERMILLE ?
		s->burst_ms + in->dt_ms : 0;
	if (!in->allowed) *why = Y2_IDLE_DISABLED;
	else if (!in->timer) *why = Y2_IDLE_TIMER;
	else if (!in->dark) *why = Y2_IDLE_SCREEN;
	else if (in->lease) *why = Y2_IDLE_LEASE;
	else if (in->demand) *why = Y2_IDLE_DEMAND;
	else if (s->burst_ms >= Y2_SYSTEM_BURST_MS) *why = Y2_IDLE_BURST;
	if (*why != Y2_IDLE_NONE) {
		if (s->quiet || s->parked)
			y2_idle_reset(s, *why);
		s->quiet = 0;
		return s->parked ? Y2_IDLE_RESTORE : Y2_IDLE_WAIT;
	}
	if (!y2_idle_after(in->now_ms, s->hold_until_ms))
		return Y2_IDLE_WAIT;
	if (!s->quiet) {
		s->quiet = 1;
		s->quiet_start_ms = in->now_ms;
	}
	if (in->now_ms - s->quiet_start_ms > s->longest_quiet_ms)
		s->longest_quiet_ms = in->now_ms - s->quiet_start_ms;
	if (in->online > 1 &&
	    y2_idle_after(in->now_ms, s->quiet_start_ms + Y2_SYSTEM_QUIET_MS) &&
	    s->load_mc <= Y2_SYSTEM_QUIET_MCORES &&
	    s->high_permille <= Y2_SYSTEM_HIGH_FREQ_PERMILLE &&
	    (!s->parked || y2_idle_after(in->now_ms, s->last_park_ms + Y2_SYSTEM_PARK_STEP_MS)))
		return Y2_IDLE_PARK;
	return Y2_IDLE_WAIT;
}
#endif
