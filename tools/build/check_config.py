"""Reject deviation from the reviewed config fragment and derived D08 essentials."""
import re
import sys
from pathlib import Path


def parse(path):
    result = {}
    for line in Path(path).read_text().splitlines():
        if match := re.fullmatch(r'(CONFIG_\w+)=(.*)', line):
            result[match[1]] = match[2]
        elif match := re.fullmatch(r'# (CONFIG_\w+) is not set', line):
            result[match[1]] = 'n'
    return result


def check(config, fragment):
    actual = parse(config)
    wanted = parse(fragment)
    wanted.update({key: 'y' for key in ['CONFIG_OF', 'CONFIG_USE_OF', 'CONFIG_ARM_GIC',
        'CONFIG_MTK_TIMER', 'CONFIG_GENERIC_IRQ_MULTI_HANDLER', 'CONFIG_CPU_V7']})
    errors = [f'{k}: expected {v}, found {actual.get(k, "n")}'
              for k, v in wanted.items() if actual.get(k, 'n') != v]
    if errors:
        raise ValueError('\n'.join(errors))
    print('Reviewed kernel configuration matches D08 and diagnostic fragment.')


if __name__ == '__main__':
    check(*sys.argv[1:])
