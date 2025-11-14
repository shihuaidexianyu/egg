use std::sync::{Arc, Mutex};

use crate::models::ApplicationInfo;

#[derive(Default)]
pub struct AppState {
    pub app_index: Arc<Mutex<Vec<ApplicationInfo>>>,
}
