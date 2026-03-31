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
        Self { upstreams, health, counter: AtomicUsize::new(0) }
    }

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

    fn make(n: usize) -> (Balancer, HealthFlags) {
        let upstreams: Vec<SocketAddr> =
            (0..n).map(|i| format!("127.0.0.1:{}", 9000 + i).parse().unwrap()).collect();
        let flags = new_flags(n);
        (Balancer::new(upstreams, Arc::clone(&flags)), flags)
    }

    #[test]
    fn test_round_robin_cycles_through_healthy_upstreams() {
        let (b, _) = make(3);
        let ports: Vec<u16> = (0..9).map(|_| b.next_healthy().unwrap().port()).collect();
        assert_eq!(ports, vec![9000, 9001, 9002, 9000, 9001, 9002, 9000, 9001, 9002]);
    }

    #[test]
    fn test_skips_unhealthy_upstream() {
        let (b, flags) = make(3);
        flags[1].store(false, Ordering::Relaxed);
        let ports: Vec<u16> = (0..4).map(|_| b.next_healthy().unwrap().port()).collect();
        assert!(!ports.contains(&9001), "unhealthy 9001 must be skipped: {:?}", ports);
    }

    #[test]
    fn test_returns_none_when_all_unhealthy() {
        let (b, flags) = make(3);
        for f in flags.iter() { f.store(false, Ordering::Relaxed); }
        assert!(b.next_healthy().is_none());
    }

    #[test]
    fn test_recovers_when_upstream_marked_healthy_again() {
        let (b, flags) = make(1);
        flags[0].store(false, Ordering::Relaxed);
        assert!(b.next_healthy().is_none());
        flags[0].store(true, Ordering::Relaxed);
        assert!(b.next_healthy().is_some());
    }
}
