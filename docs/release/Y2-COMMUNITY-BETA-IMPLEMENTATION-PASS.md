# Community beta feature completion implementation pass

Started 2026-10-02. Owner authorizes source work, fresh software validation and one
preserving BOOTIMG/Y2ROOT candidate. No flash, push or hardware promotion.

Starting Y2Linux: `9ac2c0b7018ce707ec15f8845daf58ce38ec18a0`.
Starting Reborn: `eb64b8b21058eacb1ef183f7570cca8100cbbb46`.
Existing dirty documentation is retained; snapshot under ignored
`out/feature-completion/starting-*`. All 233 inventory records remain authoritative.

Implementation is in progress across platform (1–4), audio/codecs (5–7),
Reborn/battery (4/8), tester operations and release composition (9–10).
Source implementation, software validation, physical proof and distribution
permission are tracked independently. Detailed lane receipts are linked here as
changes land. Final integrated validation and handoff are pending.

## Source implementation checkpoint

[Platform lane](implementation-platform.md): runtime tuning now renegotiates actual
bus timing, searches every32-tap eye, restores failed latch-clock tuning exactly,
and removes production lab mutations. Invalid cold SRAM evidence is rejected.
Typed sleep reports source-backed same-boot/kernel-exit/Reborn restoration; the
actual full-resume stall is still unlocalized because current4 has no valid PM
record. C2/C3 and loaded USB preserve existing fixes and exact physical gates.

[Audio/codecs lane](implementation-audio-codecs.md): actual ALSA S32 mapping and
constraint intersection fixed; packed24→FFmpeg→DSP→S24/S32 low bits tested. Native
wired24/88.2/96 remain unimplemented pending exact MT6582 fetch/rate evidence.
Private optional endpoints, actual SBC/LDAC telemetry, quality preferences,
synthetic ABR test and bounded history/load-aware Auto are implemented.

[Reborn/power lane](implementation-reborn-power.md): Productv2 EQ, typed sleep,
codec-quality settings, truthful source/output details, estimated SOC and redacted
export; retain ready-frame splash and orderly shutdown. Exact stock fuel-gauge
functions are stubs; no calibrated pack-current/coulomb/NTC path is established.

Tester/release source: separate projected diagnostic archive and verified host
retrieval, fresh empty first-owner seed, integrated guarded SSH qualification,
actual-image privacy scanning and source/configuration capability composition.
[Tester guide](Y2-COMMUNITY-TESTER-GUIDE.md) and
[source/distribution requirements](Y2-COMMUNITY-SOURCE-DELIVERY.md) are included.
Fresh complete kernel/Buildroot/Reborn and final package validation are next;
no candidate or physical readiness is claimed by this checkpoint.
