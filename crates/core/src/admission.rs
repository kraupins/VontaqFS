//! Core-owned, non-reentrant, transferable admission leases.
//! All claims for an operation are acquired together and held without the SQLite mutex.
use crate::{StorageError, StorageResult};
use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimMode {
    Shared,
    Exclusive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    scope: String,
    mode: ClaimMode,
    subtree: bool,
}

impl Claim {
    pub fn shared(scope: impl Into<String>) -> Self {
        Self {
            scope: scope.into(),
            mode: ClaimMode::Shared,
            subtree: false,
        }
    }
    pub fn exclusive(scope: impl Into<String>) -> Self {
        Self {
            scope: scope.into(),
            mode: ClaimMode::Exclusive,
            subtree: false,
        }
    }
    pub fn subtree(scope: impl Into<String>) -> Self {
        Self {
            scope: scope.into(),
            mode: ClaimMode::Exclusive,
            subtree: true,
        }
    }
    pub fn global_shared() -> Self {
        Self::shared("global")
    }
    pub fn space_shared(id: &str) -> Self {
        Self::shared(format!("space:{id}"))
    }
    pub fn space_exclusive(id: &str) -> Self {
        Self::exclusive(format!("space:{id}"))
    }
    pub fn path(id: &str, normalized_path: &str) -> Self {
        Self::subtree(format!(
            "path:{id}:{}",
            normalized_path.trim_end_matches('/')
        ))
    }
    pub fn snapshot(id: &str, snapshot_id: &str) -> Self {
        Self::exclusive(format!("snapshot:{id}:{snapshot_id}"))
    }
    pub fn destination(id: &str) -> Self {
        Self::exclusive(format!("destination:{id}"))
    }
}

#[derive(Debug, Default)]
struct Active {
    next_id: u64,
    leases: BTreeMap<u64, Vec<Claim>>,
}

#[derive(Debug, Default)]
pub struct AdmissionManager {
    active: Mutex<Active>,
    changed: Condvar,
}

#[derive(Debug)]
pub struct AdmissionLease {
    owner: Arc<AdmissionManager>,
    id: u64,
    claims: Vec<Claim>,
}

/// A private mutation context reuses an already-acquired parent lease. It must
/// never acquire a nested lease, particularly from composite `batch` operations.
#[derive(Debug)]
pub struct MutationContext<'a> {
    lease: &'a AdmissionLease,
}

impl AdmissionLease {
    pub fn context(&self) -> MutationContext<'_> {
        MutationContext { lease: self }
    }
    /// Prevent passing a mutation lease from another Core instance/root to
    /// an internal child operation, even if its caller supplied the same ID.
    pub(crate) fn belongs_to(&self, manager: &Arc<AdmissionManager>) -> bool {
        Arc::ptr_eq(&self.owner, manager)
    }
    pub fn covers(&self, claim: &Claim) -> bool {
        self.claims.iter().any(|held| {
            held.scope == claim.scope
                || (held.subtree && claim.scope.starts_with(&(held.scope.clone() + "/")))
        })
    }
}
impl MutationContext<'_> {
    pub fn covers(&self, claim: &Claim) -> bool {
        self.lease.claims.iter().any(|held| held == claim)
    }
}

fn overlaps(a: &Claim, b: &Claim) -> bool {
    if a.mode == ClaimMode::Shared && b.mode == ClaimMode::Shared {
        return false;
    }
    if a.scope == b.scope {
        return true;
    }
    (a.subtree && b.scope.starts_with(&(a.scope.clone() + "/")))
        || (b.subtree && a.scope.starts_with(&(b.scope.clone() + "/")))
}

impl AdmissionManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
    /// Nonblocking variant for async route admission: never stall a Tokio executor thread.
    pub fn try_acquire(self: &Arc<Self>, mut claims: Vec<Claim>) -> StorageResult<AdmissionLease> {
        if claims.is_empty() {
            return Err(StorageError::RequestInvalid(
                "empty admission claim set".into(),
            ));
        }
        claims.sort_by(|a, b| a.scope.cmp(&b.scope));
        claims.dedup();
        let mut active = self
            .active
            .lock()
            .map_err(|_| StorageError::Internal("admission lock poisoned".into()))?;
        if active
            .leases
            .values()
            .any(|held| claims.iter().any(|a| held.iter().any(|b| overlaps(a, b))))
        {
            return Err(StorageError::Conflict("operation scope busy".into()));
        }
        active.next_id = active
            .next_id
            .checked_add(1)
            .ok_or_else(|| StorageError::Internal("admission generation exhausted".into()))?;
        let id = active.next_id;
        active.leases.insert(id, claims.clone());
        Ok(AdmissionLease {
            owner: Arc::clone(self),
            id,
            claims,
        })
    }
    pub fn acquire(self: &Arc<Self>, mut claims: Vec<Claim>) -> StorageResult<AdmissionLease> {
        if claims.is_empty() {
            return Err(StorageError::RequestInvalid(
                "empty admission claim set".into(),
            ));
        }
        claims.sort_by(|a, b| a.scope.cmp(&b.scope));
        claims.dedup();
        let mut active = self
            .active
            .lock()
            .map_err(|_| StorageError::Internal("admission lock poisoned".into()))?;
        while active
            .leases
            .values()
            .any(|held| claims.iter().any(|a| held.iter().any(|b| overlaps(a, b))))
        {
            active = self
                .changed
                .wait(active)
                .map_err(|_| StorageError::Internal("admission wait poisoned".into()))?;
        }
        active.next_id = active
            .next_id
            .checked_add(1)
            .ok_or_else(|| StorageError::Internal("admission generation exhausted".into()))?;
        let id = active.next_id;
        active.leases.insert(id, claims.clone());
        Ok(AdmissionLease {
            owner: Arc::clone(self),
            id,
            claims,
        })
    }
}

impl Drop for AdmissionLease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.owner.active.lock() {
            state.leases.remove(&self.id);
            self.owner.changed.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, thread, time::Duration};
    #[test]
    fn same_path_blocks_but_other_path_proceeds() {
        let manager = AdmissionManager::new();
        let _first = manager
            .acquire(vec![Claim::space_shared("s"), Claim::path("s", "/a")])
            .unwrap();
        assert!(manager
            .acquire(vec![Claim::space_shared("s"), Claim::path("s", "/b")])
            .is_ok());
        let other = manager.clone();
        let (tx, rx) = mpsc::channel();
        let join = thread::spawn(move || {
            let _lease = other.acquire(vec![Claim::path("s", "/a/b")]).unwrap();
            tx.send(()).unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
        drop(_first);
        assert!(rx.recv_timeout(Duration::from_secs(2)).is_ok());
        join.join().unwrap();
    }
    #[test]
    fn exclusive_space_fences_shared_members() {
        let manager = AdmissionManager::new();
        let lease = manager.acquire(vec![Claim::space_exclusive("s")]).unwrap();
        let other = manager.clone();
        let (tx, rx) = mpsc::channel();
        let join = thread::spawn(move || {
            let _guard = other.acquire(vec![Claim::space_shared("s")]).unwrap();
            tx.send(()).unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
        drop(lease);
        assert!(rx.recv_timeout(Duration::from_secs(2)).is_ok());
        join.join().unwrap();
    }
}
