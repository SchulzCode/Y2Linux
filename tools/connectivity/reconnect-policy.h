/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef Y2_RECONNECT_POLICY_H
#define Y2_RECONNECT_POLICY_H
#include <stdint.h>
struct y2_reconnect_budget { unsigned attempts; int64_t deadline, next; };
static inline void y2_reconnect_reset(struct y2_reconnect_budget *b)
{ b->attempts=0; b->deadline=0; b->next=0; }
static inline int y2_reconnect_attempt(struct y2_reconnect_budget *b, int64_t now)
{
	if(b->attempts>=2 || now<b->next || (b->deadline && now>=b->deadline)) return 0;
	if(!b->deadline) b->deadline=now+25000000;
	b->attempts++; b->next=now+2000000; return 1;
}
#endif
