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

### Key events

A host with `UI_SERVICE_HOST_CAP_KEYBOARD` also sends
`UI_SERVICE_EVENT_KEY_DOWN` (5) for every key press and auto-repeat: `button`
is the evdev key code (`ui_core::key` names the common ones) and `reserved` the
character it types under the host's layout and modifiers, as a Unicode scalar,
0 for none. The layout of `UIServiceHostEvent` is unchanged; a host without the
capability never sends it and an app that ignores keys is unaffected. Rust sees
it as `Event::KeyDown { code, character }`, and the app runtime hands it to the
focused window.

## Login host table

`UIServiceRunLogin(host, login)` runs the fullscreen setup/login screen
(`ui-login`) before the desktop. `login` is a separate
`UIServiceLoginHostV1` (`UI_SERVICE_LOGIN_ABI_VERSION_V1`), not part of the
app host table, so ordinary apps never reach the passcode checker. Its
callbacks answer `UI_SERVICE_AUTH_*`:

| Callback | Does |
|---|---|
| `auth_status` | whether a passcode is set, failures, locked, seconds to wait |
| `auth_verify` | check a passcode (4..64 bytes); `RETRY_LATER` fills `*wait_seconds` |
| `auth_set` | set the first passcode (`old_length` 0) |

UIService decodes any unknown answer as an error, never as success, and
`UIServiceRunLogin` returns `UI_SERVICE_STATUS_OK` only once a passcode was
accepted or set. While `auth_status` fails it keeps waiting. It keeps no copy
of the passcode: the typed buffer is wiped after every call. On NXU the host
answers with tepOS's AuthenticationService over the Trusted Enclave mailbox.

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
