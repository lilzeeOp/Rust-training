use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct Balancer {
    upstreams: Vec<SocketAddr>,
    counter: AtomicUsize,
}

impl Balancer {
    pub fn new(upstreams: Vec<SocketAddr>) -> Self {
        Self {
            upstreams,
            counter: AtomicUsize::new(0),
        }
    }

    /// Returns the next upstream address using round-robin selection.
    /// Lock-free: uses AtomicUsize with Relaxed ordering.
    pub fn next(&self) -> SocketAddr {
        let idx = self.counter.fetch_add(1, Ordering::Relaxed) % self.upstreams.len();
        self.upstreams[idx]
    }

    pub fn len(&self) -> usize {
        self.upstreams.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_balancer(n: usize) -> Balancer {
        let upstreams: Vec<SocketAddr> = (0..n)
            .map(|i| format!("127.0.0.1:{}", 9000 + i).parse().unwrap())
            .collect();
        Balancer::new(upstreams)
    }

    #[test]
    fn test_round_robin_cycles_through_all_upstreams() {
        let b = make_balancer(3);
        let ports: Vec<u16> = (0..9).map(|_| b.next().port()).collect();
        assert_eq!(ports, vec![9000, 9001, 9002, 9000, 9001, 9002, 9000, 9001, 9002]);
    }

    #[test]
    fn test_wraps_after_full_cycle() {
        let b = make_balancer(3);
        let first = b.next();
        let _ = b.next();
        let _ = b.next();
        let fourth = b.next();
        assert_eq!(first, fourth);
    }

    #[test]
    fn test_single_upstream_always_returns_same() {
        let b = make_balancer(1);
        let addr = b.next();
        for _ in 0..5 {
            assert_eq!(b.next(), addr);
        }
    }
}
