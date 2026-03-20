use std::fs;
use std::path::PathBuf;
use tracing::info;

pub struct AppDirs {
    pub root: PathBuf,
    pub inbox: PathBuf,
    pub send: PathBuf,
}

impl AppDirs {
    pub fn init(override_dir: Option<PathBuf>) -> std::io::Result<Self> {
        let root = if let Some(d) = override_dir {
            if d.is_absolute() {
                d
            } else {
                std::env::current_dir()?.join(d)
            }
        } else {
            dirs::data_local_dir()
                .map(|p| p.join("sp2p"))
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "Local data directory not found"))?
        };

        if !root.exists() {
            info!("Creating root directory at {:?}", root);
            fs::create_dir_all(&root)?;
        }

        let inbox = root.join("inbox");
        let send = root.join("send");

        if !inbox.exists() {
            info!("Creating inbox directory at {:?}", inbox);
            fs::create_dir_all(&inbox)?;
        }

        if !send.exists() {
            info!("Creating send directory at {:?}", send);
            fs::create_dir_all(&send)?;
        }

        Ok(Self {
            root,
            inbox,
            send,
        })
    }
}
