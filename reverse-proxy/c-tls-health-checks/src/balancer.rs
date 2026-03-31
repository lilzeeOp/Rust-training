use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::health::HealthFlags;

pub struct Balancer {
    upstreams: Vec<SocketAddr>,
    health: HealthFlags,
    counter: AtomicUsize,
}

impl Balancer {
    pub fn new(upstreams: Vec<SocketAddr>, health: HealthFlags) -> Self {
        Self {
            upstreams,
            health,
            counter: AtomicUsize::new(0),
        }
    }

    /// Returns the next healthy upstream using round-robin selection.
    /// Returns None if all upstreams are unhealthy.
    pub fn next_healthy(&self) -> Option<SocketAddr> {
        let start = self.counter.fetch_add(1, Ordering::Relaxed);
        let n = self.upstreams.len();
        for i in 0..n {
            let idx = (start + i) % n;
            if self.health[idx].load(Ordering::Relaxed) {
                return Some(self.upstreams[idx]);
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        self.upstreams.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::new_flags;
    use std::sync::{atomic::Ordering, Arc};

    fn make_balancer(n: usize) -> (Balancer, HealthFlags) {
        let upstreams: Vec<SocketAddr> = (0..n)
            .map(|i| format!("127.0.0.1:{}", 9000 + i).parse().unwrap())
            .collect();
        let flags = new_flags(n);
        let b = Balancer::new(upstreams, Arc::clone(&flags));
        (b, flags)
    }

    #[test]
    fn test_round_robin_cycles_through_healthy_upstreams() {
        let (b, _flags) = make_balancer(3);
        let ports: Vec<u16> = (0..9).map(|_| b.next_healthy().unwrap().port()).collect();
        assert_eq!(
            ports,
            vec![9000, 9001, 9002, 9000, 9001, 9002, 9000, 9001, 9002]
        );
    }

    #[test]
    fn test_skips_unhealthy_upstream() {
        let (b, flags) = make_balancer(3);
        flags[1].store(false, Ordering::Relaxed); // mark 9001 unhealthy

        let ports: Vec<u16> = (0..4).map(|_| b.next_healthy().unwrap().port()).collect();
        assert!(
            !ports.contains(&9001),
            "unhealthy upstream 9001 must be skipped, got: {:?}",
            ports
        );
    }

    #[test]
    fn test_returns_none_when_all_unhealthy() {
        let (b, flags) = make_balancer(3);
        for flag in flags.iter() {
            flag.store(false, Ordering::Relaxed);
        }
        assert!(b.next_healthy().is_none());
    }

    #[test]
    fn test_recovers_when_upstream_marked_healthy_again() {
        let (b, flags) = make_balancer(1);
        flags[0].store(false, Ordering::Relaxed);
        assert!(b.next_healthy().is_none(), "should be None when unhealthy");
        flags[0].store(true, Ordering::Relaxed);
        assert!(b.next_healthy().is_some(), "should recover after flag set true");
    }
}
