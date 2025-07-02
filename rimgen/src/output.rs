// rimgen/src/image/mod.rs

use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub enum Output {
    Img,
    Vhd,
}

impl Output {
    pub fn from_path(path: &Path) -> anyhow::Result<Self> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        match ext.as_str() {
            "img" => Ok(Output::Img),
            "vhd" => Ok(Output::Vhd),
            _ => anyhow::bail!("Unknown output file extension: .{}", ext),
        }
    }
}
