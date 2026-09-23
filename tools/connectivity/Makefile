# SPDX-License-Identifier: GPL-2.0-only
Y2_RADIO_SOURCE ?= $(CURDIR)
Y2_RADIO_OUTPUT ?= $(CURDIR)
CFLAGS += -Wall -Wextra -Werror
.PHONY: all
all: $(addprefix $(Y2_RADIO_OUTPUT)/,y2-factory y2-calibration y2-radio-activate y2-a2dp-check y2-bt-reconnect y2-bt-observe)
$(Y2_RADIO_OUTPUT)/y2-factory: $(Y2_RADIO_SOURCE)/factory.c
	$(CC) $(CFLAGS) $< $(LDFLAGS) -lext2fs -lcom_err -o $@
$(Y2_RADIO_OUTPUT)/y2-calibration: $(Y2_RADIO_SOURCE)/calibration.c $(Y2_RADIO_SOURCE)/fs-store.h
	$(CC) $(CFLAGS) $< $(LDFLAGS) -o $@
$(Y2_RADIO_OUTPUT)/y2-radio-activate: $(Y2_RADIO_SOURCE)/activate.c
	$(CC) $(CFLAGS) $< $(LDFLAGS) -o $@
$(Y2_RADIO_OUTPUT)/y2-a2dp-check: $(Y2_RADIO_SOURCE)/a2dp-check.c
	$(CC) $(CFLAGS) $(shell $(PKG_CONFIG) --cflags gio-2.0 gio-unix-2.0 alsa) $< $(LDFLAGS) $(shell $(PKG_CONFIG) --libs gio-2.0 gio-unix-2.0 alsa) -o $@
$(Y2_RADIO_OUTPUT)/y2-bt-reconnect: $(Y2_RADIO_SOURCE)/reconnect.c $(Y2_RADIO_SOURCE)/reconnect-policy.h
	$(CC) $(CFLAGS) $(shell $(PKG_CONFIG) --cflags gio-2.0 gio-unix-2.0) $< $(LDFLAGS) $(shell $(PKG_CONFIG) --libs gio-2.0 gio-unix-2.0) -o $@
$(Y2_RADIO_OUTPUT)/y2-bt-observe: $(Y2_RADIO_SOURCE)/bt-observe.c
	$(CC) $(CFLAGS) $(shell $(PKG_CONFIG) --cflags gio-2.0) $< $(LDFLAGS) $(shell $(PKG_CONFIG) --libs gio-2.0) -o $@
