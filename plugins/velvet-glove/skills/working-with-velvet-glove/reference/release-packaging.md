# Release packaging

The plugin does not bundle prebuilt executables yet: users install
`velvet-glove` with `cargo install --locked --git https://github.com/plx/velvet-glove velvet-glove`
and Pkl separately, and the launcher warns at session start when the binary
is not on `PATH` (`VELVET_GLOVE_BIN` overrides the path). Before prebuilt
binaries are added, release automation must build and check macOS and Linux
on x86-64 and ARM64; Windows is unverified.
