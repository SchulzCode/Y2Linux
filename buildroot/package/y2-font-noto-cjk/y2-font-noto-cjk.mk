################################################################################
# Noto Sans CJK JP Regular: Reborn's runtime fallback for Japanese, Chinese
# and Korean text. One unarchived OTF and its licence from the release tag.
################################################################################
Y2_FONT_NOTO_CJK_VERSION = Sans2.004
Y2_FONT_NOTO_CJK_SITE = https://raw.githubusercontent.com/notofonts/noto-cjk/$(Y2_FONT_NOTO_CJK_VERSION)/Sans/OTF/Japanese
Y2_FONT_NOTO_CJK_SOURCE = NotoSansCJKjp-Regular.otf
Y2_FONT_NOTO_CJK_EXTRA_DOWNLOADS = https://raw.githubusercontent.com/notofonts/noto-cjk/$(Y2_FONT_NOTO_CJK_VERSION)/LICENSE
Y2_FONT_NOTO_CJK_LICENSE = OFL-1.1
Y2_FONT_NOTO_CJK_LICENSE_FILES = LICENSE

# The source is a font file, not an archive.
define Y2_FONT_NOTO_CJK_EXTRACT_CMDS
	$(INSTALL) -m 0644 $(Y2_FONT_NOTO_CJK_DL_DIR)/NotoSansCJKjp-Regular.otf $(@D)/NotoSansCJKjp-Regular.otf
	$(INSTALL) -m 0644 $(Y2_FONT_NOTO_CJK_DL_DIR)/LICENSE $(@D)/LICENSE
endef

define Y2_FONT_NOTO_CJK_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0644 $(@D)/NotoSansCJKjp-Regular.otf \
		$(TARGET_DIR)/usr/share/fonts/noto-cjk/NotoSansCJKjp-Regular.otf
	$(INSTALL) -D -m 0644 $(@D)/LICENSE \
		$(TARGET_DIR)/usr/share/fonts/noto-cjk/LICENSE
endef

$(eval $(generic-package))
