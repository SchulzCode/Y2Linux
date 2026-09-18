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
	cd $(@D) && PATH="$(dir $(REBORN_CARGO)):$(BR_PATH)" \
		REBORN_BUILD_ID="$(shell git -C $(REBORN_SITE) rev-parse HEAD)" \
		Y2_BUILDROOT_OUTPUT="$(BASE_DIR)" \
		sh tools/build/cross.sh
endef

define REBORN_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(@D)/target/armv7-unknown-linux-gnueabihf/release/reborn $(TARGET_DIR)/usr/bin/reborn
	$(INSTALL) -D -m 0755 $(@D)/target/armv7-unknown-linux-gnueabihf/release/rebornctl $(TARGET_DIR)/usr/bin/rebornctl
	mkdir -p $(TARGET_DIR)/usr/share/reborn/fixtures
	cp $(@D)/assets/fixtures/tone.* $(@D)/assets/fixtures/artwork.flac $(@D)/assets/fixtures/manifest.json $(TARGET_DIR)/usr/share/reborn/fixtures/
	$(INSTALL) -D -m 0644 $(@D)/docs/architecture/dependencies.json $(TARGET_DIR)/usr/share/reborn/dependencies.json
	$(INSTALL) -D -m 0644 $(@D)/LICENSE $(TARGET_DIR)/usr/share/reborn/LICENSE
	$(INSTALL) -D -m 0755 $(REBORN_PKGDIR)/reborn-supervise $(TARGET_DIR)/usr/libexec/reborn-supervise
	$(INSTALL) -D -m 0755 $(REBORN_PKGDIR)/S60reborn $(TARGET_DIR)/etc/init.d/S60reborn
endef

$(eval $(generic-package))
