# NXU integration

The `ui-service-nxu` crate is the freestanding adapter linked into NXU during
early graphical bring-up.

## Build

```sh
rustup target add aarch64-unknown-none-softfloat
make nxu
```

Outputs:

```text
build/libUIService.a
build/UIService.h
```

NXU can build the interactive target with:

```sh
make uiservice-about
```

`make ui-about` remains as a compatibility alias.

## Host responsibilities

NXU owns devices and exposes only normalized facilities:

- a software backbuffer surface;
- present/damage callbacks;
- normalized pointer input.

VirtIO transport details stay in NXU.

## Presentation

UIService renders a completed scene in the host backbuffer, then asks NXU to
present only the damaged region. RAMFB is paced near 60 Hz but has no real
vblank handshake; VirtIO GPU remains the preferred path for explicit
transfer/flush presentation.

## Current app bootstrap

NXU currently calls `UIServiceRunAbout`. That exported entry point is only a
bootstrap registration point; the actual runtime is generic over `ui_app::App`.
This avoids duplicating a `RunWhatever` implementation for each future app and
keeps the eventual userspace app loader as a separate problem.
