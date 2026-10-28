################################################################################
# y2-platform: the platform service in Rust (docs/planning/PLATFORM-RUST-PORT-PLAN.md),
# vendored and pinned like Reborn; Buildroot provides the C toolchain.
################################################################################
Y2_PLATFORM_VERSION = 1
Y2_PLATFORM_SITE = $(BR2_EXTERNAL_Y2LINUX_PATH)/../platform
Y2_PLATFORM_SITE_METHOD = local
Y2_PLATFORM_LICENSE = GPL-2.0-only, MIT (y2p-contract), MIT or Apache-2.0 (vendored crates)
Y2_PLATFORM_CARGO ?= $(HOME)/.cargo/bin/cargo
Y2_PLATFORM_OVERRIDE_SRCDIR_RSYNC_EXCLUSIONS = --exclude=/target
# As Reborn: this project's own code; its exact source is the Y2Linux repository at the release's
# baseline tag (platform/ with its vendored crates). legal-info would pack the Cargo target too.
Y2_PLATFORM_REDISTRIBUTE = NO

define Y2_PLATFORM_BUILD_CMDS
	cd $(@D) && PATH="$(dir $(Y2_PLATFORM_CARGO)):$(BR_PATH)" \
		Y2_BUILDROOT_OUTPUT="$(BASE_DIR)" Y2_BUILDROOT_HOST="$(HOST_DIR)" \
		sh tools/cross.sh
endef

# Replaces the shell wrapper `exec python3 -I /usr/lib/y2-platform/main.py "$@"` (post-build installs
# the Python platform it falls back to).
define Y2_PLATFORM_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(@D)/target/armv7-unknown-linux-gnueabihf/release/y2-platform \
		$(TARGET_DIR)/usr/sbin/y2-platform
endef

$(eval $(generic-package))
