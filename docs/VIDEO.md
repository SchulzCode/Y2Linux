# Videos on the Y2

Status: **in public releases since Beta 9.3.25 (2026-11-12), as an early version: built and tried on one player;
expect bugs.** This page says what it does and what it does not do yet. How it is built:
[video playback plan](planning/VIDEO-PLAYBACK-PLAN.md); what the hardware decoder does:
[MT6582 hardware video decoder](knowledge/video-decoder.md).

## Using it

Copy video files onto Internal Storage or the SD card, like songs (USB file transfer, or the card in a
computer). Any folder works; a folder of your own such as `Videos` keeps them together. After the next scan **Videos** stands on Home. A film plays on Now Playing: the
picture fills the screen, and a button press shows its title and a time line for three seconds (always while
paused). Play/Pause, Previous/Next, the wheel and the volume work as they do for a song, and the sound goes to
whatever output is chosen. A film goes on where it was left. The screen stays on while a film plays.

Subtitles show when the file carries a text track, or when a subtitle file with the same name lies beside it
(`Film.mkv` and `Film.srt`, `.ass`, `.ssa` or `.vtt`).

Films never appear among the songs, in the albums or artists, in shuffles of the library or on Last.fm.

## What plays

| | |
|---|---|
| Containers | `.mp4` `.m4v` `.mov` `.mkv` `.webm` `.avi` |
| Picture, on the hardware decoder | H.264 (Baseline, Main, High), frames and fields (interlaced too), 4:2:0, 8 bit, up to 1920×1088; MPEG-4 Part 2 (Xvid, DivX 4/5: Simple and Advanced Simple profile), progressive, up to 1920×1088 |
| Picture, in software | H.264 the hardware refuses (other chroma formats, 10 bit), MPEG-4 Part 2 the hardware refuses (interlaced, global motion compensation, data partitioning), DivX 3, H.263, VP8, VP9 |
| Sound | what songs use (AAC, MP3, FLAC, Vorbis, Opus, ALAC, PCM), and AC-3 |
| Subtitles | SubRip, ASS/SSA, WebVTT, MP4 timed text; shown as plain text |
| Not played | HEVC (H.265), AV1, MPEG-2, VC-1/WMV, DTS sound, picture subtitles (DVD, Blu-ray), files with a picture and no sound. A film whose picture format the player cannot show stands in Videos marked "Can't play · HEVC (H.265)" (or its format), never among the songs; convert it as below. |

The screen is 480×360, so every picture is shrunk to it; a 1080p file looks no better than a 480p file and
costs more battery and storage.

## The best file, and converting

MP4 with H.264 video (8 bit) and AAC sound, 640×360 (480×360 for 4:3 films), up to 30 pictures a second, about
1 Mbit/s: roughly 8 MB a minute, a two-hour film under 1 GB. With [FFmpeg](https://ffmpeg.org):

```
ffmpeg -i film.mkv -vf "scale=-2:360" -r 30 -c:v libx264 -profile:v high -pix_fmt yuv420p -crf 23 -c:a aac -b:a 160k film.mp4
```

For a 4:3 film use `scale=480:360`. In [HandBrake](https://handbrake.fr): preset Fast 480p30, format MP4,
encoder H.264, height 360 under Dimensions (settings named from its preset list, not tried here). Keep
subtitles as a `.srt` file with the film's name or as a text track. Convert films in H.265/HEVC or AV1, films
with DTS sound only, and large VP9 files; films in H.264 or Xvid/DivX up to 1920×1080 play as they are. On a
FAT32 card a file cannot be larger than 4 GB.

## Checked on the owner's player (test build 243)

- The hardware decoder's pictures are byte-identical to FFmpeg's software decoding: 95 pictures of six streams
  through the driver alone, 780 pictures of nine files through FFmpeg (details in the decoder document).
- By screen captures from the player: Videos on Home, the list with a picture of each film, films on Now
  Playing, the paused view with title and time line, seeking, going on where a film was left, subtitles from a
  `.srt` file. The time code burnt into the test film (4.13 s) lay within the sound position reported around
  the capture (4.0 to 4.6 s).
- Four test films, ten seconds each, pictures shown / dropped, and the time one picture costs:

  | Film | Decoded by | Shown / dropped | Decoding | Shrinking and colour |
  |---|---|---|---|---|
  | H.264 High 1280×720, 30 fps | hardware | 307 / 9 | 4.2 ms | 17 ms |
  | H.264 High 1920×1080, 24 fps | hardware | 238 / 0 | 12 ms | 22 ms |
  | MPEG-4 720×576, 25 fps | software | 263 / 0 | 0.5 ms in the reader (the rest on four decoder threads, 13 % of a core) | 19 ms |
  | VP9 640×360, 25 fps | software | 270 / 0 | 0.4 ms in the reader (decoder threads 23 % of a core) | 16 ms |

  That table is test build 243. Since then (test build 245) shrinking and colour run in NEON: measured alone
  on the player at full clock, 7.0 ms for a 1280×720 picture (was 11.3), 10.6 ms at 1920×1080 (was 16.7),
  5.7 ms for 640×360. What is left is mostly fetching the picture from memory. The same four films on build
  245, ten seconds each: no picture dropped in any of them, and all of Reborn used 61 % (720p H.264), 64 %
  (1080p H.264), 78 % (MPEG-4 576p) and 80 % (VP9 360p) of one core.

  Software MPEG-4 depends on the material: the flat test pattern decodes in 1.7 ms a picture at 720×576, a
  noisy 720×576 film at about 3 Mbit/s in 4.7 ms, a noisy 1280×720 film at about 6.5 Mbit/s in 10.8 ms (on the
  four decoder threads).

  Since test build 246 MPEG-4 Part 2 decodes on the hardware as well. Hardware against software, picture by
  picture on the player (`vdec-ffmpeg`, ten films, 199 to 499 pictures each): the hardware has its own inverse
  transform, as the standard allows, so pictures are not identical; no sample was further off than 5 levels
  (of 255), with Xvid's B pictures, quarter-sample motion, its own quantiser matrices and FFmpeg's
  multi-slice files. One picture takes the decoder 2.0 ms at 720×576 and 1.5 Mbit/s, 4.2 ms at 1280×720 and
  3 Mbit/s, 11 ms at 1920×1080 and 22 Mbit/s, on one thread that mostly waits. In Reborn an Xvid film at
  1280×720 used 57 % of one core in all, no picture dropped; the 720×576 pattern above 59 % (78 % in software).
  A film made with global motion compensation or as interlaced fields is taken by the software decoder as
  before.

  All of Reborn used 77 to 87 % of one of the four cores while a film played (build 243); the governor ran that core below
  its full clock, so the times above are longer than the work needs. The decoder's power goes off 200 ms after
  the last picture (seen while paused) and the device is closed when Now Playing is left.

## Not done yet

- Sleep and wake with a film open have not been tested (it needs a hand on the Power button); battery use while
  a film plays has not been measured (the player was on the cable). Shrinking the picture is the largest cost
  and is plain C; it has room to get faster.
- Damaged files, very long films and long-term reference pictures have not been tried.
- Picture and sound are aligned by the position the player reports; nobody has judged lip sync by eye and ear
  yet. Over Bluetooth the sound is later than the picture by the headphones' own delay (not corrected, by the
  owner's decision for the first version).
- Interlaced MPEG-4 Part 2 stays in software: the hardware decodes its I and P pictures, but some macroblocks
  of its B pictures come out wrong and the cause was not found. FFmpeg's software decoder itself gives
  slightly different B pictures for such films depending on its thread count (seen on the PC too).
- No choice of subtitle or sound track (the first text track and the default sound track are used), no switch
  to turn subtitles off.
- Skins draw their own lists; the film itself always uses Reborn's own picture screen.
- A file with pictures and no sound is refused.
