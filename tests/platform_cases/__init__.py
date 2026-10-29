"""Golden cases per port group (tests/test_platform_differential.py collects them; the goldens are
tests/platform_goldens/<group>/<case>.json).

Each module has `CASES` ((name, argv, options) as in test_platform_differential.CASES) and `fixture(root)`,
which adds the files its cases need to the shared player-shaped root. `EXTRA_CASES` and `SERVED_CASES` are run
by the group's own module (tests/test_platform_<group>.py), with what a plain run cannot give.
"""
GROUPS = ('observation', 'connectivity', 'devices', 'storage', 'service', 'reports', 'power', 'suspend')
