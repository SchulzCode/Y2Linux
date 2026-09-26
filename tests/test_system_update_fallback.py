"""A root-overlay fallback must match both its real ext4 image and old kernel."""
import copy
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from tools.production.layout import TARGETS, digest
from tools.production.system_update import overlay_fallback
from tools.production.validate import validate_checksums


class OverlayFallback(unittest.TestCase):
    def test_nested_checksum_receipts_are_deliverables_too(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            (root/'evidence').mkdir()
            nested=root/'evidence/SHA256SUMS'
            nested.write_text('retained evidence receipt\n')
            (root/'manifest.json').write_text('{}\n')
            sums=root/'SHA256SUMS'
            sums.write_text(''.join(digest(p)+'  '+str(p.relative_to(root))+'\n'
                                   for p in (nested,root/'manifest.json')))
            validate_checksums(root)
            nested.write_text('altered evidence receipt\n')
            with self.assertRaises(ValueError):
                validate_checksums(root)
            # Omitting the nested receipt must also fail, even if the rest matches.
            sums.write_text(digest(root/'manifest.json')+'  manifest.json\n')
            with self.assertRaises(ValueError):
                validate_checksums(root)

    @unittest.skipUnless(all(shutil.which(name) for name in ('mke2fs', 'debugfs', 'e2fsck', 'blkid')),
                         'requires native e2fsprogs/blkid; run explicitly on the packaging host')
    def test_real_image_versions_and_compatible_boot_are_required(self):
        with tempfile.TemporaryDirectory() as directory:
            area = Path(directory)
            image = area/'Y2ROOT.img'
            image.write_bytes(b'')
            with image.open('r+b') as stream:
                stream.truncate(8 * 1024 * 1024)
            root_contract = TARGETS['ANDROID']
            subprocess.run(['mke2fs', '-q', '-t', 'ext4', '-F', '-L', root_contract['label'],
                            '-U', root_contract['uuid'], str(image)], check=True,
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            versions = dict(kernel_version='kernel-fixture', layout_version=1,
                            data_schema_version=1, platform_api_version=1,
                            kernel_source_commit='a'*40, build_git_commit='b'*40,
                            rootfs_version='overlay-fixture')
            source = area/'versions.json'
            source.write_text(json.dumps(versions))
            for command in ('mkdir /etc', 'mkdir /etc/y2linux',
                            f'write {source} /etc/y2linux/versions.json'):
                subprocess.run(['debugfs', '-w', '-R', command, str(image)], check=True,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            boot = dict(target_partition='BOOTIMG', raw=dict(file='BOOTIMG.img',
                        size_bytes=1024, sha256='c'*64))
            root = dict(target_partition='ANDROID', version='base-fixture',
                        raw=dict(file='Y2ROOT.img', size_bytes=image.stat().st_size, sha256='d'*64),
                        filesystem=dict(type='ext4', label=root_contract['label'], uuid=root_contract['uuid']))
            previous = dict(versions, build_git_commit='a'*40,
                            payloads=[boot, root, dict(target_partition='USRDATA')])
            overlay = dict(schema='org.y2linux.telemetry-root-overlay/v1',
                           platform_versions=versions, source_commit='b'*40,
                           base_platform_source_commit='a'*40,
                           required_installed_bootimg=boot['raw'],
                           selected_partitions=['ANDROID'], bootimg_payload_included=False,
                           root=dict(file='Y2ROOT.img', bytes=image.stat().st_size, sha256=digest(image)))
            result = overlay_fallback(previous, overlay, image)
            self.assertEqual(result['version'], 'overlay-fixture')
            self.assertEqual(result['raw']['sha256'], digest(image))
            self.assertEqual(previous['payloads'][1]['version'], 'base-fixture')

            mutations = [
                ('boot hash', lambda x: x['required_installed_bootimg'].update(sha256='e'*64)),
                ('root hash', lambda x: x['root'].update(sha256='e'*64)),
                ('root path', lambda x: x['root'].update(file='../Y2ROOT.img')),
                ('geometry', lambda x: x['root'].update(bytes=image.stat().st_size+512)),
                ('kernel', lambda x: x['platform_versions'].update(kernel_version='other')),
                ('data schema', lambda x: x['platform_versions'].update(data_schema_version=2)),
                ('source', lambda x: x.update(source_commit='e'*40)),
                ('image versions', lambda x: x['platform_versions'].update(rootfs_version='invented')),
                ('scope', lambda x: x.update(selected_partitions=['ANDROID', 'USRDATA'])),
            ]
            for name, change in mutations:
                with self.subTest(name=name):
                    altered = copy.deepcopy(overlay)
                    change(altered)
                    with self.assertRaises(ValueError):
                        overlay_fallback(previous, altered, image)


if __name__ == '__main__':
    unittest.main()
