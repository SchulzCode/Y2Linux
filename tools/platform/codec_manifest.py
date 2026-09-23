#!/usr/bin/env python3
"""Build-time codec inventory from the configured pinned BlueALSA source."""
# SPDX-License-Identifier: GPL-2.0-only
import hashlib
import json
from pathlib import Path
import re
import sys


def manifest(build):
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
                        'gate': 'PHYSICAL_GATE' if name == 'SBC' else 'BLOCKED_BY_EVIDENCE'}
    unexpected = re.findall(r'^#define ENABLE_(AAC|APTX|APTX_HD|LDAC|LHDC|MPEG|OPUS|LC3PLUS|FASTSTREAM|ASHA) 1$', config, re.M)
    if unexpected:
        raise ValueError('production_optional_codec_not_approved:' + ','.join(unexpected))
    return {'schema': 1, 'bluealsa_version': '5.0.0', 'sbc_version': '2.2',
            'bluealsa_source_sha256': 'e1249cbebd24925c977814f62adab71f2ebd771e28e6a4ac58bb1ac97ce3beb5',
            'sbc_source_sha256': 'a1ada76ef35e5af9c2fbd063754dc9e37a8d989417c6eb1ecebb089b1383ae9e',
            'config_sha256': hashlib.sha256(config.encode()).hexdigest(), 'codecs': codecs,
            'sbc_quality': 'conformant_default_high', 'sbc_xq_enabled': False,
            'live_bitrate_observation': False, 'le_audio_iso_supported': False}


if __name__ == '__main__':
    print(json.dumps(manifest(Path(sys.argv[1])), sort_keys=True, indent=2))
