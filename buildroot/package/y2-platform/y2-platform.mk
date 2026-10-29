################################################################################
# y2-platform: the platform service in Rust (docs/planning/PLATFORM-RUST-PORT-PLAN.md),
# vendored and pinned like Reborn; Buildroot provides the C toolchain.
################################################################################
# The git tree of platform/ (builds start from committed source): a changed crate is a new version and so a
# new build folder. A fixed version let a resumed build reuse the old folder: test build 167 shipped 166's
# binary.
Y2_PLATFORM_VERSION := $(shell git -C $(BR2_EXTERNAL_Y2LINUX_PATH)/.. rev-parse --short=12 HEAD:platform 2>/dev/null)
ifeq ($(Y2_PLATFORM_VERSION),)
$(error y2-platform: no git tree for platform/ (commit it first))
endif
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

# The whole platform service: every subcommand, daemon and programme (post-build links its other
# names to it); there is no Python on the player.
define Y2_PLATFORM_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(@D)/target/armv7-unknown-linux-gnueabihf/release/y2-platform \
		$(TARGET_DIR)/usr/sbin/y2-platform
endef

$(eval $(generic-package))
