
#![feature(asm)]
#![feature(const_fn)]
#![feature(core_intrinsics)]
#![feature(integer_atomics)]
#![feature(libc)]

extern crate libc;

use libc::{c_void, MSG_DONTWAIT};
use std::io::{Cursor, Write};
use std::net::UdpSocket;
use std::os::unix::io::{AsRawFd, IntoRawFd};
use std::mem;
use std::sync::atomic::{AtomicI32, Ordering};

const SOCKET_FD_INIT: i32 = -1;

static mut SOCKET_FD: AtomicI32 = AtomicI32::new(SOCKET_FD_INIT);
static mut PREFIX: [u8; 576] = [0; 576];
static mut PREFIX_LEN: usize = 0;

pub fn init(address: &str, prefix: &str) -> Result<(), std::io::Error>
{
    let socket = UdpSocket::bind("0:0")?; // NB: CLOEXEC by default
    socket.set_nonblocking(true)?;
    socket.connect(address)?;
    // so, we could protect this like log does, using a CAS on a state
    // and so on, in the case that you choose to call `init` from a
    // thousand different threads at once.  but, that's not a likely
    // use-case here, so we do all the work up front, and just use the
    // CAS to see if we won the race or not.  if we lost, free what we
    // allocated and go home.
    if SOCKET_FD_INIT != unsafe { SOCKET_FD.compare_and_swap(SOCKET_FD_INIT,
                                                             socket.as_raw_fd(),
                                                             Ordering::SeqCst) } {
        return Ok(())
    }
    unsafe {
        for (i,c) in prefix.bytes().enumerate() {
            PREFIX[i] = c;
        }
        PREFIX_LEN = prefix.len();
        if PREFIX[PREFIX_LEN-1] != b'.' {
            PREFIX[PREFIX_LEN] = b'.';
            PREFIX_LEN += 1;
        }
    }
    socket.into_raw_fd();       // so this doesn't get closed
    // XXX, that's technically a leaked fd; by design, basically.
    Ok(())
}

#[doc(hidden)]
pub fn send(strings: &[&[u8]]) {
    // ideally we'd construct an iov here
    let fd = unsafe { SOCKET_FD.load(Ordering::Relaxed) };
    if SOCKET_FD_INIT == fd { return }
    let mut buf: [u8; 576];
    buf = unsafe { mem::uninitialized() };
    let len = {
        let mut cursor = Cursor::new(&mut buf[..]);
        let _ = cursor.write_all(unsafe { &PREFIX[..PREFIX_LEN] });
        for s in strings { let _ = cursor.write_all(s); }
        cursor.position() as usize
    };
    unsafe {
        libc::send(fd, buf.as_ptr() as *mut c_void, len, MSG_DONTWAIT);
    }
}

use std::cell::RefCell;

#[cfg(target_arch = "x86_64")]
fn rdtsc() -> u64 {
    let s: u32;
    let t: u32;
    unsafe { asm!("rdtsc" : "={edx}"(s), "={eax}"(t) ::) }
    ((s as u64) << 32) | (t as u64)
}

fn seed() -> u64 {
    let seed = 5573589319906701683_u64;
    let seed = seed.wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
        .wrapping_add(rdtsc());
    seed.wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
}

fn pcg32() -> u32 {
    thread_local! {
        static PCG32_STATE: RefCell<u64> = RefCell::new(seed());
    }

    PCG32_STATE.with(|state| {
        let oldstate: u64 = *state.borrow();
        // XXX could generate the increment from the thread ID
        *state.borrow_mut() = oldstate.wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((((oldstate >> 18) ^ oldstate) >> 27) as u32)
            .rotate_right((oldstate >> 59) as u32)
    })
}

pub fn within_rate(r: u32) -> bool {
    pcg32() > r
}


// XXX we need to redo this as a syntax extension that stringifies all
// the constant arguments so we don't need to format them.  This is
// particularly important for rate.
#[macro_export]
macro_rules! statsd {
    (increment, $key:expr, $count:expr, $rate:expr) => ({
        if unsafe { ::std::intrinsics::unlikely($crate::within_rate(((1.0 - $rate) * ::std::u32::MAX as f64) as u32)) } {
            let count = format!("{}", $count);
            let rate = format!("{}", $rate);
            $crate::send(&[$key, b":", count.as_bytes(), b"|c", b"|@", rate.as_bytes()])
        }
    });
    (gauge, $key:expr, $value:expr, $rate:expr) => ({
        if unsafe { ::std::intrinsics::unlikely($crate::within_rate(((1.0 - $rate) * ::std::u32::MAX as f64) as u32)) } {
            let value = format!("{}", $value);
            let rate = format!("{}", $rate);
            $crate::send(&[$key, b":", value.as_bytes(), b"|g", b"|@", rate.as_bytes()])
        }
    });
}


#[test]
fn basics() {
    const STATSD_SAMPLING_RATE: f64 = 0.5;
    init("localhost:8125", "foo.bar").unwrap();
    for _ in 0..10 {
        statsd!(increment, b"boring", 1, STATSD_SAMPLING_RATE);
    }
}


#[test]
fn basic_behavior_of_pcg32() {
    let mut v = Vec::new();
    for _ in 0..100 { v.push(pcg32()) }
    v.sort();
    for w in v.windows(2) { assert_ne!(w[0], w[1]) }
}


#[test]
fn test_sampling() {
    let rate = 0.01;
    let variance = rate * (1.0 - rate); // variance of the Bernoulli distribution
    let n = 10000_f64;
    let observed = (0..n as u64)
        .fold(0, |sum, _| sum + if within_rate(((1.0 - rate) * std::u32::MAX as f64) as u32) {1} else {0}) as f64;
    let expected = ((n*rate)-(n*variance),
                    (n*rate)+(n*variance));
    println!("observed: {}; should be within {:?}", observed, expected);
    assert!(expected.0 < observed);
    assert!(expected.1 > observed);
}
