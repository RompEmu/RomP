CARGO_VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
VERSION ?= $(CARGO_VERSION)
RELEASE := $(VERSION:v%=%)
ARCH := $(shell uname -m)

DIST := dist
BIN := target/release
BUNDLE_ID := io.github.rompemu.romp
ICON := crates/romp-app/assets/icon-macos.png

APP := $(DIST)/Romp.app
ICONSET := $(DIST)/Romp.iconset
MACOS_ZIP := $(DIST)/Romp-$(RELEASE)-macos-$(ARCH).zip
LINUX_NAME := Romp-$(RELEASE)-linux-$(ARCH)
LINUX_TAR := $(DIST)/$(LINUX_NAME).tar.gz

ifeq ($(shell uname -s),Darwin)
PLATFORM := macos
else
PLATFORM := linux
endif

define checksum
cd $(DIST) && shasum -a 512 $(notdir $(1)) > $(notdir $(1)).sha512
endef

define zip_app
rm -f $(MACOS_ZIP)
ditto -c -k --keepParent $(APP) $(MACOS_ZIP)
$(call checksum,$(MACOS_ZIP))
endef

.PHONY: build app dist dist-macos dist-linux clean

build:
	cargo build --release --locked

app: build
	rm -rf $(APP) $(ICONSET)
	mkdir -p $(APP)/Contents/MacOS $(APP)/Contents/Resources $(ICONSET)
	cp $(BIN)/romp $(BIN)/romp-runner $(APP)/Contents/MacOS/
	cp LICENSE $(APP)/Contents/Resources/
	sed -e 's/@VERSION@/$(CARGO_VERSION)/g' -e 's/@BUNDLE_ID@/$(BUNDLE_ID)/g' \
		packaging/macos/Info.plist > $(APP)/Contents/Info.plist
	for size in 16 32 128 256 512; do \
		sips -z $$size $$size $(ICON) --out $(ICONSET)/icon_$${size}x$${size}.png >/dev/null && \
		sips -z $$((size * 2)) $$((size * 2)) $(ICON) --out $(ICONSET)/icon_$${size}x$${size}@2x.png >/dev/null || exit 1; \
	done
	iconutil --convert icns --output $(APP)/Contents/Resources/Romp.icns $(ICONSET)
	rm -rf $(ICONSET)
	codesign --force --sign - $(APP)/Contents/MacOS/romp-runner
	codesign --force --sign - $(APP)

dist: dist-$(PLATFORM)

dist-macos: app
	$(zip_app)

dist-linux: build
	rm -rf $(DIST)/$(LINUX_NAME) $(LINUX_TAR)
	mkdir -p $(DIST)/$(LINUX_NAME)
	cp $(BIN)/romp $(BIN)/romp-runner README.md LICENSE $(DIST)/$(LINUX_NAME)/
	tar -czf $(LINUX_TAR) -C $(DIST) $(LINUX_NAME)
	rm -rf $(DIST)/$(LINUX_NAME)
	$(call checksum,$(LINUX_TAR))

clean:
	rm -rf $(DIST)

-include signing/signing.mk
