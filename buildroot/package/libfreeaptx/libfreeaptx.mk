################################################################################
# libfreeaptx: deliberately avoid GPL-3 libopenaptx when linking FDK-AAC
################################################################################
LIBFREEAPTX_VERSION = 0.2.2
LIBFREEAPTX_SITE = $(call github,regularhunter,libfreeaptx,$(LIBFREEAPTX_VERSION))
LIBFREEAPTX_LICENSE = LGPL-2.1+
LIBFREEAPTX_LICENSE_FILES = COPYING
LIBFREEAPTX_INSTALL_STAGING = YES
LIBFREEAPTX_MAKE_OPTS = $(TARGET_CONFIGURE_OPTS) PREFIX=/usr CFLAGS="$(TARGET_CFLAGS)" LDFLAGS="$(TARGET_LDFLAGS)"

define LIBFREEAPTX_BUILD_CMDS
	$(TARGET_MAKE_ENV) $(MAKE) -C $(@D) $(LIBFREEAPTX_MAKE_OPTS)
endef

define LIBFREEAPTX_INSTALL_STAGING_CMDS
	$(TARGET_MAKE_ENV) $(MAKE) -C $(@D) $(LIBFREEAPTX_MAKE_OPTS) DESTDIR=$(STAGING_DIR) install
endef

define LIBFREEAPTX_INSTALL_TARGET_CMDS
	$(TARGET_MAKE_ENV) $(MAKE) -C $(@D) $(LIBFREEAPTX_MAKE_OPTS) DESTDIR=$(TARGET_DIR) install
endef

$(eval $(generic-package))
