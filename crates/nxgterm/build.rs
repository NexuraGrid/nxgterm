//! Embeds the application icon and version information in the Windows
//! executable. Other targets need nothing from this script.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/nxgterm.ico");
    println!("cargo:rerun-if-env-changed=NXGTERM_REQUIRE_ICON");
    // Cargo matches `cfg`s of build dependencies against the host, so
    // `winresource` (and the rc.exe/windres it drives) is only present on a
    // Windows host. Cross builds from Linux or macOS skip the icon.
    #[cfg(windows)]
    windows::embed_icon();
}

#[cfg(windows)]
mod windows {
    use std::env;

    pub fn embed_icon() {
        if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
            return;
        }
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../assets/nxgterm.ico");
        if let Err(error) = resource.compile() {
            // Release builds set NXGTERM_REQUIRE_ICON so a missing resource
            // compiler fails loudly instead of shipping an icon-less exe.
            if env::var_os("NXGTERM_REQUIRE_ICON").is_some() {
                panic!("failed to embed the Windows icon: {error}");
            }
            println!("cargo:warning=nxgterm: Windows icon not embedded: {error}");
        }
    }
}
