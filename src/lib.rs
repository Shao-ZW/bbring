mod bbring;

pub use bbring::*;

#[cfg(test)]
mod tests {
    use crate::RingBuffer;

    #[test]
    fn simple_push() {
        let bbring = RingBuffer::<String, 32, 32>::new();
        let r1 = bbring.push(String::from("hello"));
        assert!(r1.is_ok());
        let r2 = bbring.push(String::from("world"));
        assert!(r2.is_ok());
    }

    #[test]
    fn simple_pop() {
        let bbring = RingBuffer::<String, 32, 32>::new();
        let r1 = bbring.push(String::from("hello"));
        assert!(r1.is_ok());
        let r2 = bbring.push(String::from("world"));
        assert!(r2.is_ok());
        let r1 = bbring.pop().unwrap();
        assert_eq!(r1, String::from("hello"));
        let r2 = bbring.pop().unwrap();
        assert_eq!(r2, String::from("world"));
    }
}
