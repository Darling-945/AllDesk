//! Lightweight ICE-like candidate gathering for NAT traversal.
//!
//! Collects local and server-reflexive (STUN) candidates, performs
//! connectivity checks, and selects the best working candidate pair.

use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use alldesk_core::error::Error;
use alldesk_core::Result;

/// A network candidate representing a possible connection path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct IceCandidate {
    /// Unique candidate ID.
    pub id: String,
    /// The candidate type.
    pub candidate_type: IceCandidateType,
    /// The socket address of this candidate.
    pub address: SocketAddr,
    /// Priority value (higher = preferred).
    pub priority: u32,
}

/// Type of ICE candidate.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum IceCandidateType {
    /// Local interface address (host candidate).
    Host,
    /// Server-reflexive address from STUN (mapped external address).
    ServerReflexive,
    /// Peer-reflexive address discovered during connectivity checks.
    PeerReflexive,
    /// Relay address (TURN/relay candidate).
    Relay,
}

impl std::fmt::Display for IceCandidateType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IceCandidateType::Host => write!(f, "host"),
            IceCandidateType::ServerReflexive => write!(f, "srflx"),
            IceCandidateType::PeerReflexive => write!(f, "prflx"),
            IceCandidateType::Relay => write!(f, "relay"),
        }
    }
}

/// Result of a connectivity check between two candidates.
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub local: IceCandidate,
    pub remote: IceCandidate,
    pub success: bool,
    pub rtt: Option<Duration>,
}

/// ICE-lite agent that collects candidates and performs connectivity checks.
pub struct IceAgent {
    /// Collected local candidates.
    local_candidates: Vec<IceCandidate>,
    /// Remote candidates received from the peer.
    remote_candidates: Vec<IceCandidate>,
    /// The selected working candidate pair, if any.
    selected_pair: Option<(IceCandidate, IceCandidate)>,
    /// Candidate ID counter.
    next_id: u32,
}

impl IceAgent {
    /// Create a new ICE agent.
    pub fn new() -> Self {
        Self {
            local_candidates: Vec::new(),
            remote_candidates: Vec::new(),
            selected_pair: None,
            next_id: 0,
        }
    }

    /// Gather host candidates from local network interfaces.
    pub fn gather_host_candidates(&mut self, port: u16) -> Result<Vec<IceCandidate>> {
        let mut candidates = Vec::new();
        let mut seen: HashSet<IpAddr> = HashSet::new();

        // Use a simple UDP socket binding to discover local IPs
        let socket = std::net::UdpSocket::bind("0.0.0.0:0")
            .map_err(|e| Error::Network(format!("bind for host candidates: {}", e)))?;

        let local_addr = socket
            .local_addr()
            .map_err(|e| Error::Network(format!("get local addr: {}", e)))?;

        // Add the primary local address
        let ip = local_addr.ip();
        if seen.insert(ip) {
            let candidate = IceCandidate {
                id: format!("h{}", self.next_id),
                candidate_type: IceCandidateType::Host,
                address: SocketAddr::new(ip, port),
                priority: Self::host_priority(),
            };
            self.next_id += 1;
            candidates.push(candidate.clone());
            self.local_candidates.push(candidate);
        }

        // Try to enumerate additional local addresses
        // by connecting a UDP socket to known public IPs
        for target in &["8.8.8.8:80", "1.1.1.1:80"] {
            if socket.connect(target).is_ok() {
                if let Ok(addr) = socket.local_addr() {
                    let ip = addr.ip();
                    if seen.insert(ip) {
                        let candidate = IceCandidate {
                            id: format!("h{}", self.next_id),
                            candidate_type: IceCandidateType::Host,
                            address: SocketAddr::new(ip, port),
                            priority: Self::host_priority(),
                        };
                        self.next_id += 1;
                        candidates.push(candidate.clone());
                        self.local_candidates.push(candidate);
                    }
                }
            }
        }

        info!("Gathered {} host candidate(s)", candidates.len());
        Ok(candidates)
    }

    /// Add a server-reflexive candidate (e.g., from STUN binding response).
    pub fn add_server_reflexive(&mut self, mapped_addr: SocketAddr) -> IceCandidate {
        // Check if we already have this candidate
        for c in &self.local_candidates {
            if c.address == mapped_addr && c.candidate_type == IceCandidateType::ServerReflexive {
                return c.clone();
            }
        }

        let candidate = IceCandidate {
            id: format!("s{}", self.next_id),
            candidate_type: IceCandidateType::ServerReflexive,
            address: mapped_addr,
            priority: Self::srflx_priority(),
        };
        self.next_id += 1;
        self.local_candidates.push(candidate.clone());
        candidate
    }

    /// Add a relay candidate.
    pub fn add_relay_candidate(&mut self, relay_addr: SocketAddr) -> IceCandidate {
        let candidate = IceCandidate {
            id: format!("r{}", self.next_id),
            candidate_type: IceCandidateType::Relay,
            address: relay_addr,
            priority: Self::relay_priority(),
        };
        self.next_id += 1;
        self.local_candidates.push(candidate.clone());
        candidate
    }

    /// Set the remote candidates received from the peer via signaling.
    pub fn set_remote_candidates(&mut self, candidates: Vec<IceCandidate>) {
        self.remote_candidates = candidates;
    }

    /// Get all local candidates.
    pub fn local_candidates(&self) -> &[IceCandidate] {
        &self.local_candidates
    }

    /// Get the selected candidate pair, if one has been nominated.
    pub fn selected_pair(&self) -> Option<&(IceCandidate, IceCandidate)> {
        self.selected_pair.as_ref()
    }

    /// Generate candidate pair list sorted by priority (highest first).
    /// Pair priority uses the formula from ICE: min(G, D) * 2^32 + max(G, D)
    /// where G and D are the priorities, with the controlling agent's priority first.
    pub fn sorted_candidate_pairs(&self) -> Vec<(&IceCandidate, &IceCandidate)> {
        let mut pairs: Vec<(&IceCandidate, &IceCandidate)> = Vec::new();

        for local in &self.local_candidates {
            for remote in &self.remote_candidates {
                pairs.push((local, remote));
            }
        }

        // Sort by combined priority (descending)
        pairs.sort_by(|a, b| {
            let prio_a = Self::pair_priority(a.0.priority, a.1.priority);
            let prio_b = Self::pair_priority(b.0.priority, b.1.priority);
            prio_b.cmp(&prio_a)
        });

        pairs
    }

    /// Perform connectivity checks on candidate pairs.
    /// Returns the first successful pair, or None if all fail.
    pub async fn perform_connectivity_checks(
        &mut self,
        timeout: Duration,
    ) -> Option<(IceCandidate, IceCandidate)> {
        let pairs = self.sorted_candidate_pairs();
        info!("Starting connectivity checks on {} pair(s)", pairs.len());

        for (local, remote) in &pairs {
            debug!(
                "Checking {} {} -> {} {}",
                local.candidate_type, local.address, remote.candidate_type, remote.address
            );

            // Attempt UDP connectivity check
            let check_result =
                Self::check_connectivity(local.address, remote.address, timeout).await;

            if check_result {
                info!(
                    "Connectivity check succeeded: {} {} -> {} {} (RTT: {:?})",
                    local.candidate_type,
                    local.address,
                    remote.candidate_type,
                    remote.address,
                    "measured"
                );

                let pair = ((*local).clone(), (*remote).clone());
                self.selected_pair = Some(pair.clone());
                return Some(pair);
            }
        }

        warn!("All connectivity checks failed");
        None
    }

    /// Check UDP connectivity from local to remote address.
    async fn check_connectivity(
        local_addr: SocketAddr,
        remote_addr: SocketAddr,
        timeout: Duration,
    ) -> bool {
        let bind_addr = SocketAddr::new(local_addr.ip(), 0);
        let socket = match tokio::net::UdpSocket::bind(bind_addr).await {
            Ok(s) => s,
            Err(e) => {
                debug!("Failed to bind for connectivity check: {}", e);
                return false;
            }
        };

        // Send a connectivity check message
        let check_msg = b"ALDESK_ICE_CHECK";
        if let Err(e) = socket.send_to(check_msg, remote_addr).await {
            debug!("Failed to send connectivity check: {}", e);
            return false;
        }

        // Wait for response or timeout
        let mut buf = [0u8; 64];
        match tokio::time::timeout(timeout, socket.recv_from(&mut buf)).await {
            Ok(Ok((len, _))) => len > 0,
            _ => false,
        }
    }

    /// Calculate host candidate priority.
    fn host_priority() -> u32 {
        126 // Highest priority type
    }

    /// Calculate server-reflexive candidate priority.
    fn srflx_priority() -> u32 {
        100
    }

    /// Calculate relay candidate priority.
    fn relay_priority() -> u32 {
        0 // Lowest priority
    }

    /// Calculate pair priority using ICE formula.
    fn pair_priority(controlling_prio: u32, controlled_prio: u32) -> u64 {
        let (g, d) = if controlling_prio >= controlled_prio {
            (controlling_prio, controlled_prio)
        } else {
            (controlled_prio, controlling_prio)
        };
        (std::cmp::min(g, d) as u64) << 32 | (std::cmp::max(g, d) as u64)
    }

    /// Clear all candidates and state.
    pub fn reset(&mut self) {
        self.local_candidates.clear();
        self.remote_candidates.clear();
        self.selected_pair = None;
        self.next_id = 0;
    }
}

impl Default for IceAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ice_candidate_type_display() {
        assert_eq!(IceCandidateType::Host.to_string(), "host");
        assert_eq!(IceCandidateType::ServerReflexive.to_string(), "srflx");
        assert_eq!(IceCandidateType::PeerReflexive.to_string(), "prflx");
        assert_eq!(IceCandidateType::Relay.to_string(), "relay");
    }

    #[test]
    fn test_ice_candidate_serde_roundtrip() {
        let candidate = IceCandidate {
            id: "h0".to_string(),
            candidate_type: IceCandidateType::Host,
            address: "192.168.1.10:21116".parse().unwrap(),
            priority: 126,
        };
        let json = serde_json::to_string(&candidate).unwrap();
        let decoded: IceCandidate = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, "h0");
        assert_eq!(decoded.candidate_type, IceCandidateType::Host);
        assert_eq!(decoded.priority, 126);
    }

    #[test]
    fn test_ice_agent_new() {
        let agent = IceAgent::new();
        assert!(agent.local_candidates.is_empty());
        assert!(agent.remote_candidates.is_empty());
        assert!(agent.selected_pair.is_none());
    }

    #[test]
    fn test_ice_agent_gather_host_candidates() {
        let mut agent = IceAgent::new();
        let candidates = agent.gather_host_candidates(21116).unwrap();
        assert!(!candidates.is_empty());
        assert!(candidates
            .iter()
            .all(|c| c.candidate_type == IceCandidateType::Host));
        // Should have at least one candidate
        assert!(!agent.local_candidates.is_empty());
    }

    #[test]
    fn test_ice_agent_add_srflx() {
        let mut agent = IceAgent::new();
        let addr: SocketAddr = "203.0.113.5:12345".parse().unwrap();
        let candidate = agent.add_server_reflexive(addr);
        assert_eq!(candidate.candidate_type, IceCandidateType::ServerReflexive);
        assert_eq!(candidate.address, addr);
        assert_eq!(agent.local_candidates.len(), 1);
    }

    #[test]
    fn test_ice_agent_add_srflx_dedup() {
        let mut agent = IceAgent::new();
        let addr: SocketAddr = "203.0.113.5:12345".parse().unwrap();
        agent.add_server_reflexive(addr);
        agent.add_server_reflexive(addr);
        assert_eq!(agent.local_candidates.len(), 1);
    }

    #[test]
    fn test_ice_agent_add_relay() {
        let mut agent = IceAgent::new();
        let addr: SocketAddr = "10.0.0.1:21119".parse().unwrap();
        let candidate = agent.add_relay_candidate(addr);
        assert_eq!(candidate.candidate_type, IceCandidateType::Relay);
        assert_eq!(agent.local_candidates.len(), 1);
    }

    #[test]
    fn test_ice_agent_sorted_pairs() {
        let mut agent = IceAgent::new();
        agent.add_server_reflexive("1.2.3.4:21116".parse().unwrap());
        agent.add_relay_candidate("10.0.0.1:21119".parse().unwrap());

        agent.set_remote_candidates(vec![IceCandidate {
            id: "rh0".into(),
            candidate_type: IceCandidateType::Host,
            address: "5.6.7.8:21116".parse().unwrap(),
            priority: 126,
        }]);

        let pairs = agent.sorted_candidate_pairs();
        assert_eq!(pairs.len(), 2);
        // srflx (100) should be before relay (0)
        assert_eq!(pairs[0].0.candidate_type, IceCandidateType::ServerReflexive);
        assert_eq!(pairs[1].0.candidate_type, IceCandidateType::Relay);
    }

    #[test]
    fn test_ice_agent_reset() {
        let mut agent = IceAgent::new();
        agent.add_server_reflexive("1.2.3.4:21116".parse().unwrap());
        agent.set_remote_candidates(vec![IceCandidate {
            id: "rh0".into(),
            candidate_type: IceCandidateType::Host,
            address: "5.6.7.8:21116".parse().unwrap(),
            priority: 126,
        }]);
        agent.reset();
        assert!(agent.local_candidates.is_empty());
        assert!(agent.remote_candidates.is_empty());
        assert!(agent.selected_pair.is_none());
    }

    #[test]
    fn test_pair_priority() {
        let p1 = IceAgent::pair_priority(126, 100);
        let p2 = IceAgent::pair_priority(100, 126);
        // Should be the same regardless of order
        assert_eq!(p1, p2);
        // Higher priority pairs should have higher values
        let p3 = IceAgent::pair_priority(126, 126);
        assert!(p3 > p1);
    }

    #[test]
    fn test_ice_candidate_type_ordering() {
        // Host > ServerReflexive > Relay
        assert!(IceAgent::host_priority() > IceAgent::srflx_priority());
        assert!(IceAgent::srflx_priority() > IceAgent::relay_priority());
    }
}
