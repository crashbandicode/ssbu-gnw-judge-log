# judges.tsv

One row per Judge (Game & Watch's side special), tab-separated, header in the
first line. Rows are appended when G&W leaves the side special, so the Judge's
own result is known. The context columns describe the frame the number
appeared on. Every G&W in the match is logged, CPUs included.

| Column | Meaning |
|---|---|
| `boot` | when the game started (`YYYYMMDD-hhmmss`, console time) |
| `session` | match number in this boot (a pause over 5 s also starts a new one) |
| `frame` | G&W's frame in the session when the number appeared |
| `number`, `nine` | the Judge number 1-9; `nine` = 1 for a 9 |
| `excluded` | the number the game held as the previous Judge, which cannot come up; 0 when it held none (first Judge of a match, first after G&W is KO'd) |
| `last_number` | the previous Judge in this session, 0 for the first |
| `judge_hit` | what the Judge hit: `hit`, `shield`, `clank`, `none`, `-` if the match ended mid-Judge |
| `players` | fighters in the match |
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

`*_ago` counts frames from the start of that status to the number's frame.
`*_hit` is what that status's own hitboxes connected with: `hit` beats
`shield` beats `clank` beats `none`. Projectiles are separate objects and do
not count. Statuses and animations use the game's names; hex means the name is
not in the plugin's tables (fighter-specific statuses of other fighters).

`plugin.log` holds the install line, one `gamewatch` line when a G&W starts
a session, and after each match a `session_end` line per G&W
(`special_s` = side specials started, `judges` = numbers logged; they match
unless a side special was cancelled before its number appeared).

## Table for statistics

```text
python3 tools/judge_table.py <judges.tsv or folders>... -o judges.csv
```

Merges files (dropping duplicates), adds `match_id`, `judge_index`,
`frames_since_last_judge`, `damage_diff`, `repeat`, binned damage and
distance, and `p_nine` (a fair draw's chance of a 9 for that row: 1/8, 0 when
9 is excluded, 1/9 when nothing is), then prints the 9 rate per context
against that expectation.
