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
APPIMAGETOOL_VERSION := 1.9.1
APPIMAGETOOL_SHA256 := ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
APPIMAGETOOL := target/appimagetool-$(APPIMAGETOOL_VERSION)-x86_64.AppImage
# appimagetool otherwise embeds the runtime from type2-runtime's moving "continuous" release.
APPIMAGE_RUNTIME_VERSION := 20251108
APPIMAGE_RUNTIME_SHA256 := 2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
APPIMAGE_RUNTIME := target/appimage-runtime-$(APPIMAGE_RUNTIME_VERSION)-x86_64

APP := $(DIST)/RomP.app
ICONSET := $(DIST)/RomP.iconset
MACOS_ZIP := $(DIST)/RomP-$(RELEASE)-macos-$(ARCH).zip
LINUX_NAME := RomP-$(RELEASE)-linux-$(ARCH)
LINUX_TAR := $(DIST)/$(LINUX_NAME).tar.gz
APPDIR := $(DIST)/RomP.AppDir
APPIMAGE := $(DIST)/RomP-$(RELEASE)-$(ARCH).AppImage
WINDOWS_NAME := RomP-$(RELEASE)-windows-$(ARCH)
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

.PHONY: build app appimage glibc-check dist dist-macos dist-linux dist-windows clean
.PHONY: check fmt-check clippy test deny machete typos workflows translation-tools translations translations-check pseudo release-notes update-rcheevos

build:
	cargo build --release --locked

$(MOLTENVK_TAR):
	mkdir -p $(dir $@)
	curl -fsSL -o $@.tmp https://github.com/KhronosGroup/MoltenVK/releases/download/v$(MOLTENVK)/MoltenVK-macos.tar
	echo "$(MOLTENVK_SHA256)  $@.tmp" | shasum -a 256 -c -
	mv $@.tmp $@

$(APPIMAGETOOL):
	mkdir -p $(dir $@)
	curl -fsSL -o $@.tmp https://github.com/AppImage/appimagetool/releases/download/$(APPIMAGETOOL_VERSION)/appimagetool-x86_64.AppImage
	echo "$(APPIMAGETOOL_SHA256)  $@.tmp" | sha256sum -c -
	chmod +x $@.tmp
	mv $@.tmp $@

$(APPIMAGE_RUNTIME):
	mkdir -p $(dir $@)
	curl -fsSL -o $@.tmp https://github.com/AppImage/type2-runtime/releases/download/$(APPIMAGE_RUNTIME_VERSION)/runtime-x86_64
	echo "$(APPIMAGE_RUNTIME_SHA256)  $@.tmp" | sha256sum -c -
	mv $@.tmp $@

app: build $(MOLTENVK_TAR)
	rm -rf $(APP) $(ICONSET)
	mkdir -p $(APP)/Contents/MacOS $(APP)/Contents/Frameworks $(APP)/Contents/Resources $(ICONSET)
	cp $(BIN)/romp $(BIN)/romp-runner $(APP)/Contents/MacOS/
	tar -xOf $(MOLTENVK_TAR) MoltenVK/MoltenVK/dynamic/dylib/macOS/libMoltenVK.dylib > $(APP)/Contents/Frameworks/libMoltenVK.dylib
	lipo -thin arm64 -output $(APP)/Contents/Frameworks/libMoltenVK.dylib $(APP)/Contents/Frameworks/libMoltenVK.dylib
	cp LICENSE $(APP)/Contents/Resources/
	cp -R crates/romp-app/shaders $(APP)/Contents/Resources/
	tar -xOf $(MOLTENVK_TAR) MoltenVK/LICENSE > $(APP)/Contents/Resources/MoltenVK-LICENSE
	sed -e 's/@VERSION@/$(CARGO_VERSION)/g' -e 's/@BUNDLE_ID@/$(BUNDLE_ID)/g' \
		packaging/macos/Info.plist > $(APP)/Contents/Info.plist
	for size in 16 32 128 256 512; do \
		sips -z $$size $$size $(ICON) --out $(ICONSET)/icon_$${size}x$${size}.png >/dev/null && \
		sips -z $$((size * 2)) $$((size * 2)) $(ICON) --out $(ICONSET)/icon_$${size}x$${size}@2x.png >/dev/null || exit 1; \
	done
	iconutil --convert icns --output $(APP)/Contents/Resources/RomP.icns $(ICONSET)
	rm -rf $(ICONSET)
	codesign --force --sign - $(APP)/Contents/Frameworks/libMoltenVK.dylib
	codesign --force --sign - $(APP)/Contents/MacOS/romp-runner
	codesign --force --sign - $(APP)

dist: dist-$(PLATFORM)

dist-macos: app
	$(zip_app)

appimage: build $(APPIMAGETOOL) $(APPIMAGE_RUNTIME)
	rm -rf $(APPDIR) $(APPIMAGE)
	mkdir -p $(APPDIR)/usr/bin $(APPDIR)/usr/share/applications $(APPDIR)/usr/share/icons/hicolor/256x256/apps
	cp $(BIN)/romp $(BIN)/romp-runner $(APPDIR)/usr/bin/
	cp -R crates/romp-app/shaders $(APPDIR)/usr/bin/
	cp packaging/linux/romp.desktop $(APPDIR)/
	cp packaging/linux/romp.desktop $(APPDIR)/usr/share/applications/
	cp packaging/linux/romp.png $(APPDIR)/
	cp packaging/linux/romp.png $(APPDIR)/usr/share/icons/hicolor/256x256/apps/
	ln -s usr/bin/romp $(APPDIR)/AppRun
	ARCH=$(ARCH) $(APPIMAGETOOL) --no-appstream --runtime-file $(APPIMAGE_RUNTIME) $(APPDIR) $(APPIMAGE)
	rm -rf $(APPDIR)
	$(call checksum,$(APPIMAGE))

# The oldest supported Ubuntu LTS has this one; a newer requirement wouldn't start there.
GLIBC_MAX := 2.35

glibc-check:
	@for bin in $(BIN)/romp $(BIN)/romp-runner; do \
		need=$$(objdump -T $$bin | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -V | tail -n 1); \
		echo "$$bin needs glibc $$need"; \
		[ "$$(printf '%s\n' $$need $(GLIBC_MAX) | sort -V | tail -n 1)" = "$(GLIBC_MAX)" ] || { echo "that's newer than $(GLIBC_MAX)"; exit 1; }; \
	done

dist-linux: appimage
	rm -rf $(DIST)/$(LINUX_NAME) $(LINUX_TAR)
	mkdir -p $(DIST)/$(LINUX_NAME)
	cp $(BIN)/romp $(BIN)/romp-runner README.md LICENSE $(DIST)/$(LINUX_NAME)/
	cp -R crates/romp-app/shaders $(DIST)/$(LINUX_NAME)/
	tar -czf $(LINUX_TAR) -C $(DIST) $(LINUX_NAME)
	rm -rf $(DIST)/$(LINUX_NAME)
	$(call checksum,$(LINUX_TAR))

dist-windows: build
	rm -rf $(DIST)/$(WINDOWS_NAME) $(WINDOWS_ZIP)
	mkdir -p $(DIST)/$(WINDOWS_NAME)
	cp $(BIN)/romp.exe $(BIN)/romp-runner.exe README.md LICENSE $(DIST)/$(WINDOWS_NAME)/
	cp -R crates/romp-app/shaders $(DIST)/$(WINDOWS_NAME)/
	cd $(DIST) && 7z a -bso0 $(WINDOWS_NAME).zip $(WINDOWS_NAME)
	rm -rf $(DIST)/$(WINDOWS_NAME)
	$(call checksum,$(WINDOWS_ZIP))

clean:
	rm -rf $(DIST)

check: fmt-check clippy test deny machete typos workflows translations-check

TRANSLATIONS := crates/romp-app/translations
SLINT_TR := slint-tr-extractor@1.18.1
XTR := xtr@0.1.11

# Line numbers and dates would change the templates whenever code moves.
define tidy_pot
	awk '/^"POT-Creation-Date:/ { next } \
		/^#: / { n = split($$0, refs, " "); out = "#:"; split("", seen); \
			for (i = 2; i <= n; i++) { sub(/:[0-9]+$$/, "", refs[i]); if (!seen[refs[i]]++) out = out " " refs[i] } \
			print out; next } \
		{ print }' $(1) > $(1).tmp && mv $(1).tmp $(1)
endef

translation-tools:
	@command -v slint-tr-extractor > /dev/null || cargo install --locked $(SLINT_TR)
	@command -v xtr > /dev/null || cargo install --locked $(XTR)

translations: translation-tools
	slint-tr-extractor --no-default-translation-context -d romp-app -o $(TRANSLATIONS)/romp-app.pot crates/romp-app/ui/*.slint
	$(call tidy_pot,$(TRANSLATIONS)/romp-app.pot)
	# xtr follows modules from main.rs but misses one of two modules with the same name.
	xtr --add-location file -o $(TRANSLATIONS)/rust.pot $$(find crates/romp-app/src -name '*.rs' | sort)
	$(call tidy_pot,$(TRANSLATIONS)/rust.pot)

pseudo: translations
	python3 $(TRANSLATIONS)/pseudo.py $(TRANSLATIONS)

translations-check: translations
	git diff --exit-code -- $(TRANSLATIONS)/*.pot

fmt-check:
	cargo fmt --all --check

clippy:
	CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets --locked -- -D warnings

test:
	CARGO_INCREMENTAL=0 cargo test --workspace --locked

deny:
	cargo deny check

machete:
	cargo machete

typos:
	typos

workflows:
	actionlint
	zizmor .github

release-notes:
	@awk -v heading="## [$(VERSION)]" ' \
		index($$0, "## [") == 1 { if (found) exit; found = index($$0, heading) == 1; next } \
		found && (printed || NF) { print; printed = 1 } \
		END { if (!printed) { print "No changelog entry for $(VERSION)" > "/dev/stderr"; exit 1 } }' CHANGELOG.md

RCHEEVOS_DIR := crates/romp-cheevos/rcheevos
# Only RAIntegration on Windows and external clients use these, and RomP builds neither.
RCHEEVOS_UNUSED := src/rc_client_external.c src/rc_client_raintegration.c

# Replaces the bundled rcheevos with a release: the latest, or RCHEEVOS=v12.5.0.
update-rcheevos:
	@set -e; \
	tag="$(RCHEEVOS)"; \
	if [ -z "$$tag" ]; then \
		tag=$$(curl -fsSL https://api.github.com/repos/RetroAchievements/rcheevos/releases/latest \
			| sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p'); \
	fi; \
	test -n "$$tag"; \
	tmp=$$(mktemp -d); \
	trap 'rm -rf "$$tmp"' EXIT; \
	curl -fsSL "https://github.com/RetroAchievements/rcheevos/archive/refs/tags/$$tag.tar.gz" \
		| tar -xz -C "$$tmp" --strip-components 1; \
	rm -rf $(RCHEEVOS_DIR)/include $(RCHEEVOS_DIR)/src; \
	cp -R "$$tmp/include" "$$tmp/src" "$$tmp/LICENSE" $(RCHEEVOS_DIR)/; \
	rm -f $(addprefix $(RCHEEVOS_DIR)/,$(RCHEEVOS_UNUSED)); \
	echo "$$tag" > $(RCHEEVOS_DIR)/VERSION; \
	echo "rcheevos is now $$tag. Changes: https://github.com/RetroAchievements/rcheevos/releases/tag/$$tag"

-include signing/signing.mk
