# Buttons: give every button the job you want

Status: **new in Beta 9.3.29 (13 November 2026).** Tried on one player with its real buttons: changing a
button, Save, Keep and the automatic undo, a changed button in the menus and with the screen off, the Quick Menu
tile. Holds of the front buttons and the start-up escape were tested on the computer only.

**Settings → Customize → Buttons** decides what every button, every hold and every turn of the wheel does. The
buttons answer in three places, each with its own page:

- **In Menus**: every list and settings page.
- **On Now Playing**: while something plays there. A button you leave alone here does what it does in the menus.
- **Screen Off**: with the screen dark and the player awake (music playing, or just paused). Only commands that
  make sense in a pocket are offered: play/pause, skip, seek, volume, shuffle, repeat, the sleep timer, or nothing.

**Next Chapter** and **Previous Chapter** move one chapter in an audiobook, however long the chapter is; with
music they skip a song (Y2Reborn #177). They work with the screen off too.

## Changing a button

1. Open a place, for example **In Menus**. The page draws the player. Turn the wheel to pick a button, or simply
   press Previous, Next, Play/Pause or a volume button: it lights up.
2. Press Select. The button's page shows what a **Press** and a **Hold** do (the wheel: a **Turn**).
3. Choose one and pick the new command from the list.
4. Back on **Buttons**, choose **Save Changes**. The player checks the layout, then asks **Keep These Buttons?**
   Choose **Keep** with the new buttons. If you don't (or can't) within 15 seconds, the old buttons come back by
   themselves.

Your first saved layout is called **My Buttons**; you can keep up to four. **Layout** switches between them and the
ones Reborn brings: **Reborn Default** (how the buttons always worked), **Pocket** (screen off: only Play/Pause
and volume), **Wheel Seeks** (on Now Playing the wheel moves through the song) and **One Hand** (screen off: the
wheel is the volume, the centre plays and pauses, the side buttons skip).

**Hold Time** sets how long a press must last to count as a hold (Short, Normal, Long).

## You can't lock yourself out

- **Power is never changed.** A short press sleeps and wakes the player; holding it opens the Quick Menu. Holding
  it for about ten seconds restarts the player, whatever the software does.
- **The Quick Menu** (hold Power) always uses the normal buttons, and while another layout is in use it has a
  **Buttons** tile (**Use Default**): one Select and the buttons are as they came.
- **The Buttons pages** always use the normal buttons too, so the place to fix a layout always works.
- **A layout that would trap you is not saved**: the menus must still be able to select, go back and scroll both
  ways, and Now Playing must have a way out. The player also walks through its menus with the new layout before
  it asks you to keep it.
- **Back held for five seconds** within the first minute after the player starts brings back Reborn Default.
- With **Haptics** on, a button that did something with the screen off gives one short buzz.

## Not there yet

Buttons do nothing while the player is **asleep**: only Power wakes it (the front buttons can't wake the chip as
they are wired up today; see [NOT-PLANNED](NOT-PLANNED.md)). Double presses, sharing layouts as files, and
Bluetooth or wired headphone buttons are not part of this.

How it is built: [plan](https://github.com/SchulzCode/Y2Reborn/blob/main/docs/ui/REBORN-BUTTON-MAPPER.md).
