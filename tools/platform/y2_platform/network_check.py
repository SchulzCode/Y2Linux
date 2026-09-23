"""Explicit owner-run IP and throughput qualification, never run during boot."""
# SPDX-License-Identifier: GPL-2.0-only
import ipaddress
import json
from .observe import snapshot


def check(ctx, peer, seconds=10, throughput=False, interface='wlan0'):
    peer = str(ipaddress.IPv4Address(peer))
    if interface not in ('wlan0', 'usb0') or not 1 <= seconds <= 600:
        raise ValueError('invalid_network_qualification_parameters')
    record = ctx.record('network-qualification', {'peer': peer, 'interface': interface,
                        'seconds': seconds, 'throughput_requested': throughput})
    before = snapshot(ctx)
    result = {'schema': 'org.y2linux.benchmark/v1', 'record': record, 'before': before}
    if interface == 'wlan0' and before['wifi']['state'] != 'Online':
        record.update(result='FAILED', failure='wifi_not_online')
        return result
    result['ping'] = ctx.command(['ping', '-I', interface, '-c', '5', '-W', '2', peer], timeout=15)
    if throughput:
        # Existing pinned iperf3 package, no new network protocol or package.
        answer = ctx.command(['iperf3', '--bind-dev', interface, '-c', peer, '-t', str(seconds), '-J'],
                             timeout=seconds+10, limit=262144)
        try:
            result['iperf3'] = json.loads(answer['output']) if answer['ok'] else answer
        except (TypeError, ValueError):
            result['iperf3'] = {'ok': False, 'reason': 'invalid_iperf_json'}
        measured = result['iperf3']
        valid = (isinstance(measured, dict) and isinstance(measured.get('end'), dict) and
                 isinstance(measured['end'].get('sum_sent'), dict) and not measured.get('error'))
        record.update(result='OK' if answer['ok'] and valid and result['ping']['ok'] else 'FAILED')
    else:
        record['result'] = 'OK' if result['ping']['ok'] else 'FAILED'
    record['failure'] = None if record['result'] == 'OK' else 'network_test_failed_see_measurements'
    result['after'] = snapshot(ctx)
    return result
