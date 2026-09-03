//! Embeds the app icon into the Windows `.exe`.
//!
//! GPUI's own `build.rs` already embeds a DPI-awareness manifest this same way (see its
//! `resources/windows/gpui.rc`) — but that resource is a fixed manifest, not an icon, and
//! GPUI's runtime `WindowOptions`/`TitlebarOptions` expose no icon field at all. On Windows the
//! taskbar and titlebar icon come from the `.exe`'s own resources, so this is the only place
//! an icon can be set.

fn main() {
    #[cfg(target_os = "windows")]
    {
        let rc_file = std::path::Path::new("resources/windows/icon.rc");
        println!("cargo:rerun-if-changed={}", rc_file.display());
        println!("cargo:rerun-if-changed=assets/icon.ico");
        // `manifest_optional`, not `manifest_required`: this resource is an icon, not a
        // manifest — a build that can't find a resource compiler should still produce a
        // working (if icon-less) binary rather than fail outright.
        embed_resource::compile(rc_file, embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
}
