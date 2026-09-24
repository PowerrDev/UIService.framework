NXU_TARGET ?= aarch64-unknown-none-softfloat
NXU_ARCHIVE := target/$(NXU_TARGET)/release/libui_service_nxu.a
INTER_REGULAR ?= assets/fonts/Inter-Regular.ttf
INTER_SEMIBOLD ?= assets/fonts/Inter-SemiBold.ttf
CURSOR_ARROW ?= assets/Cursors/arrow.cur
CURSOR_HAND ?= assets/Cursors/hand.cur
CURSOR_MOVE ?= assets/Cursors/move.cur
CURSOR_RESIZE_EW ?= assets/Cursors/resize-ew.cur
CURSOR_RESIZE_NS ?= assets/Cursors/resize-ns.cur
CURSOR_RESIZE_NESW ?= assets/Cursors/resize-nesw.cur
CURSOR_RESIZE_NWSE ?= assets/Cursors/resize-nwse.cur
CURSOR_TEXT ?= assets/Cursors/text.cur
BACKGROUND ?= assets/Backgrounds/DefaultWallpaper.jpg
MENUBAR_LOGO ?= assets/Branding/logo.png

APP ?= sample-app
NAME ?= Sample App
ID ?= com.butterscotch.sample-app
WIDTH ?= 640
HEIGHT ?= 420

.PHONY: check demo about voyager docs assets new-app nxu nxu-i386 clean

check:
	cargo check --workspace --exclude ui-service-nxu

demo:
	cargo run -p hello-window

about:
	cargo run -p about-sevos-preview

voyager:
	cargo run -p voyager-preview

docs:
	cargo doc --workspace --exclude ui-service-nxu --no-deps

new-app:
	python3 tools/new_app.py \
		--package "$(APP)" \
		--name "$(NAME)" \
		--identifier "$(ID)" \
		--width "$(WIDTH)" \
		--height "$(HEIGHT)"

assets:
	@test -f "$(INTER_REGULAR)" || { \
		echo "Missing $(INTER_REGULAR)"; \
		echo "Place Inter-Regular.ttf in assets/fonts/."; \
		exit 1; \
	}
	@echo "UIService: Inter assets ready; Cargo embeds them automatically during make nxu."

nxu:
	@if [ ! -f "$(INTER_REGULAR)" ]; then \
		echo "UIService: Inter-Regular.ttf not present; NXU will use bootstrap text."; \
	fi
	UISERVICE_INTER_REGULAR="$(abspath $(INTER_REGULAR))" \
	UISERVICE_INTER_SEMIBOLD="$(abspath $(INTER_SEMIBOLD))" \
	UISERVICE_CURSOR_ARROW="$(abspath $(CURSOR_ARROW))" \
	UISERVICE_CURSOR_HAND="$(abspath $(CURSOR_HAND))" \
	UISERVICE_CURSOR_MOVE="$(abspath $(CURSOR_MOVE))" \
	UISERVICE_CURSOR_RESIZE_EW="$(abspath $(CURSOR_RESIZE_EW))" \
	UISERVICE_CURSOR_RESIZE_NS="$(abspath $(CURSOR_RESIZE_NS))" \
	UISERVICE_CURSOR_RESIZE_NESW="$(abspath $(CURSOR_RESIZE_NESW))" \
	UISERVICE_CURSOR_RESIZE_NWSE="$(abspath $(CURSOR_RESIZE_NWSE))" \
	UISERVICE_CURSOR_TEXT="$(abspath $(CURSOR_TEXT))" \
	UISERVICE_BACKGROUND="$(abspath $(BACKGROUND))" \
	UISERVICE_MENUBAR_LOGO="$(abspath $(MENUBAR_LOGO))" \
	cargo build -p ui-service-nxu --release --target $(NXU_TARGET)
	mkdir -p build
	cp $(NXU_ARCHIVE) build/libUIService.a
	cp include/UIService.h build/UIService.h
	@echo "UIService NXU archive: build/libUIService.a"
	@echo "UIService NXU header:  build/UIService.h"

# i686-nxu-none.json is a custom bare-metal x86-32 target (no built-in rustc
# triple covers freestanding i686), soft-float and no SSE/MMX for the same
# reason the arm64 NXU_TARGET is softfloat: the i386 port does not save
# FPU/SSE context across a context switch. A .json target has no prebuilt
# std, so it needs nightly plus -Z build-std; NXU_TARGET's real rustc triple
# already has a prebuilt std, so the plain `nxu` target above is left alone.
NXU_I386_TARGET_SPEC := i686-nxu-none.json
NXU_I386_TARGET := i686-nxu-none
NXU_I386_ARCHIVE := target/$(NXU_I386_TARGET)/release/libui_service_nxu.a
CARGO_BUILD_STD_FLAGS := -Z build-std=core,alloc,compiler_builtins -Z build-std-features=compiler-builtins-mem -Z json-target-spec

nxu-i386:
	@if [ ! -f "$(INTER_REGULAR)" ]; then \
		echo "UIService: Inter-Regular.ttf not present; NXU will use bootstrap text."; \
	fi
	UISERVICE_INTER_REGULAR="$(abspath $(INTER_REGULAR))" \
	UISERVICE_INTER_SEMIBOLD="$(abspath $(INTER_SEMIBOLD))" \
	UISERVICE_CURSOR_ARROW="$(abspath $(CURSOR_ARROW))" \
	UISERVICE_CURSOR_HAND="$(abspath $(CURSOR_HAND))" \
	UISERVICE_CURSOR_MOVE="$(abspath $(CURSOR_MOVE))" \
	UISERVICE_CURSOR_RESIZE_EW="$(abspath $(CURSOR_RESIZE_EW))" \
	UISERVICE_CURSOR_RESIZE_NS="$(abspath $(CURSOR_RESIZE_NS))" \
	UISERVICE_CURSOR_RESIZE_NESW="$(abspath $(CURSOR_RESIZE_NESW))" \
	UISERVICE_CURSOR_RESIZE_NWSE="$(abspath $(CURSOR_RESIZE_NWSE))" \
	UISERVICE_CURSOR_TEXT="$(abspath $(CURSOR_TEXT))" \
	UISERVICE_BACKGROUND="$(abspath $(BACKGROUND))" \
	UISERVICE_MENUBAR_LOGO="$(abspath $(MENUBAR_LOGO))" \
	cargo +nightly build -p ui-service-nxu --release $(CARGO_BUILD_STD_FLAGS) --target $(NXU_I386_TARGET_SPEC)
	mkdir -p build-i386
	cp $(NXU_I386_ARCHIVE) build-i386/libUIService.a
	cp include/UIService.h build-i386/UIService.h
	@echo "UIService NXU i386 archive: build-i386/libUIService.a"
	@echo "UIService NXU i386 header:  build-i386/UIService.h"

clean:
	cargo clean
	rm -rf build build-i386
