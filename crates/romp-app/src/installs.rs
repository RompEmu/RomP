//! Games sent from RomM, waiting their turn: one downloads at a time, in the order they came.
use crate::romm::types::InstallRequest;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cancelled {
    Queued,
    Running,
    Unknown,
}

#[derive(Default)]
pub struct Queue {
    waiting: VecDeque<InstallRequest>,
    running: Option<InstallRequest>,
    /// RomM withdrew the running request, so a stopped download needn't be reported.
    romm_cancelled: bool,
}

impl Queue {
    /// Adds requests not already waiting or running: a claim returns every request still held.
    pub fn add(&mut self, requests: Vec<InstallRequest>) {
        for request in requests {
            let known = self.running.as_ref().is_some_and(|r| r.id == request.id)
                || self.waiting.iter().any(|r| r.id == request.id);
            if !known {
                self.waiting.push_back(request);
            }
        }
    }

    /// The next request to run, once nothing is running.
    pub fn next(&mut self) -> Option<InstallRequest> {
        if self.running.is_some() {
            return None;
        }
        self.romm_cancelled = false;
        self.running = self.waiting.pop_front();
        self.running.clone()
    }

    pub fn running(&self) -> Option<&InstallRequest> {
        self.running.as_ref()
    }

    pub fn finish(&mut self) {
        self.running = None;
        self.romm_cancelled = false;
    }

    /// Forgets everything, the running request too: for when nothing is downloading it.
    pub fn abandon(&mut self) {
        self.waiting.clear();
        self.finish();
    }

    pub fn cancelled_by_romm(&self, id: &str) -> bool {
        self.romm_cancelled && self.running.as_ref().is_some_and(|r| r.id == id)
    }

    pub fn cancel(&mut self, id: &str) -> Cancelled {
        if self.running.as_ref().is_some_and(|r| r.id == id) {
            self.romm_cancelled = true;
            return Cancelled::Running;
        }
        let before = self.waiting.len();
        self.waiting.retain(|r| r.id != id);
        if self.waiting.len() < before {
            Cancelled::Queued
        } else {
            Cancelled::Unknown
        }
    }

    /// Drops what's waiting; the running request finishes.
    pub fn clear(&mut self) {
        self.waiting.clear();
    }

    #[cfg(test)]
    pub fn is_idle(&self) -> bool {
        self.running.is_none() && self.waiting.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: &str, rom_id: i64) -> InstallRequest {
        InstallRequest {
            id: id.into(),
            rom_id,
            file_ids: vec![],
            status: "taken".into(),
        }
    }

    #[test]
    fn requests_run_in_order_one_at_a_time() {
        let mut q = Queue::default();
        q.add(vec![request("a", 1), request("b", 2)]);
        assert_eq!(q.next().unwrap().id, "a");
        assert!(q.next().is_none(), "one at a time");
        assert_eq!(q.running().unwrap().id, "a");
        q.finish();
        assert_eq!(q.next().unwrap().id, "b");
        q.finish();
        assert!(q.is_idle());
    }

    #[test]
    fn a_request_claimed_again_is_queued_once() {
        let mut q = Queue::default();
        q.add(vec![request("a", 1)]);
        let _ = q.next();
        q.add(vec![request("a", 1), request("b", 2)]);
        q.add(vec![request("b", 2)]);
        q.finish();
        assert_eq!(q.next().unwrap().id, "b");
        q.finish();
        assert!(q.next().is_none());
    }

    #[test]
    fn cancelling_drops_a_waiting_request_or_says_the_running_one_stops() {
        let mut q = Queue::default();
        q.add(vec![request("a", 1), request("b", 2)]);
        let _ = q.next();
        assert_eq!(q.cancel("b"), Cancelled::Queued);
        assert_eq!(q.cancel("a"), Cancelled::Running);
        assert_eq!(q.cancel("zzz"), Cancelled::Unknown);
    }

    #[test]
    fn a_stop_from_romm_is_told_apart_from_a_pause_here() {
        let mut q = Queue::default();
        q.add(vec![request("a", 1)]);
        let _ = q.next();
        assert!(!q.cancelled_by_romm("a"), "a pause on this device");
        assert_eq!(q.cancel("a"), Cancelled::Running);
        assert!(q.cancelled_by_romm("a"));
        q.finish();
        assert!(!q.cancelled_by_romm("a"), "forgotten once finished");
    }

    #[test]
    fn a_running_request_with_nothing_downloading_can_be_dropped() {
        let mut q = Queue::default();
        q.add(vec![request("a", 1), request("b", 2)]);
        let _ = q.next();
        q.abandon();
        assert!(q.is_idle());
    }

    #[test]
    fn clearing_leaves_the_running_one_to_finish() {
        let mut q = Queue::default();
        q.add(vec![request("a", 1), request("b", 2)]);
        let _ = q.next();
        q.clear();
        assert_eq!(q.running().unwrap().id, "a");
        q.finish();
        assert!(q.is_idle());
    }
}
