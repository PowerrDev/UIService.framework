# UIService system fonts

UIService does not vendor font binaries. Put your Inter files here:

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
