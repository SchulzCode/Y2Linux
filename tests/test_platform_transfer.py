"""The image's SSH programs. The transfer and ssh-daemon tests of the platform are Rust:
platform/crates/y2-platform/src/commands/devices/{transfer,ssh}_tests.rs."""
import tarfile
import unittest

from tools.production.validate import validate_ssh_programs


class Transfer(unittest.TestCase):
    def test_package_accepts_only_existing_dropbear_client_alias(self):
        alias = tarfile.TarInfo('usr/bin/ssh')
        alias.type = tarfile.SYMTYPE
        alias.linkname = '../sbin/dropbear'
        validate_ssh_programs({'usr/bin/ssh': alias})
        validate_ssh_programs({})
        for name in ('usr/sbin/sshd', 'usr/bin/sftp', 'usr/bin/ssh'):
            with self.assertRaises(ValueError):
                validate_ssh_programs({name: tarfile.TarInfo(name)})
        alias.linkname = '../bin/unreviewed-client'
        with self.assertRaises(ValueError):
            validate_ssh_programs({'usr/bin/ssh': alias})


if __name__=='__main__': unittest.main()
