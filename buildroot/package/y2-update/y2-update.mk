################################################################################
# Native signed full-system updater (OTA-35) and the recovery screen
# (no Python in initramfs)
################################################################################
Y2_UPDATE_VERSION = 2
Y2_UPDATE_SITE = $(BR2_EXTERNAL_Y2LINUX_PATH)/../tools/update
Y2_UPDATE_SITE_METHOD = local
Y2_UPDATE_LICENSE = GPL-2.0-only, GPL-2.0 (Terminus 16x32 glyphs from Linux lib/fonts)
Y2_UPDATE_DEPENDENCIES = libsodium json-c zlib libdrm

define Y2_UPDATE_BUILD_CMDS
	$(TARGET_CC) $(TARGET_CFLAGS) -std=c11 -Os -Wall -Wextra -Werror -pthread \
		$(@D)/core.c -o $(@D)/y2-update-core $(TARGET_LDFLAGS) -lsodium -ljson-c -lz
	$(TARGET_CC) $(TARGET_CFLAGS) -std=c11 -Os -Wall -Wextra -Werror \
		-I$(STAGING_DIR)/usr/include/libdrm $(@D)/recovery-ui.c \
		$(TARGET_LDFLAGS) -Wl,-z,relro,-z,now -ldrm -o $(@D)/y2-recovery-ui
endef

define Y2_UPDATE_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(@D)/y2-update-core $(TARGET_DIR)/usr/sbin/y2-update-core
	$(INSTALL) -D -m 0755 $(@D)/y2-recovery-ui $(TARGET_DIR)/usr/libexec/y2-recovery-ui
endef

$(eval $(generic-package))
