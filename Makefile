CARGO_VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
VERSION ?= $(CARGO_VERSION)
RELEASE := $(VERSION:v%=%)
ARCH := $(shell uname -m)

DIST := dist
BIN := target/release
BUNDLE_ID := io.github.rompemu.romp
ICON := crates/romp-app/assets/icon-macos.png
MOLTENVK := 1.4.1
MOLTENVK_SHA256 := 5ea0c259df7ded9a275444820f09cced54d6e5a7c7a31d262de62a5cdb7e15cf
MOLTENVK_TAR := target/MoltenVK-$(MOLTENVK)-macos.tar

APP := $(DIST)/Romp.app
ICONSET := $(DIST)/Romp.iconset
MACOS_ZIP := $(DIST)/Romp-$(RELEASE)-macos-$(ARCH).zip
LINUX_NAME := Romp-$(RELEASE)-linux-$(ARCH)
LINUX_TAR := $(DIST)/$(LINUX_NAME).tar.gz
APPDIR := $(DIST)/Romp.AppDir
APPIMAGE := $(DIST)/$(LINUX_NAME).AppImage
APPIMAGETOOL ?= appimagetool
WINDOWS_NAME := Romp-$(RELEASE)-windows-$(ARCH)
WINDOWS_ZIP := $(DIST)/$(WINDOWS_NAME).zip

ifeq ($(OS),Windows_NT)
PLATFORM := windows
else ifeq ($(shell uname -s),Darwin)
PLATFORM := macos
else
PLATFORM := linux
endif

ifeq ($(PLATFORM),macos)
SHA512 := shasum -a 512
else
SHA512 := sha512sum
endif

define checksum
cd $(DIST) && $(SHA512) $(notdir $(1)) > $(notdir $(1)).sha512
endef

define zip_app
rm -f $(MACOS_ZIP)
ditto -c -k --keepParent $(APP) $(MACOS_ZIP)
$(call checksum,$(MACOS_ZIP))
endef

.PHONY: build app appimage dist dist-macos dist-linux dist-windows clean
.PHONY: check fmt-check clippy test deny machete typos workflows

build:
	cargo build --release --locked

$(MOLTENVK_TAR):
	mkdir -p $(dir $@)
	curl -fsSL -o $@.tmp https://github.com/KhronosGroup/MoltenVK/releases/download/v$(MOLTENVK)/MoltenVK-macos.tar
	echo "$(MOLTENVK_SHA256)  $@.tmp" | shasum -a 256 -c -
	mv $@.tmp $@

app: build $(MOLTENVK_TAR)
	rm -rf $(APP) $(ICONSET)
	mkdir -p $(APP)/Contents/MacOS $(APP)/Contents/Frameworks $(APP)/Contents/Resources $(ICONSET)
	cp $(BIN)/romp $(BIN)/romp-runner $(APP)/Contents/MacOS/
	tar -xOf $(MOLTENVK_TAR) MoltenVK/MoltenVK/dynamic/dylib/macOS/libMoltenVK.dylib > $(APP)/Contents/Frameworks/libMoltenVK.dylib
	lipo -thin arm64 -output $(APP)/Contents/Frameworks/libMoltenVK.dylib $(APP)/Contents/Frameworks/libMoltenVK.dylib
	cp LICENSE $(APP)/Contents/Resources/
	tar -xOf $(MOLTENVK_TAR) MoltenVK/LICENSE > $(APP)/Contents/Resources/MoltenVK-LICENSE
	sed -e 's/@VERSION@/$(CARGO_VERSION)/g' -e 's/@BUNDLE_ID@/$(BUNDLE_ID)/g' \
		packaging/macos/Info.plist > $(APP)/Contents/Info.plist
	for size in 16 32 128 256 512; do \
		sips -z $$size $$size $(ICON) --out $(ICONSET)/icon_$${size}x$${size}.png >/dev/null && \
		sips -z $$((size * 2)) $$((size * 2)) $(ICON) --out $(ICONSET)/icon_$${size}x$${size}@2x.png >/dev/null || exit 1; \
	done
	iconutil --convert icns --output $(APP)/Contents/Resources/Romp.icns $(ICONSET)
	rm -rf $(ICONSET)
	codesign --force --sign - $(APP)/Contents/Frameworks/libMoltenVK.dylib
	codesign --force --sign - $(APP)/Contents/MacOS/romp-runner
	codesign --force --sign - $(APP)

dist: dist-$(PLATFORM)

dist-macos: app
	$(zip_app)

appimage: build
	rm -rf $(APPDIR) $(APPIMAGE)
	mkdir -p $(APPDIR)/usr/bin $(APPDIR)/usr/share/applications $(APPDIR)/usr/share/icons/hicolor/256x256/apps
	cp $(BIN)/romp $(BIN)/romp-runner $(APPDIR)/usr/bin/
	cp packaging/linux/romp.desktop $(APPDIR)/
	cp packaging/linux/romp.desktop $(APPDIR)/usr/share/applications/
	cp packaging/linux/romp.png $(APPDIR)/
	cp packaging/linux/romp.png $(APPDIR)/usr/share/icons/hicolor/256x256/apps/
	ln -s usr/bin/romp $(APPDIR)/AppRun
	ARCH=$(ARCH) $(APPIMAGETOOL) --no-appstream $(APPDIR) $(APPIMAGE)
	rm -rf $(APPDIR)
	$(call checksum,$(APPIMAGE))

dist-linux: appimage
	rm -rf $(DIST)/$(LINUX_NAME) $(LINUX_TAR)
	mkdir -p $(DIST)/$(LINUX_NAME)
	cp $(BIN)/romp $(BIN)/romp-runner README.md LICENSE $(DIST)/$(LINUX_NAME)/
	tar -czf $(LINUX_TAR) -C $(DIST) $(LINUX_NAME)
	rm -rf $(DIST)/$(LINUX_NAME)
	$(call checksum,$(LINUX_TAR))

dist-windows: build
	rm -rf $(DIST)/$(WINDOWS_NAME) $(WINDOWS_ZIP)
	mkdir -p $(DIST)/$(WINDOWS_NAME)
	cp $(BIN)/romp.exe $(BIN)/romp-runner.exe README.md LICENSE $(DIST)/$(WINDOWS_NAME)/
	cd $(DIST) && 7z a -bso0 $(WINDOWS_NAME).zip $(WINDOWS_NAME)
	rm -rf $(DIST)/$(WINDOWS_NAME)
	$(call checksum,$(WINDOWS_ZIP))

clean:
	rm -rf $(DIST)

check: fmt-check clippy test deny machete typos workflows

fmt-check:
	cargo fmt --all --check

clippy:
	cargo clippy --workspace --all-targets --locked -- -D warnings

test:
	cargo test --workspace --locked

deny:
	cargo deny check

machete:
	cargo machete

typos:
	typos

workflows:
	actionlint
	zizmor .github/workflows

-include signing/signing.mk
