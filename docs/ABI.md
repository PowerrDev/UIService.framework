# UIService host ABI

`ui-abi` is the binary boundary between UIService and NXU. No Rust object
crosses the C ABI boundary, and applications never include this API directly.

## ABI v1: display

`UIServiceHostV1` contains a borrowed XRGB8888 surface plus presentation
callbacks.

## ABI v2: interactive host

`UIServiceHostV2` preserves the v1 prefix and appends `poll_event` plus the
`UI_SERVICE_HOST_CAP_INPUT` capability. NXU translates VirtIO mouse state into
platform-neutral UIService pointer events before Rust sees them.

### Scroll events

`UI_SERVICE_EVENT_SCROLL` (4) reports the scroll wheel, or a trackpad's
two-finger scroll, as `UIServiceHostEvent`: `x`/`y` are the pointer position and
`reserved` carries the signed number of notches turned since the last event (an
`int32_t` stored as its `uint32_t` bit pattern; positive is wheel up). Reusing
`reserved` keeps the struct's size and layout unchanged, so a host or app that
predates scrolling never sets or reads it. Rust sees it as `Event::Scroll`. A
trackpad delivers many small deltas, so apps should animate towards where they
add up (see `App::tick`) instead of jumping.

## Public naming

The framework rename is complete at the binary boundary:

```c
UIServiceAPIVersion();
UIServiceABIVersion();
UIServiceValidateHostV2(&host);
UIServiceRunAbout(&host);
```

The header is `UIService.h` and the NXU archive is `libUIService.a`.

## Versioning and ownership

Every host table begins with `{ struct_size, abi_version }`. Incompatible
layouts receive a new ABI version/struct instead of mutating an old layout.

NXU owns framebuffer/backbuffer storage. `get_surface` lends UIService a mapping
for an operation; UIService does not free or retain it. Callback results use
`UI_SERVICE_STATUS_*` values.
