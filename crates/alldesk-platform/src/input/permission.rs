use std::sync::atomic::{AtomicBool, Ordering};

/// Manages input injection permissions. The remote peer must grant
/// permission before input events can be injected.
pub struct InputPermission {
    /// Whether the remote peer has granted input permission.
    granted: AtomicBool,
}

impl InputPermission {
    pub fn new() -> Self {
        Self {
            granted: AtomicBool::new(false),
        }
    }

    /// Grant input permission (called when remote side approves).
    pub fn grant(&self) {
        self.granted.store(true, Ordering::SeqCst);
    }

    /// Revoke input permission (called on disconnect or explicit revoke).
    pub fn revoke(&self) {
        self.granted.store(false, Ordering::SeqCst);
    }

    /// Check if input is currently permitted.
    pub fn is_granted(&self) -> bool {
        self.granted.load(Ordering::SeqCst)
    }
}

impl Default for InputPermission {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_denied() {
        let perm = InputPermission::new();
        assert!(!perm.is_granted(), "permission should be denied by default");
    }

    #[test]
    fn test_grant_allows() {
        let perm = InputPermission::new();
        perm.grant();
        assert!(perm.is_granted(), "permission should be granted after grant()");
    }

    #[test]
    fn test_revoke_denies() {
        let perm = InputPermission::new();
        perm.grant();
        perm.revoke();
        assert!(!perm.is_granted(), "permission should be denied after revoke()");
    }

    #[test]
    fn test_grant_revoke_cycle() {
        let perm = InputPermission::new();
        assert!(!perm.is_granted(), "initially denied");

        perm.grant();
        assert!(perm.is_granted(), "granted after first grant");

        perm.revoke();
        assert!(!perm.is_granted(), "denied after revoke");

        perm.grant();
        assert!(perm.is_granted(), "granted after second grant");

        perm.revoke();
        assert!(!perm.is_granted(), "denied after second revoke");
    }
}
