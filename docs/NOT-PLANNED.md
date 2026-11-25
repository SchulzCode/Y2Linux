# Ideas that won't be built, and why

Thank you for every idea. Some have been asked for more than once and won't be built, for the reasons below. If
yours is on this list, please don't open a new request for it; if you have a new argument or a different way to
do it, comment on the issue linked there instead.

Something missing here may still be a duplicate: please search the open and closed issues of
[Y2Linux](https://github.com/SchulzCode/Y2Linux/issues?q=is%3Aissue) and
[Y2Reborn](https://github.com/SchulzCode/Y2Reborn/issues?q=is%3Aissue) first.

## Battery and charging

| Idea | Why not | Issue |
|---|---|---|
| Battery percentage on the charging screen while the player is off | The Y2 can't measure the charge going in or out, so the percentage is estimated from the battery's voltage. While charging, that voltage reads too high; the running player corrects for it, a switched-off one can't. The number would be wrong most of the time. Switch the player on to see the level. | [Y2Reborn #72](https://github.com/SchulzCode/Y2Reborn/issues/72), [Y2Linux #109](https://github.com/SchulzCode/Y2Linux/issues/109) |
| Stop charging at 80 % or 90 % | For the same reason the percentage isn't precise enough for a charge limit to mean what it says. | [Y2Reborn #64](https://github.com/SchulzCode/Y2Reborn/issues/64) |
| Battery level of Bluetooth headphones | Most headphones report it only over the phone-call side of Bluetooth, which the player doesn't run: a second connection to every pair of headphones makes Bluetooth less steady. Without it only a few would show a level (AirPods never). | [Y2Reborn #95](https://github.com/SchulzCode/Y2Reborn/issues/95) |
| A battery history with drain per mode and time left | The level is estimated from the battery's voltage, which swings with load; a history would look precise, but drain per mode and time left would be guesses. | [Y2Reborn #148](https://github.com/SchulzCode/Y2Reborn/issues/148) |

## Buttons and wheel

| Idea | Why not | Issue |
|---|---|---|
| Double tap on the centre button | To tell a double tap from a single one, the player would have to wait after every tap, which makes the button slower everywhere. Tap selects, hold opens the menu. | [Y2Reborn #98](https://github.com/SchulzCode/Y2Reborn/issues/98) |
| Wake the player with the centre button or the wheel | Only Power wakes it, so it can't wake in a pocket or bag. What the other buttons do with the screen off is in Settings → Customize → Buttons → Screen Off. | [Y2Linux #71](https://github.com/SchulzCode/Y2Linux/issues/71) |
| A setting for the wheel's seek step | The wheel already seeks faster the faster you turn it. | [Y2Reborn #50](https://github.com/SchulzCode/Y2Reborn/issues/50) |
| A setting for button debounce | If buttons bounce on a player, that gets fixed for everyone rather than turned into a setting. Send an error report if you see doubled presses. | [Y2Reborn #54](https://github.com/SchulzCode/Y2Reborn/issues/54) |
| Letter scrolling by a double tap on the wheel | Telling a double tap from a touch needs a new wheel driver and makes every single touch wait. Spinning quickly already steps by letter. | [Y2Reborn #88](https://github.com/SchulzCode/Y2Reborn/issues/88) |
| Fast scrolling by two letters (Aa, Ab, Ac…) | Most libraries have few items under each pair; a spin would step through dozens of tiny groups. One letter per detent, then row by row. | [Y2Linux #114](https://github.com/SchulzCode/Y2Linux/issues/114) |
| Keep the highlight at the bottom while letter-scrolling down | Each letter's first row goes to the top, both directions; the bottom felt clunky ([Y2Reborn #91](https://github.com/SchulzCode/Y2Reborn/issues/91)). | [Y2Reborn #142](https://github.com/SchulzCode/Y2Reborn/issues/142) |

## Menus and screens

| Idea | Why not | Issue |
|---|---|---|
| Reorder the rows of a menu | Every menu lists its actions in the same order, so each action is in the same place wherever you are. (The places on Home can be reordered: Settings → Customize → Home Menu.) | [Y2Linux #66](https://github.com/SchulzCode/Y2Linux/issues/66), [Y2Reborn #98](https://github.com/SchulzCode/Y2Reborn/issues/98) |
| Play state and track time in the standard status bar | The standard bar stays clock, Wi-Fi, Bluetooth and battery. A skin can show both anywhere: see the [skin guide](https://github.com/SchulzCode/Y2Reborn/blob/main/docs/skins.md). | [Y2Linux #69](https://github.com/SchulzCode/Y2Linux/issues/69) |
| Choose what Now Playing shows (for example hide the "Lossless" line) | What a screen shows is what skins are for. A skin can lay out only Now Playing and keep Reborn's own look everywhere else: copy the Now Playing of [Ember](https://github.com/SchulzCode/Y2Reborn/tree/main/docs/skin-example/Ember), which is close to Reborn's, and leave out the `{quality}` text (see the [skin guide](https://github.com/SchulzCode/Y2Reborn/blob/main/docs/skins.md)). | [Y2Linux #131](https://github.com/SchulzCode/Y2Linux/issues/131) |
| More Cover Flow and artist display modes | Different looks are what skins are for; the player itself keeps one. | [Y2Reborn #65](https://github.com/SchulzCode/Y2Reborn/issues/65) |
| Sort options for Audiobooks | One order: in progress first, then new ones A to Z, then finished. | [Y2Reborn #27](https://github.com/SchulzCode/Y2Reborn/issues/27) |
| A visual skin editor | A whole application of its own that would have to follow every change of the skin format. Start from a shipped skin instead. | [Y2Reborn #115](https://github.com/SchulzCode/Y2Reborn/issues/115) |
| A timer or stopwatch | Reborn is a music player; the Sleep Timer covers falling asleep. | [Y2Reborn #121](https://github.com/SchulzCode/Y2Reborn/issues/121) |
| Notes (writing down ideas on the player) | Reborn is music-first; writing with a wheel and an on-screen keyboard is slow, and a notes app is a different kind of program. | [Y2Reborn #203](https://github.com/SchulzCode/Y2Reborn/issues/203) |
| Schedules for skins and brightness | Many settings for one use. A light Reborn skin is planned (Y2Linux #112); switching stays in Settings → Skin. | [Y2Reborn #89](https://github.com/SchulzCode/Y2Reborn/issues/89) |
| A text size, bold or font setting | Every screen is laid out with fixed text sizes for the 480 × 360 screen; larger text everywhere would cut off titles, rows and values on every screen. Skins choose their own fonts and sizes: the **Big Type** skin (Settings → Skin) has much larger, high-contrast text, and a skin can use any free font, such as OpenDyslexic, in bold or at a larger size (see the [skin guide](https://github.com/SchulzCode/Y2Reborn/blob/main/docs/skins.md)). | [Y2Reborn #59](https://github.com/SchulzCode/Y2Reborn/issues/59), [Y2Linux #77](https://github.com/SchulzCode/Y2Linux/issues/77), [#146](https://github.com/SchulzCode/Y2Linux/issues/146) |
| The equalizer in the Quick Menu instead of the Sleep Timer | The Quick Menu holds what you change on the go; the equalizer is set once, in Settings → Sound. | [Y2Reborn #159](https://github.com/SchulzCode/Y2Reborn/issues/159) |

## Sound and playback

| Idea | Why not | Issue |
|---|---|---|
| More volume or gain for Bluetooth, or with an equalizer profile | The player lowers the level just enough that the profile's highest boost can't distort. Louder would mean distortion; a profile with smaller boosts keeps more volume. | [Y2Linux #80](https://github.com/SchulzCode/Y2Linux/issues/80) |
| Playing above 44.1 kHz on the headphone jack (96 kHz and up, "bit perfect") | The Y2's chip sends audio to the DAC at 44.1 or 48 kHz only, and 48 kHz needs the DAC's PLL, which stopped the crystal on some players ([Y2Linux #52](https://github.com/SchulzCode/Y2Linux/issues/52)); the stock firmware plays everything at 44.1 kHz too. Higher rates are converted down; 24-bit depth is kept. | [Y2Reborn #122](https://github.com/SchulzCode/Y2Reborn/issues/122) |
| Crossfade or gapless on a manual skip | A skip should be immediate. Gapless and crossfade apply when one song runs into the next. | [Y2Linux #65](https://github.com/SchulzCode/Y2Linux/issues/65) |
| More Bluetooth range or steadier Bluetooth (dropouts in a pocket, at a distance or in crowded places; AirPods silent after a dropout) | Every Bluetooth report was checked again for Beta 9.3.34: the player's side is clean, and it already does what a phone does (adaptive rate, staying off the Wi-Fi channel, Wi-Fi resting while music plays, bounded catch-up). What remains is the Y2's one small radio, which shares its antenna with Wi-Fi. For listening without dropouts, use wired headphones. | [Y2Linux #99](https://github.com/SchulzCode/Y2Linux/issues/99), [#129](https://github.com/SchulzCode/Y2Linux/issues/129), [#130](https://github.com/SchulzCode/Y2Linux/issues/130), [Y2Reborn #132](https://github.com/SchulzCode/Y2Reborn/issues/132) |
| A Shuffle of its own per list, or a default Shuffle per kind of list | One Shuffle setting (Now Playing's menu: Off, Songs, Albums) decides how lists play; an album always plays in order, and every list has Shuffle All for a shuffle just this once. | [Y2Reborn #63](https://github.com/SchulzCode/Y2Reborn/issues/63) |
| Previous goes to the song above in the list after Shuffle is turned off | Previous goes back through what you actually heard, the History at the top of Up Next, the same everywhere. Turning Shuffle off changes only what comes next. | [Y2Reborn #160](https://github.com/SchulzCode/Y2Reborn/issues/160) |
| A "random" button in the lists, and stop at the end of an album | Shuffle Albums plays whole albums in random order, each in its own order; Music's Recently Added, Recently Played and Most Played help pick something. | [Y2Reborn #108](https://github.com/SchulzCode/Y2Reborn/issues/108) |
| Smart playlists with rules | A rule editor is a lot to work through with a wheel. Music's Recently Added, Recently Played and Most Played cover what they are mostly used for. | [Y2Reborn #110](https://github.com/SchulzCode/Y2Reborn/issues/110) |
| One audiobook file longer than about 30 hours | Rare, and it would need a new way to read the book's index in pieces. Settings → Library says when a file is too long; books split into parts play with chapters. | [Y2Linux #84](https://github.com/SchulzCode/Y2Linux/issues/84) |

## Wi-Fi

| Idea | Why not | Issue |
|---|---|---|
| WPA3-only Wi-Fi networks | The Y2's Wi-Fi chip is too old for WPA3. A "WPA2/WPA3 mixed" setting on the router works. | [Y2Linux #91](https://github.com/SchulzCode/Y2Linux/issues/91) |
