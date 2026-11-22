# Y2Linux 1.0 Beta 9.3.38: two new skins, and Bluetooth off means off

Release `1.0.0-beta.9.3.38`, Y2Reborn `1.0.0-beta.9.3.38`, kernel `6.18.56-y2linux-1.0.0-beta.9.3.38`, update
sequence 17800, root `2026.08-platform-v1.296`. Published as GitHub release `baseline-273` (build 273). Previous
release: [Beta 9.3.37](Y2-LINUX-1.0-BETA-9.3.37.md).

This report is written for people who use the player. A short technical appendix is at the end.

## In one paragraph

Two new skins come with the player, Pixel Quest and Gilded Grove, both drawn for this release. And a fix from an
error report ([Y2Linux #136](https://github.com/SchulzCode/Y2Linux/issues/136)): after you turned Bluetooth off,
Play could still say "Waiting for …" for your headphones.

## What changed

- **Pixel Quest** (Settings → Skin): a pixel-art adventure at dusk. A sky with stars, a moon and mountains behind
  every screen; bevelled windows with gold gems; gold name plates for titles and the track; a gem cursor on a gold
  bar; Now Playing as a dialogue box with the artist as the speaker.
- **Gilded Grove** (Settings → Skin): a storybook forest. A glade with light shafts and fireflies; gold-framed
  panels with leaves and emerald gems; a laurel crest under the title; the cover in a glowing gold medallion; an
  emerald meter; elegant serif type.
- **Bluetooth off is off** ([#136](https://github.com/SchulzCode/Y2Linux/issues/136)): turning Bluetooth off
  disconnects your headphones, and the player took that for headphones that dropped out. Play then waited for them
  ("Waiting for …") for up to 20 seconds, and again after the player woke within two minutes. Now nothing waits for
  headphones while Bluetooth is switched off, however you switched it (Settings, the Quick Menu or a button).

## How it was tested

- **Tests**: the platform's full check, Reborn's tests and the tests of the release images passed. New tests: the
  two skins load without warnings and have the clock, the keyboard, live radio, status icons, track numbers, the
  sleep timer and Cover Flow; a loss while Bluetooth is switched off leaves nothing to wait for.
- **Previews**: all 226 screens of both skins rendered on a computer and looked through.
- **On the developer's player** (build 273): both skins are installed and load without problems; playing on
  AirPods, Bluetooth off, then Play: the music went on at once on the speaker, without "Waiting for …".
- **Not tested on a player**: how the two skins look on the player's own screen.

## Updating

Settings → System → Software Update → **Update Now**. Your music, playlists, settings, Wi-Fi networks and Bluetooth
pairings stay. **Restore Previous System** (Settings → System → About, tap Version seven times, then Diagnostics) goes
back to the version you had.

## Known issues

- When headphones connect the moment Bluetooth turns on, it can take up to about 10 seconds before sound goes to
  them (#136): wait a moment, or press Play, rather than switching Bluetooth off and on.
- Everything listed for [Beta 9.3.34](Y2-LINUX-1.0-BETA-9.3.34.md#known-issues) still applies.

## Technical appendix

- Skins: `tools/assets/build_skins.py` `pixel_quest()` and `gilded_grove()` (all pictures generated; Pixel Quest's
  pixel art is drawn at its stored size because the skin atlas is filtered linearly; Pixelify Sans and Noto Serif /
  Noto Serif Display, SIL OFL 1.1). 19 shipped skins.
- #136: `LostPeer::after_loss` creates no lost peer while `Runtime::bluetooth_off_by_user` is set (Effect
  `BluetoothPower`, `rebornctl bluetooth off`); `awaits_lost_peer` is false then. Report AA96E00A: Bluetooth off at
  18:30:11, the radio's `peer_disconnected` after it, Play waited at 18:35:52 after a wake at 18:34:08.
- Images built from Linux `e58737ef`, Reborn `a00d530`.
