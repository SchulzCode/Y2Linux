# Audio and FM evidence

Date: 2026-09-08. Related task: Y2E-101; archived evidence reviewed, no active audio investigation in this session.

CONFIRMED in the integrity-verified `2026-07-29_004935` snapshot: `1-0030` binds `cs43131_dac`, `1-0058` binds `aw87559_pa`, `/proc/asound/cards` reports no soundcards. These observations identify a vendor-controlled system; `/dev/cs43131_dac` is not established as a PCM sink.

The exact archived stock primary HAL hashes to `5c5162f6a68f7db57febd050ee88cc886779dcce5948937149d6cd211eb0e6de`. Later `Y2_AUDIO_PATH_PHASE4_SECOND_I2S.md` and `out/afe-runtime/2026-07-30_135519/` supersede parts of earlier primary-I2S analysis and support a second-I2S/DL1 route. Treat the complete electrical/analog route, PMIC role, actual slot width, supported rates and analog performance as INFERRED/UNKNOWN until their specific evidence is reviewed. Android's observed 44.1-kHz PCM16 output does not set a Linux hardware maximum.

The current generated boot manifest records a CS43131 idle-resume kernel patch. Its installed presence is UNKNOWN. Avoid conflating that artifact with the exact original kernel analyzed offline.

FM feasibility reports record chip ID 0x6627 and tuning/routing experiments. A successful open/tune is not evidence of useful RF reception or a populated antenna path on this unit. Release notes distinguish hardware revisions, but the current unit's revision and original manufacturer evidence have not been established here.

Next passive proof: re-identify bound audio devices on the current build and reconcile exact HAL/kernel/firmware lineage. Later pin down reset/IRQ/supply/mute/clock ordering and headphone/speaker/PMIC topology before designing ASoC work. No raw node writes, guessed ioctls, live register dumps, bus scans, playback/rate changes or kernel patches are authorized by this document.
