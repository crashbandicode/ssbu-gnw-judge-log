"""Tests for judge_table.py: python3 -I -m unittest discover -s tools"""
import io
import random
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import judge_table  # noqa: E402

HEADER = (Path(__file__).resolve().parent.parent / 'src' / 'model.rs').read_text(encoding='utf-8')


def header_columns():
    start = HEADER.index('pub const HEADER: &str = "') + len('pub const HEADER: &str = "')
    text = HEADER[start:HEADER.index('";', start)]
    return text.replace('\\t\\\n', '\t').replace('\\t', '\t').split('\t')


def write_tsv(path, rows):
    columns = header_columns()
    lines = ['\t'.join(columns)]
    for row in rows:
        lines.append('\t'.join(str(row.get(c, '-')) for c in columns))
    path.write_text('\n'.join(lines) + '\n', encoding='utf-8')


def fair_rows(count, seed):
    rng = random.Random(seed)
    rows, prev = [], 0
    for i in range(count):
        number = rng.choice([k for k in range(1, 10) if k != prev])
        rows.append({'boot': 'b', 'session': 1 + i // 20, 'frame': 100 * i, 'number': number,
                     'nine': int(number == 9), 'prev_number': prev, 'gw_entry': 0,
                     'gw_place': rng.choice(['ground', 'air', 'platform']),
                     'gw_damage': rng.randrange(0, 150), 'opp_damage': rng.randrange(0, 150),
                     'distance': rng.randrange(0, 120)})
        prev = number
    return rows


class JudgeTableTest(unittest.TestCase):
    def test_chi_square_tail(self):
        self.assertAlmostEqual(judge_table.chi2_sf(15.507, 8), 0.05, places=3)
        self.assertAlmostEqual(judge_table.chi2_sf(2.733, 8), 0.95, places=3)
        self.assertAlmostEqual(judge_table.chi2_sf(20.090, 8), 0.01, places=3)

    def test_fair_draw_probabilities(self):
        self.assertEqual(judge_table.p_nine(9), 0.0)
        self.assertEqual(judge_table.p_nine(3), 1 / 8)
        self.assertEqual(judge_table.p_nine(0), 1 / 9)
        self.assertEqual(judge_table.p_number(4, 4), 0.0)

    def test_merges_dedupes_and_derives(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            rows = fair_rows(400, 7)
            (tmp / 'a').mkdir()
            write_tsv(tmp / 'a' / 'judges.tsv', rows)
            write_tsv(tmp / 'copy.tsv', rows[:50])  # the same Judges again
            files, merged = judge_table.read_rows([tmp / 'a', tmp / 'copy.tsv'])
            self.assertEqual(len(files), 2)
            self.assertEqual(len(merged), 400)
            judge_table.derive(merged)
            first = merged[0]
            self.assertEqual(first['judge_index'], 1)
            self.assertEqual(first['frames_since_last_judge'], '')
            self.assertEqual(merged[1]['frames_since_last_judge'], 100)
            out = io.StringIO()
            judge_table.summary(merged, 10, out)
            text = out.getvalue()
            self.assertIn('400 Judges, 20 matches', text)
            self.assertIn('chi-square vs a fair draw', text)
            self.assertIn('gw_place', text)

    def test_a_biased_draw_is_flagged(self):
        rows = fair_rows(2000, 11)
        for row in rows[::3]:
            if row['prev_number'] != 9:
                row['number'], row['nine'] = 9, 1
        for row in rows:
            for key, value in list(row.items()):
                row[key] = str(value)
        judge_table.derive(rows)
        out = io.StringIO()
        judge_table.summary(rows, 10, out)
        line = next(l for l in out.getvalue().splitlines() if l.startswith('9s:'))
        self.assertIn('p = 0.000', line)

    def test_header_matches_the_plugin(self):
        columns = header_columns()
        self.assertEqual(columns[:4], ['boot', 'session', 'frame', 'number'])
        for column in judge_table.CONTEXTS:
            if not column.endswith('_bin'):
                self.assertIn(column, columns)


if __name__ == '__main__':
    unittest.main()
