"""Test whether any context predicts a 9 window, from windows.tsv.

Usage:
  window_table.py <windows.tsv or folder>... [--min 30] [--all-stages]

Each windows.tsv row is one fighter-stream state G&W lived through; its
`predicted` number is what a side special started then would show. A fair draw
gives a 9 with probability 1/(numbers allowed): 1/8 normally, 1/9 with nothing
excluded, 1/7 with 7 blocked too, 0 right after a 9. Per context the script
compares the 9 states seen with that expectation (a z score) and marks the ones
that stay significant after correcting for the number of groups tested."""
import argparse
import collections
import csv
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from judge_table import LEGAL_STAGES  # noqa: E402

CONTEXTS = ['stage', 'gw_place', 'gw_status', 'gw_prev', 'gw_prev_hit', 'gw_last_move',
            'gw_last_move_hit', 'opp_fighter', 'opp_place', 'opp_status', 'opp_prev',
            'opp_last_move', 'opp_last_move_hit', 'excluded', 'seven_block', 'damage_bin',
            'opp_damage_bin']


def p_nine(row):
    excluded, seven = int(row['excluded']), row['seven_block'] == '1'
    allowed = [n for n in range(1, 10) if n != excluded and not (seven and n == 7)]
    return (9 in allowed) / len(allowed)


def bin_of(text):
    try:
        value = float(text)
    except ValueError:
        return '-'
    low = min(int(value // 30) * 30, 150)
    return f'{low}+' if low == 150 else f'{low}-{low + 30}'


def read(paths):
    rows = []
    for path in map(Path, paths):
        for file in sorted(path.rglob('windows*.tsv')) if path.is_dir() else [path]:
            with open(file, newline='', encoding='utf-8') as handle:
                rows += list(csv.DictReader(handle, delimiter='\t'))
    for row in rows:
        row['damage_bin'] = bin_of(row.get('gw_damage', ''))
        row['opp_damage_bin'] = bin_of(row.get('opp_damage', ''))
    return rows


def test(group):
    seen = sum(row['nine'] == '1' for row in group)
    expected = sum(p_nine(row) for row in group)
    variance = sum(p_nine(row) * (1 - p_nine(row)) for row in group)
    z = (seen - expected) / math.sqrt(variance) if variance else 0.0
    return seen, expected, z


def summary(rows, minimum, out):
    write = lambda text='': print(text, file=out)
    seen, expected, z = test(rows)
    frames = sum(int(row['frames']) for row in rows)
    write(f'{len(rows)} stream states over {frames} frames: {seen} were a 9, '
          f'a fair draw gives {expected:.1f} (z = {z:+.2f})')
    lines = []
    for column in CONTEXTS:
        groups = collections.defaultdict(list)
        for row in rows:
            groups[row.get(column, '-')].append(row)
        for value, group in groups.items():
            if len(group) >= minimum:
                lines.append((column, value) + (len(group),) + test(group))
    tests = len(lines)
    # Two-sided 5 % after Bonferroni over `tests` groups.
    limit = math.sqrt(2) * erfinv_approx(1 - 0.05 / max(tests, 1))
    write(f'{tests} groups of at least {minimum} states; |z| > {limit:.2f} is significant')
    write(f'{"context":<18} {"value":<24} {"states":>6} {"9s":>5} {"fair":>7} {"z":>6}')
    for column, value, size, seen, expected, z in sorted(lines, key=lambda l: -abs(l[5]))[:25]:
        flag = ' *' if abs(z) > limit else ''
        write(f'{column:<18} {value[:24]:<24} {size:>6} {seen:>5} {expected:>7.1f} {z:>+6.2f}{flag}')


def erfinv_approx(y):
    """Inverse error function by bisection (enough for thresholds)."""
    low, high = 0.0, 6.0
    for _ in range(80):
        mid = (low + high) / 2
        if math.erf(mid) < y:
            low = mid
        else:
            high = mid
    return (low + high) / 2


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('inputs', nargs='+', help='windows.tsv files or folders holding them')
    parser.add_argument('--min', type=int, default=30, help='smallest group to test')
    parser.add_argument('--all-stages', action='store_true',
                        help='keep states on stages outside LEGAL_STAGES')
    args = parser.parse_args(argv)
    rows = read(args.inputs)
    if not args.all_stages:
        rows = [row for row in rows if row.get('stage') in LEGAL_STAGES]
    summary(rows, args.min, sys.stdout)


if __name__ == '__main__':
    main()
