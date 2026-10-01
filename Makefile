MACOSX_DEPLOYMENT_TARGET := 26.0
export MACOSX_DEPLOYMENT_TARGET

CRATE := contents_title_core
STATIC_LIB := core/target/release/lib$(CRATE).a
HEADERS := core/target/bindings/Headers
KIT := app/Kit
XCFRAMEWORK := $(KIT)/$(CRATE)FFI.xcframework
BINDINGS := $(KIT)/Sources/ContentsTitleCore
DERIVED := app/build
APP := $(DERIVED)/Build/Products/Release/Contents Title.app

# The generator builds in the debug profile so its `cli` feature never leaks into the release
# archive, and runs inside the crate because it reads `cargo metadata`.
BINDGEN := cd core && cargo run --quiet --features cli --bin uniffi-bindgen-swift --

RUST_SOURCES := $(shell find core/src -name '*.rs') core/Cargo.toml core/Cargo.lock

.PHONY: core project build run test clean

core: $(XCFRAMEWORK)

$(XCFRAMEWORK): $(RUST_SOURCES)
	cargo build --manifest-path core/Cargo.toml --release --lib
	rm -rf $(HEADERS) $(XCFRAMEWORK)
	$(BINDGEN) ../$(STATIC_LIB) ../$(BINDINGS) --swift-sources
	$(BINDGEN) ../$(STATIC_LIB) ../$(HEADERS) --headers --modulemap \
		--module-name $(CRATE)FFI --modulemap-filename module.modulemap
	xcodebuild -create-xcframework -library $(STATIC_LIB) -headers $(HEADERS) -output $(XCFRAMEWORK)

project: core
	cd app && xcodegen generate

build: project
	xcodebuild -project app/ContentsTitle.xcodeproj -scheme ContentsTitle \
		-configuration Release -derivedDataPath $(DERIVED) build

run: build
	open "$(APP)"

test: core
	cargo test --manifest-path core/Cargo.toml
	swift test --package-path $(KIT)

clean:
	cargo clean --manifest-path core/Cargo.toml
	rm -rf $(DERIVED) $(XCFRAMEWORK) $(BINDINGS) app/ContentsTitle.xcodeproj $(KIT)/.build
