################################################################################
# Complete release archive includes the pinned AOSP libldac submodule.
################################################################################
LIBLDAC_VERSION = 2.0.2.3
LIBLDAC_SOURCE = ldacBT-$(LIBLDAC_VERSION).tar.gz
LIBLDAC_SITE = https://github.com/EHfive/ldacBT/releases/download/v$(LIBLDAC_VERSION)
LIBLDAC_LICENSE = Apache-2.0
LIBLDAC_LICENSE_FILES = LICENSE libldac/LICENSE libldac/NOTICE
LIBLDAC_INSTALL_STAGING = YES
LIBLDAC_CONF_OPTS = -DLDAC_SOFT_FLOAT=OFF

$(eval $(cmake-package))
