# Software-renderer cache comparisons

The build script selects the font representation explicitly.
It does not require `SLINT_EMBED_TEXTURES` or `SLINT_EMBED_RESOURCES`.
Run from the Slint repository root:

```sh
cargo test --release --manifest-path tests/software-renderer-cache/Cargo.toml
cargo test --release --manifest-path tests/software-renderer-cache/Cargo.toml --no-default-features
cargo test --release --manifest-path tests/software-renderer-cache/Cargo.toml --features vector-fonts
cargo test --release --manifest-path tests/software-renderer-cache/Cargo.toml --features sdf-fonts
```

The default fixture embeds bitmap glyphs and asserts that layouts and masks are reused.
The no-default-features bitmap fixture asserts that disabled caches remain empty.
The other configurations assert that vector and SDF text bypass the bitmap cache.
Each configuration compares full and partial rendering over 1,440 frames across all four rotations.
The comparisons vary text, wrapping, elision, alignment, dimensions, clipping, opacity, colors, font size, italic style, letter spacing, line height, and negative or fractional positions.
They check retained budgets, long-label fallback, and component cleanup.
The MCU package workflow requires these tests and renderer unit tests with caches enabled and disabled before publishing a release.
