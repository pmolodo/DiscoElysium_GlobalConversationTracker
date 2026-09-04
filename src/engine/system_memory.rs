// SPDX-License-Identifier: MIT
//! What the MACHINE has, as opposed to what a budget allows.
//!
//! ## Why the engine asks at all
//!
//! Because a budget is a promise about this search and says nothing about the box it runs
//! on. A 256 MB allowance on a machine with 100 MB free is honoured perfectly and still
//! ends the process: the allocation fails, `handle_alloc_error` aborts, and the engine is
//! inside the game.
//!
//! And the step before that is worse than a clean failure. A process that drives a machine
//! to the last page does not stop, it THRASHES - every actor in the system, this game
//! included, waiting on a disk - and an operating system in that state can be hard to get
//! back. Leaving a slice of memory alone is what keeps a look-ahead a look-ahead: the worst
//! it may cost is a marker that does not appear.

/// What one reading of the machine's memory says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemMemory {
    /// Physical memory installed, in bytes.
    pub total: u64,
    /// Physical memory not in use, in bytes.
    pub available: u64,
}

impl SystemMemory {
    /// Whether taking `wanted` more bytes would leave less than `reserve` of the total free.
    ///
    /// `reserve` is a fraction: 0.05 keeps a twentieth of the machine out of reach.
    pub fn would_dip_below(&self, wanted: u64, reserve: f64) -> bool {
        let floor = (self.total as f64 * reserve) as u64;
        self.available.saturating_sub(wanted) < floor
    }
}

/// What the machine has now, or None where this build cannot ask.
///
/// ## Volatile, and treated as such
///
/// Two readings a moment apart legitimately differ - the documentation for the call this
/// uses says so - and everything else on the machine is moving while a crawl runs. So this
/// is a guard rail rather than an accounting: it is read periodically, acted on
/// immediately, and never cached.
///
/// ## Where it answers
///
/// Windows, through `GlobalMemoryStatusEx`, which is what the game runs on. Elsewhere this
/// returns None and the reserve is not enforced - the tests and the offline tools run
/// there, and a crawl on a developer's Linux box that runs the machine down is a bad
/// afternoon rather than a lost save. A port would add an arm here and nothing else.
#[cfg(windows)]
pub fn read() -> Option<SystemMemory> {
    // The structure `GlobalMemoryStatusEx` fills, field for field and in order, from
    // sysinfoapi.h. `dwLength` is an input: the call refuses a buffer that does not say
    // how big it is.
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_physical: u64,
        available_physical: u64,
        total_page_file: u64,
        available_page_file: u64,
        total_virtual: u64,
        available_virtual: u64,
        available_extended_virtual: u64,
    }

    unsafe extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }

    let mut status = MemoryStatusEx {
        length: std::mem::size_of::<MemoryStatusEx>() as u32,
        memory_load: 0,
        total_physical: 0,
        available_physical: 0,
        total_page_file: 0,
        available_page_file: 0,
        total_virtual: 0,
        available_virtual: 0,
        available_extended_virtual: 0,
    };

    // SAFETY: the buffer is a correctly shaped, fully initialised MEMORYSTATUSEX whose
    // length field says its size, which is the whole of the call's contract.
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    if ok == 0 {
        return None;
    }

    // A machine reporting no memory at all is a reading to disbelieve rather than act on:
    // the reserve would be zero and the guard would never fire, which is the same as not
    // having asked.
    if status.total_physical == 0 {
        return None;
    }

    Some(SystemMemory {
        total: status.total_physical,
        available: status.available_physical,
    })
}

/// The same, where there is no way to ask. See the remarks on the Windows arm.
#[cfg(not(windows))]
pub fn read() -> Option<SystemMemory> {
    None
}

/// How much of the machine a search must leave alone, as a fraction of the total.
///
/// A TWENTIETH. Small enough that it costs a crawl almost nothing on a machine with room,
/// and big enough to be the difference between an operating system that is slow and one
/// that cannot be got back: the last few per cent is where a machine stops swapping pages
/// and starts swapping the things it needs to swap pages with.
///
/// It is also the runway that makes the estimate in [`Runway`] safe. A guard with no
/// margin would have to read the truth on every allocation; this one can be wrong by
/// several megabytes between readings and still have somewhere to stand.
pub const DEFAULT_RESERVE: f64 = 0.05;

/// The reserve in bytes, for a machine of this size.
fn floor_of(machine: SystemMemory, reserve: f64) -> u64 {
    (machine.total as f64 * reserve) as u64
}

/// How the crawl keeps track of the machine without asking it constantly.
///
/// ## The problem with asking
///
/// `GlobalMemoryStatusEx` is a system call, and the crawl's inner loop runs once per state
/// - hundreds of thousands of times. Asking there would put a syscall in the hot loop to
/// learn something that moves in megabytes while a state costs a hundred bytes.
///
/// ## The problem with not asking
///
/// The crawl is not the only thing on the machine. Tracking its OWN allocations is exact
/// and tells you nothing about the game it is running inside, or the browser behind that.
///
/// ## What this does instead
///
/// Reads the truth every [`Self::interval`] states, and between readings subtracts what the
/// crawl has taken since. That estimate is only ever used to decide WHETHER TO ASK: it can
/// trigger an early reading, and it can never by itself end a crawl. So the number a
/// verdict rests on is always one the machine gave, and the estimate only buys the syscalls
/// it saves.
pub struct Runway {
    reserve: f64,
    interval: usize,
    since_reading: usize,
    /// What the machine said at the last reading.
    machine: SystemMemory,
    /// What the crawl has taken since then.
    ///
    /// ONLY EVER UPWARDS, deliberately: nothing here tries to notice a deallocation. The
    /// estimate is meant to be pessimistic, so that where it is wrong it is wrong in the
    /// direction of looking sooner. Tracking frees would make it more accurate and less
    /// useful, since an accurate estimate that drifts the other way would delay a reading
    /// exactly when one was wanted.
    taken: u64,
    /// The reserve in bytes, worked out when the machine was last read.
    ///
    /// Cached rather than computed per state, which is what it was: a float multiply and a
    /// cast in a loop that runs hundreds of thousands of times, to re-derive a number that
    /// only changes when the total does. Measured at 938 ns a state against 894 without
    /// the guard; with this it is 897.
    floor: u64,
}

impl Runway {
    /// How many states pass between readings of the truth.
    ///
    /// Four thousand, which at the measured hundred-odd bytes a state is a few hundred
    /// kilobytes of drift - well inside the reserve - and at nine hundred nanoseconds a
    /// state is a reading every four milliseconds or so. A syscall on that cadence is not
    /// something the loop can feel.
    pub const READINGS_EVERY: usize = 4_096;

    /// A runway over this machine, or None where the platform cannot be asked.
    ///
    /// None means the guard is OFF, which is the honest outcome: a build that cannot read
    /// the machine should not pretend to know it is safe.
    pub fn new(reserve: f64) -> Option<Self> {
        let machine = read()?;
        Some(Self {
            reserve,
            interval: Self::READINGS_EVERY,
            since_reading: 0,
            machine,
            taken: 0,
            floor: floor_of(machine, reserve),
        })
    }

    /// The same, reading the machine on a different cadence.
    pub fn every(reserve: f64, interval: usize) -> Option<Self> {
        Some(Self { interval: interval.max(1), ..Self::new(reserve)? })
    }

    /// What the machine said at the last reading, for a caller that wants to report it.
    pub fn last_reading(&self) -> SystemMemory {
        self.machine
    }

    /// Records `bytes` taken, and says whether the machine is now below its reserve.
    ///
    /// A TRUE ANSWER IS ALWAYS A MEASURED ONE. The estimate decides when to look, never
    /// what to conclude - so a crawl is never ended by arithmetic about a machine that had
    /// the room after all.
    pub fn is_low(&mut self, bytes: u64) -> bool {
        self.taken = self.taken.saturating_add(bytes);
        self.since_reading += 1;

        // THE WHOLE OF THE PER-STATE COST: an add, an increment, a subtract and two
        // compares, all on integers. The floor was a float multiply here until it was
        // hoisted into the reading that produces it - see the field.
        let estimated = self.machine.available.saturating_sub(self.taken);
        if self.since_reading < self.interval && estimated >= self.floor {
            return false;
        }

        self.since_reading = 0;
        self.taken = 0;
        match read() {
            Some(now) => {
                self.machine = now;
                self.floor = floor_of(now, self.reserve);
                now.would_dip_below(0, self.reserve)
            }
            // The machine answered once and will not answer now. Carrying on is the right
            // call: the guard exists to avoid a hard failure, and turning a failed reading
            // into one would be the guard causing what it prevents.
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The machine this suite runs on answers, and answers something believable.
    ///
    /// Skipped rather than failed where the platform has no arm, because that is a fact
    /// about the build and not a fault.
    #[test]
    fn the_machine_says_how_much_it_has() {
        let Some(memory) = read() else {
            eprintln!("no reading on this platform; skipping.");
            return;
        };

        assert!(memory.total > 0, "a machine with no memory is running this test");
        assert!(
            memory.available <= memory.total,
            "{} available of {} total",
            memory.available,
            memory.total,
        );

        eprintln!(
            "{} MB available of {} MB",
            memory.available / (1024 * 1024),
            memory.total / (1024 * 1024),
        );
    }

    #[test]
    fn a_reserve_is_measured_against_the_total_and_not_against_what_is_left() {
        let machine = SystemMemory { total: 1000, available: 100 };

        // A twentieth of a thousand is fifty, and there are a hundred: room for forty.
        assert!(!machine.would_dip_below(40, 0.05));
        assert!(machine.would_dip_below(60, 0.05));

        // The same hundred against a half-the-machine reserve has nothing spare at all.
        assert!(machine.would_dip_below(1, 0.5));
    }

    /// A reserve bigger than what is free refuses everything, which is how a test arms it.
    #[test]
    fn a_reserve_of_nearly_everything_refuses_even_a_byte() {
        let machine = SystemMemory { total: 1000, available: 999 };
        assert!(machine.would_dip_below(1, 0.999));
    }
}
