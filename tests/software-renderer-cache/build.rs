// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

fn main() {
    let kind = if cfg!(feature = "vector-fonts") {
        slint_build::EmbedResourcesKind::EmbedFiles
    } else {
        slint_build::EmbedResourcesKind::EmbedForSoftwareRenderer
    };
    let config = slint_build::CompilerConfiguration::new().embed_resources(kind);
    #[cfg(feature = "sdf-fonts")]
    let config = config.with_sdf_fonts(true);
    slint_build::compile_with_config("ui.slint", config).unwrap();
}
