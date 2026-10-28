"""Differential cases per port group (tests/test_platform_differential.py collects them).

Each module has `CASES` ((name, argv, options) as in test_platform_differential.CASES) and `fixture(root)`,
which adds the files its cases need to the shared player-shaped root.
"""
GROUPS = ('observation', 'connectivity', 'devices', 'storage', 'service', 'reports', 'power', 'suspend')
