################################################################################
# Reborn: vendored, pinned Rust sources; Buildroot owns native dependencies/SDK.
################################################################################
REBORN_VERSION = 0.1.0-baseline.01
REBORN_SITE = $(BR2_EXTERNAL_Y2LINUX_PATH)/../../Y2Reborn
REBORN_SITE_METHOD = local
REBORN_LICENSE = MIT
REBORN_LICENSE_FILES = LICENSE
REBORN_DEPENDENCIES = ffmpeg sqlite alsa-lib dbus mesa3d libdrm util-linux host-pkgconf
REBORN_CARGO ?= $(HOME)/.cargo/bin/cargo
REBORN_OVERRIDE_SRCDIR_RSYNC_EXCLUSIONS = --exclude=/target --exclude=/out --exclude=/.git

define REBORN_BUILD_CMDS
	$(TARGET_CC) $(TARGET_CFLAGS) -Os -Wall -Wextra -Werror -I$(STAGING_DIR)/usr/include/libdrm \
		$(BR2_EXTERNAL_Y2LINUX_PATH)/../tools/graphics/reborn-splash.c \
		$(TARGET_LDFLAGS) -Wl,-z,relro,-z,now -ldrm -o $(@D)/reborn-splash
	cd $(@D) && PATH="$(dir $(REBORN_CARGO)):$(BR_PATH)" \
		REBORN_BUILD_ID="$(shell git -C $(REBORN_SITE) rev-parse HEAD)" \
		Y2_BUILDROOT_OUTPUT="$(BASE_DIR)" \
		sh tools/build/cross.sh
endef

define REBORN_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(@D)/reborn-splash $(TARGET_DIR)/usr/libexec/reborn-splash
	$(INSTALL) -D -m 0755 $(REBORN_PKGDIR)/reborn-boot-services $(TARGET_DIR)/usr/libexec/reborn-boot-services
	$(INSTALL) -D -m 0644 $(REBORN_PKGDIR)/syslogd.conf $(TARGET_DIR)/etc/default/syslogd
	$(INSTALL) -D -m 0755 $(@D)/target/armv7-unknown-linux-gnueabihf/release/reborn $(TARGET_DIR)/usr/bin/reborn
	$(INSTALL) -D -m 0755 $(@D)/target/armv7-unknown-linux-gnueabihf/release/rebornctl $(TARGET_DIR)/usr/bin/rebornctl
	media_so=$$(find $(@D)/target/armv7-unknown-linux-gnueabihf/release/build -type f -name libreborn_media.so -print -quit); \
	[ -n "$$media_so" ] || { echo 'missing lazy FFmpeg media membrane' >&2; exit 1; }; \
	$(INSTALL) -D -m 0755 "$$media_so" $(TARGET_DIR)/usr/lib/reborn/libreborn_media.so
	mkdir -p $(TARGET_DIR)/usr/share/reborn/fixtures
	cp -a $(@D)/assets/fixtures/. $(TARGET_DIR)/usr/share/reborn/fixtures/
	$(INSTALL) -D -m 0644 $(@D)/docs/architecture/dependencies.json $(TARGET_DIR)/usr/share/reborn/dependencies.json
	$(INSTALL) -D -m 0644 $(@D)/LICENSE $(TARGET_DIR)/usr/share/reborn/LICENSE
	$(INSTALL) -D -m 0755 $(REBORN_PKGDIR)/reborn-supervise $(TARGET_DIR)/usr/libexec/reborn-supervise
	# Y2DATA is prepared by S02y2-data. Start Reborn before optional radio and
	# network services; those workers retry until their providers are ready.
	rm -f $(TARGET_DIR)/etc/init.d/S60reborn
	$(INSTALL) -D -m 0755 $(REBORN_PKGDIR)/S05reborn $(TARGET_DIR)/etc/init.d/S05reborn
endef

$(eval $(generic-package))
