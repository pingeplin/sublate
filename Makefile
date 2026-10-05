MACOSX_DEPLOYMENT_TARGET := 26.0
export MACOSX_DEPLOYMENT_TARGET

# Every build is signed for distribution, so the Keychain item holding the API key stays
# readable across rebuilds and the app that is tested is the app that ships.
SIGN_IDENTITY ?= Developer ID Application: YingPing Lin (5Q458ND242)
# Created once with `xcrun notarytool store-credentials $(NOTARY_PROFILE)`.
NOTARY_PROFILE ?= sublate

CRATE := sublate_core
STATIC_LIB := core/target/release/lib$(CRATE).a
HEADERS := core/target/bindings/Headers
KIT := app/Kit
XCFRAMEWORK := $(KIT)/$(CRATE)FFI.xcframework
BINDINGS := $(KIT)/Sources/SublateCore
DERIVED := app/build
APP := $(DERIVED)/Build/Products/Release/Sublate.app
TOOLS := vendor/tools
TOOLS_READY := $(TOOLS)/deno
VERSION := $(shell awk '$$1 == "MARKETING_VERSION:" { print $$2 }' app/project.yml)
DMG := dist/Sublate-$(VERSION).dmg

# The generator builds in the debug profile so its `cli` feature never leaks into the release
# archive, and runs inside the crate because it reads `cargo metadata`.
BINDGEN := cd core && cargo run --quiet --features cli --bin uniffi-bindgen-swift --

RUST_SOURCES := $(shell find core/src -name '*.rs') core/Cargo.toml core/Cargo.lock

.PHONY: core tools project build run test dmg release clean

core: $(XCFRAMEWORK)

tools: $(TOOLS_READY)

$(TOOLS_READY): vendor/fetch.sh vendor/tools.lock vendor/deno.entitlements $(wildcard vendor/licenses/*)
	SIGN_IDENTITY="$(SIGN_IDENTITY)" vendor/fetch.sh

$(XCFRAMEWORK): $(RUST_SOURCES)
	cargo build --manifest-path core/Cargo.toml --release --lib
	rm -rf $(HEADERS) $(XCFRAMEWORK)
	$(BINDGEN) ../$(STATIC_LIB) ../$(BINDINGS) --swift-sources
	$(BINDGEN) ../$(STATIC_LIB) ../$(HEADERS) --headers --modulemap \
		--module-name $(CRATE)FFI --modulemap-filename module.modulemap
	xcodebuild -create-xcframework -library $(STATIC_LIB) -headers $(HEADERS) -output $(XCFRAMEWORK)

project: core tools
	cd app && xcodegen generate

build: project
	xcodebuild -project app/Sublate.xcodeproj -scheme Sublate \
		-configuration Release -derivedDataPath $(DERIVED) \
		CODE_SIGN_IDENTITY="$(SIGN_IDENTITY)" build
	codesign --verify --deep --strict "$(APP)"

# A signed disk image for trying the installer locally; Gatekeeper rejects it elsewhere.
dmg: build
	scripts/package.sh "$(APP)" "$(DMG)" "$(SIGN_IDENTITY)"

# The disk image to publish: app and image are both notarized and stapled.
release: build
	scripts/package.sh "$(APP)" "$(DMG)" "$(SIGN_IDENTITY)" "$(NOTARY_PROFILE)"

run: build
	open "$(APP)"

test: core
	cargo test --manifest-path core/Cargo.toml
	swift test --package-path $(KIT)

clean:
	cargo clean --manifest-path core/Cargo.toml
	rm -rf $(DERIVED) $(XCFRAMEWORK) $(BINDINGS) app/Sublate.xcodeproj $(KIT)/.build
	rm -rf $(TOOLS) vendor/.stage dist
