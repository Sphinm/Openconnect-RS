fn main() {
    // Only embed manifest on Windows (release builds)
    #[cfg(target_os = "windows")]
    {
        let profile = std::env::var("PROFILE").expect("PROFILE env var missing");
        if profile == "release" {
            embed_resource::compile("resources/manifest.rc", embed_resource::NONE);
        }
    }
}
