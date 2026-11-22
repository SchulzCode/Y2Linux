# Y2Linux 1.0 Beta 9.3.39: the cover stays while you seek, and films that can't play say so

Release `1.0.0-beta.9.3.39`, Y2Reborn `1.0.0-beta.9.3.39`, kernel `6.18.56-y2linux-1.0.0-beta.9.3.39`, update
sequence 18000, root `2026.08-platform-v1.298`. Published as GitHub release `baseline-275` (build 275; test build 274
came before it on the developer's player). Previous release: [Beta 9.3.38](Y2-LINUX-1.0-BETA-9.3.38.md).

This report is written for people who use the player. A short technical appendix is at the end.

## In one paragraph

Two fixes. Seeking within a song (holding Next or Previous) no longer blanks the cover and no longer flashes
"Starting…" ([Y2Reborn #151](https://github.com/SchulzCode/Y2Reborn/issues/151), fixed by jonasevcik in
[#154](https://github.com/SchulzCode/Y2Reborn/pull/154)). And a film in a picture format the player cannot show
(HEVC/H.265, AV1 and others) is no longer listed among your songs: it stands in Videos and says it can't play.

## What changed

- **The cover stays while seeking** (#151, thanks to jonasevcik): the cover on Now Playing and Home turned into the
  music note for about half a second after every seek, and the quality line showed an orange "Starting…". Now both
  stay, also when Play continues a paused song. The short blank when a *different* song starts is not changed yet.
- **The restored song's cover survives Play** (#150): the cover shown for the song a restart brought back now stays
  when you press Play, and it is fetched again once the SD card is ready.
- **Films the player cannot show** (reported on Reddit): an MP4 or MKV with an HEVC (H.265) picture, the usual format
  of anime episodes, was read as its sound alone and appeared among the songs. Now it is a film: Videos lists it as
  "Can't play · HEVC (H.265)" (or AV1, MPEG-2, …), and selecting it says to convert it to H.264 MP4 instead of
  playing. It never appears among songs, albums, shuffles or on Last.fm. After the update the player reads every
  file's details once in the background, so files already in your library move to Videos by themselves.

## How it was tested

- **Tests**: the platform's full check, Reborn's tests and the tests of the release images passed. New tests: the
  same song keeps its cover and another song, another occurrence of it or a station logo do not; "Starting…" only
  for a song opened for the first time; an HEVC film is read as a film without a picture, an H.264 film as a film,
  a song's embedded cover stays a song; the Videos note and message.
- **On the developer's player**, the screen filmed at 10 pictures a second while seeking in a 3-minute MP3 with an
  embedded cover: Beta 9.3.38 showed the note instead of the cover in 12 of 40 pictures (three seeks); with the fix
  0 of 40, and 0 of 50 with "Starting…" never shown when seeking like a held button. An HEVC test film was read as
  `hevc/aac`, listed in Videos as "Can't play · HEVC (H.265)", and selecting it showed the message and played
  nothing.
- **Not tested on a player**: AV1 and the other formats (they take the same path), a library of thousands of songs
  being read again after the update.

## Updating

Settings → System → Software Update → **Update Now**. Your music, playlists, settings, Wi-Fi networks and Bluetooth
pairings stay. **Restore Previous System** (Settings → System → About, tap Version seven times, then Diagnostics) goes
back to the version you had.

## Known issues

- After the update the player reads every file's details once in the background (as after Beta 9.3.33's year);
  on a very large library the first minutes are a little slower.
- When a new song starts, its cover still takes a moment to appear.
- Everything listed for [Beta 9.3.34](Y2-LINUX-1.0-BETA-9.3.34.md#known-issues) still applies.

## Technical appendix

- #151 / #154 (jonasevcik, rebase-merged): `Runtime::art_owner` (queue entry + library id) and
  `runtime::keeps_art`: `load()` keeps `art` for the same owner; `AppModel::reopening` set by `apply_seek`, ended by
  `TrackStarted`, a boundary or `load()`; `views_playing::starting`. #150 follow-up: `restored_picture` returns
  the owner, `track_art::sources_changed`.
- Films: `rb_video_unplayable_codec` (`reborn-media/native/video.c`: HEVC, AV1, VVC, MPEG-1/2, VC-1, WMV1–3,
  MS-MPEG4 v1/v2, Theora, FLV1, VP6, ProRes, RealVideo); `admit_streams` and `extract_metadata` name such a file
  `"<picture>/<sound>"` with no artwork; `Track::video_codec`, `Track::video_playable`; Videos
  `unplayable_note` / `unplayable_message`; `METADATA_REVISION` 6. Fixtures `film-hevc.mp4`, `film-h264.mp4`
  (`tools/build/fixtures.py --films`).
- Images built from Linux `a3794d82`, Reborn `9d2930a`.
