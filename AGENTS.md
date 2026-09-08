# Y2Linux working rules

Before activating, closing or crossing a major milestone boundary, or materially
changing hardware/memory/production scope, repeat the roadmap/gap audit using
real hardware evidence and update the roadmap before proceeding. Follow
[the standing audit procedure](docs/planning/roadmap-gap-audit.md#standing-milestone-boundary-rule).
Do not treat a planning epic as implementation or hardware authorization.

Use targeted validation for localized changes. Reserve broader qualification
for architecture/memory/packaging changes, new hardware subsystems, milestone
qualification and release candidates. A documentation-only audit does not build
or flash anything and does not repeat unchanged source/ROM/recovery provenance.

Preserve the existing configured Git author/committer identity and authenticated
GitHub account. Do not modify identity settings or add model attribution.
