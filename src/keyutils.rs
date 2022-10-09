//! Example implementation of the keyring crate's trait interface
//! using linux-keyutils
use linux_keyutils::{KeyRing, KeyRingIdentifier};

use super::credential::{Credential, CredentialApi, CredentialBuilder, CredentialBuilderApi};
use super::error::{decode_password, Error as ErrorCode, Result};

/// Since the CredentialBuilderApi::build method does not provide
/// an initial secret, this wraps a linux_keyutils::KeyRing instead
/// of a linux_keyutils::Key. Since it is impossible to have a
/// zero-length key.
///
/// The added benefit is any call to get_password before set_password
/// will result in a proper error as the key does not exist until
/// set_password is called.
#[derive(Debug, Clone)]
pub struct KeyutilsCredential {
    /// Host keyring
    pub inner: KeyRing,
    /// Description of the key entry
    pub description: String,
}

impl CredentialApi for KeyutilsCredential {
    /// Set a password in the underlying store
    ///
    /// This will overwrite the entry if it already exists since
    /// it's using `add_key` under the hood.
    fn set_password(&self, password: &str) -> Result<()> {
        self.inner
            .add_key(&self.description, password)
            .map_err(|e| ErrorCode::PlatformFailure(e.into()))?;
        Ok(())
    }

    /// Retrieve a password from the underlying store
    ///
    /// This requires a call to `Key::read` with checked conversions
    /// to a utf8 Rust string.
    fn get_password(&self) -> Result<String> {
        // Verify that the key exists and is valid
        let key = self
            .inner
            .search(&self.description)
            .map_err(|e| ErrorCode::PlatformFailure(e.into()))?;
        // Read in the key (making sure we have enough room)
        let mut buffer = vec![0u8; 65535];
        let len = key
            .read(&mut buffer)
            .map_err(|e| ErrorCode::PlatformFailure(e.into()))?;
        unsafe {
            buffer.set_len(len);
        }
        // Attempt utf-8 conversion
        decode_password(buffer)
    }

    /// Delete a password from the underlying store.
    ///
    /// Under the hood this uses `Key::invalidate` to immediately
    /// invalidate the key and prevent any further successful
    /// searches.
    fn delete_password(&self) -> Result<()> {
        // Verify that the key exists and is valid
        let key = self
            .inner
            .search(&self.description)
            .map_err(|e| ErrorCode::PlatformFailure(e.into()))?;
        // Invalidate the key immediately
        key.invalidate()
            .map_err(|e| ErrorCode::PlatformFailure(e.into()))?;
        Ok(())
    }

    /// Cast the credential object to std::any::Any.  This allows clients
    /// to downcast the credential to its concrete type so they
    /// can do platform-specific things with it (e.g, unlock it)
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl KeyutilsCredential {
    /// Construct a credential from the underlying platform credential
    /// This is basically a no-op, because we don't keep any extra attributes.
    /// But at least we make sure the underlying platform credential exists.
    pub fn get_credential(&self) -> Result<Self> {
        self.inner
            .search(&self.description)
            .map_err(|e| ErrorCode::PlatformFailure(e.into()))?;
        Ok(self.clone())
    }

    /// Create the platform credential for a Keyutils entry.
    ///
    /// A target string is interpreted as the KeyRing to use for the entry.
    pub fn new_with_target(target: Option<&str>, service: &str, user: &str) -> Result<Box<Self>> {
        // Construct the credential with a URI-style description
        Ok(Box::new(Self {
            inner: keyring_from_target(target)?,
            description: format!("keyring-rs:{}@{}", user, service),
        }))
    }
}

/// Simple abstraction around building access to a persistent
/// keyring.
#[derive(Debug, Copy, Clone)]
struct KeyutilsCredentialBuilder {}

/// A keyutils credential builder based off the persistent user-session
/// keyring.
impl CredentialBuilderApi for KeyutilsCredentialBuilder {
    /// Attempt to access the persistent user-session keyring. The
    /// keyring::Entry will be invalid until Entry::set_password is set.
    fn build(&self, target: Option<&str>, service: &str, user: &str) -> Result<Box<Credential>> {
        KeyutilsCredential::new_with_target(target, service, user)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn keyring_from_target(target: Option<&str>) -> Result<KeyRing> {
    match target.unwrap_or("UserSession") {
        "UserSession" => Ok(KeyRing::get_persistent(KeyRingIdentifier::UserSession)
            .map_err(|e| ErrorCode::PlatformFailure(e.into()))?),
        "UserSession.transient" => Ok(KeyRing::from_special_id(
            KeyRingIdentifier::UserSession,
            true,
        )
        .map_err(|e| ErrorCode::PlatformFailure(e.into()))?),
        "Session" => KeyRing::get_persistent(KeyRingIdentifier::Session)
            .map_err(|e| ErrorCode::PlatformFailure(e.into()))?,
        "Session.transient" => Ok(KeyRing::from_special_id(KeyRingIdentifier::Session, true)
            .map_err(|e| ErrorCode::PlatformFailure(e.into()))?),
        "User" => KeyRing::get_persistent(KeyRingIdentifier::User)
            .map_err(|e| ErrorCode::PlatformFailure(e.into()))?,
        "User.transient" => Ok(KeyRing::from_special_id(KeyRingIdentifier::User, true)
            .map_err(|e| ErrorCode::PlatformFailure(e.into()))?),
        _ => Err(ErrorCode::Invalid(
            "target".to_string(),
            "must be one of User, Session, or UserSession, optionally followed by .transient"
                .to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use crate::{tests::generate_random_string, tests::test_round_trip, Credential, Entry, Error};

    use super::KeyutilsCredential;

    fn entry_new(service: &str, user: &str) -> Entry {
        crate::tests::entry_from_constructor(KeyutilsCredential::new_with_target, service, user)
    }

    #[test]
    fn test_invalid_parameter() {
        let credential = KeyutilsCredential::new_with_target(Some(""), "service", "user");
        assert!(
            matches!(credential, Err(Error::Invalid(_, _))),
            "Created entry with empty target"
        );
    }

    #[test]
    fn test_empty_service_and_user() {
        crate::tests::test_empty_service_and_user(entry_new);
    }

    #[test]
    fn test_missing_entry() {
        crate::tests::test_missing_entry(entry_new);
    }

    #[test]
    fn test_empty_password() {
        crate::tests::test_empty_password(entry_new);
    }

    #[test]
    fn test_round_trip_ascii_password() {
        crate::tests::test_round_trip_ascii_password(entry_new);
    }

    #[test]
    fn test_round_trip_non_ascii_password() {
        crate::tests::test_round_trip_non_ascii_password(entry_new);
    }

    #[test]
    fn test_update() {
        crate::tests::test_update(entry_new);
    }

    #[test]
    fn test_get_credential() {
        let name = generate_random_string();
        let entry = entry_new(&name, &name);
        let credential: &KeyutilsCredential = entry
            .get_credential()
            .downcast_ref()
            .expect("Not a Keyutils credential");
        assert!(
            credential.get_credential().is_err(),
            "Platform credential shouldn't exist yet!"
        );
        entry
            .set_password("test get_credential")
            .expect("Can't set password for get_credential");
        assert!(credential.get_credential().is_ok());
        entry
            .delete_password()
            .expect("Couldn't delete after get_credential");
        assert!(matches!(entry.get_password(), Err(Error::NoEntry)));
    }
}
