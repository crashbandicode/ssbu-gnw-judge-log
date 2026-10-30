"""Tests for the tools here: python3 -I -m unittest discover -s tools"""
import contextlib
import csv
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
                     'nine': int(number == 9), 'excluded': prev, 'last_number': prev,
                     'gw_entry': 0,
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
            if row['excluded'] != 9:
                row['number'], row['nine'] = 9, 1
        for row in rows:
            for key, value in list(row.items()):
                row[key] = str(value)
        judge_table.derive(rows)
        out = io.StringIO()
        judge_table.summary(rows, 10, out)
        line = next(l for l in out.getvalue().splitlines() if l.startswith('9s:'))
        self.assertIn('p = 0.000', line)

    def test_only_legal_stages_by_default(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            rows = fair_rows(40, 3)
            for i, row in enumerate(rows):
                row['stage'] = 'battlefield' if i % 4 else 'mariobros'
            write_tsv(tmp / 'judges.tsv', rows)
            out = tmp / 'out.csv'
            with contextlib.redirect_stdout(io.StringIO()):
                judge_table.main([str(tmp / 'judges.tsv'), '-o', str(out)])
            self.assertEqual(len(out.read_text().splitlines()), 1 + 30)
            with contextlib.redirect_stdout(io.StringIO()):
                judge_table.main([str(tmp / 'judges.tsv'), '-o', str(out), '--all-stages'])
            self.assertEqual(len(out.read_text().splitlines()), 1 + 40)

    def test_window_table_flags_only_a_real_bias(self):
        import window_table
        rng = random.Random(5)
        rows = []
        for i in range(6000):
            place = rng.choice(['ground', 'air'])
            excluded = rng.choice([0, 3])
            allowed = [n for n in range(1, 10) if n != excluded]
            number = rng.choice(allowed)
            if place == 'air' and i % 4 == 0:
                number = 9  # a planted bias
            rows.append({'frames': '2', 'nine': str(int(number == 9)), 'predicted': str(number),
                         'excluded': str(excluded), 'seven_block': '0', 'gw_place': place,
                         'stage': 'battlefield', 'gw_damage': '10', 'opp_damage': '20'})
        out = io.StringIO()
        window_table.summary(rows, 30, out)
        text = out.getvalue()
        air = next(l for l in text.splitlines() if l.startswith('gw_place') and ' air ' in l)
        self.assertTrue(air.endswith('*'), text)
        self.assertAlmostEqual(window_table.p_nine({'excluded': '9', 'seven_block': '0'}), 0.0)
        self.assertAlmostEqual(window_table.p_nine({'excluded': '3', 'seven_block': '1'}), 1 / 7)

    def test_header_matches_the_plugin(self):
        columns = header_columns()
        self.assertEqual(columns[:4], ['boot', 'session', 'frame', 'number'])
        for column in judge_table.CONTEXTS:
            if not column.endswith('_bin'):
                self.assertIn(column, columns)

    def test_match_report_checks_forward_steps(self):
        import match_report
        state = (0x12345678, 0x9ABCDEF0, 0x0FEDCBA9, 0x87654321)
        lines, count = [], 100
        for frame in range(6):
            lines.append(f'1\t0\t{frame}\t{count}\t' + ':'.join(f'{v:08x}' for v in state))
            for _ in range(frame % 3):
                state = match_report.step(state)
            count += frame % 3
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'frames.tsv'
            path.write_text('\n'.join(lines) + '\n', encoding='utf-8')
            self.assertEqual(match_report.check_frames(path), (6, 3, [], 0, 0))
            # a state the stream could not have reached by stepping
            tail = '00000000' if not lines[-1].endswith('00000000') else 'ffffffff'
            lines[-1] = lines[-1][:-8] + tail
            path.write_text('\n'.join(lines) + '\n', encoding='utf-8')
            self.assertEqual(len(match_report.check_frames(path)[2]), 1)

    def test_match_report_landing_event(self):
        import match_report
        self.assertEqual(match_report.landing({'gw_status': 'wait', 'gw_status_ago': '5',
                                               'opp_status': 'attack', 'opp_status_ago': '2'}),
                         'nothing_visible')
        self.assertEqual(match_report.landing({'gw_status': 'damage_fly', 'gw_status_ago': '0',
                                               'opp_status': 'guard_on', 'opp_status_ago': '1'}),
                         'gw_is_hit+shield')

    def test_match_report_pools_runs(self):
        import match_report
        rng = random.Random(9)
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            runs = []
            for run in ('r1', 'r2'):
                folder = tmp / run / 'ssbu-gnw-judge-log'
                folder.mkdir(parents=True)
                rows = []
                for i in range(300):
                    excluded = rng.choice([0, 4])
                    number = rng.choice([n for n in range(1, 10) if n != excluded])
                    rows.append({'session': '1', 'start_frame': str(3 * i), 'frames': '3',
                                 'predicted': str(number), 'nine': str(int(number == 9)),
                                 'excluded': str(excluded), 'seven_block': '0', 'end_draws': '1',
                                 'stage': 'end', 'opp_fighter': 'mario', 'gw_status': 'wait',
                                 'gw_status_ago': '9', 'opp_status': 'wait', 'opp_status_ago': '9',
                                 'gw_damage': '0', 'opp_damage': '0'})
                with open(folder / 'windows.tsv', 'w', newline='', encoding='utf-8') as handle:
                    writer = csv.DictWriter(handle, fieldnames=list(rows[0]), delimiter='\t')
                    writer.writeheader()
                    writer.writerows(rows)
                runs.append(str(tmp / run))
            out = io.StringIO()
            match_report.report(runs, out)
            text = out.getvalue()
            self.assertIn('pooled: 600 states', text)
            self.assertIn('on 2 matches', text)
            self.assertIn('nothing_visible', text)


if __name__ == '__main__':
    unittest.main()
