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

NXU calls `UIServiceRunDesktop`: the wallpaper, the menu bar and nothing
else. No app is linked in. Apps are processes in `/Applications/<Name>.app`
that NXU's Dock starts; each runs `crates/ui-app-nxu` (an archive from
`bundles/<app>`, built by `make nxu-apps` / `nxu-apps-i386`) and puts its
window on the desktop through NXU's UI session system calls. The desktop's
side is `bridge.rs` (the kernel's `ui_bridge_*` functions, called by symbol
like `timer_get_microseconds`), `remote.rs` (an app's window) and
`dockhost.rs` (the Dock's). The ABI is `crates/ui-session`, the mirror of
NXU's `kern/syscall/ui_session_defs.h`; NXU's `doc/apps-and-dock.md` has the
whole picture.
