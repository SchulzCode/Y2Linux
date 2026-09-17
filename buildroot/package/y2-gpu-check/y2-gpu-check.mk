Y2_GPU_CHECK_VERSION = 1
Y2_GPU_CHECK_SITE = $(BR2_EXTERNAL_Y2LINUX_PATH)/../tools/graphics
Y2_GPU_CHECK_SITE_METHOD = local
Y2_GPU_CHECK_LICENSE = MIT, GPL-2.0 WITH Linux-syscall-note OR MIT (Lima UAPI header)
Y2_GPU_CHECK_LICENSE_FILES = LICENSE
Y2_GPU_CHECK_DEPENDENCIES = mesa3d libdrm host-pkgconf

define Y2_GPU_CHECK_BUILD_CMDS
	$(TARGET_MAKE_ENV) $(TARGET_CC) $(TARGET_CFLAGS) -I$(@D)/include -std=c11 -Wall -Wextra -Werror \
		$(shell $(PKG_CONFIG_HOST_BINARY) --cflags libdrm gbm egl glesv2) \
		$(@D)/gpu-check.c -o $(@D)/y2-gpu-check $(TARGET_LDFLAGS) \
		$(shell $(PKG_CONFIG_HOST_BINARY) --libs libdrm gbm egl glesv2) -lm
endef

define Y2_GPU_CHECK_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(@D)/y2-gpu-check $(TARGET_DIR)/usr/bin/y2-gpu-check
	$(INSTALL) -D -m 0755 $(@D)/collect.sh $(TARGET_DIR)/usr/sbin/y2-gpu-collect
endef

$(eval $(generic-package))
