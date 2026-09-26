fn main() {
    let style = std::env::var("SLINT_STYLE").unwrap_or_else(|_| "fluent-dark".into());
    let config = slint_build::CompilerConfiguration::new().with_style(style);
    slint_build::compile_with_config("ui/main.slint", config).expect("compile slint ui");
}
