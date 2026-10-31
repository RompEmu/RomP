fn main() {
    build_revision();
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent-dark".into())
        .with_bundled_translations("translations")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/main.slint", config).expect("compile slint ui");
    println!("cargo:rerun-if-changed=translations");
    #[cfg(windows)]
    winresource::WindowsResource::new()
        .set_icon("assets/icon.ico")
        .compile()
        .expect("compile windows resources");
}

fn build_revision() {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|text| text.trim().to_owned())
    };
    // Watch the resolved paths too: .git may be a file when building in a worktree.
    let head = git(&["symbolic-ref", "-q", "HEAD"]);
    for name in [Some("HEAD"), Some("packed-refs"), head.as_deref()]
        .into_iter()
        .flatten()
    {
        if let Some(path) = git(&["rev-parse", "--git-path", name])
            .filter(|path| std::path::Path::new(path).exists())
        {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    let revision = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=ROMP_BUILD_REVISION={revision}");
}
