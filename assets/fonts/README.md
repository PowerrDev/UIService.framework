# UIService system fonts

UIService does not vendor Inter. Put your Inter files here:

- `Inter-Regular.ttf`
- `Inter-SemiBold.ttf` (optional; regular is used as the fallback weight)

No manual conversion step is required. `make nxu` passes these paths to the
`ui-service-nxu` Cargo build script, which embeds the fonts into Cargo's private
`OUT_DIR`. If Inter Regular is absent, the bring-up bridge still builds with the
bootstrap text renderer.

You can also point the build at other locations:

```sh
make nxu INTER_REGULAR=/path/to/Inter-Regular.ttf \
    INTER_SEMIBOLD=/path/to/Inter-SemiBold.ttf
```

## Borel

`Borel-Regular.ttf` is vendored (SIL Open Font License 1.1, see
`LICENSE-Borel.txt`): the setup greeting is set in it, and a first boot must
not depend on fonts the build host happens to have. `make nxu` embeds it like
Inter (`BOREL_REGULAR` overrides the path); without it the greeting falls
back to Inter.
