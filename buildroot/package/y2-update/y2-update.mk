################################################################################
# Native signed root-only rescue updater (no Python in initramfs)
################################################################################
Y2_UPDATE_VERSION = 1
Y2_UPDATE_SITE = $(BR2_EXTERNAL_Y2LINUX_PATH)/../tools/update
Y2_UPDATE_SITE_METHOD = local
Y2_UPDATE_LICENSE = GPL-2.0-only
Y2_UPDATE_DEPENDENCIES = libsodium json-c zlib

define Y2_UPDATE_BUILD_CMDS
	$(TARGET_CC) $(TARGET_CFLAGS) -std=c11 -Os -Wall -Wextra -Werror \
		$(@D)/core.c -o $(@D)/y2-update-core $(TARGET_LDFLAGS) -lsodium -ljson-c -lz
endef

define Y2_UPDATE_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(@D)/y2-update-core $(TARGET_DIR)/usr/sbin/y2-update-core
endef

$(eval $(generic-package))
