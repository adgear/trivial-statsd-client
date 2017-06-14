/// PCG32 random number generation for fast sampling
// TODO use https://github.com/codahale/pcg instead?
use std::cell::RefCell;
use time;

fn seed() -> u64 {
    let seed = 5573589319906701683_u64;
    let seed = seed.wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
        .wrapping_add(time::precise_time_ns());
    seed.wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
}

pub fn random() -> u32 {
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

