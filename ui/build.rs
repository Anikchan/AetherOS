fn main() {
    // Debug info keeps element ids in the generated item tree, which is what
    // the headless element-query tests in `aether-ui` and every app use.
    let config = slint_build::CompilerConfiguration::new().with_debug_info(true);
    slint_build::compile_with_config("design.slint", config)
        .expect("compile the aether-ui design system");
}
