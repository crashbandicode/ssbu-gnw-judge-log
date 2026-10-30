# judges.tsv

One row per Judge (Game & Watch's side special) in 1v1 matches, tab-separated,
header in the first line. Rows are appended when G&W leaves the side special,
so the Judge's own result is known. The context columns describe the frame the
number appeared on. Both players are logged if both are G&W, CPUs included.
Matches without G&W, or with more than two fighters, leave no trace.

| Column | Meaning |
|---|---|
| `boot` | when the game started (`YYYYMMDD-hhmmss`, console time) |
| `session` | match number in this boot (a pause over 5 s also starts a new one) |
| `frame` | G&W's frame in the session when the number appeared |
| `number`, `nine` | the Judge number 1-9; `nine` = 1 for a 9 |
| `excluded` | the number the game held as the previous Judge, which cannot come up; 0 when it held none (first Judge of a match, first after G&W is KO'd) |
| `last_number` | the previous Judge in this session, 0 for the first |
| `judge_hit` | what the Judge hit: `hit`, `shield`, `clank`, `none`, `-` if the match ended mid-Judge |
| `stage` | the game's stage id name: `battlefield`, `battlefield_s`, `end` (Final Destination), `animal_village` (Smashville), `animal_city` (Town and City), `poke_stadium2`, `poke_kalos`, `trail_castle` (Hollow Bastion)...; `battle_*` / `end_*` are Battlefield and Omega forms |
| `gw_entry` | G&W's player slot (0 = P1) |
| `gw_place`, `opp_place` | `ground`, `platform`, `air`, `ledge` |
| `gw_damage`, `opp_damage` | percent |
| `gw_status`, `gw_motion`, `gw_status_ago` | G&W's status (`special_s`), its animation, which names the number (`special_s_9`, `special_air_s_9` in the air), and frames into it (0: the number is drawn on the side special's first frame) |
| `gw_prev*` | the status G&W was in before the Judge, its animation, frames since it started, its result |
| `gw_last_move*` | G&W's most recent attack, special, grab or throw before the Judge |
| `opp_fighter` | opponent's fighter (the lowest other player slot) |
| `opp_status*` | what the opponent was doing on that frame (`_hit`: result so far) |
| `opp_prev*`, `opp_last_move*` | as for G&W |
| `gw_x`, `gw_y`, `opp_x`, `opp_y`, `distance` | positions and their distance |

| `predicted` | what the plugin computed, before the draw, that this Judge would show |
| `rng_draw`, `rng_draw_count` | the fighter RNG stream (state, draws so far) at the start of the frame the number was drawn |
| `rng_before`, `rng_before_count`, `rng_count_after` | the same one frame earlier, and the draw count one frame later |
| `seven_block` | G&W's "no 7" timer (frames) at the draw frame |

`*_ago` counts frames from the start of that status to the number's frame.
`*_hit` is what that status's own hitboxes connected with: `hit` beats
`shield` beats `clank` beats `none`. Projectiles are separate objects and do
not count. Statuses and animations use the game's names; hex means the name is
not in the plugin's tables (fighter-specific statuses of other fighters).

`plugin.log` holds the install line, one `gamewatch` line when a G&W starts
a session, and after each match a `session_end` line per G&W
(`special_s` = side specials started, `judges` = numbers logged; they match
unless a side special was cancelled before its number appeared).

# windows.tsv

The 9 windows. A Judge's number is fixed by the fighter RNG stream at the
moment of the draw (see [HOOKS.md](HOOKS.md)), and that stream only moves when
fighter code draws from it. So at any frame there is one number a side
special started then would show. One row per stream state G&W lived through
in a 1v1: `start_frame`, `frames` it lasted, `predicted` (the number), `nine`,
`excluded` and `seven_block` (what the draw could not give), `end_draws` (how
many draws ended it; 0 = the excluded number changed), and the same context
columns as `judges.tsv` taken on its first frame. The last state of a match is
not written. States on the results screen carry stage `resultstage`; the
scripts below leave them out.

# frames.tsv and draws.tsv (diag build only)

The `diag` cargo feature (Eden only, never shipped) adds `frames.tsv`, one
line per fighter frame start while G&W plays: `session entry frame count
x:y:z:w`, the fighter RNG stream's draw count and full state (count
4294967295 when it could not be read). `draws.tsv` lists every
`sv_math::rand(fighter, n)` call: `session count n caller`.

## Tables for statistics

```text
python3 tools/judge_table.py <judges.tsv or folders>... -o judges.csv
```

Merges files (dropping duplicates), adds `match_id`, `judge_index`,
`frames_since_last_judge`, `damage_diff`, `repeat`, binned damage and
distance, and `p_nine` (a fair draw's chance of a 9 for that row: 1/8, 0 when
9 is excluded, 1/9 when nothing is), then prints the 9 rate per context
against that expectation. Only tournament-legal stages are kept unless
`--all-stages` is given; the list is `LEGAL_STAGES` at the top of the script.

```text
python3 tools/window_table.py <windows.tsv or folders>...
```

The same per context for the 9 windows: every stream state against the
fair chance of its `predicted` being a 9, Bonferroni-corrected over the
groups tested.

```text
python3 tools/match_report.py <run folders>...
```

One line per match (a folder with the three files): stage, opponent, how
often the stream moved, 9 states against a fair draw by state and by frame,
Judges predicted, and from `frames.tsv` whether every change of the stream
was a forward step of the xorshift. Then pooled: the spread of per-match 9
counts against a fair draw and the 9 share by what was visible on the frame
each state began on (a hit, a KO, a shield, a Judge, nothing).
