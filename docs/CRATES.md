# Crate map

| Crate | Responsibility | Normal apps? |
| --- | --- | --- |
| `ui` | Public umbrella/prelude | Yes |
| `ui-app` | `App`, app actions, window preferences | Via `ui` |
| `ui-core` | Geometry, color, normalized events | Via `ui` |
| `ui-render` | Canvas/surface drawing | Advanced via `ui::render` |
| `ui-widgets` | Reusable controls/views | Via `ui` |
| `ui-window` | Standard chrome and dragging | Via `ui` |
| `ui-text` | TrueType parsing/rasterization | Via `ui` |
| `ui-login` | First-boot setup and login screen (host-testable; `examples/login-preview` renders it) | NXU adapter only |
| `ui-assets` | Cursor/resource decoding | Via `ui` |
| `ui-support` | App/system metadata | Via `ui` |
| `ui-platform` | Safe wrappers around NXU host tables | No |
| `ui-abi` | `repr(C)` ABI structs/constants | No |
| `ui-service-nxu` | NXU staticlib/runtime adapter | No |

If app code needs `ui-abi` or `ui-platform`, that is usually a layering smell.
