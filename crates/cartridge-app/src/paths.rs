use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    std::env::var_os("CARTRIDGE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::data_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("Cartridge")
        })
}

pub fn runner_exe() -> std::io::Result<PathBuf> {
    Ok(std::env::current_exe()?.with_file_name("cartridge-runner"))
}
