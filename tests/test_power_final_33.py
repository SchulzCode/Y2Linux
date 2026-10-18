"""POWER-FINAL-33: Reborn skins ship with the image and stay licensed."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]


class ShippedSkins(unittest.TestCase):
    mk = (ROOT / 'buildroot/package/reborn/reborn.mk').read_text()

    def test_skins_are_installed_read_only_under_usr_share(self):
        install = self.mk[self.mk.index('define REBORN_INSTALL_TARGET_CMDS'):]
        install = install[:install.index('endef')]
        self.assertIn('cp -a $(@D)/skins/. $(TARGET_DIR)/usr/share/reborn/skins/', install)
        # A rebuild never keeps a skin removed from the source.
        self.assertLess(install.index('rm -rf $(TARGET_DIR)/usr/share/reborn/skins'),
                        install.index('cp -a $(@D)/skins/.'))
        self.assertIn('-type f -exec chmod 0644', install)
        self.assertIn('-type d -exec chmod 0755', install)

    def test_the_classic_skin_fonts_carry_their_licence(self):
        self.assertIn('skins/classic/fonts/OpenSans-OFL.txt', self.mk)
        # POWER-FINAL-34: the example skins add Noto fonts.
        self.assertIn('skins/paper/fonts/Noto-OFL.txt', self.mk)
        self.assertIn('Pixelify Sans, DM Sans, Syne and IBM Plex Mono fonts of the shipped skins', self.mk)
        self.assertIn('skins/bunny-bow/fonts/PixelifySans-OFL.txt', self.mk)
        for name in ('DMSans', 'Syne', 'IBMPlexMono'):
            self.assertIn(f'skins/gallery/fonts/{name}-OFL.txt', self.mk)


if __name__ == '__main__':
    unittest.main()
