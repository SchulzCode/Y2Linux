################################################################################
# Y2 production factory provider, bounded calibration service and qualification client
################################################################################
Y2_CONNECTIVITY_VERSION = 1
Y2_CONNECTIVITY_SITE = $(BR2_EXTERNAL_Y2LINUX_PATH)/../tools/connectivity
Y2_CONNECTIVITY_SITE_METHOD = local
Y2_CONNECTIVITY_LICENSE = GPL-2.0-only
Y2_CONNECTIVITY_DEPENDENCIES = e2fsprogs libglib2 alsa-lib bluez5_utils host-pkgconf

define Y2_CONNECTIVITY_BUILD_CMDS
	$(TARGET_MAKE_ENV) $(TARGET_CONFIGURE_OPTS) $(MAKE) -f $(Y2_CONNECTIVITY_SITE)/Makefile \
		Y2_RADIO_SOURCE=$(abspath $(Y2_CONNECTIVITY_SITE)) Y2_RADIO_OUTPUT=$(@D)
endef

define Y2_CONNECTIVITY_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(@D)/y2-factory $(TARGET_DIR)/usr/sbin/y2-factory
	$(INSTALL) -D -m 0755 $(@D)/y2-calibration $(TARGET_DIR)/usr/sbin/y2-calibration
	$(INSTALL) -D -m 0755 $(@D)/y2-radio-activate $(TARGET_DIR)/usr/sbin/y2-radio-activate
	$(INSTALL) -D -m 0755 $(@D)/y2-a2dp-check $(TARGET_DIR)/usr/sbin/y2-a2dp-check
	$(INSTALL) -D -m 0755 $(@D)/y2-bt-reconnect $(TARGET_DIR)/usr/sbin/y2-bt-reconnect
endef

$(eval $(generic-package))
