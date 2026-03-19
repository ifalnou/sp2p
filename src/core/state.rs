use std::sync::RwLock;
use std::time::SystemTime;

#[derive(Clone, Debug)]
pub struct ActiveTransfer {
    pub id: String,
    pub filename: String,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub is_sending: bool,
}

#[derive(Clone, Debug)]
pub struct TransferHistoryItem {
    pub filename: String,
    pub is_sending: bool,
    pub success: bool,
    pub timestamp: SystemTime,
}

pub struct TransferState {
    pub active_transfers: Vec<ActiveTransfer>,
    pub history: Vec<TransferHistoryItem>,
}

impl TransferState {
    pub fn new() -> Self {
        Self {
            active_transfers: Vec::new(),
            history: Vec::new(),
        }
    }
}

lazy_static::lazy_static! {
    pub static ref GLOBAL_STATE: RwLock<TransferState> = RwLock::new(TransferState::new());
}
