"""Delivered capabilities from selected build config and installed contracts."""
# SPDX-License-Identifier: GPL-2.0-only
import hashlib
import json
from pathlib import Path
import re
from tools.production.privacy import public_distribution, release_tree


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def config(path):
    return dict(re.findall(r'^(\w+)=(.*)$', Path(path).read_text(), re.M))


def delivered(build, manifest):
    build = Path(build)
    installed = build/'buildroot/target/etc/y2linux'
    kernel = config(build/'kernel/.config')
    root = config(build/'buildroot/.config')
    caps = json.loads((installed/'capabilities.json').read_text())
    codecs = json.loads((installed/'bluetooth-codecs.json').read_text())
    audio = json.loads((installed/'audio-enabled.json').read_text())
    versions = json.loads((installed/'versions.json').read_text())
    for field in ('build_git_commit', 'reborn_source_commit', 'kernel_version', 'rootfs_version'):
        if versions.get(field) != manifest.get(field):
            raise ValueError('delivered_source_identity:'+field)
    # Built configuration is evidence of implementation, never hardware success.
    symbols = {'kernel': {k:v for k,v in kernel.items() if v in ('y','m')},
               'buildroot': {k:v for k,v in root.items() if v == 'y'}}
    bindings = {
        'CONFIG_Y2_PLATFORM': ('usb_device', 'usb_host', 'display_off'),
        'CONFIG_USB_INVENTRA_DMA': ('usb_dma',), 'CONFIG_USB_G_NCM': ('usb_ncm',),
        'CONFIG_Y2_POWER': ('power_observation', 'deep_suspend', 'slow_idle', 'cpu_power_down_idle',
            'charging', 'system_idle_coordinator', 'local_timer', 'dvfs', 'workload_qos', 'system_watchdog'),
        'CONFIG_Y2_CONNECTIVITY': ('wifi', 'bluetooth', 'wifi_power_save', 'radio_coexistence'),
        'CONFIG_MMC_MTK': ('storage', 'emmc', 'sd'),
        'CONFIG_CPU_IDLE': ('cpuidle',), 'CONFIG_HIGH_RES_TIMERS': ('high_resolution_timers',),
        'CONFIG_NO_HZ_IDLE': ('no_hz_idle',), 'CONFIG_CPU_FREQ': ('cpufreq',),
        'CONFIG_HOTPLUG_CPU': ('cpu_hotplug',), 'CONFIG_RTC_DRV_MT6397': ('rtc_time','rtc_alarm'),
        'CONFIG_SND_SOC_MT6582': ('audio','wired_48'), 'CONFIG_SND_SOC_CS43130': ('cs43131_controls',),
        'CONFIG_DRM_LIMA': ('gpu_runtime_pm',),
    }
    symbol_for = {name: symbol for symbol,names in bindings.items() for name in names}
    modules = {'telemetry':'observe.py','health':'health.py','shutdown':'shutdown.py',
               'low_battery_shutdown':'power.py','battery_soc':'battery.py','ota':'update.py',
               'diagnostics':'diagnostics.py','memory_telemetry':'observe.py'}
    codec_names = {'bluetooth_sbc':'SBC','bluetooth_sbc_xq':'SBC','bluetooth_aac':'AAC',
                   'bluetooth_aptx':'aptX','bluetooth_aptx_hd':'aptX-HD','bluetooth_ldac':'LDAC'}
    conditional = {'cpu_power_down_idle','deep_suspend','wired_s32','wired_native_88200','wired_native_96000'}
    entries = {}
    for name, value in caps['capabilities'].items():
        symbol = symbol_for.get(name)
        enabled, experimental = bool(value['enabled']), bool(value['experimental'])
        if symbol:
            compiled, evidence = kernel.get(symbol) in ('y','m'), symbol
        elif name in modules:
            evidence = 'usr/lib/y2-platform/y2_platform/'+modules[name]
            compiled = (build/'buildroot/target'/evidence).is_file()
        elif name in codec_names:
            item = codecs['codecs'][codec_names[name]]
            compiled, evidence = item['compiled_locally'], 'configured BlueALSA encoder'
            enabled = item.get('default_runtime_enabled', name == 'bluetooth_sbc')
            experimental = item.get('experimental', False)
            if name == 'bluetooth_sbc_xq':
                enabled = codecs.get('sbc_xq_enabled', False)
                experimental = True
        elif name == 'audio_rate_conversion':
            evidence = 'usr/lib/reborn/libreborn_media.so'
            compiled = (build/'buildroot/target'/evidence).is_file()
        elif not value['implemented']:
            compiled, evidence = False, 'source contract explicitly unimplemented'
        else:
            compiled, evidence = None, 'no automatic build binding; source implementation only'
        if enabled and compiled is False:
            raise ValueError('enabled_capability_not_compiled:'+name)
        entries[name] = {'compiled': compiled, 'implemented': bool(value['implemented']),
                         'enabled': enabled, 'experimental': experimental, 'physically_qualified': False,
                         'hardware_conditional': name in conditional,
                         'build_evidence': evidence, 'reason': value['reason']}
    return {'schema': 'org.y2linux.delivered-capabilities/v1', 'versions': versions,
            'selected_build_symbols': symbols, 'capabilities': entries, 'audio': audio, 'codecs': codecs,
            'current_candidate_physical_qualification': 'NOT_RUN',
            'distribution': public_distribution(manifest, codecs),
            'inputs': {name: digest(build/path) for name,path in
                       (('kernel_config','kernel/.config'),('buildroot_config','buildroot/.config'),
                        ('capabilities','buildroot/target/etc/y2linux/capabilities.json'),
                        ('audio','buildroot/target/etc/y2linux/audio-enabled.json'),
                        ('codecs','buildroot/target/etc/y2linux/bluetooth-codecs.json'))}}


def write(build, out, manifest, privacy):
    out = Path(out)
    caps = delivered(build, manifest)
    path = out/'metadata/delivered-capabilities.json'
    path.write_text(json.dumps(caps, indent=2)+'\n')
    value = {'schema': 'org.y2linux.release-composition/v1',
             'y2linux_commit': manifest['build_git_commit'], 'reborn_commit': manifest['reborn_source_commit'],
             'kernel': manifest['kernel_version'], 'rootfs': manifest['rootfs_version'],
             'release': manifest['release_version'], 'platform_api_version': manifest['platform_api_version'],
             'buildroot': json.loads((out/'metadata/buildroot-inputs.lock.json').read_text())['version'],
             'payloads': {p['target_partition']:p['raw'] for p in manifest['payloads']},
             'fallback': manifest['fallback'], 'firmware_expectations': manifest['owner_firmware'],
             'capabilities_sha256': digest(path), 'private_state_check': privacy,
             'distribution': caps['distribution'], 'distribution_intent': manifest.get('distribution_intent', 'owner-local'),
             'byte_reproducibility_proven': False,
             'source_obligations': 'Ship corresponding pinned sources, modifications, scripts and notices; legal-info alone is not a legal conclusion.'}
    (out/'metadata/release-composition.json').write_text(json.dumps(value, indent=2)+'\n')
    return {'composition_sha256': digest(out/'metadata/release-composition.json'),
            'capabilities_sha256': digest(path), 'private_state_checked': True,
            'public_distribution_ready': caps['distribution']['public_distribution_ready']}


def validate(out, manifest):
    """Check composition against the actual delivered payload and manifest axes."""
    out = Path(out)
    receipt = manifest.get('release_composition')
    if receipt is None:
        return  # Historical sealed packages predate composition metadata.
    composition = out/'metadata/release-composition.json'
    capabilities = out/'metadata/delivered-capabilities.json'
    if (digest(composition) != receipt.get('composition_sha256') or
            digest(capabilities) != receipt.get('capabilities_sha256')):
        raise ValueError('release_composition_hash')
    value, caps = json.loads(composition.read_text()), json.loads(capabilities.read_text())
    expected = {'y2linux_commit': manifest['build_git_commit'],
                'reborn_commit': manifest['reborn_source_commit'], 'kernel': manifest['kernel_version'],
                'rootfs': manifest['rootfs_version'], 'release': manifest['release_version'],
                'platform_api_version': manifest['platform_api_version'],
                'payloads': {p['target_partition']: p['raw'] for p in manifest['payloads']},
                'fallback': manifest['fallback'], 'firmware_expectations': manifest['owner_firmware'],
                'capabilities_sha256': digest(capabilities),
                'distribution_intent': manifest.get('distribution_intent', 'owner-local'),
                'buildroot': json.loads((out/'metadata/buildroot-inputs.lock.json').read_text())['version']}
    if value.get('schema') != 'org.y2linux.release-composition/v1' or any(
            value.get(k) != v for k, v in expected.items()):
        raise ValueError('release_composition_identity')
    for field in ('build_git_commit', 'reborn_source_commit', 'kernel_version', 'rootfs_version'):
        if caps.get('versions', {}).get(field) != manifest[field]:
            raise ValueError('release_capabilities_identity:'+field)
    if (caps.get('schema') != 'org.y2linux.delivered-capabilities/v1' or
            caps.get('current_candidate_physical_qualification') != 'NOT_RUN' or
            not isinstance(caps.get('capabilities'), dict) or not caps['capabilities'] or
            any(v.get('physically_qualified') is not False or
                v.get('enabled') and v.get('compiled') is False for v in caps.get('capabilities', {}).values())):
        raise ValueError('release_capabilities_evidence')
    checks = value.get('private_state_check', {})
    roots = {'new_root': next(p['raw'] for p in manifest['payloads'] if p['target_partition'] == 'ANDROID'),
             'fallback_root': next(p for p in manifest['fallback']['images'] if p['file'].endswith('/Y2ROOT.img'))}
    for name, payload in roots.items():
        check = checks.get(name, {})
        if (check.get('image_sha256') != payload['sha256'] or
                check.get('image_bytes_checked') != payload['size_bytes'] or
                check.get('filesystem_checked') is not True or check.get('private_state_found') is not False):
            raise ValueError('release_privacy_payload_binding:'+name)
    distribution = public_distribution(manifest, caps['codecs'])
    if (receipt.get('private_state_checked') is not True or
            value.get('distribution') != distribution or caps.get('distribution') != distribution or
            receipt.get('public_distribution_ready') != distribution['public_distribution_ready'] or
            manifest.get('distribution_intent') == 'public' and not distribution['public_distribution_ready']):
        raise ValueError('release_composition_distribution')
    release_tree(out, manifest)

