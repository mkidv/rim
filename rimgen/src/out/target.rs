use std::path::{Path, PathBuf};

use rimio::prelude::*;

#[derive(Debug, Clone, Copy)]
pub enum DryRunMode {
    /// Rien n’est écrit — on log le plan uniquement.
    Plan,
    /// Écrit dans un fichier temporaire sparse, supprimé en fin de run.
    Tempfile,
    /// Pas de dry-run.
    Off,
}

pub struct TargetImage {
    /// On garde la possession pour la durée de vie du run.
    file: Option<std::fs::File>,
    /// Si tempfile, on le garde pour empêcher son unlink avant la fin.
    _tmp: Option<tempfile::NamedTempFile>,
    /// Chemin réel si “Off”, sinon chemin du tempfile (utile pour réouvrir).
    pub path: std::path::PathBuf,

    pub mode: DryRunMode,
}

impl TargetImage {
    pub fn open(output: &Path, total_bytes: u64, mode: DryRunMode) -> anyhow::Result<Self> {
        match mode {
            DryRunMode::Plan => {
                // Pas de fichier : on renvoie un handle “virtuel”
                Ok(Self {
                    file: None,
                    _tmp: None,
                    path: PathBuf::new(),
                    mode,
                })
            }
            DryRunMode::Tempfile => {
                let tmp = tempfile::NamedTempFile::new()?;
                let f = tmp.reopen()?; // indépendant du handle “tmp” pour set_len
                f.set_len(total_bytes)?;
                // Hint: sur la plupart des FS modernes, set_len produit un sparse
                let path = tmp.path().to_path_buf();
                Ok(Self {
                    file: Some(f),
                    _tmp: Some(tmp),
                    path,
                    mode,
                })
            }
            DryRunMode::Off => {
                let f = std::fs::File::options()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(output)?;
                f.set_len(total_bytes)?;
                Ok(Self {
                    file: Some(f),
                    _tmp: None,
                    path: output.to_path_buf(),
                    mode,
                })
            }
        }
    }

    pub fn as_io<'a>(&'a mut self) -> anyhow::Result<StdBlockIO<'a, std::fs::File>> {
        let file = self.file.as_mut().ok_or_else(|| {
            anyhow::anyhow!("No file backing in this mode (Plan). Use Tempfile or Off.")
        })?;
        Ok(StdBlockIO::new(file))
    }
}
