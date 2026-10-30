# Archived tools

Development scripts kept for the record of how earlier measurements were made. None of them works against
Y2Linux 9.x: each runs Python on the player or loads the Python platform (`/usr/lib/y2-platform`), and the
player has had neither since Beta 9.0, when the platform was rewritten in Rust. Nothing in the build, the tests
or the release tooling uses them. Paths below `tools/archive/` mirror where they lived under `tools/`.

| Script | What it measured |
| --- | --- |
| `development/measure-bluetooth-idle.py` | Bluetooth idle wake-ups and CPU (Python sampler piped to the player) |
| `development/qualify-cpu-fix01.py` | the first CPU-idle fix's qualification (superseded by fix02/fix03) |
| `development/power-session/arm-suspend-probe.py` | starts `suspend-probe.py` on the player for an armed suspend |
| `development/power-session/deepsleep-*-session.py` | deep-sleep cycle, diagnostic, idle, idle follow-up and suspend sessions through the Python platform's suspend qualification |
