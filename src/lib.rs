#![cfg_attr(feature = "bench", feature(test))]

#[cfg(feature="bench")]
extern crate test;

extern crate time;

use std::net::UdpSocket;
use std::io::Result;

mod pcg32;

/// Use a safe maximum size for UDP to prevent fragmentation.
const MAX_UDP_PAYLOAD: usize = 576;

pub const FULL_SAMPLING_RATE: f64 = 1.0;

pub trait SendStats: Sized {
    fn send_stats(&self, str: String);
}

/// Real implementation, send a UDP packet for every stat
impl SendStats for UdpSocket {
    fn send_stats(&self, str: String) {
        match self.send(str.as_bytes()) {
            Ok(_) => {}, // TODO count packets sent for batch reporting
            _ => {}// TODO count send errors for batch reporting
        }
    }
}

/// A client to send application metrics to a statsd server over UDP.
/// Multiple instances may be required if different sampling rates or prefix a required within the same application.
pub struct StatsdOutlet<S: SendStats> {
    sender: S,
    prefix: String,
    int_rate: u32,
    gauge_suffix: String,
    count_suffix: String,
    time_suffix: String
}

pub type StatsdClient = StatsdOutlet<UdpSocket>;

impl StatsdClient {
    /// Create a new `StatsdClient` sending packets to the specified `address`.
    /// Sent metric keys will be prepended with `prefix`.
    /// Subsampling is performed according to `float_rate` where
    /// - 1.0 is full sampling and
    /// - 0.0 means _no_ samples will be taken
    /// See crate method `to_int_rate` for more details and a nice table
    pub fn new(address: &str, prefix_str: &str, float_rate: f64) -> Result<StatsdClient> {
        let udp_socket = UdpSocket::bind("0.0.0.0:0")?; // NB: CLOEXEC by default
        udp_socket.set_nonblocking(true)?;
        udp_socket.connect(address)?;
        StatsdOutlet::outlet(udp_socket, prefix_str, float_rate)
    }
}

/// A point in time from which elapsed time can be determined
pub struct StartTime (u64);

impl StartTime {
    /// The number of milliseconds elapsed between now and this StartTime
    fn elapsed_ms(self) -> u64 {
        (time::precise_time_ns() - self.0) / 1_000_000
    }
}

impl<S: SendStats> StatsdOutlet<S> {

    /// Create a new `StatsdClient` sending packets to the specified `address`.
    /// Sent metric keys will be prepended with `prefix`.
    /// Subsampling is performed according to `float_rate` where
    /// - 1.0 is full sampling and
    /// - 0.0 means _no_ samples will be taken
    /// See crate method `to_int_rate` for more details and a nice table
    fn outlet(sender: S, prefix_str: &str, float_rate: f64) -> Result<StatsdOutlet<S>> {
        assert!(float_rate <= 1.0 && float_rate >= 0.0);
        let prefix = prefix_str.to_string();
        let rate_suffix = if float_rate < 1.0 { format!("|@{}", float_rate)} else { "".to_string() };
        Ok(StatsdOutlet {
            sender,
            prefix,
            int_rate: to_int_rate(float_rate),
            time_suffix: format!("|ms{}", rate_suffix),
            gauge_suffix: format!("|g{}", rate_suffix),
            count_suffix: format!("|c{}", rate_suffix)
        })
    }

    /// Report to statsd a count of items.
    pub fn count(&self, key: &str, value: u64) {
        if accept_sample(self.int_rate)  {
            let count = &value.to_string();
            self.send( &[key, ":", count, &self.count_suffix] )
        }
    }

    /// Report to statsd a non-cumulative (instant) count of items.
    pub fn gauge(&self, key: &str, value: u64) {
        if accept_sample(self.int_rate)  {
            let count = &value.to_string();
            self.send( &[key, ":", count, &self.gauge_suffix] )
        }
    }

    /// Report to statsd a time interval of items.
    pub fn time_interval_ms(&self, key: &str, interval_ms: u64) {
        if accept_sample(self.int_rate)  {
            self.send_time_ms(key, interval_ms);
        }
    }

    /// Query current time to use eventually with `stop_time()`
    pub fn start_time(&self) -> StartTime {
        StartTime( time::precise_time_ns() )
    }

    /// An efficient timer that skips querying for stop time if sample will not be collected.
    /// Caveat : Random sampling overhead of a few ns will be included in any reported time interval.
    pub fn stop_time(&self, key: &str, start_time: StartTime) {
        if accept_sample(self.int_rate)  {
            self.send_time_ms(key, start_time.elapsed_ms());
        }
    }

    fn send_time_ms(&self, key: &str, interval_ms: u64) {
        let value = &interval_ms.to_string();
        self.send( &[key, ":", value, &self.time_suffix] )
    }

    /// Concatenate text parts into a single buffer and send it over UDP
    fn send(&self, strings: &[&str]) {
        let mut str = String::with_capacity(MAX_UDP_PAYLOAD);
        str.push_str(&self.prefix);
        for s in strings { str.push_str(s); }
        self.sender.send_stats(str)
    }

}

/// Convert a floating point sampling rate to an integer so that a fast integer RNG can be used
/// Float rate range is between 1.0 (send 100% of the samples) and 0.0 (_no_ samples taken)
/// .    | float rate | int rate | percentage
/// ---- | ---------- | -------- | ----
/// all  | 1.0        | 0x0      | 100%
/// none | 0.0        | 0xFFFFFFFF | 0%
fn to_int_rate(float_rate: f64) -> u32 {
    assert!(float_rate <= 1.0 && float_rate >= 0.0);
    ((1.0 - float_rate) * ::std::u32::MAX as f64) as u32
}

fn accept_sample(int_rate: u32) -> bool {
    pcg32::random() > int_rate
}

/// A convenience macro to wrap a block or an expression with a start / stop timer.
/// Elapsed time is sent to the supplied statsd client after the computation has been performed.
/// Expression result (if any) is transparently returned.
#[macro_export]
macro_rules! time {
    ($client: expr, $key: expr, $body: block) => (
        let start_time = $client.start_time();
        $body
        $client.stop_time($key, start_time);
    );
}


/// Integrated testing with a live statsd server can be performed according to the instructions in the README.
#[cfg(test)]
mod tests {

    use pcg32;
    use super::StatsdOutlet;
    use std::cell::RefCell;

    impl super::SendStats for RefCell<Vec<String>> {
        fn send_stats(&self, str: String) {
            self.borrow_mut().push(str);
        }
    }

    fn test_client() -> StatsdOutlet<RefCell<Vec<String>>> {
        StatsdOutlet::outlet(RefCell::new(Vec::new()), "", super::FULL_SAMPLING_RATE).unwrap()
    }

    fn test_sampling_client() -> StatsdOutlet<RefCell<Vec<String>>> {
        StatsdOutlet::outlet(RefCell::new(Vec::new()), "", 0.999).unwrap()
    }

    #[test]
    fn test_count() {
        let statsd = test_client(); 
        statsd.count("bouring", 22);
        let str = statsd.sender.borrow_mut().pop();
        assert_eq!(str.unwrap(), "bouring:22|c")
    }

    #[test]
    fn test_gauge() {
        let statsd = test_client();
        statsd.gauge("bearing", 33);
        let str = statsd.sender.borrow_mut().pop();
        assert_eq!(str.unwrap(), "bearing:33|g")
    }

    #[test]
    fn test_time() {
        let statsd = test_client();
        statsd.time_interval_ms("barry", 44);
        let str = statsd.sender.borrow_mut().pop();
        assert_eq!(str.unwrap(), "barry:44|ms")
    }

    #[test]
    fn test_sampling_count() {
        let statsd = test_sampling_client();
        statsd.count("bouring", 22);
        let str = statsd.sender.borrow_mut().pop();
        assert_eq!(str.unwrap(), "bouring:22|c|@0.999")
    }

    #[test]
    fn test_smapling_gauge() {
        let statsd = test_sampling_client();
        statsd.gauge("bearing", 33);
        let str = statsd.sender.borrow_mut().pop();
        assert_eq!(str.unwrap(), "bearing:33|g|@0.999")
    }

    #[test]
    fn test_sampling_time() {
        let statsd = test_sampling_client();
        statsd.time_interval_ms("barry", 44);
        let str = statsd.sender.borrow_mut().pop();
        assert_eq!(str.unwrap(), "barry:44|ms|@0.999")
    }

    #[test]
    fn test_time_macro() {
        let statsd = test_client();
        time!(statsd, "berry", {
            let mut sum: i64 = 1;
            for i in 0..100_000 {
                sum += if i % 2 == 0  {i} else {-i};
            };
            println!("{}", sum);
        });
        let str = statsd.sender.borrow_mut().pop();
        assert!(str.unwrap().starts_with("berry"))
    }

    #[test]
    fn basic_behavior_of_pcg32() {
        let mut v = Vec::new();
        for _ in 0..100 { v.push(pcg32::random()) }
        v.sort();
        for w in v.windows(2) { assert_ne!(w[0], w[1]) }
    }

    fn validate_rate_distribution(rate: f64) {
        let variance = rate * (1.0 - rate); // variance of the Bernoulli distribution
        let sampling = super::to_int_rate(rate);
        let n: u64 = 10000;

        let observed = (0..n).filter(|_| super::accept_sample(sampling)).count();
        let f = n as f64;
        let expected = ((f * rate) - (f * variance),
                        (f * rate) + (f * variance));
        println!("rate: {}, variance: {}, observed: {}; should be within {:?}", rate, variance, observed as f64, expected);
        println!("expected.0: {}, expected.1: {}", expected.0, expected.1);

        assert!(expected.0 < observed as f64);
        assert!(expected.1 > observed as f64);
    }

    #[test]
    fn test_sampling_low_rate() {
        validate_rate_distribution(0.01);
    }

    #[test]
    fn test_sampling_mid_rate() {
        validate_rate_distribution(0.5);
    }

    #[test]
    fn test_sampling_hi_rate() {
        validate_rate_distribution(0.99);
    }
}


/// Run benchmarks with `cargo +nightly bench --features bench`
/// Rough results on T460S, nightly 1.19.0:
/// - PCG32 random sampling go/no-go takes ~6ns/measure
/// - Assembling the string to send takes ~173ns/measure (measured with udp_socket.send() commented out)
/// - Sending the packet takes ~4000ns/measure
///
/// Setting #[cold] on send() method had no apparent effect.
///
/// The moral of the story is : if you need to spend less time doing metrics, send less packets.
/// The first thing to optimize would be to send multiple measures per packet.
/// Also, sending asynchronously would minimize work thread jitter which might be desirable in interactive apps.
/// If performance is still a problem (really?), maybe attack packet formatting?
#[cfg(feature="bench")]
mod bench {

    use test::Bencher;

    #[bench]
    fn time_bench_ten_percent(b: &mut Bencher) {
        let statsd = super::StatsdClient::new("localhost:8125", "a.b.c", 0.1).unwrap();
        b.iter(|| statsd.time_interval_ms("barry", 44));
    }

    #[bench]
    fn time_bench_one_percent(b: &mut Bencher) {
        let statsd = super::StatsdClient::new("localhost:8125", "a.b.c", 0.01).unwrap();
        b.iter(|| statsd.time_interval_ms("barry", 44));
    }

    #[bench]
    fn time_bench_point_one_percent(b: &mut Bencher) {
        let statsd = super::StatsdClient::new("localhost:8125", "a.b.c", 0.001).unwrap();
        b.iter(|| statsd.time_interval_ms("barry", 44));
    }

    #[bench]
    fn time_bench_full_sampling(b: &mut Bencher) {
        let statsd = super::StatsdClient::new("localhost:8125", "a.b.c", 1.0).unwrap();
        b.iter(|| statsd.time_interval_ms("barry", 44));
    }

    #[bench]
    fn time_bench_never(b: &mut Bencher) {
        let statsd = super::StatsdClient::new("localhost:8125", "a.b.c", 0.0).unwrap();
        b.iter(|| statsd.time_interval_ms("barry", 44));
    }

}