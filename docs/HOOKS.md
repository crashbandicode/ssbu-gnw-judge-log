# Hook and reads

## Site (13.0.5 `.text`)

| Offset | What it is |
|---|---|
| `0x614630` | Entry of the Fighter vtable's per-frame function (vtable slot `0x4f80568`), run once per frame for each fighter before it updates. x0 = the Fighter. |

Checked against its 64 instruction bytes and the "13.0.5" display version at
boot; on any mismatch nothing is installed and `plugin.log` says why.

## What the hook reads, per fighter and frame

Only the fighter whose frame it is, through the game's own getters:
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

## Why it is safe online

The online check compares only how often each of the game's checked RNG
streams has been drawn (see `ssbu-desync-detector/docs/WHAT-IS-CHECKED.md`).
The hook calls no RNG, skips nothing and writes no game memory: getters and
reads only. Files are written by the plugin's own thread. No heap allocations
(`heap_allocations=0` in `plugin.log`).
