import json
from pathlib import Path
import tempfile
import unittest
from tools.production import composition as c
from tools.production import system_update as u


class Composition(unittest.TestCase):
    def fixture(self, root):
        build, out = root/'build', root/'package'
        installed = build/'buildroot/target/etc/y2linux'
        installed.mkdir(parents=True)
        (build/'kernel').mkdir()
        (out/'metadata').mkdir(parents=True)
        (build/'kernel/.config').write_text('CONFIG_Y2_PLATFORM=y\nCONFIG_USB_INVENTRA_DMA=y\n')
        (build/'buildroot/.config').write_text('BR2_PACKAGE_BLUEALSA=y\n')
        versions = dict(build_git_commit='a'*40, reborn_source_commit='b'*40,
                        kernel_version='test-kernel', rootfs_version='test-root')
        manifest = dict(versions, release_version='test-release', platform_api_version=1,
                        distribution_intent='owner-local',
                        owner_firmware={'redistribution_permission_established': False},
                        payloads=[{'target_partition':'ANDROID','raw':{'file':'Y2ROOT.img','sha256':'c'*64, 'size_bytes':42}}],
                        fallback={'images':[{'file':'fallback/Y2ROOT.img','sha256':'d'*64,'size_bytes':84}]})
        codecs = {'codecs': {'SBC': {'compiled_locally': True}}}
        for name, value in [('versions',versions), ('audio-enabled', {}), ('bluetooth-codecs',codecs),
                            ('capabilities',{'capabilities': {'usb_dma': dict(implemented=True, enabled=True,
                                 experimental=False, reason='test'), 'usb_ncm': dict(implemented=True,
                                 enabled=False, experimental=True, reason='test')}})]:
            (installed/(name+'.json')).write_text(json.dumps(value))
        (build/'owner-firmware.json').write_text(json.dumps(manifest['owner_firmware']))
        (out/'metadata/buildroot-inputs.lock.json').write_text('{"version":"2026.02"}')
        privacy = {'private_state_found':False}
        for name, size, digest in [('new_root',42,'c'*64),('fallback_root',84,'d'*64)]:
            privacy[name] = dict(image_bytes_checked=size,image_sha256=digest,
                                 filesystem_checked=True,private_state_found=False)
        manifest['release_composition'] = c.write(build,out,manifest,privacy)
        return build,out,manifest

    def rewrite(self, out, manifest, name, change):
        path = out/'metadata'/name
        value = json.loads(path.read_text());change(value);path.write_text(json.dumps(value))
        key = 'composition_sha256' if name == 'release-composition.json' else 'capabilities_sha256'
        manifest['release_composition'][key] = c.digest(path)
        if key == 'capabilities_sha256':
            self.rewrite(out,manifest,'release-composition.json',
                         lambda value: value.update(capabilities_sha256=c.digest(path)))

    def test_valid_composition_and_distinct_usb_build_states(self):
        with tempfile.TemporaryDirectory() as temporary:
            build,out,manifest = self.fixture(Path(temporary))
            c.validate(out,manifest)
            caps = c.delivered(build,manifest)['capabilities']
            self.assertTrue(caps['usb_dma']['compiled'])
            self.assertFalse(caps['usb_ncm']['compiled'])
            (build/'kernel/.config').write_text('CONFIG_Y2_PLATFORM=y\n')
            with self.assertRaisesRegex(ValueError,'enabled_capability_not_compiled:usb_dma'):
                c.delivered(build,manifest)

    def test_rehashed_misleading_receipts_are_rejected(self):
        changes = [
            ('release-composition.json',lambda value: value.update(y2linux_commit='f'*40),'identity'),
            ('release-composition.json',lambda value: value.update(buildroot='wrong'),'identity'),
            ('release-composition.json',lambda value: value['private_state_check']['fallback_root'].update(image_sha256='0'*64),'privacy_payload_binding'),
            ('release-composition.json',lambda value: value['private_state_check']['new_root'].update(filesystem_checked=False),'privacy_payload_binding'),
            ('delivered-capabilities.json',lambda value: value['capabilities']['usb_dma'].update(physically_qualified=True),'capabilities_evidence'),
            ('delivered-capabilities.json',lambda value: value.update(capabilities={}), 'capabilities_evidence'),
        ]
        for name,change,error in changes:
            with self.subTest(error=error), tempfile.TemporaryDirectory() as temporary:
                _,out,manifest=self.fixture(Path(temporary))
                self.rewrite(out,manifest,name,change)
                with self.assertRaisesRegex(ValueError,error):c.validate(out,manifest)

    def test_public_packaging_fails_before_output_exists(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);build,_,_=self.fixture(root);out=root/'not-created'
            self.assertFalse(u.distribution_gate(build,False)['public_distribution_ready'])
            with self.assertRaisesRegex(ValueError,'public distribution unresolved'):
                u.package(build,root/'no-base',root/'no-fallback',out,public=True)
            self.assertFalse(out.exists())


if __name__ == '__main__':unittest.main()
