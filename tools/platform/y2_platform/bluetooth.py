"""Normalize actual D-Bus observations, never codec requests, for platform clients."""
# SPDX-License-Identifier: GPL-2.0-only
import re


def normalize(ctx, raw):
    result = {'state': 'Unavailable', 'reason': 'dbus_observation_unavailable',
              'adapter': None, 'selected_peer': None, 'transport': None,
              'negotiated_codec': None, 'pcm': None,
              'remote_advertised': None, 'remote_advertised_reason': 'API_exposes_mutual_subset_only',
              'runtime_enabled': None, 'mutually_usable': None,
              'preferred_codec': None, 'requested_codec': None,
              'actual_bitrate_bps': None, 'packet_loss': None,
              'reconnect_owner': 'y2-bt-reconnect',
              'reconnect': ctx.json('/run/y2/bt-reconnect.json'),
              'codec_inventory': ctx.json('/etc/y2linux/bluetooth-codecs.json')}
    if not isinstance(raw, dict) or raw.get('schema') != 1 or not raw.get('stable_owners'):
        return result
    bluez, bluealsa = raw.get('bluez'), raw.get('bluealsa')
    if not isinstance(bluez, dict):
        return result
    adapters = [(path, interfaces['org.bluez.Adapter1']) for path, interfaces in bluez.items()
                if isinstance(interfaces, dict) and isinstance(interfaces.get('org.bluez.Adapter1'), dict)]
    if len(adapters) != 1:
        result['reason'] = 'no_unique_adapter'
        return result
    path, adapter = adapters[0]
    result['adapter'] = {'path': path, 'powered': adapter.get('Powered'),
                         'discovering': adapter.get('Discovering'), 'bluez_owner': raw.get('bluez_owner')}
    if adapter.get('Powered') is not True:
        result['reason'] = 'radio_off'
        return result
    if not raw.get('bluealsa_owner') or not isinstance(bluealsa, dict) or not isinstance(raw.get('manager'), dict):
        result.update(state='Starting', reason='bluealsa_not_ready')
        return result
    result.update(state='Ready', reason='controller_bluez_bluealsa_observed')
    codes = raw['manager'].get('Codecs')
    result['runtime_enabled'] = [code.split(':', 1)[1] for code in codes
                                 if isinstance(code, str) and code.lower().startswith('a2dp-source:')] if isinstance(codes, list) else None
    if not result['runtime_enabled'] or 'SBC' not in result['runtime_enabled']:
        result.update(state='Degraded', reason='a2dp_source_sbc_not_enabled')
    if str(raw['manager'].get('Version', '')).lstrip('v') != '5.0.0':
        result.update(state='Degraded', reason='bluealsa_runtime_version_mismatch')
    devices = []
    for device_path, interfaces in bluez.items():
        device = interfaces.get('org.bluez.Device1') if isinstance(interfaces, dict) else None
        if not isinstance(device, dict):
            continue
        devices.append({'path': device_path, 'address': device.get('Address'),
                        'paired': device.get('Paired'), 'trusted': device.get('Trusted'),
                        'connected': device.get('Connected'), 'services_resolved': device.get('ServicesResolved')})
    result['devices'] = devices[:128]
    preferred = ctx.read('/data/bluetooth/preferred-audio')
    if preferred and re.fullmatch(r'(?:[A-Fa-f0-9]{2}:){5}[A-Fa-f0-9]{2}', preferred):
        selected = [d for d in devices if str(d['address']).lower() == preferred.lower()]
    else:
        selected = [d for d in devices if d['connected'] is True]
    if len(selected) != 1:
        result['transport_reason'] = 'no_unique_selected_peer'
        return result
    device = result['selected_peer'] = selected[0]
    if device['connected'] is not True:
        result['transport_reason'] = 'selected_peer_disconnected'
        return result
    pcms = [(path, interfaces['org.bluealsa.PCM1']) for path, interfaces in bluealsa.items()
            if isinstance(interfaces, dict) and isinstance(interfaces.get('org.bluealsa.PCM1'), dict) and
            interfaces['org.bluealsa.PCM1'].get('Device') == device['path'] and
            interfaces['org.bluealsa.PCM1'].get('Transport') == 'A2DP-source' and
            interfaces['org.bluealsa.PCM1'].get('Mode') == 'sink']
    if len(pcms) != 1:
        result['transport_reason'] = 'no_unique_playback_pcm'
        return result
    path, pcm = pcms[0]
    formats = {0x8210: ('S16_LE', 16, 16), 0x8418: ('S24_LE', 24, 32), 0x8420: ('S32_LE', 32, 32)}
    format_code = pcm.get('Format')
    fmt = formats.get(format_code) if type(format_code) is int else None
    rate, channels = pcm.get('Rate'), pcm.get('Channels')
    result['pcm'] = {'format_code': format_code, 'format': fmt[0] if fmt else None,
                     'valid_bits': fmt[1] if fmt else None, 'physical_bits': fmt[2] if fmt else None,
                     'rate_hz': rate, 'channels': channels}
    result['negotiated_codec'] = pcm.get('Codec') if isinstance(pcm.get('Codec'), str) else None
    result['transport'] = {'object': path, 'bluealsa_owner': raw['bluealsa_owner'],
                           'connection_sequence': pcm.get('Sequence'), 'running': pcm.get('Running'),
                           'lifetime_note': 'snapshot; use ObjectManager removal signals for per-open lifetime'}
    if not fmt or type(rate) is not int or not 8000 <= rate <= 192000 or channels != 2:
        result.update(state='Degraded', reason='unsupported_or_incomplete_pcm')
    if raw.get('codec_pcm') == path and isinstance(raw.get('mutually_available_codecs'), dict):
        result['mutually_usable'] = raw['mutually_available_codecs']
    return result
