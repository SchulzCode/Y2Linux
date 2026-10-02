"""Separate peer configuration, encoder observation and quality preference."""
# SPDX-License-Identifier: GPL-2.0-only


def quality(codec, pcm, settings):
    config = pcm.get('CodecConfiguration')
    config = config if isinstance(config, list) and all(type(v) is int and 0 <= v <= 255 for v in config) else []
    stats = pcm.get('EncoderStats')
    live = (pcm.get('Running') is True and isinstance(stats, dict) and stats.get('Active') == 1)
    def observed(key, minimum=0, maximum=2**31-1):
        value = stats.get(key) if live else None
        return value if type(value) is int and minimum <= value <= maximum else None
    value = {'preference': settings.get('requested'),
             'encoder_observed': live, 'bitrate_bps': None,
             'bitrate_scope': 'encoder_payload_nominal_not_radio_throughput',
             'sbc_quality': None, 'sbc_bitpool': None, 'sbc_xq_peer_mode': None,
             'ldac_quality_index': None, 'ldac_nominal_choices_kbps': None,
             'ldac_abr_enabled': None, 'ldac_abr_adjustments': None,
             'ldac_abr_adaptation_observed': False, 'encoder_errors': observed('Errors')}
    if codec == 'SBC':
        if len(config) == 4:
            # A2DP SBC: 44.1kHz dual-channel, 16 blocks, 8 subbands, loudness.
            xq_mode = config[0] == 0x24 and config[1] == 0x15
            value['sbc_xq_peer_mode'] = xq_mode and config[3] >= 38
            value['sbc_negotiated_bitpool_range'] = config[2:4]
            bitpool = observed('Bitpool', 2, 250)
            value['sbc_bitpool'] = bitpool
            if bitpool is not None:
                value['sbc_quality'] = 'xq+' if xq_mode and bitpool == 47 else 'xq' if xq_mode and bitpool == 38 else 'standard'
    elif codec == 'LDAC':
        rate = pcm.get('Rate')
        value['ldac_nominal_choices_kbps'] = [303, 606, 909] if rate in (44100, 88200) else [330, 660, 990] if rate in (48000, 96000) else None
        value['ldac_quality_index'] = observed('QualityIndex', 0, 15)
        enabled = observed('AbrEnabled', 0, 1)
        value['ldac_abr_enabled'] = bool(enabled) if enabled is not None else None
        value['ldac_abr_adjustments'] = observed('AbrAdjustments')
        value['ldac_abr_adaptation_observed'] = enabled == 1 and (value['ldac_abr_adjustments'] or 0) > 0
    if codec in ('SBC', 'LDAC'):
        bitrate = observed('BitrateKbps', 1, 2000)
        value['bitrate_bps'] = bitrate * 1000 if bitrate else None
    return value
