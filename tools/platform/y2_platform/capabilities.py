"""Runtime enablement is independent of source implementation and qualification."""
# SPDX-License-Identifier: GPL-2.0-only
def status(ctx):
    result = ctx.json('/etc/y2linux/capabilities.json', {'schema': 'org.y2linux.capabilities/v1', 'state': 'Unavailable'})
    result['platform_version'] = ctx.json('/etc/y2linux/versions.json', {}).get('release_version')
    caps = result.get('capabilities', {})
    for name, path in [('wifi', '/data/network/enabled'), ('bluetooth', '/data/bluetooth/enabled')]:
        if name in caps:
            caps[name]['enabled'] = ctx.integer(path) == 1
    inventory = ctx.json('/etc/y2linux/bluetooth-codecs.json', {})
    policy = ctx.json('/data/bluetooth/codec-policy.json', {})
    mapping = {'sbc': 'SBC', 'aac': 'AAC', 'aptx': 'aptX', 'aptx_hd': 'aptX-HD', 'ldac': 'LDAC'}
    for key, name in mapping.items():
        cap = caps.get('bluetooth_' + key)
        if cap is None:
            continue
        actual = inventory.get('codecs', {}).get(name, {})
        cap['implemented'] = actual.get('compiled_locally') is True
        cap['enabled'] = cap['implemented'] and (name == 'SBC' or (
            actual.get('distribution_approved') is True and actual.get('platform_qualified') is True) or (
            policy.get('schema') == 1 and policy.get('experimental') is True and actual.get('owner_private_experiment') is True))
        cap['qualified'] = actual.get('platform_qualified') is True
        cap['enablement_scope'] = 'daemon policy; active PCM reported separately'
    if 'bluetooth_sbc_xq' in caps:
        caps['bluetooth_sbc_xq']['enabled'] = (policy.get('schema') == 1 and policy.get('experimental') is True
                                               and policy.get('sbc_quality') in ('xq', 'xq+'))
    if 'wifi_power_save' in caps:
        caps['wifi_power_save']['enabled'] = ctx.json('/run/y2/wifi-power.json', {}).get('enabled') is True
    return result
