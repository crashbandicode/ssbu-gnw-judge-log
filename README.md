# ssbu-gnw-judge-log

> **Disclaimer:** this project was developed with the assistance of AI
> (Claude, Codex and other coding agents) under the operator's direction and
> review. Nothing is claimed beyond the dated tests cited below.

Skyline plugin for Super Smash Bros. Ultimate 13.0.5 that logs every Game &
Watch Judge: the number, what both players were doing, where they were, their
damage and whether their moves hit. After enough matches the log is a dataset
for asking when 9s come up. Read-only: safe to leave on online.

![Judge](docs/media/judge.gif)

*A 6 out of a dash that hits a CPU Ridley, logged below (Eden, 2026-10-29).*

## How it works

**1. Play G&W.** Each Judge becomes one row of `judges.tsv` (some of its 44
columns):

```text
number=6  excluded=5  last_number=5  judge_hit=hit
gw:  ground  19.5%  special_s_6  prev=dash (5 frames)  last_move=attack_s4 (none)
opp: ridley  ground  0.0%  landing_fall_special  distance=21.4
```

**2. Collect the log** from `sd:/ultimate/ssbu-gnw-judge-log/judges.tsv`.

**3. Make the table.** One CSV for any statistics tool, plus a first look:

```text
python3 tools/judge_table.py judges.tsv -o judges.csv
```

It prints how often each number came up and, per context (place, previous
move, hit or not, damage...), the 9s seen against what a fair draw gives. The
game never repeats the number it holds as the previous one (it forgets it when
G&W is KO'd), so a fair draw gives a 9 one time in 8.

## Install

Copy `libssbu_gnw_judge_log.nro` to
`sd:/atmosphere/contents/01006A800016E000/romfs/skyline/plugins/`. About 0.5 KB
of log per Judge.

## More

- [docs/FORMAT.md](docs/FORMAT.md): every column.
- [docs/HOOKS.md](docs/HOOKS.md): the hook, what it reads, why it is safe online.

## Build

```text
cargo test
cargo skyline build --release
```

## License

MIT
