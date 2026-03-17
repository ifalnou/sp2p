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
            d
        } else {
            let mut exe = std::env::current_exe()?;
            exe.pop(); // Remove the executable name
            exe
        };

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
