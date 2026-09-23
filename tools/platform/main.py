#!/usr/bin/python3
# SPDX-License-Identifier: GPL-2.0-only
import sys
sys.path.insert(0, '/usr/lib/y2-platform')
from y2_platform.cli import main
try:
    raise SystemExit(main())
except (OSError, ValueError, RuntimeError) as error:
    import json
    print(json.dumps({'schema': 'org.y2linux.error/v1', 'state': 'Failed',
                      'reason': str(error), 'type': type(error).__name__}))
    raise SystemExit(1)
