"""Turn one or more judges.tsv files into one table for statistics.

Usage:
  judge_table.py <judges.tsv or folder>... [-o judges.csv] [--min 10]

Merges the files (a folder means every judges.tsv under it), drops duplicate
rows, adds a few derived columns and writes a CSV. Then prints how often each
number came up and, per context (place, previous move, hit or not, damage...),
how many 9s came up against how many a fair draw would give.

A fair draw here means uniform over the numbers it can give: Judge cannot
repeat the number the game still holds as the previous one (`excluded`; the
game clears it at the start of a match and after G&W is KO'd), so a Judge has
P(9) = 1/8, 0 right after a 9 and 1/9 when nothing is excluded. The script
uses that per row, so contexts that follow a 9 more often are not mistaken
for unlucky ones. `repeat` = 1 when a Judge equals the previous one."""
import argparse
import csv
import math
import sys
from collections import Counter, defaultdict
from pathlib import Path

# Tournament-legal stages as the plugin names them (edit to match your ruleset):
# Battlefield, Small Battlefield, Final Destination, Pokemon Stadium 2,
# Smashville, Town and City, Kalos, Hollow Bastion, Yoshi's Story, Lylat Cruise.
LEGAL_STAGES = {
    'battlefield', 'battlefield_s', 'end', 'poke_stadium2', 'animal_village',
    'animal_city', 'poke_kalos', 'trail_castle', 'yoshi_story', 'fox_lylatcruise',
}

CONTEXTS = [
    'stage',
    'gw_place', 'opp_place',
    'gw_prev', 'gw_prev_hit', 'gw_last_move', 'gw_last_move_motion', 'gw_last_move_hit',
    'opp_fighter', 'opp_status', 'opp_status_hit', 'opp_prev', 'opp_prev_hit',
    'opp_last_move', 'opp_last_move_motion', 'opp_last_move_hit',
    'gw_damage_bin', 'opp_damage_bin', 'distance_bin', 'excluded', 'last_number',
    'judge_index_bin',
]


def p_nine(excluded):
    """P(9) for a fair draw that cannot give the excluded number."""
    if excluded == 9:
        return 0.0
    if excluded == 0:
        return 1 / 9
    return 1 / 8


def p_number(number, excluded):
    if number == excluded:
        return 0.0
    return 1 / 9 if excluded == 0 else 1 / 8


def read_rows(paths):
    files = []
    for path in map(Path, paths):
        files += sorted(path.rglob('judges.tsv')) if path.is_dir() else [path]
    seen, rows = set(), []
    for file in files:
        with file.open(newline='', encoding='utf-8') as handle:
            for row in csv.DictReader(handle, delimiter='\t'):
                key = (row['boot'], row['session'], row['frame'], row['gw_entry'])
                if key not in seen:
                    seen.add(key)
                    rows.append(row)
    return files, rows


def number(text):
    try:
        return float(text)
    except (TypeError, ValueError):
        return None


def bin_of(value, width, last):
    if value is None:
        return '-'
    low = min(int(value // width) * width, last)
    return f'{low:g}+' if low == last else f'{low:g}-{low + width:g}'


def derive(rows):
    """Adds match_id, judge_index (1 = first Judge of this G&W in the match),
    frames_since_last_judge, damage_diff and binned columns."""
    rows.sort(key=lambda r: (r['boot'], int(r['session']), int(r['gw_entry']), int(r['frame'])))
    last = {}
    for row in rows:
        match = (row['boot'], row['session'], row['gw_entry'])
        index, frame = last.get(match, (0, None))
        row['match_id'] = f"{row['boot']}/{row['session']}"
        row['judge_index'] = index + 1
        row['frames_since_last_judge'] = '' if frame is None else int(row['frame']) - frame
        last[match] = (index + 1, int(row['frame']))
        gw, opp = number(row['gw_damage']), number(row['opp_damage'])
        row['damage_diff'] = '' if gw is None or opp is None else round(opp - gw, 1)
        row['gw_damage_bin'] = bin_of(gw, 30, 150)
        row['opp_damage_bin'] = bin_of(opp, 30, 150)
        row['distance_bin'] = bin_of(number(row['distance']), 20, 100)
        row['judge_index_bin'] = str(min(row['judge_index'], 6)) + ('+' if row['judge_index'] >= 6 else '')
        row['p_nine'] = round(p_nine(int(row['excluded'])), 6)
        row['repeat'] = int(row['number'] == row['last_number'])


def chi2_sf(x, dof):
    """P(chi-square with `dof` degrees of freedom >= x)."""
    a, z = dof / 2, x / 2
    if z <= 0:
        return 1.0
    if z < a + 1:  # series for the lower gamma, then complement
        term = total = 1 / a
        for n in range(1, 500):
            term *= z / (a + n)
            total += term
            if term < total * 1e-15:
                break
        return max(0.0, 1 - total * math.exp(-z + a * math.log(z) - math.lgamma(a)))
    # continued fraction for the upper gamma (Lentz)
    b, c, d = z + 1 - a, 1 / 1e-300, 1 / (z + 1 - a)
    h = d
    for i in range(1, 500):
        an = -i * (i - a)
        b += 2
        d = an * d + b
        d = 1e-300 if abs(d) < 1e-300 else d
        c = b + an / c
        c = 1e-300 if abs(c) < 1e-300 else c
        d = 1 / d
        h *= d * c
        if abs(d * c - 1) < 1e-15:
            break
    return min(1.0, math.exp(-z + a * math.log(z) - math.lgamma(a)) * h)


def two_sided_p(z):
    return math.erfc(abs(z) / math.sqrt(2))


def summary(rows, minimum, out):
    write = lambda text='': print(text, file=out)
    n = len(rows)
    write(f'{n} Judges, {len({r["match_id"] for r in rows})} matches')
    if not n:
        return
    counts = Counter(int(r['number']) for r in rows)
    expected = [sum(p_number(k, int(r['excluded'])) for r in rows) for k in range(1, 10)]
    write('number   count  expected')
    for k in range(1, 10):
        write(f'{k:>6} {counts[k]:>7} {expected[k - 1]:>9.1f}')
    chi2 = sum((counts[k] - e) ** 2 / e for k, e in zip(range(1, 10), expected) if e > 0)
    write(f'chi-square vs a fair draw: {chi2:.2f} on 8 dof, p = {chi2_sf(chi2, 8):.3f}')
    nines = counts[9]
    exp9 = sum(float(r['p_nine']) for r in rows)
    var9 = sum(float(r['p_nine']) * (1 - float(r['p_nine'])) for r in rows)
    z = (nines - exp9) / math.sqrt(var9) if var9 else 0.0
    write(f'9s: {nines} of {n} ({nines / n:.1%}), fair draw expects {exp9:.1f}'
          f' (z = {z:+.2f}, p = {two_sided_p(z):.3f})')
    tests = 0
    lines = []
    for column in CONTEXTS:
        groups = defaultdict(list)
        for row in rows:
            groups[str(row.get(column, '-'))].append(row)
        for value, group in sorted(groups.items(), key=lambda kv: -len(kv[1])):
            if len(group) < minimum:
                continue
            obs = sum(r['number'] == '9' for r in group)
            exp = sum(float(r['p_nine']) for r in group)
            var = sum(float(r['p_nine']) * (1 - float(r['p_nine'])) for r in group)
            z = (obs - exp) / math.sqrt(var) if var else 0.0
            tests += 1
            lines.append((column, value, len(group), obs, exp, z, two_sided_p(z)))
    write()
    write(f'9s by context (groups of at least {minimum} Judges)')
    write(f'{"context":<22} {"value":<24} {"n":>5} {"9s":>4} {"fair":>6} {"z":>6} {"p":>6}')
    for column, value, size, obs, exp, z, p in lines:
        flag = ' *' if p * tests < 0.05 else ''
        write(f'{column:<22} {value[:24]:<24} {size:>5} {obs:>4} {exp:>6.1f} {z:>+6.2f} {p:>6.3f}{flag}')
    write(f'* = still significant after correcting for {tests} tests (Bonferroni, 5 %).')


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('inputs', nargs='+', help='judges.tsv files or folders holding them')
    parser.add_argument('-o', '--output', help='CSV to write (default: judges.csv here)',
                        default='judges.csv')
    parser.add_argument('--min', type=int, default=10, help='smallest group to report')
    parser.add_argument('--all-stages', action='store_true',
                        help='keep Judges on stages outside LEGAL_STAGES')
    args = parser.parse_args(argv)
    files, rows = read_rows(args.inputs)
    if not args.all_stages:
        kept = [row for row in rows if row.get('stage') in LEGAL_STAGES]
        if len(kept) < len(rows):
            print(f'{len(rows) - len(kept)} Judge(s) on other stages left out (--all-stages keeps them)')
        rows = kept
    if not files:
        parser.error('no judges.tsv found')
    derive(rows)
    columns = list(rows[0].keys()) if rows else []
    with open(args.output, 'w', newline='', encoding='utf-8') as handle:
        writer = csv.DictWriter(handle, fieldnames=columns)
        writer.writeheader()
        writer.writerows(rows)
    print(f'{len(files)} file(s) -> {args.output}')
    summary(rows, args.min, sys.stdout)


if __name__ == '__main__':
    main()
