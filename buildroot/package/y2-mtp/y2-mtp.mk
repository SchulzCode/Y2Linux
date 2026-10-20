################################################################################
# File-level USB transfer. Upstream pin plus reviewed Y2 safety patch/helpers.
################################################################################
Y2_MTP_VERSION = 2241a803c27f1fff7537cb9504befeb810b880d3
Y2_MTP_SITE = $(call github,viveris,uMTP-Responder,$(Y2_MTP_VERSION))
Y2_MTP_SOURCE = y2-mtp-$(Y2_MTP_VERSION).tar.gz
Y2_MTP_SITE_METHOD = wget
Y2_MTP_LICENSE = GPL-3.0+
Y2_MTP_LICENSE_FILES = LICENSE
# Music tags come from Reborn's library (read-only, patch 0005).
Y2_MTP_DEPENDENCIES = sqlite

define Y2_MTP_COPY_SAFETY_HELPERS
	$(INSTALL) -m 0644 $(BR2_EXTERNAL_Y2LINUX_PATH)/../tools/usb/mtp-safe.h $(@D)/inc/mtp-safe.h
	$(INSTALL) -m 0644 $(BR2_EXTERNAL_Y2LINUX_PATH)/../tools/usb/mtp-safe.c $(@D)/src/mtp-safe.c
	# Old external-toolchain UAPI predates openat2. Use the actual pinned kernel
	# ABI, including generated ARM EABI numbers; never guess syscall constants.
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_Y2LINUX_PATH)/../.cache/sources/linux-6.18/include/uapi/linux/openat2.h $(@D)/inc/linux/openat2.h
	$(INSTALL) -D -m 0644 $(Y2_ARTIFACT_DIR)/kernel/arch/arm/include/generated/uapi/asm/unistd-eabi.h $(@D)/inc/asm/unistd-eabi.h
endef
Y2_MTP_POST_PATCH_HOOKS += Y2_MTP_COPY_SAFETY_HELPERS

define Y2_MTP_BUILD_CMDS
	$(TARGET_MAKE_ENV) $(MAKE) -C $(@D) CC="$(TARGET_CC)" CPPFLAGS="$(TARGET_CPPFLAGS)" CFLAGS="$(TARGET_CFLAGS) -I./inc -Wall" LDFLAGS="$(TARGET_LDFLAGS) -lpthread -lrt -lsqlite3"
endef

define Y2_MTP_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(@D)/umtprd $(TARGET_DIR)/usr/sbin/umtprd
endef

$(eval $(generic-package))
