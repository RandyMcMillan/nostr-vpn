use std::collections::{HashMap, HashSet};

use crate::control::{PeerAnnouncement, select_peer_endpoint};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PeerPathSource {
    Local,
    Public,
    Legacy,
    Observed,
}

impl PeerPathSource {
    fn merge(self, other: Self) -> Self {
        self.max(other)
    }

    fn rank(self, same_subnet_local: bool) -> u8 {
        if same_subnet_local {
            return 3;
        }

        match self {
            Self::Public | Self::Observed => 2,
            Self::Legacy => 1,
            Self::Local => 0,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct PeerPathState {
    current_endpoint: Option<String>,
    endpoints: HashMap<String, TrackedPeerPath>,
}

#[derive(Debug, Clone)]
struct TrackedPeerPath {
    source: PeerPathSource,
    announced_at: u64,
    last_selected_at: Option<u64>,
    last_success_at: Option<u64>,
}

impl TrackedPeerPath {
    fn new(source: PeerPathSource, announced_at: u64) -> Self {
        Self {
            source,
            announced_at,
            last_selected_at: None,
            last_success_at: None,
        }
    }

    fn freshness_at(&self) -> u64 {
        self.announced_at
            .max(self.last_selected_at.unwrap_or(0))
            .max(self.last_success_at.unwrap_or(0))
    }
}

#[derive(Debug, Clone, Default)]
pub struct PeerPathBook {
    peers: HashMap<String, PeerPathState>,
}

impl PeerPathBook {
    pub fn refresh_from_announcement(
        &mut self,
        participant: impl Into<String>,
        announcement: &PeerAnnouncement,
        seen_at: u64,
    ) -> bool {
        let participant = participant.into();
        let state = self.peers.entry(participant).or_default();
        let mut changed = false;

        for (endpoint, source) in announcement_endpoints(announcement) {
            let entry = state
                .endpoints
                .entry(endpoint)
                .or_insert_with(|| TrackedPeerPath::new(source, seen_at));

            let merged_source = entry.source.merge(source);
            if entry.source != merged_source {
                entry.source = merged_source;
                changed = true;
            }
            if entry.announced_at < seen_at {
                entry.announced_at = seen_at;
                changed = true;
            }
        }

        changed
    }

    pub fn note_selected(
        &mut self,
        participant: impl Into<String>,
        endpoint: &str,
        selected_at: u64,
    ) -> bool {
        let participant = participant.into();
        let state = self.peers.entry(participant).or_default();
        let entry = state
            .endpoints
            .entry(endpoint.to_string())
            .or_insert_with(|| TrackedPeerPath::new(PeerPathSource::Observed, selected_at));

        let mut changed = false;
        if entry.last_selected_at != Some(selected_at) {
            entry.last_selected_at = Some(selected_at);
            changed = true;
        }
        if state.current_endpoint.as_deref() != Some(endpoint) {
            state.current_endpoint = Some(endpoint.to_string());
            changed = true;
        }

        changed
    }

    pub fn note_success(
        &mut self,
        participant: impl Into<String>,
        endpoint: &str,
        success_at: u64,
    ) -> bool {
        let participant = participant.into();
        let state = self.peers.entry(participant).or_default();
        let entry = state
            .endpoints
            .entry(endpoint.to_string())
            .or_insert_with(|| TrackedPeerPath::new(PeerPathSource::Observed, success_at));

        if entry.last_success_at.unwrap_or(0) >= success_at {
            return false;
        }

        entry.last_success_at = Some(success_at);
        true
    }

    pub fn prune_stale(&mut self, now: u64, stale_after_secs: u64) -> bool {
        if stale_after_secs == 0 {
            return false;
        }

        let cutoff = now.saturating_sub(stale_after_secs);
        let mut changed = false;
        self.peers.retain(|_, state| {
            let before = state.endpoints.len();
            state
                .endpoints
                .retain(|_, endpoint| endpoint.freshness_at() > cutoff);
            if state.endpoints.len() != before {
                changed = true;
            }
            if let Some(current) = state.current_endpoint.as_deref()
                && !state.endpoints.contains_key(current)
            {
                state.current_endpoint = None;
                changed = true;
            }
            let keep = !state.endpoints.is_empty();
            if !keep {
                changed = true;
            }
            keep
        });
        changed
    }

    pub fn retain_participants(&mut self, participants: &HashSet<String>) {
        self.peers
            .retain(|participant, _| participants.contains(participant));
    }

    pub fn select_endpoint(
        &self,
        participant: &str,
        announcement: &PeerAnnouncement,
        own_local_endpoint: Option<&str>,
        now: u64,
        retry_after_secs: u64,
    ) -> Option<String> {
        let default_endpoint = select_peer_endpoint(announcement, own_local_endpoint);
        let state = self.peers.get(participant);
        let Some(state) = state else {
            return Some(default_endpoint);
        };
        if state.endpoints.is_empty() {
            return Some(default_endpoint);
        }

        let preferred = state
            .endpoints
            .iter()
            .max_by_key(|(endpoint, tracked)| {
                candidate_rank(endpoint, tracked, own_local_endpoint, &default_endpoint)
            })
            .map(|(endpoint, _)| endpoint.clone())?;

        let Some(current_endpoint) = state
            .current_endpoint
            .as_ref()
            .filter(|endpoint| state.endpoints.contains_key(*endpoint))
        else {
            return Some(preferred);
        };

        let current = state
            .endpoints
            .get(current_endpoint)
            .expect("current endpoint should exist");
        let preferred_state = state
            .endpoints
            .get(&preferred)
            .expect("preferred endpoint should exist");

        if current_endpoint == &preferred {
            return Some(preferred);
        }

        if let Some(current_success_at) = current.last_success_at {
            if preferred_state.last_success_at.unwrap_or(0) > current_success_at {
                return Some(preferred);
            }
            return Some(current_endpoint.clone());
        }

        let can_rotate = current
            .last_selected_at
            .map(|selected_at| now.saturating_sub(selected_at) >= retry_after_secs)
            .unwrap_or(true);

        if can_rotate {
            Some(preferred)
        } else {
            Some(current_endpoint.clone())
        }
    }
}

fn announcement_endpoints(announcement: &PeerAnnouncement) -> Vec<(String, PeerPathSource)> {
    let mut seen = HashSet::new();
    let mut endpoints = Vec::new();

    if let Some(local_endpoint) = announcement.local_endpoint.as_deref()
        && !local_endpoint.trim().is_empty()
        && seen.insert(local_endpoint.to_string())
    {
        endpoints.push((local_endpoint.to_string(), PeerPathSource::Local));
    }

    if let Some(public_endpoint) = announcement.public_endpoint.as_deref()
        && !public_endpoint.trim().is_empty()
        && seen.insert(public_endpoint.to_string())
    {
        endpoints.push((public_endpoint.to_string(), PeerPathSource::Public));
    }

    if !announcement.endpoint.trim().is_empty() && seen.insert(announcement.endpoint.clone()) {
        endpoints.push((announcement.endpoint.clone(), PeerPathSource::Legacy));
    }

    endpoints
}

fn candidate_rank(
    endpoint: &str,
    tracked: &TrackedPeerPath,
    own_local_endpoint: Option<&str>,
    default_endpoint: &str,
) -> (u64, u8, u8, u64) {
    let same_subnet_local = tracked.source == PeerPathSource::Local
        && own_local_endpoint.is_some_and(|own| endpoints_share_private_ipv4_subnet(endpoint, own));
    let default_match = endpoint == default_endpoint;

    (
        tracked.last_success_at.unwrap_or(0),
        tracked.source.rank(same_subnet_local),
        u8::from(default_match),
        tracked.announced_at,
    )
}

fn endpoints_share_private_ipv4_subnet(left: &str, right: &str) -> bool {
    let Ok(left_addr) = left.parse::<std::net::SocketAddr>() else {
        return false;
    };
    let Ok(right_addr) = right.parse::<std::net::SocketAddr>() else {
        return false;
    };

    let (std::net::SocketAddr::V4(left_v4), std::net::SocketAddr::V4(right_v4)) =
        (left_addr, right_addr)
    else {
        return false;
    };

    let left_ip = *left_v4.ip();
    let right_ip = *right_v4.ip();
    (left_ip.is_private() || left_ip.is_link_local())
        && (right_ip.is_private() || right_ip.is_link_local())
        && left_ip.octets()[0..3] == right_ip.octets()[0..3]
}
