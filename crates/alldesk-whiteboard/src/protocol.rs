use alldesk_core::Result;
use serde::{Deserialize, Serialize};
use crate::stroke::Stroke;

/// Events that can occur on the whiteboard. Serializable for network sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WhiteboardEvent {
    /// A new stroke has started.
    StrokeStart { stroke: Stroke },
    /// A point has been added to an in-progress stroke.
    StrokePoint { stroke_id: String, point: (f64, f64) },
    /// A stroke has been completed.
    StrokeEnd { stroke_id: String },
    /// Clear all strokes from the whiteboard.
    Clear,
    /// Undo the last completed stroke.
    Undo,
}

/// A serializable snapshot of the full whiteboard state.
/// Used for initial sync when a new peer joins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhiteboardSnapshot {
    /// All completed strokes.
    pub completed_strokes: Vec<Stroke>,
    /// Version counter for conflict detection.
    pub version: u64,
    /// Lamport clock value at snapshot time.
    pub lamport_clock: u64,
    /// Set of deleted stroke IDs (for CRDT tombstones).
    pub deleted_stroke_ids: Vec<String>,
}

/// A timestamped operation for causal ordering in conflict resolution.
/// Uses Lamport timestamps to establish happens-before relationships.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimestampedEvent {
    /// The whiteboard event.
    pub event: WhiteboardEvent,
    /// Lamport timestamp for causal ordering.
    pub lamport_ts: u64,
    /// Origin node ID to distinguish events from different peers.
    pub origin: String,
}

/// Manages the whiteboard state: tracks all strokes and applies events.
/// Supports CRDT-style conflict resolution for concurrent edits.
pub struct WhiteboardSync {
    /// All completed strokes, in the order they were finished.
    completed_strokes: Vec<Stroke>,
    /// Currently in-progress strokes (keyed by stroke id).
    active_strokes: Vec<Stroke>,
    /// Monotonically increasing version counter for sync.
    version: u64,
    /// Lamport clock for causal ordering of events across peers.
    lamport_clock: u64,
    /// Unique node ID for this whiteboard instance.
    node_id: String,
    /// Set of stroke IDs that have been deleted (tombstones for CRDT).
    deleted_stroke_ids: std::collections::HashSet<String>,
    /// Buffered remote events awaiting application after merge.
    #[allow(dead_code)]
    pending_remote_events: Vec<TimestampedEvent>,
}

impl WhiteboardSync {
    /// Create a new empty whiteboard with a random node ID.
    pub fn new() -> Self {
        Self {
            completed_strokes: Vec::new(),
            active_strokes: Vec::new(),
            version: 0,
            lamport_clock: 0,
            node_id: uuid::Uuid::new_v4().to_string(),
            deleted_stroke_ids: std::collections::HashSet::new(),
            pending_remote_events: Vec::new(),
        }
    }

    /// Create a whiteboard with a specific node ID (for testing).
    pub fn with_node_id(node_id: &str) -> Self {
        Self {
            node_id: node_id.to_string(),
            ..Self::new()
        }
    }

    /// Get this node's ID.
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    /// Get the current Lamport clock value.
    pub fn lamport_clock(&self) -> u64 {
        self.lamport_clock
    }

    /// Increment the Lamport clock (call before sending an event).
    pub fn tick(&mut self) -> u64 {
        self.lamport_clock += 1;
        self.lamport_clock
    }

    /// Merge a received Lamport timestamp into our clock.
    /// Sets our clock to max(our_clock, received_ts) + 1.
    pub fn merge_clock(&mut self, received_ts: u64) {
        self.lamport_clock = self.lamport_clock.max(received_ts) + 1;
    }

    /// Apply a whiteboard event, mutating the state accordingly.
    /// Increments the version counter and Lamport clock on each mutation.
    pub fn apply_event(&mut self, event: WhiteboardEvent) -> Result<()> {
        match &event {
            WhiteboardEvent::StrokeStart { stroke } => {
                // Skip if this stroke was already deleted (tombstone).
                if self.deleted_stroke_ids.contains(&stroke.id) {
                    return Ok(());
                }
            }
            _ => {}
        }

        match event {
            WhiteboardEvent::StrokeStart { stroke } => {
                self.active_strokes.push(stroke);
                self.version += 1;
                self.lamport_clock += 1;
                Ok(())
            }
            WhiteboardEvent::StrokePoint { stroke_id, point } => {
                if let Some(stroke) = self.active_strokes.iter_mut().find(|s| s.id == stroke_id) {
                    stroke.add_point(crate::stroke::Point { x: point.0, y: point.1 });
                }
                self.version += 1;
                self.lamport_clock += 1;
                Ok(())
            }
            WhiteboardEvent::StrokeEnd { stroke_id } => {
                if let Some(pos) = self.active_strokes.iter().position(|s| s.id == stroke_id) {
                    let stroke = self.active_strokes.remove(pos);
                    // Only add if not tombstoned.
                    if !self.deleted_stroke_ids.contains(&stroke.id) {
                        self.completed_strokes.push(stroke);
                    }
                }
                self.version += 1;
                self.lamport_clock += 1;
                Ok(())
            }
            WhiteboardEvent::Clear => {
                // Clear is idempotent: remove all strokes and record tombstones.
                for stroke in &self.completed_strokes {
                    self.deleted_stroke_ids.insert(stroke.id.clone());
                }
                for stroke in &self.active_strokes {
                    self.deleted_stroke_ids.insert(stroke.id.clone());
                }
                self.completed_strokes.clear();
                self.active_strokes.clear();
                self.version += 1;
                self.lamport_clock += 1;
                Ok(())
            }
            WhiteboardEvent::Undo => {
                if let Some(stroke) = self.completed_strokes.pop() {
                    self.deleted_stroke_ids.insert(stroke.id);
                }
                self.version += 1;
                self.lamport_clock += 1;
                Ok(())
            }
        }
    }

    /// Apply a timestamped remote event. Merges the Lamport clock first.
    pub fn apply_timestamped_event(&mut self, ts_event: TimestampedEvent) -> Result<()> {
        self.merge_clock(ts_event.lamport_ts);
        self.apply_event(ts_event.event)
    }

    /// Apply a remote event (deserialized from network). Same as apply_event
    /// but named explicitly for clarity in sync code paths.
    pub fn apply_remote_event(&mut self, event: WhiteboardEvent) -> Result<()> {
        self.apply_event(event)
    }

    /// Merge state from a remote snapshot. Uses CRDT "add-wins" semantics:
    /// - Strokes present in either local or remote state are kept (union).
    /// - Deleted strokes (tombstones) take precedence over additions.
    /// - The resulting version is max(local, remote) + 1.
    pub fn merge(&mut self, remote: WhiteboardSnapshot) {
        // Merge Lamport clocks.
        self.lamport_clock = self.lamport_clock.max(remote.lamport_clock) + 1;

        // Collect all stroke IDs from both sides.
        let mut seen = std::collections::HashSet::new();
        let mut merged = Vec::new();

        // Add remote strokes first (they may be older).
        for stroke in remote.completed_strokes {
            if !self.deleted_stroke_ids.contains(&stroke.id) {
                seen.insert(stroke.id.clone());
                merged.push(stroke);
            }
        }

        // Add local strokes, avoiding duplicates.
        for stroke in self.completed_strokes.drain(..) {
            if !seen.contains(&stroke.id) && !self.deleted_stroke_ids.contains(&stroke.id) {
                seen.insert(stroke.id.clone());
                merged.push(stroke);
            }
        }

        // Merge remote tombstones into local tombstones.
        for id in remote.deleted_stroke_ids {
            self.deleted_stroke_ids.insert(id);
        }

        // Remove any tombstoned strokes from the merged result.
        merged.retain(|s| !self.deleted_stroke_ids.contains(&s.id));

        self.completed_strokes = merged;
        self.version = self.version.max(remote.version) + 1;
    }

    /// Returns all completed strokes.
    pub fn completed_strokes(&self) -> &[Stroke] {
        &self.completed_strokes
    }

    /// Returns all currently active (in-progress) strokes.
    pub fn active_strokes(&self) -> &[Stroke] {
        &self.active_strokes
    }

    /// Returns all strokes (both active and completed) for rendering.
    pub fn all_strokes(&self) -> Vec<&Stroke> {
        self.completed_strokes
            .iter()
            .chain(self.active_strokes.iter())
            .collect()
    }

    /// Get the current version counter.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Check if a stroke has been deleted (tombstoned).
    pub fn is_deleted(&self, stroke_id: &str) -> bool {
        self.deleted_stroke_ids.contains(stroke_id)
    }

    /// Create a serializable snapshot of the current state (completed strokes only).
    /// Includes tombstones for proper CRDT merge.
    pub fn snapshot(&self) -> WhiteboardSnapshot {
        WhiteboardSnapshot {
            completed_strokes: self.completed_strokes.clone(),
            version: self.version,
            lamport_clock: self.lamport_clock,
            deleted_stroke_ids: self.deleted_stroke_ids.iter().cloned().collect(),
        }
    }

    /// Restore state from a snapshot. Clears all current state.
    pub fn restore(&mut self, snapshot: WhiteboardSnapshot) {
        self.completed_strokes = snapshot.completed_strokes;
        self.active_strokes.clear();
        self.version = snapshot.version;
        self.lamport_clock = snapshot.lamport_clock;
        self.deleted_stroke_ids = snapshot.deleted_stroke_ids.into_iter().collect();
    }

    /// Serialize an event to JSON bytes for network transmission.
    pub fn serialize_event(event: &WhiteboardEvent) -> Result<Vec<u8>> {
        serde_json::to_vec(event)
            .map_err(|e| alldesk_core::Error::Other(format!("serialize whiteboard event: {}", e)))
    }

    /// Deserialize an event from JSON bytes received over the network.
    pub fn deserialize_event(data: &[u8]) -> Result<WhiteboardEvent> {
        serde_json::from_slice(data)
            .map_err(|e| alldesk_core::Error::Other(format!("deserialize whiteboard event: {}", e)))
    }

    /// Serialize a timestamped event to JSON bytes.
    pub fn serialize_timestamped_event(event: &TimestampedEvent) -> Result<Vec<u8>> {
        serde_json::to_vec(event)
            .map_err(|e| alldesk_core::Error::Other(format!("serialize timestamped event: {}", e)))
    }

    /// Deserialize a timestamped event from JSON bytes.
    pub fn deserialize_timestamped_event(data: &[u8]) -> Result<TimestampedEvent> {
        serde_json::from_slice(data)
            .map_err(|e| alldesk_core::Error::Other(format!("deserialize timestamped event: {}", e)))
    }

    /// Serialize a snapshot to JSON bytes.
    pub fn serialize_snapshot(snapshot: &WhiteboardSnapshot) -> Result<Vec<u8>> {
        serde_json::to_vec(snapshot)
            .map_err(|e| alldesk_core::Error::Other(format!("serialize whiteboard snapshot: {}", e)))
    }

    /// Deserialize a snapshot from JSON bytes.
    pub fn deserialize_snapshot(data: &[u8]) -> Result<WhiteboardSnapshot> {
        serde_json::from_slice(data)
            .map_err(|e| alldesk_core::Error::Other(format!("deserialize whiteboard snapshot: {}", e)))
    }
}

impl Default for WhiteboardSync {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stroke::Point;

    #[test]
    fn test_whiteboard_stroke_lifecycle() {
        let mut wb = WhiteboardSync::new();

        // Start a stroke
        let mut stroke = Stroke::new((255, 0, 0), 2.0);
        let stroke_id = stroke.id.clone();
        stroke.add_point(Point { x: 1.0, y: 1.0 });

        wb.apply_event(WhiteboardEvent::StrokeStart { stroke }).unwrap();
        assert_eq!(wb.active_strokes().len(), 1);
        assert!(wb.completed_strokes().is_empty());

        // Add points
        wb.apply_event(WhiteboardEvent::StrokePoint {
            stroke_id: stroke_id.clone(),
            point: (2.0, 2.0),
        }).unwrap();
        assert_eq!(wb.active_strokes()[0].points.len(), 2);

        // End the stroke
        wb.apply_event(WhiteboardEvent::StrokeEnd {
            stroke_id: stroke_id.clone(),
        }).unwrap();
        assert!(wb.active_strokes().is_empty());
        assert_eq!(wb.completed_strokes().len(), 1);
        assert_eq!(wb.completed_strokes()[0].id, stroke_id);
    }

    #[test]
    fn test_whiteboard_clear() {
        let mut wb = WhiteboardSync::new();
        let s1 = Stroke::new((0, 0, 0), 1.0);
        let s2 = Stroke::new((255, 255, 255), 2.0);

        wb.apply_event(WhiteboardEvent::StrokeStart { stroke: s1 }).unwrap();
        wb.apply_event(WhiteboardEvent::StrokeStart { stroke: s2 }).unwrap();
        assert_eq!(wb.active_strokes().len(), 2);

        wb.apply_event(WhiteboardEvent::Clear).unwrap();
        assert!(wb.active_strokes().is_empty());
        assert!(wb.completed_strokes().is_empty());
    }

    #[test]
    fn test_whiteboard_undo() {
        let mut wb = WhiteboardSync::new();

        // Add and complete two strokes
        let s1 = Stroke::new((255, 0, 0), 1.0);
        let _id1 = s1.id.clone();
        wb.apply_event(WhiteboardEvent::StrokeStart { stroke: s1 }).unwrap();
        wb.apply_event(WhiteboardEvent::StrokeEnd { stroke_id: _id1 }).unwrap();

        let s2 = Stroke::new((0, 255, 0), 2.0);
        let id2 = s2.id.clone();
        wb.apply_event(WhiteboardEvent::StrokeStart { stroke: s2 }).unwrap();
        wb.apply_event(WhiteboardEvent::StrokeEnd { stroke_id: id2 }).unwrap();

        assert_eq!(wb.completed_strokes().len(), 2);

        // Undo removes the last completed stroke
        wb.apply_event(WhiteboardEvent::Undo).unwrap();
        assert_eq!(wb.completed_strokes().len(), 1);
        assert_eq!(wb.completed_strokes()[0].color, (255, 0, 0));

        // Undo again
        wb.apply_event(WhiteboardEvent::Undo).unwrap();
        assert!(wb.completed_strokes().is_empty());

        // Undo on empty is a no-op
        wb.apply_event(WhiteboardEvent::Undo).unwrap();
        assert!(wb.completed_strokes().is_empty());
    }

    #[test]
    fn test_whiteboard_all_strokes() {
        let mut wb = WhiteboardSync::new();

        let mut s1 = Stroke::new((255, 0, 0), 1.0);
        let _id1 = s1.id.clone();
        s1.add_point(Point { x: 1.0, y: 1.0 });
        wb.apply_event(WhiteboardEvent::StrokeStart { stroke: s1 }).unwrap();

        let s2 = Stroke::new((0, 255, 0), 2.0);
        let id2 = s2.id.clone();
        wb.apply_event(WhiteboardEvent::StrokeStart { stroke: s2 }).unwrap();
        wb.apply_event(WhiteboardEvent::StrokeEnd { stroke_id: id2 }).unwrap();

        let all = wb.all_strokes();
        assert_eq!(all.len(), 2); // 1 active + 1 completed
    }

    #[test]
    fn test_whiteboard_point_on_unknown_stroke() {
        let mut wb = WhiteboardSync::new();
        // Point event for non-existent stroke should be a no-op
        wb.apply_event(WhiteboardEvent::StrokePoint {
            stroke_id: "nonexistent".to_string(),
            point: (1.0, 1.0),
        }).unwrap();
        assert!(wb.active_strokes().is_empty());
    }

    #[test]
    fn test_whiteboard_end_unknown_stroke() {
        let mut wb = WhiteboardSync::new();
        wb.apply_event(WhiteboardEvent::StrokeEnd {
            stroke_id: "nonexistent".to_string(),
        }).unwrap();
        assert!(wb.completed_strokes().is_empty());
    }

    #[test]
    fn test_whiteboard_default() {
        let wb = WhiteboardSync::default();
        assert!(wb.active_strokes().is_empty());
        assert!(wb.completed_strokes().is_empty());
        assert_eq!(wb.version(), 0);
    }

    #[test]
    fn test_whiteboard_version_increments() {
        let mut wb = WhiteboardSync::new();
        assert_eq!(wb.version(), 0);

        wb.apply_event(WhiteboardEvent::Clear).unwrap();
        assert_eq!(wb.version(), 1);

        let s = Stroke::new((0, 0, 0), 1.0);
        let id = s.id.clone();
        wb.apply_event(WhiteboardEvent::StrokeStart { stroke: s }).unwrap();
        assert_eq!(wb.version(), 2);

        wb.apply_event(WhiteboardEvent::StrokeEnd { stroke_id: id }).unwrap();
        assert_eq!(wb.version(), 3);

        wb.apply_event(WhiteboardEvent::Undo).unwrap();
        assert_eq!(wb.version(), 4);
    }

    #[test]
    fn test_whiteboard_snapshot_restore() {
        let mut wb = WhiteboardSync::new();
        let mut s1 = Stroke::new((255, 0, 0), 2.0);
        let id1 = s1.id.clone();
        s1.add_point(Point { x: 1.0, y: 2.0 });
        wb.apply_event(WhiteboardEvent::StrokeStart { stroke: s1 }).unwrap();
        wb.apply_event(WhiteboardEvent::StrokeEnd { stroke_id: id1 }).unwrap();

        let snapshot = wb.snapshot();
        assert_eq!(snapshot.completed_strokes.len(), 1);
        assert!(snapshot.version > 0);

        // Restore into a fresh whiteboard
        let mut wb2 = WhiteboardSync::new();
        wb2.restore(snapshot);
        assert_eq!(wb2.completed_strokes().len(), 1);
        assert_eq!(wb2.version(), wb.version());
    }

    #[test]
    fn test_whiteboard_serialize_deserialize_event() {
        let mut s = Stroke::new((128, 64, 32), 3.0);
        s.add_point(Point { x: 10.0, y: 20.0 });
        let id = s.id.clone();

        let events = vec![
            WhiteboardEvent::StrokeStart { stroke: s },
            WhiteboardEvent::StrokePoint { stroke_id: id.clone(), point: (30.0, 40.0) },
            WhiteboardEvent::StrokeEnd { stroke_id: id },
            WhiteboardEvent::Undo,
            WhiteboardEvent::Clear,
        ];

        for event in &events {
            let bytes = WhiteboardSync::serialize_event(event).unwrap();
            let restored = WhiteboardSync::deserialize_event(&bytes).unwrap();
            let orig_json = serde_json::to_string(event).unwrap();
            let restored_json = serde_json::to_string(&restored).unwrap();
            assert_eq!(orig_json, restored_json);
        }
    }

    #[test]
    fn test_whiteboard_serialize_deserialize_snapshot() {
        let mut wb = WhiteboardSync::new();
        let s = Stroke::new((0, 255, 0), 1.5);
        let id = s.id.clone();
        wb.apply_event(WhiteboardEvent::StrokeStart { stroke: s }).unwrap();
        wb.apply_event(WhiteboardEvent::StrokeEnd { stroke_id: id }).unwrap();

        let snapshot = wb.snapshot();
        let bytes = WhiteboardSync::serialize_snapshot(&snapshot).unwrap();
        let restored = WhiteboardSync::deserialize_snapshot(&bytes).unwrap();

        assert_eq!(restored.version, snapshot.version);
        assert_eq!(restored.completed_strokes.len(), 1);
        assert_eq!(restored.completed_strokes[0].color, (0, 255, 0));
    }

    #[test]
    fn test_whiteboard_apply_remote_event() {
        let mut wb = WhiteboardSync::new();
        let s = Stroke::new((100, 100, 100), 5.0);
        let id = s.id.clone();

        let event = WhiteboardEvent::StrokeStart { stroke: s };
        let bytes = WhiteboardSync::serialize_event(&event).unwrap();
        let remote_event = WhiteboardSync::deserialize_event(&bytes).unwrap();

        wb.apply_remote_event(remote_event).unwrap();
        assert_eq!(wb.active_strokes().len(), 1);

        let end_event = WhiteboardEvent::StrokeEnd { stroke_id: id };
        wb.apply_remote_event(end_event).unwrap();
        assert_eq!(wb.completed_strokes().len(), 1);
    }

    // === Conflict Resolution Tests ===

    #[test]
    fn test_lamport_clock_basic() {
        let mut wb = WhiteboardSync::new();
        assert_eq!(wb.lamport_clock(), 0);
        wb.apply_event(WhiteboardEvent::Clear).unwrap();
        assert_eq!(wb.lamport_clock(), 1);
    }

    #[test]
    fn test_lamport_clock_merge() {
        let mut wb = WhiteboardSync::new();
        wb.merge_clock(10);
        assert_eq!(wb.lamport_clock(), 11);
        // Merging a lower value should still increment.
        wb.merge_clock(5);
        assert_eq!(wb.lamport_clock(), 12);
    }

    #[test]
    fn test_merge_concurrent_strokes() {
        // Peer A has stroke 1.
        let mut wb_a = WhiteboardSync::with_node_id("peer-a");
        let mut s1 = Stroke::new((255, 0, 0), 2.0);
        let id1 = s1.id.clone();
        s1.add_point(Point { x: 1.0, y: 1.0 });
        wb_a.apply_event(WhiteboardEvent::StrokeStart { stroke: s1 }).unwrap();
        wb_a.apply_event(WhiteboardEvent::StrokeEnd { stroke_id: id1 }).unwrap();

        // Peer B has stroke 2.
        let mut wb_b = WhiteboardSync::with_node_id("peer-b");
        let mut s2 = Stroke::new((0, 255, 0), 3.0);
        let id2 = s2.id.clone();
        s2.add_point(Point { x: 2.0, y: 2.0 });
        wb_b.apply_event(WhiteboardEvent::StrokeStart { stroke: s2 }).unwrap();
        wb_b.apply_event(WhiteboardEvent::StrokeEnd { stroke_id: id2 }).unwrap();

        // A merges B's state — should have both strokes.
        let snapshot_b = wb_b.snapshot();
        wb_a.merge(snapshot_b);
        assert_eq!(wb_a.completed_strokes().len(), 2);

        // B merges A's state — should also have both.
        let snapshot_a = wb_a.snapshot();
        wb_b.merge(snapshot_a);
        assert_eq!(wb_b.completed_strokes().len(), 2);
    }

    #[test]
    fn test_merge_with_tombstone() {
        // Peer A has stroke 1 + deletes it (undo).
        let mut wb_a = WhiteboardSync::with_node_id("peer-a");
        let mut s1 = Stroke::new((255, 0, 0), 2.0);
        let id1 = s1.id.clone();
        s1.add_point(Point { x: 1.0, y: 1.0 });
        wb_a.apply_event(WhiteboardEvent::StrokeStart { stroke: s1 }).unwrap();
        wb_a.apply_event(WhiteboardEvent::StrokeEnd { stroke_id: id1.clone() }).unwrap();
        wb_a.apply_event(WhiteboardEvent::Undo).unwrap();
        assert!(wb_a.completed_strokes().is_empty());
        assert!(wb_a.is_deleted(&id1));

        // Peer B has stroke 2.
        let mut wb_b = WhiteboardSync::with_node_id("peer-b");
        let mut s2 = Stroke::new((0, 255, 0), 3.0);
        let id2 = s2.id.clone();
        s2.add_point(Point { x: 2.0, y: 2.0 });
        wb_b.apply_event(WhiteboardEvent::StrokeStart { stroke: s2 }).unwrap();
        wb_b.apply_event(WhiteboardEvent::StrokeEnd { stroke_id: id2 }).unwrap();

        // B merges A — stroke 1 should stay deleted (tombstone wins).
        let snapshot_a = wb_a.snapshot();
        wb_b.merge(snapshot_a);
        assert_eq!(wb_b.completed_strokes().len(), 1);
        assert!(wb_b.is_deleted(&id1));
        assert_eq!(wb_b.completed_strokes()[0].color, (0, 255, 0));
    }

    #[test]
    fn test_merge_clear_then_stroke() {
        // Peer A clears everything, then adds a new stroke.
        let mut wb_a = WhiteboardSync::with_node_id("peer-a");
        let mut s1 = Stroke::new((255, 0, 0), 2.0);
        let id1 = s1.id.clone();
        s1.add_point(Point { x: 1.0, y: 1.0 });
        wb_a.apply_event(WhiteboardEvent::StrokeStart { stroke: s1 }).unwrap();
        wb_a.apply_event(WhiteboardEvent::StrokeEnd { stroke_id: id1.clone() }).unwrap();
        wb_a.apply_event(WhiteboardEvent::Clear).unwrap();

        // Now A has a new stroke.
        let mut s3 = Stroke::new((0, 0, 255), 1.0);
        let id3 = s3.id.clone();
        s3.add_point(Point { x: 5.0, y: 5.0 });
        wb_a.apply_event(WhiteboardEvent::StrokeStart { stroke: s3 }).unwrap();
        wb_a.apply_event(WhiteboardEvent::StrokeEnd { stroke_id: id3.clone() }).unwrap();

        // Peer B still has the old stroke 1 (pre-clear).
        let mut wb_b = WhiteboardSync::with_node_id("peer-b");
        let mut s1_b = Stroke::new((255, 0, 0), 2.0);
        // Same ID to simulate having received it before clear.
        s1_b.id = id1.clone();
        s1_b.add_point(Point { x: 1.0, y: 1.0 });
        wb_b.apply_event(WhiteboardEvent::StrokeStart { stroke: s1_b }).unwrap();
        wb_b.apply_event(WhiteboardEvent::StrokeEnd { stroke_id: id1.clone() }).unwrap();

        // B merges A — stroke 1 should be deleted (A's clear tombstone).
        let snapshot_a = wb_a.snapshot();
        wb_b.merge(snapshot_a);
        assert_eq!(wb_b.completed_strokes().len(), 1);
        assert_eq!(wb_b.completed_strokes()[0].id, id3);
    }

    #[test]
    fn test_timestamped_event() {
        let mut wb = WhiteboardSync::new();
        let s = Stroke::new((128, 128, 128), 4.0);
        let id = s.id.clone();

        let ts_event = TimestampedEvent {
            event: WhiteboardEvent::StrokeStart { stroke: s },
            lamport_ts: 5,
            origin: "remote-peer".to_string(),
        };

        wb.apply_timestamped_event(ts_event).unwrap();
        // Lamport clock should be max(0, 5) + 1 = 6 from merge, then +1 from apply = 7.
        assert_eq!(wb.lamport_clock(), 7);
        assert_eq!(wb.active_strokes().len(), 1);
    }

    #[test]
    fn test_timestamped_event_serialize_roundtrip() {
        let s = Stroke::new((255, 0, 0), 2.0);
        let ts_event = TimestampedEvent {
            event: WhiteboardEvent::StrokeStart { stroke: s },
            lamport_ts: 42,
            origin: "node-1".to_string(),
        };

        let bytes = WhiteboardSync::serialize_timestamped_event(&ts_event).unwrap();
        let restored = WhiteboardSync::deserialize_timestamped_event(&bytes).unwrap();
        assert_eq!(restored.lamport_ts, 42);
        assert_eq!(restored.origin, "node-1");
    }

    #[test]
    fn test_snapshot_includes_tombstones() {
        let mut wb = WhiteboardSync::new();
        let s = Stroke::new((255, 0, 0), 2.0);
        let id = s.id.clone();
        wb.apply_event(WhiteboardEvent::StrokeStart { stroke: s }).unwrap();
        wb.apply_event(WhiteboardEvent::StrokeEnd { stroke_id: id.clone() }).unwrap();
        wb.apply_event(WhiteboardEvent::Undo).unwrap();

        let snapshot = wb.snapshot();
        assert!(snapshot.deleted_stroke_ids.contains(&id));
        assert!(snapshot.completed_strokes.is_empty());
    }

    #[test]
    fn test_node_id_unique() {
        let wb1 = WhiteboardSync::new();
        let wb2 = WhiteboardSync::new();
        assert_ne!(wb1.node_id(), wb2.node_id());
    }

    #[test]
    fn test_merge_idempotent() {
        let mut wb_a = WhiteboardSync::with_node_id("a");
        let mut s = Stroke::new((255, 0, 0), 2.0);
        let id = s.id.clone();
        s.add_point(Point { x: 1.0, y: 1.0 });
        wb_a.apply_event(WhiteboardEvent::StrokeStart { stroke: s }).unwrap();
        wb_a.apply_event(WhiteboardEvent::StrokeEnd { stroke_id: id }).unwrap();

        let mut wb_b = WhiteboardSync::with_node_id("b");

        // Merge twice with the same snapshot.
        let snap = wb_a.snapshot();
        wb_b.merge(snap.clone());
        assert_eq!(wb_b.completed_strokes().len(), 1);

        wb_b.merge(snap);
        // Should still be 1 (idempotent).
        assert_eq!(wb_b.completed_strokes().len(), 1);
    }
}
