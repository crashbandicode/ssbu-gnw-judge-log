"""Regenerates src/names.rs: status, fighter and motion names for the log.

Usage: gen_names.py <skyline-smash src/lua_const.rs> > src/names.rs
Status and fighter names come from skyline-smash; motion names are the list
below, stored as hash40 (CRC-32 of the name | length << 32)."""
import re
import sys
import zlib

COMMON_LAST = 0x1E0  # FIGHTER_STATUS_KIND_FINAL; fighter-specific kinds follow
GAMEWATCH = 0x1C

MOTIONS = """
wait wait1 wait2 wait3 wait4 wait5 walk_slow walk_middle walk_fast walk_brake
dash run run_brake_l run_brake_r turn turn_dash turn_run turn_run_brake
squat squat_wait squat_rv jump_squat jump_f jump_b jump_aerial_f jump_aerial_b
fall fall_f fall_b fall_aerial fall_aerial_f fall_aerial_b fall_special
landing_light landing_heavy landing_fall_special pass stop_ceil
attack_11 attack_12 attack_13 attack_100 attack_100_end attack_100_sub
attack_dash attack_s3_hi attack_s3_s attack_s3_lw attack_hi3 attack_lw3
attack_s4_start attack_s4_hold attack_s4_hi attack_s4_s attack_s4_lw
attack_hi4_start attack_hi4_hold attack_hi4 attack_lw4_start attack_lw4_hold attack_lw4
attack_air_n attack_air_f attack_air_b attack_air_hi attack_air_lw
landing_air_n landing_air_f landing_air_b landing_air_hi landing_air_lw
special_n special_s special_hi special_lw
special_air_n special_air_s special_air_hi special_air_lw
special_n_start special_n_end special_air_n_start special_air_n_end
special_s_start special_s_end special_air_s_start special_air_s_end
special_hi_start special_hi_end special_air_hi_start special_air_hi_end
special_lw_start special_lw_end special_air_lw_start special_air_lw_end
special_hi_fall special_hi_open special_hi_close special_lw_hit special_air_lw_hit
special_lw_shoot special_air_lw_shoot special_lw_catch special_air_lw_catch
special_lw_wait special_air_lw_wait special_lw_wait_start special_air_lw_wait_start
special_lw_reflect special_air_lw_reflect
catch catch_dash catch_turn catch_wait catch_attack catch_pull catch_dash_pull
catch_cut throw_f throw_b throw_hi throw_lw
capture_pulled_hi capture_pulled_lw capture_wait_hi capture_wait_lw
capture_damage_hi capture_damage_lw capture_cut thrown_f thrown_b thrown_hi thrown_lw
guard guard_on guard_off guard_damage escape escape_f escape_b escape_n
escape_air escape_air_slide
damage_hi_1 damage_hi_2 damage_hi_3 damage_n_1 damage_n_2 damage_n_3
damage_lw_1 damage_lw_2 damage_lw_3 damage_air_1 damage_air_2 damage_air_3
damage_fly_hi damage_fly_n damage_fly_lw damage_fly_top damage_fly_roll
damage_fly_meteor damage_elec wall_damage ceil_damage
down_bound_u down_bound_d down_wait_u down_wait_d down_stand_u down_stand_d
down_attack_u down_attack_d down_forward_u down_forward_d down_back_u down_back_d
down_damage_u down_damage_d down_damage_u3 down_damage_d3 down_eat_u down_eat_d
passive passive_stand_f passive_stand_b passive_wall passive_wall_jump passive_ceil
cliff_catch cliff_wait cliff_climb_quick cliff_climb_slow cliff_attack_quick
cliff_attack_slow cliff_escape_quick cliff_escape_slow cliff_jump_quick1
cliff_jump_quick2 cliff_jump_slow1 cliff_jump_slow2 cliff_catch_move
furafura furafura_start_u furafura_start_d furafura_end shield_break_fly
shield_break_down_u shield_break_down_d swallowed swallowed_wait
miss_foot slip slip_wait slip_stand slip_attack ottotto ottotto_wait
appeal_hi_l appeal_hi_r appeal_s_l appeal_s_r appeal_lw_l appeal_lw_r
entry_l entry_r rebirth final final_air final_start final_air_start
item_light_get item_heavy_get item_light_throw_f item_light_throw_b
item_light_throw_hi item_light_throw_lw item_light_throw_dash
item_light_throw_air_f item_light_throw_air_b item_light_throw_air_hi
item_light_throw_air_lw item_heavy_throw_f item_heavy_throw_b damage_fall
""".split() + [f'special_{air}s_{n}' for air in ('', 'air_') for n in range(1, 10)]
# Game & Watch's Judge animation names carry the number (special_s_1..9).


def consts(text, prefix):
    for name, value in re.findall(r'^\s*' + prefix + r'([A-Z0-9_]+): 0x([0-9A-F]+),', text, re.M):
        yield name.lower(), int(value, 16)


def is_move(name):
    for head in ('attack', 'special', 'catch', 'throw', 'final', 'cliff_attack',
                 'down_stand_attack', 'slip_stand_attack', 'item_throw', 'item_swing',
                 'item_shoot', 'ladder_attack'):
        if name == head or name.startswith(head + '_'):
            return True
    return False


def table(entries, first, last):
    by_value = {}
    for name, value in entries:
        if first <= value <= last:
            by_value.setdefault(value, name)
    return [by_value.get(v) for v in range(first, last + 1)]


def emit_status(var, names):
    print(f'pub static {var}: [(&str, bool); {len(names)}] = [')
    for name in names:
        print(f'    ("{name or ""}", {"true" if name and is_move(name) else "false"}),')
    print('];')


def main():
    text = open(sys.argv[1], encoding='utf-8').read()
    common = table(consts(text, 'FIGHTER_STATUS_KIND_'), 0, COMMON_LAST)
    gamewatch = table(consts(text, 'FIGHTER_GAMEWATCH_STATUS_KIND_'), COMMON_LAST + 1, 0x204)
    # Range markers (BASE_HEAD, OTHER_NUM, ...) share values with real fighters.
    markers = re.compile(r'^(base|other|base_append)_(head|tail|num)$|^(all|none|term)$')
    fighters = table(((n, v) for n, v in consts(text, 'FIGHTER_KIND_') if not markers.match(n)),
                     0, 0x5d)
    motions = sorted({zlib.crc32(m.encode()) | len(m) << 32: m for m in MOTIONS}.items())
    print('//! Generated by `tools/gen_names.py` from skyline-smash `lua_const.rs`; do')
    print('//! not edit. Names are the game\'s own constants, lowercased, without prefix.')
    print()
    print(f'pub const COMMON_STATUS_LAST: i32 = {COMMON_LAST:#x};')
    print(f'pub const GAMEWATCH: u32 = {GAMEWATCH:#x};')
    print()
    print('/// Common fighter statuses `0..=COMMON_STATUS_LAST`: name, whether it is a move.')
    emit_status('COMMON_STATUS', common)
    print()
    print('/// Game & Watch statuses from `COMMON_STATUS_LAST + 1`.')
    emit_status('GAMEWATCH_STATUS', gamewatch)
    print()
    print('/// Fighter kinds by value.')
    print(f'pub static FIGHTER: [&str; {len(fighters)}] = [')
    for name in fighters:
        print(f'    "{name or ""}",')
    print('];')
    print()
    print('/// Motion names by hash40, sorted for binary search.')
    print(f'pub static MOTION: [(u64, &str); {len(motions)}] = [')
    for value, name in motions:
        print(f'    ({value:#012x}, "{name}"),')
    print('];')


main()
