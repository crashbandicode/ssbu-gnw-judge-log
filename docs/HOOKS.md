# Hook and reads

## Site (13.0.5 `.text`)

| Offset | What it is |
|---|---|
| `0x614630` | Entry of the Fighter vtable's per-frame function (vtable slot `0x4f80568`), run once per frame for each fighter before it updates. x0 = the Fighter. |

Checked against its 64 instruction bytes and the "13.0.5" display version at
boot; on any mismatch nothing is installed and `plugin.log` says why.

## What the hook reads, per fighter and frame

It reads the fighter's kind first. Until a Game & Watch has played a frame in
the current match, every other fighter returns right there: a match without
G&W costs a few memory reads per fighter and frame, and writes nothing. With
G&W it reads only the fighter whose frame it is, through the game's own getters:
status and situation (`StatusModule`), animation (`MotionModule`), soft
platform (`GroundModule::is_passable_ground`), damage, position, and
`AttackModule::is_infliction_status` (hit, shield, clank). For G&W also the
instance work ints `SPECIAL_S_KIND` (`0x100000c2`, the panel: 0-8 for 1-9, -1
for none) and `SPECIAL_S_PREV_KIND` (`0x100000c1`). The opponent's columns are
the snapshot its own frame published. `app::fighter::get_id_from_entry_id`
picks the active fighter of a slot (Ice Climbers, Pokémon Trainer).

The number itself is drawn in G&W's fighter scripts (`lua2cpp_gamewatch`) on
the side special's first frame. In that same frame
`FighterSpecializer_GameWatch::init_panel` (`.text+0xa84470`) moves it to
`SPECIAL_S_PREV_KIND` and resets `SPECIAL_S_KIND` to -1, so between frames the
panel only shows in `SPECIAL_S_PREV_KIND` (Eden, 2026-10-29: `special_s=8
judges=0` when watching the other one). The plugin reports a Judge when that
value changes; the value it replaced is the number the draw excluded. The
animation G&W then plays (`special_s_1` to `special_s_9`) matched the logged
number in every Eden row.

## How the number is drawn (G&W's script, 13.0.5)

Read from a dump of `lua2cpp_gamewatch`'s code around the draw (Eden,
2026-10-29, `diag` build): for each panel 0..8 the script takes a weight from
`param_special_s` (1 for every panel in vanilla), 0 for the previous panel,
and 0 for panel 7 while `SPECIAL_S_PANEL_7_INVALID_FRAME` > 0; it draws
`r = sv_math::rand(hash40("fighter"), total weight)` and picks the first panel
whose running total exceeds `r`. Nine more `sv_math::rand(fighter, 100)` draws
follow in the same frame. `sv_math::rand` (`.text+0x2275320`) steps stream 1
of the RNG manager (`*(*(.text+0x52c35b0))`, 20-byte xorshift128 states) and
returns the output modulo the range.

At the start of each G&W frame the plugin copies that stream's state and
computes the number a side special started then would show (`predicted`).
The copy is plain memory reads; the game's stream is never stepped.

## Why it is safe online

The online check compares only how often each of the game's checked RNG
streams has been drawn (see `ssbu-desync-detector/docs/WHAT-IS-CHECKED.md`).
The hook calls no RNG, skips nothing and writes no game memory: getters and
reads only. Files are written by the plugin's own thread. No heap allocations
(`heap_allocations=0` in `plugin.log`).

Replay A/B in Eden (2026-10-29), as in that document: the same replay played
with only the desync detector, then with the detector and this plugin.

| Replay | Build | Frames | Result |
|---|---|---|---|
| Pikachu vs Wolf | reads every fighter (before the G&W gate) | 12,640 + 2,355 | identical check values and positions |
| G&W vs Captain Falcon, Umbra Clock Tower | gated | 15,374 | identical |
| Pikachu vs Wolf | gated (no G&W: only the install line in `plugin.log`) | 12,640 | identical |
| G&W vs Captain Falcon, Umbra Clock Tower | reads the fighter RNG stream every G&W frame (current) | 15,374 | identical |

Live Eden matches against CPUs logged every side special (`special_s` =
`judges` in `plugin.log`), and every logged number matched G&W's
`special_s_N` animation and the panels in the recordings.
