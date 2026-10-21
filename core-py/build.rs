fn main() {
    pyo3_build_config::use_pyo3_cfgs();

    #[cfg(target_os = "macos")]
    {
        let conf = pyo3_build_config::get();
        if let Some(lib_dir) = conf.lib_dir() {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir);
        }
    }
}
