################################################################################
# Reborn: vendored, pinned Rust sources; Buildroot owns native dependencies/SDK.
################################################################################
REBORN_SITE = $(if $(Y2_REBORN_SOURCE),$(Y2_REBORN_SOURCE),$(BR2_EXTERNAL_Y2LINUX_PATH)/../../Y2Reborn)
REBORN_VERSION = $(shell python3 "$(BR2_EXTERNAL_Y2LINUX_PATH)/../tools/production/application.py" --source-version "$(REBORN_SITE)/Cargo.toml")
REBORN_SITE_METHOD = local
REBORN_LICENSE = MIT, OFL-1.1 (Open Sans glyph data in the binary; Open Sans, Noto, Pixelify Sans, DM Sans, Syne and IBM Plex Mono fonts of the shipped skins)
REBORN_LICENSE_FILES = LICENSE assets/fonts/OpenSans-OFL.txt skins/classic/fonts/OpenSans-OFL.txt skins/paper/fonts/Noto-OFL.txt skins/bunny-bow/fonts/PixelifySans-OFL.txt skins/gallery/fonts/DMSans-OFL.txt skins/gallery/fonts/Syne-OFL.txt skins/gallery/fonts/IBMPlexMono-OFL.txt
REBORN_DEPENDENCIES = ffmpeg sqlite alsa-lib dbus mesa3d libdrm util-linux host-pkgconf
REBORN_CARGO ?= $(HOME)/.cargo/bin/cargo
REBORN_OVERRIDE_SRCDIR_RSYNC_EXCLUSIONS = --exclude=/target --exclude=/out --exclude=/.git
# legal-info would pack the whole build tree (Cargo target, ~2.6 GB). Reborn is
# this project's own MIT code; its exact source is the Y2Reborn repository at
# the same baseline tag (and its vendored crates), named in each release.
REBORN_REDISTRIBUTE = NO

define REBORN_BUILD_CMDS
	$(TARGET_CC) $(TARGET_CFLAGS) -Os -Wall -Wextra -Werror -I$(STAGING_DIR)/usr/include/libdrm \
		$(BR2_EXTERNAL_Y2LINUX_PATH)/../tools/graphics/reborn-splash.c \
		$(TARGET_LDFLAGS) -Wl,-z,relro,-z,now -ldrm -o $(@D)/reborn-splash
	cd $(@D) && PATH="$(dir $(REBORN_CARGO)):$(BR_PATH)" \
		REBORN_BUILD_ID="$(shell git -C $(REBORN_SITE) rev-parse HEAD)" \
		Y2_BUILDROOT_OUTPUT="$(BASE_DIR)" Y2_BUILDROOT_HOST="$(HOST_DIR)" \
		sh tools/build/cross.sh
endef

define REBORN_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(@D)/reborn-splash $(TARGET_DIR)/usr/libexec/reborn-splash
	$(INSTALL) -D -m 0755 $(REBORN_PKGDIR)/reborn-boot-services $(TARGET_DIR)/usr/libexec/reborn-boot-services
	$(INSTALL) -D -m 0644 $(REBORN_PKGDIR)/syslogd.conf $(TARGET_DIR)/etc/default/syslogd
	$(INSTALL) -D -m 0755 $(@D)/target/armv7-unknown-linux-gnueabihf/release/reborn $(TARGET_DIR)/usr/bin/reborn
	$(INSTALL) -D -m 0755 $(@D)/target/armv7-unknown-linux-gnueabihf/release/rebornctl $(TARGET_DIR)/usr/bin/rebornctl
	$(INSTALL) -D -m 0755 $(@D)/target/armv7-unknown-linux-gnueabihf/release/reborn-bench $(TARGET_DIR)/usr/bin/reborn-bench
	media_so=$$(find $(@D)/target/armv7-unknown-linux-gnueabihf/release/build -type f -name libreborn_media.so -print -quit); \
	[ -n "$$media_so" ] || { echo 'missing lazy FFmpeg media membrane' >&2; exit 1; }; \
	$(INSTALL) -D -m 0755 "$$media_so" $(TARGET_DIR)/usr/lib/reborn/libreborn_media.so
	mkdir -p $(TARGET_DIR)/usr/share/reborn/fixtures
	cp -a $(@D)/assets/fixtures/. $(TARGET_DIR)/usr/share/reborn/fixtures/
	$(INSTALL) -D -m 0644 $(@D)/docs/architecture/dependencies.json $(TARGET_DIR)/usr/share/reborn/dependencies.json
	$(INSTALL) -D -m 0644 $(@D)/LICENSE $(TARGET_DIR)/usr/share/reborn/LICENSE
	$(INSTALL) -D -m 0644 $(@D)/assets/fonts/OpenSans-OFL.txt $(TARGET_DIR)/usr/share/reborn/OpenSans-OFL.txt
	# Shipped skins; user skins are folders in Themes on the media storage.
	rm -rf $(TARGET_DIR)/usr/share/reborn/skins
	mkdir -p $(TARGET_DIR)/usr/share/reborn/skins
	cp -a $(@D)/skins/. $(TARGET_DIR)/usr/share/reborn/skins/
	find $(TARGET_DIR)/usr/share/reborn/skins -type d -exec chmod 0755 {} +
	find $(TARGET_DIR)/usr/share/reborn/skins -type f -exec chmod 0644 {} +
	$(INSTALL) -D -m 0755 $(REBORN_PKGDIR)/reborn-supervise $(TARGET_DIR)/usr/libexec/reborn-supervise
	# Y2DATA is prepared by S02y2-data. Reborn starts last, after the platform
	# services and (when enabled) the radios are up; S90reborn waits for them.
	rm -f $(TARGET_DIR)/etc/init.d/S05reborn $(TARGET_DIR)/etc/init.d/S60reborn
	$(INSTALL) -D -m 0755 $(REBORN_PKGDIR)/S90reborn $(TARGET_DIR)/etc/init.d/S90reborn
endef

$(eval $(generic-package))
