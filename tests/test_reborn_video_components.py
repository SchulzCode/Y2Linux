"""Local video needs target codecs; unrelated FFmpeg capabilities must stay disabled."""
import importlib.util
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    'verify_ffmpeg_video', ROOT / 'tools/production/verify-ffmpeg.py')
VERIFY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFY)


class VideoComponents(unittest.TestCase):
    def manifest(self):
        return {
            'version': '9.0.2', 'libraries': VERIFY.EXPECTED_LIBRARIES,
            'configuration': '--disable-network --disable-avdevice --disable-programs',
            'demuxers': list(VERIFY.REQUIRED_DEMUXERS),
            'audio_decoders': list(VERIFY.REQUIRED_DECODERS),
            'artwork_decoders': list(VERIFY.REQUIRED_ARTWORK),
            'video_decoders': list(VERIFY.REQUIRED_VIDEO),
            'parsers': list(VERIFY.REQUIRED_PARSERS),
            'filters': list(VERIFY.REQUIRED_FILTERS),
            'protocols': ['file'], 'encoders': [], 'muxers': [],
        }

    def test_complete_local_manifest_is_accepted(self):
        self.assertEqual(VERIFY.check(self.manifest()), [])

    def test_old_audio_only_image_cannot_qualify_video(self):
        value = self.manifest()
        value['video_decoders'] = ['vp8']
        self.assertIn('missing video_decoders: h264', VERIFY.check(value))
        value['demuxers'].remove('matroska')
        self.assertIn('missing demuxers: matroska', VERIFY.check(value))

    def test_video_does_not_weaken_the_protocol_or_encoder_policy(self):
        value = self.manifest()
        value['protocols'].append('https')
        value['encoders'].append('h264')
        errors = VERIFY.check(value)
        self.assertTrue(any('protocol policy' in error for error in errors))
        self.assertTrue(any('unexpected encoders' in error for error in errors))

    def test_buildroot_explicitly_selects_every_new_component(self):
        text = (ROOT / 'buildroot/configs/y2_production_defconfig').read_text()
        def configured(name):
            return set(re.search(r'^BR2_PACKAGE_FFMPEG_' + name + r'="([^"]*)"',
                                 text, re.M).group(1).split())
        self.assertTrue(VERIFY.REQUIRED_VIDEO <= configured('DECODERS'))
        self.assertTrue({'h264', 'vp8'} <= configured('PARSERS'))
        self.assertIn('matroska', configured('DEMUXERS'))
        self.assertEqual(configured('PROTOCOLS'), {'file'})
        self.assertEqual(configured('ENCODERS'), set())


if __name__ == '__main__':
    unittest.main()
