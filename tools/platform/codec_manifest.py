#!/usr/bin/env python3
"""Build-time codec inventory from the configured pinned BlueALSA source."""
# SPDX-License-Identifier: GPL-2.0-only
import hashlib
import json
from pathlib import Path
import re
import sys


def manifest(build, profile='sbc-only'):
    if profile not in ('sbc-only', 'owner-private-experiments'):
        raise ValueError('unknown_codec_profile')
    experiments = profile == 'owner-private-experiments'
    config = (build / 'config.h').read_text()
    if '#define PACKAGE_VERSION "v5.0.0"' not in config:
        raise ValueError('unexpected_bluealsa_api_version')
    if not (build / 'src/bluealsad').is_file():
        raise ValueError('bluealsad_not_built')
    codecs = {}
    for name, flag in [('SBC', None), ('AAC', 'AAC'), ('aptX', 'APTX'), ('aptX-HD', 'APTX_HD'), ('LDAC', 'LDAC')]:
        compiled = flag is None or bool(re.search(r'^#define ENABLE_' + flag + r' 1$', config, re.M))
        codecs[name] = {'compiled_locally': compiled, 'distribution_approved': name == 'SBC',
                        'platform_qualified': False,
                        'owner_private_experiment': experiments and compiled and name != 'SBC',
                        'default_runtime_enabled': name == 'SBC',
                        'gate': 'PHYSICAL_GATE' if name == 'SBC' else 'PUBLIC_DISTRIBUTION_AND_PHYSICAL_GATE'}
    unexpected = re.findall(r'^#define ENABLE_(AAC|APTX|APTX_HD|LDAC|LHDC|MPEG|OPUS|LC3PLUS|FASTSTREAM|ASHA) 1$', config, re.M)
    allowed = {'AAC', 'APTX', 'APTX_HD', 'LDAC'} if experiments else set()
    if set(unexpected) - allowed:
        raise ValueError('production_optional_codec_not_approved:' + ','.join(unexpected))
    if experiments and set(unexpected) != allowed:
        raise ValueError('owner_private_codec_profile_incomplete')
    if experiments and ('#define WITH_LIBFREEAPTX 1' not in config or
                        '#define WITH_LIBOPENAPTX 1' in config):
        raise ValueError('owner_private_codec_library_mismatch')
    return {'schema': 1, 'profile': profile, 'bluealsa_version': '5.0.0', 'sbc_version': '2.2',
            'bluealsa_source_sha256': 'e1249cbebd24925c977814f62adab71f2ebd771e28e6a4ac58bb1ac97ce3beb5',
            'sbc_source_sha256': 'a1ada76ef35e5af9c2fbd063754dc9e37a8d989417c6eb1ecebb089b1383ae9e',
            'config_sha256': hashlib.sha256(config.encode()).hexdigest(), 'codecs': codecs,
            'sbc_quality': 'conformant_default_high', 'sbc_xq_enabled': False,
            'live_bitrate_observation': False, 'le_audio_iso_supported': False,
            'optional_encoder_provenance': json.loads((Path(__file__).with_name('codec-sources.json')).read_text())
                if experiments else {}}


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('build', type=Path)
    parser.add_argument('--profile', choices=('sbc-only', 'owner-private-experiments'), default='sbc-only')
    parser.add_argument('--daemon-args', type=Path)
    args = parser.parse_args()
    result = manifest(args.build, args.profile)
    if args.daemon_args:
        # BlueALSA enables AAC by default when compiled. Merely listing SBC
        # does not disable other endpoints; use explicit negative codec flags.
        disabled = ['--codec=-' + name for name, codec in result['codecs'].items()
                    if name != 'SBC' and codec['compiled_locally']]
        args.daemon_args.write_text(' '.join(disabled) + '\n')
    print(json.dumps(result, sort_keys=True, indent=2))
