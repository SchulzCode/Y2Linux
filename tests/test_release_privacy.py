import io
import json
import shutil
import subprocess
from pathlib import Path
import tempfile
import unittest
from tools.production import privacy as p


class Privacy(unittest.TestCase):
    def test_large_and_split_private_key_rejected(self):
        for prefix in (b'x'*70000, b'x'*(65536-12)):
            with self.assertRaisesRegex(ValueError, 'private_state_content'):
                p.check_stream(io.BytesIO(prefix+b'-----BEGIN OPENSSH PRIVATE KEY-----\n'+b'A'*80), 'large')
        with self.assertRaisesRegex(ValueError, 'private_state_content'):
            p.check_stream(io.BytesIO(b'network={\n  psk="secret"\n}'), 'network')

    def test_paths_and_public_gate(self):
        for name in ('NVRAM.img', 'preloader_y2.bin', 'PROTECT_F.img', 'Y2DATA.img',
                     'data/ssh/host-keys/anything', 'library.db', 'home/owner/id_ed25519'):
            with self.subTest(name=name), self.assertRaises(ValueError):
                p.check_name(name)
        for name in ('BOOTIMG.img', 'Y2ROOT.img', 'usr/bin/reborn', 'etc/ssl/cert.pem'):
            p.check_name(name)
        result = p.public_distribution({'owner_firmware': {'redistribution_permission_established': False}},
                                      {'codecs': {'SBC': {'compiled_locally': True}, 'LDAC': {'compiled_locally': True}}})
        self.assertFalse(result['public_distribution_ready'])
        self.assertEqual(len(result['blockers']), 2)

    def test_public_gate_requires_complete_typed_codec_inventory(self):
        manifest = {'owner_firmware': {'redistribution_permission_established': True}}
        records = {name: {'compiled_locally': name == 'SBC', 'distribution_approved': name == 'SBC'}
                   for name in ('SBC', 'AAC', 'aptX', 'aptX-HD', 'LDAC')}
        self.assertTrue(p.public_distribution(manifest, {'schema': 1, 'codecs': records})['public_distribution_ready'])
        for value in ({}, None, {'schema': 1, 'codecs': {}}, {'codecs': records}):
            self.assertFalse(p.public_distribution(manifest, value)['public_distribution_ready'])
        records['LDAC']['compiled_locally'] = 'false'
        self.assertFalse(p.public_distribution(manifest, {'schema': 1, 'codecs': records})['public_distribution_ready'])

    def test_tree_does_not_follow_live_data_links(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)/'root'; root.mkdir()
            secret = Path(temporary)/'secret';secret.write_bytes(b'-----BEGIN PRIVATE KEY-----\n'+b'A'*80)
            (root/'private-link').symlink_to(secret)
            (root/'public').write_bytes(b'plain safe fixture')
            self.assertEqual(p.tree(root)['regular_files_checked'], 1)
            (root/'private').write_bytes(secret.read_bytes())
            with self.assertRaises(ValueError):
                p.tree(root)


    def test_release_namespace_rejects_extra_images_and_private_metadata(self):
        manifest={'payloads':[{'raw':{'file':'BOOTIMG.img'}},{'raw':{'file':'Y2ROOT.img'}}],
                  'fallback':{'images':[{'file':'fallback/Y2ROOT.img'}]}}
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);(root/'BOOTIMG.img').write_bytes(b'fixture')
            p.release_tree(root,manifest)
            for name,data,error in [('spare.img',b'fixture','unexpected_image'),
                                    ('NVRAM.img',b'fixture','private_state_path'),
                                    ('owner.json',b'{"password":"secret"}','private_state_json')]:
                path=root/name;path.write_bytes(data)
                with self.subTest(name=name),self.assertRaisesRegex(ValueError,error):p.release_tree(root,manifest)
                path.unlink()
            (root/'linked').symlink_to(root/'BOOTIMG.img')
            with self.assertRaisesRegex(ValueError,'release_symlink'):p.release_tree(root,manifest)

    def test_structured_credentials_and_public_certificates(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary)
            (root/'certificate.pem').write_text('-----BEGIN CERTIFICATE-----\n'+'A'*80+'\n-----END CERTIFICATE-----\n')
            self.assertEqual(p.tree(root)['regular_files_checked'],1)
            for key in ('password','psk','LongTermKey'):
                (root/'settings.json').write_text(json.dumps({'nested':{key:'secret'}}))
                with self.subTest(key=key),self.assertRaisesRegex(ValueError,'private_state_json'):
                    p.tree(root)


    def test_pinned_public_wifi_template_only(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);(root/'etc').mkdir()
            config=root/'etc/wpa_supplicant.conf'
            config.write_bytes(p.PUBLIC_WIFI_TEMPLATE+b'\n')
            self.assertEqual(p.tree(root)['regular_files_checked'],1)
            config.write_bytes(p.PUBLIC_WIFI_TEMPLATE.replace(b'key_mgmt=NONE',b'ssid="owner-network"'))
            with self.assertRaisesRegex(ValueError,'private_state_path'):p.tree(root)
            p.check_name('usr/share/dbus-1/system.d/wpa_supplicant.conf')

    def test_root_image_symlink_is_rejected_before_resolving(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);(root/'real').write_bytes(b'fixture');(root/'link').symlink_to(root/'real')
            with self.assertRaisesRegex(ValueError,'regular_root_image_required'):p.ext4(root/'link')

    @unittest.skipUnless(all(shutil.which(name) for name in ('mke2fs','debugfs','e2fsck')), 'e2fsprogs required')
    def test_whole_ext4_includes_deleted_private_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);source=root/'seed';(source/'etc').mkdir(parents=True)
            (source/'etc/release').write_text('public fixture')
            image=root/'root.img'
            with image.open('wb') as stream:stream.truncate(32*1024**2)
            subprocess.run(['mke2fs','-q','-F','-t','ext4','-E','lazy_itable_init=0,lazy_journal_init=0',
                            '-d',str(source),str(image)],check=True,capture_output=True)
            receipt=p.ext4(image)
            self.assertEqual(receipt['image_bytes_checked'],image.stat().st_size)
            self.assertEqual(receipt['image_sha256'],p.image_digest(image))
            key=root/'key';key.write_bytes(b'-----BEGIN OPENSSH PRIVATE KEY-----\n'+b'A'*80+b'\n')
            for command in ('write '+str(key)+' /erased-key','rm /erased-key'):
                subprocess.run(['debugfs','-w','-R',command,str(image)],check=True,capture_output=True)
            with self.assertRaisesRegex(ValueError,'private_state_content'):p.ext4(image)


if __name__ == '__main__': unittest.main()
