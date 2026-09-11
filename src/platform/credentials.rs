use super::{CredentialStore, PlatformError};

#[derive(Debug, Default, Clone, Copy)]
pub struct UnsupportedCredentialStore;

impl CredentialStore for UnsupportedCredentialStore {
    fn get(&self, _key: &str) -> Result<String, PlatformError> {
        Err(PlatformError::Unsupported(
            "credential store is not available in MVP".into(),
        ))
    }
}
