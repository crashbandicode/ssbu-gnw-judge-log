"""Per-match report over a set of logged matches.

Usage:
  match_report.py <run folder>... [--all-stages]

A run folder holds `judges.tsv` and `windows.tsv` (and, from the diag build,
`frames.tsv`), or a `ssbu-gnw-judge-log/` folder with them. For each match:
stage, opponent, how often the fighter stream moved, the 9 states seen against
a fair draw (by state and weighted by how long each lasted), Judges predicted,
and from `frames.tsv` whether every change of the stream was a forward step of
the game's xorshift. Pooled: the spread of the per-match 9 counts against a
fair draw (chi-square; a fair draw gives about one per match) and the 9 share
by what was visible on the frame the stream landed on."""
import argparse
import collections
import csv
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import judge_table  # noqa: E402
import window_table  # noqa: E402

MASK = 0xFFFFFFFF
HURT = ('damage', 'capture', 'thrown', 'bury', 'shield_break', 'down', 'passive')


def step(state):
    """One xorshift128 step, as `sv_math::rand` does it (state = x, y, z, w)."""
    x, y, z, w = state
    t = x ^ (x << 11) & MASK
    t ^= t >> 8
    return (y, z, w, t ^ w ^ (w >> 19))


def check_frames(path, limit=4096):
    """Reads a diag `frames.tsv` (`session entry frame count x:y:z:w`) and
    steps the stream forward between consecutive rows. Returns the row count,
    how many rows moved the stream, the rows whose state was not reached by
    stepping, and how many went backwards or jumped more than `limit`."""
    rows = 0
    moved = 0
    bad, backwards, big = [], 0, 0
    prev = None
    with open(path, encoding='utf-8') as handle:
        for line in handle:
            parts = line.rstrip('\n').split('\t')
            if len(parts) != 5:
                continue
            session, entry, frame, count = (int(v) for v in parts[:4])
            state = tuple(int(v, 16) for v in parts[4].split(':'))
            rows += 1
            if count == MASK:  # the stream could not be read on that frame
                prev = None
                continue
            if prev and prev[0] == session:
                delta = count - prev[3]
                if delta < 0:
                    backwards += 1
                elif delta > limit:
                    big += 1
                else:
                    moved += delta > 0
                    expected = prev[4]
                    for _ in range(delta):
                        expected = step(expected)
                    if expected != state:
                        bad.append((session, entry, frame, prev[3], count))
            prev = (session, entry, frame, count, state)
    return rows, moved, bad, backwards, big


def judges(path):
    """(Judges logged, Judges whose `predicted` matched `number`)."""
    if not path.exists():
        return 0, 0
    with open(path, newline='', encoding='utf-8') as handle:
        rows = list(csv.DictReader(handle, delimiter='\t'))
    return len(rows), sum(r.get('predicted') == r.get('number') for r in rows)


def landing(row):
    """What was visible on the frame a stream state began on."""
    fresh = lambda ago: ago in ('0', '1')
    gw, gw_ago = row.get('gw_status', '-'), row.get('gw_status_ago', '-')
    opp, opp_ago = row.get('opp_status', '-'), row.get('opp_status_ago', '-')
    tags = []
    if gw.startswith('special_s') and fresh(gw_ago):
        tags.append('judge')
    if opp.startswith(HURT) and fresh(opp_ago):
        tags.append('gw_hits')
    if gw.startswith(HURT) and fresh(gw_ago):
        tags.append('gw_is_hit')
    if opp.startswith(('dead', 'rebirth')) and fresh(opp_ago):
        tags.append('opp_ko')
    if gw.startswith(('dead', 'rebirth')) and fresh(gw_ago):
        tags.append('gw_ko')
    if (gw.startswith('guard') and fresh(gw_ago)) or (opp.startswith('guard') and fresh(opp_ago)):
        tags.append('shield')
    return '+'.join(tags) or 'nothing_visible'


def nines(rows, weight=lambda row: 1):
    """(9 states seen, fair expectation, variance, z) over `rows`, each
    counted `weight(row)` times."""
    seen = sum(weight(r) for r in rows if r['nine'] == '1')
    expected = sum(weight(r) * window_table.p_nine(r) for r in rows)
    variance = sum(weight(r) ** 2 * window_table.p_nine(r) * (1 - window_table.p_nine(r))
                   for r in rows)
    return seen, expected, variance, (seen - expected) / math.sqrt(variance) if variance else 0.0


def data_folder(folder):
    inner = folder / 'ssbu-gnw-judge-log'
    return inner if inner.is_dir() else folder


def states(data, all_stages):
    """The match's stream states: rows of windows.tsv on the stage played
    (the results screen, stage `resultstage*`, is left out; so are stages
    outside LEGAL_STAGES unless `all_stages`)."""
    if not (data / 'windows.tsv').exists():
        return []
    rows = window_table.read([data / 'windows.tsv'])
    return [r for r in rows if r.get('frames', '').isdigit()
            and not r.get('stage', '').startswith('resultstage')
            and (all_stages or r.get('stage') in judge_table.LEGAL_STAGES)]


def report(folders, out, all_stages=False):
    write = lambda text='': print(text, file=out)
    write(f'{"run":<6} {"stage":<14} {"opp":<10} {"frames":>6} {"states":>6} {"moves/s":>7} '
          f'{"9s/fair":>11} {"z":>6} {"frame 9%":>8} {"fair%":>6} {"judges":>7}  stream check')
    pooled, events, spread, matches = [], collections.defaultdict(list), 0.0, 0
    for folder in map(Path, folders):
        data = data_folder(folder)
        rows = states(data, all_stages)
        if not rows:
            write(f'{folder.name:<6} no states on a legal stage (--all-stages keeps every stage)')
            continue
        stage = collections.Counter(r['stage'] for r in rows).most_common(1)[0][0]
        opp = collections.Counter(r['opp_fighter'] for r in rows).most_common(1)
        frames = sum(int(r['frames']) for r in rows)
        seen, expected, variance, z = nines(rows)
        frame_seen, frame_expected, _, _ = nines(rows, lambda r: int(r['frames']))
        total, predicted = judges(data / 'judges.tsv')
        check = '-'
        count, moved, bad, backwards, big = (
            check_frames(data / 'frames.tsv') if (data / 'frames.tsv').exists() else (0,) * 5)
        if count:
            check = f'{count} rows, {moved} moves, {len(bad)} not forward, {backwards} back, {big} jumps'
        write(f'{folder.name:<6} {stage:<14} {(opp[0][0] if opp else "-"):<10} {frames:>6} '
              f'{len(rows):>6} {60 * len(rows) / frames if frames else 0:>7.1f} '
              f'{seen:>5}/{expected:>5.1f} {z:>+6.2f} '
              f'{100 * frame_seen / frames if frames else 0:>7.1f}% '
              f'{100 * frame_expected / frames if frames else 0:>5.1f}% '
              f'{predicted:>3}/{total:<3}  {check}')
        if variance:
            spread += (seen - expected) ** 2 / variance
            matches += 1
        for before, after in zip(rows, rows[1:]):
            if before['session'] == after['session'] and before['end_draws'] != '0':
                events[landing(after)].append(after)
        pooled += rows
    write()
    seen, expected, _, z = nines(pooled)
    frames = sum(int(r['frames']) for r in pooled) or 1
    frame_seen, frame_expected, _, frame_z = nines(pooled, lambda r: int(r['frames']))
    write(f'pooled: {len(pooled)} states, 9s {seen} vs fair {expected:.1f} (z {z:+.2f}); '
          f'by frame {100 * frame_seen / frames:.2f}% vs {100 * frame_expected / frames:.2f}% '
          f'(z {frame_z:+.2f})')
    if matches:
        write(f'spread of per-match 9 counts: chi-square {spread:.1f} on {matches} matches '
              f'(p = {judge_table.chi2_sf(spread, matches):.2f}; a fair draw gives about {matches})')
    write(f'{"landing event":<22} {"states":>6} {"9s":>5} {"fair":>7} {"z":>6}')
    for tag, group in sorted(events.items(), key=lambda item: -len(item[1])):
        seen, expected, _, z = nines(group)
        write(f'{tag:<22} {len(group):>6} {seen:>5} {expected:>7.1f} {z:>+6.2f}')


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('runs', nargs='+', help='run folders (one match each)')
    parser.add_argument('--all-stages', action='store_true',
                        help='keep matches on stages outside LEGAL_STAGES')
    args = parser.parse_args(argv)
    report(args.runs, sys.stdout, args.all_stages)


if __name__ == '__main__':
    main()
