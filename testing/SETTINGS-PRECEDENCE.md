# Which settings source the game actually obeys

Measured, not inferred. Two sources describe the game's window:

- `%AppData%\LocalLow\ZAUM Studio\Disco Elysium\Settings\Settings.json` -
  `resolutionWidth`, `resolutionHeight`, `DISPLAY MODE`
- `HKCU\Software\ZAUM Studio\Disco Elysium` - Unity's own PlayerPrefs,
  `Screenmanager Resolution Width` / `Height` / `Fullscreen mode` /
  `Resolution Use Native`

## The answer

| | decided by |
|---|---|
| resolution the window OPENS at | registry |
| window style it OPENS with | registry |
| final resolution | **the LARGER of the two sources** |
| final window style | settings file - `DISPLAY MODE`, applied during startup |

Neither source simply wins. The window opens at the registry's size, and during
startup the game applies the settings file - but the resolution only ever grows.
Ask for something smaller than the window already is and nothing happens; ask
for something larger and it takes effect.

So **to run at a small resolution, BOTH sources must be set**. Either one alone
is enough to be ignored: the registry alone opens small and then the file
enlarges it, and the file alone is ignored because the registry opened larger.

A fullscreen style carries its own resolution on top of this: Unity's
`FullScreenWindow` is documented as using the native display resolution, so a
`DISPLAY MODE` that switches the window to borderless makes the size the
desktop's whatever either source asked for.

### Mechanism not established

`max(file, registry)` describes every measurement, but so does "the resolution
never shrinks after the window opens" - the two cannot be told apart here,
because the window always opens at the registry's size. Whether the comparison
is per-dimension or by area is also untested; every case tried had one
resolution larger in both dimensions.

## How to set the resolution for a test

Write both, to the same values. `GameSettings.Install` stages the file and
`InstallScreenPrefs` writes the registry; `Backup`/`Restore` put the player's
values back.

All four registry values have to move together. `Resolution Use Native` is the
one that catches people out: while it is 1, Unity takes the display's resolution
and ignores the stored width and height, so editing only width and height
changes nothing.

    Screenmanager Resolution Use Native   -> 0
    Screenmanager Fullscreen mode         -> 3   (Unity's Windowed)
    Screenmanager Resolution Width        -> 1280
    Screenmanager Resolution Height       -> 720

Match the file's `DISPLAY MODE` to the registry's mode as well, or the game
switches the style partway through startup - harmless in itself, but it moves
the window under a test that is watching it.

### Two numbering schemes, both using 1, meaning opposite things

    Settings.json  DISPLAY MODE 0 = fullscreen   1 = windowed
    registry       Fullscreen mode 0 = exclusive, 1 = BORDERLESS FULLSCREEN,
                                    2 = maximised, 3 = windowed

The registry follows Unity's `FullScreenMode` enum; the file follows the game's
own options dropdown. Crossing them produces a full-screen window where a small
one was asked for.

## Registry value names

Unity suffixes each key with `_h` and a DJB2 hash of the key name:

    uint hash = 5381;
    foreach (char c in name) hash = hash * 33 ^ c;

Depends on the name alone, so it is stable and worth computing rather than
copying - a mistyped hash does not fail, it writes a value Unity never reads and
silently does nothing. `UnityPlayerPrefs.ValueName` computes it, and its tests
check the result against four names taken from a real installation.

## The experiments

Six launches, each recording the window's size and style every four seconds from
launch to main menu.

| # | launch | settings file | registry | opened | ended |
|---|---|---|---|---|---|
| 1 | exe | 1280x720 windowed | 3840x1200 borderless | 3840x1200 borderless | 3840x1200 windowed |
| 2 | Steam | 1280x720 windowed | 3840x1200 borderless | 3840x1200 borderless | 3840x1200 windowed |
| 3 | exe | 3840x1200 fullscreen | 1280x720 windowed | 1280x720 windowed | 3840x1200 borderless |
| 4 | Steam | 3840x1200 fullscreen | 1280x720 windowed | 1280x720 windowed | 3840x1200 borderless |
| 5 | exe | 1280x720 windowed | 1920x1080 windowed | 1920x1080 windowed | 1920x1080 windowed |
| 6 | exe | 1920x1080 windowed | 1280x720 windowed | 1280x720 windowed | 1920x1080 windowed |

Reading them:

- **1 and 2**: the style becomes windowed while the size stays at the registry's
  3840x1200. The file's `DISPLAY MODE` was obeyed, its smaller resolution was
  not.
- **3 and 4** end at 3840x1200, but the style is borderless and borderless means
  native, so these say nothing about resolution on their own.
- **5**: both sources ask for a window and differ only in size. The game ran at
  the registry's larger 1920x1080 throughout and never went near the file's
  1280x720.
- **6** is the one that matters. Same shape as 5 with the sizes swapped, so the
  file now asks for the LARGER window - and it opened at the registry's 1280x720
  and grew to the file's 1920x1080.

Runs 1 to 5 were all consistent with "the registry always wins", and that was
the first conclusion drawn from them. It was wrong: in every one of those cases
the registry's value was also the larger value, so the two explanations could
not be told apart. Run 6 separates them, and the file wins when it asks for
more.

### The same confound caught an earlier manual test

Before these runs, hand testing had concluded the opposite - that the settings
file wins and the registry is only a backup. That test had the file asking for
the larger resolution, so "the file wins" and "the larger wins" predicted the
same outcome, exactly as runs 1 to 5 made "the registry wins" look right for the
reverse reason. Two opposite conclusions, both from data that could not
distinguish them.

That earlier test also observed something these runs cannot: after launch, the
registry had been updated to match the resolution the game settled on. Every run
here restores the registry afterwards, so the write-back is invisible to them.
It is consistent with everything measured, and it is the reason the registry has
to be backed up at all - a run changes it as a side effect.

### The launch method makes no difference

Runs 1 and 2 are the same configuration through `disco.exe` and through
`steam://run/632470`; so are 3 and 4. Both pairs match, including the timing of
the style change to within a tenth of a second. Whatever else differs between
the two launch paths - Steam's cloud sync does - it does not change which
settings source the game honours.

## Timings, for anything waiting on startup

Consistent across all six runs:

    0-4s     first screen
    4-12s    horse statue and LOADING
    12-37s   black legal notice, about 25 seconds, most of startup
    37-46s   logo, menu assembling
    46s+     main menu

The settings file's `DISPLAY MODE` is applied around 37s when it enlarges the
window, and around 13-17s when it switches to borderless.

Two properties of that sequence defeat the obvious ways to detect "loading has
finished", and both are measured:

- The legal notice holds still for 25 seconds at a frame-to-frame difference
  around 0.0002, far below any sensible threshold for "the screen stopped
  changing". Waiting for stillness settles there, less than halfway through.
- Its detail is around 0.008, below the 0.02 floor that distinguishes a painted
  window from an unpainted one. A wait STARTING during the notice would read the
  game as a window that has not drawn yet.

The main menu animates, at differences of 0.006 to 0.05, so it never goes still
either. Matching a reference image is the only approach left.
