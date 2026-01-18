mod helpers;
pub mod img;
pub mod qcow2;
pub mod target;
pub mod vdi;
pub mod vhd;
pub mod vmdk;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub enum Output {
    Img,
    Qcow2,
    Vdi,
    Vhd,
    Vmdk,
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
            "qcow2" => Ok(Output::Qcow2),
            "vdi" => Ok(Output::Vdi),
            "vhd" => Ok(Output::Vhd),
            "vmdk" => Ok(Output::Vmdk),
            _ => anyhow::bail!("Unknown output file extension: .{}", ext),
        }
    }
}
