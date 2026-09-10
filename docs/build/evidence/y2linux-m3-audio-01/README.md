# Retained packaging metadata

result.json, SHA256SUMS and rescue-manifest.json are unchanged copies from the
ignored output directory at packaging time. Their physical_status/deployment
fields are historical, not the current live state. SHA256SUMS names artifacts
in that original output directory; binaries remain ignored, not duplicated here.
Current deployment and listening evidence is in
../../../knowledge/m3-audio-01-live-result.md and the session handoff.
No build or artifact hash was regenerated during session close.
