fn main() {
    let config = slint_build::CompilerConfiguration::new().with_debug_info(true);
    slint_build::compile_with_config("ui/activity-manager.slint", config)
        .expect("Activity Manager UI compiles");
}
