use std::fmt;

use interrupt::{IrqController, Interrupt};
use memory::Addressable;

use MASTER_CLOCK_HZ;

#[derive(RustcDecodable, RustcEncodable)]
pub struct Rtc {
    /// True if the RTC is paused
    paused: bool,
    /// Master clock divider counter to get a 2Hz signal
    divider: u32,
    /// Current seconds: [00...59]
    seconds: Bcd,
    /// Current minutes: [00...59]
    minutes: Bcd,
    /// Current hours: [00...23]
    hours: Bcd,
    /// Week day; [01...07] (01 is Sunday, 02 Monday ... 07 Saturday)
    week_day: Bcd,
    /// Day of the month: [01...31]
    day: Bcd,
    /// Month: [01..12]
    month: Bcd,
    /// Year: [00..99]
    year: Bcd,
    /// Value to be adjusted when writing to ADJUST register, see the
    /// `set_adjust` function for its meaning
    adjust: u8,
    /// XXX hack: the BIOS always adjusts the various counters twice
    /// at a time and if we don't ignore ore of them we can't reach
    /// any odd number which causes a deadlock. Need to investigate
    /// how the real hardware behaves in this situation.
    skip: bool,
}

impl Rtc {
    pub fn new() -> Rtc {
        Rtc {
            paused: false,
            divider: MASTER_DIVIDER,
            seconds: Bcd::zero(),
            minutes: Bcd::zero(),
            hours: Bcd::zero(),
            week_day: Bcd::one(),
            day: Bcd::one(),
            month: Bcd::one(),
            year: Bcd::from_bcd(0x99).unwrap(),
            adjust: 0,
            skip: false,
        }
    }

    pub fn tick(&mut self,
                irq: &mut IrqController,
                mut master_ticks: u32) {

        while master_ticks > 0 {
            if self.divider >= master_ticks {
                self.divider -= master_ticks;

                master_ticks = 0;
            } else {
                master_ticks -= self.divider;

                self.divider = MASTER_DIVIDER;

                // We exhausted the divider, toggle the RTC signal
                let level = !irq.raw_interrupt(Interrupt::Rtc);

                if level == true {
                    // XXX Not sure how the paused bit is handled
                    if !self.paused {
                        self.second_elapsed();
                        debug!("RTC: {:?}", self);
                    }
                }

                irq.set_raw_interrupt(Interrupt::Rtc, level);
            }
        }
    }

    pub fn store<A: Addressable>(&mut self, offset: u32, val: u32) {
        match offset {
            0 => self.set_mode(val),
            4 => self.set_adjust(val),
            _ => panic!("Unhandled RTC register {:x}", offset),
        }
    }

    pub fn load<A: Addressable>(&self, offset: u32) -> u32 {
        match offset {
            0x8 => self.time(),
            0xc => self.date(),
            _ => panic!("Unhandled RTC register {:x}", offset),
        }
    }

    pub fn set_seconds(&mut self, bcd: Bcd) {
        let v = bcd.bcd();

        assert!(v <= 0x59);

        self.seconds = bcd;
    }

    pub fn set_minutes(&mut self, bcd: Bcd) {
        let v = bcd.bcd();

        assert!(v <= 0x59);

        self.minutes = bcd;
    }

    pub fn set_hours(&mut self, bcd: Bcd) {
        let v = bcd.bcd();

        assert!(v <= 0x23);

        self.hours = bcd;
    }

    pub fn set_week_day(&mut self, bcd: Bcd) {
        let v = bcd.bcd();

        assert!(v >= 0x01 && v <= 0x07);

        self.week_day = bcd;
    }

    pub fn set_day(&mut self, bcd: Bcd) {
        let v = bcd.bcd();

        assert!(v >= 0x01 && v <= 0x31);

        self.day = bcd;
    }

    pub fn set_month(&mut self, bcd: Bcd) {
        let v = bcd.bcd();

        assert!(v >= 0x01 && v <= 0x12);

        self.month = bcd;
    }

    pub fn set_year(&mut self, bcd: Bcd) {
        self.year = bcd;
    }

    fn time(&self) -> u32 {
        let seconds = self.seconds.bcd() as u32;
        let minutes = self.minutes.bcd() as u32;
        let hours = self.hours.bcd() as u32;
        let week_day = self.week_day.bcd() as u32;

        seconds | (minutes << 8) | (hours << 16) | (week_day << 24)
    }

    fn date(&self) -> u32 {
        let day = self.day.bcd() as u32;
        let month = self.month.bcd() as u32;
        let year = self.year.bcd() as u32;

        // XXX What is the high byte exactly?
        day | (month << 8) | (year << 16)
    }

    fn set_mode(&mut self, val: u32) {
        self.paused = (val & 1) != 0;

        self.adjust = ((val >> 1) & 7) as u8;
    }

    fn set_adjust(&mut self, _val: u32) {
        // XXX ugly hack, fix me.
        if self.skip {
            self.skip = false;
            return;
        }

        // I don't understand how that register works, I just reset it
        // to the default value for now so that it doesn't lock up in
        // the reset sequence
        let (counter, min, max) =
            match self.adjust {
                0 => (&mut self.seconds, 0x00, 0x59),
                1 => (&mut self.minutes, 0x00, 0x59),
                2 => (&mut self.hours, 0x00, 0x23),
                3 => (&mut self.week_day, 0x01, 0x07),
                4 => (&mut self.day, 0x01, 0x31),
                5 => (&mut self.month, 0x01, 0x31),
                6 => (&mut self.year, 0x00, 0x99),
                _ => panic!("Unsupported adjust {:x}", self.adjust),
            };

        *counter =
            if counter.bcd() < max {
                counter.next().unwrap()
            } else {
                Bcd::from_bcd(min).unwrap()
            };

        self.skip = true;
    }

    fn second_elapsed(&mut self) {

        let inc_overflow = |bcd: &mut Bcd, max| {
            if bcd.bcd() < max {
                *bcd = bcd.next().unwrap();

                false
            } else {
                *bcd = Bcd::zero();

                true
            }
        };

        if inc_overflow(&mut self.seconds, 0x59) {
            if inc_overflow(&mut self.minutes, 0x59) {
                if inc_overflow(&mut self.hours, 0x23) {
                    self.day_elapsed();
                }
            }
        }
    }

    fn day_elapsed(&mut self) {
        let inc_overflow = |bcd: &mut Bcd, max, min| {
            if bcd.bcd() < max {
                *bcd = bcd.next().unwrap();

                false
            } else {
                *bcd = Bcd::from_bcd(min).unwrap();

                true
            }
        };

        inc_overflow(&mut self.week_day, 0x07, 0x01);

        let days_in_month =
            match self.month.bcd() {
                0x01 => 0x31,
                // XXX The RTC doesn't store the century, so it's
                // probably not able to handle leap years at all? Does
                // the BIOS handle it?
                0x02 => 0x28,
                0x03 => 0x31,
                0x04 => 0x30,
                0x05 => 0x31,
                0x06 => 0x30,
                0x07 => 0x31,
                0x08 => 0x31,
                0x09 => 0x30,
                0x10 => 0x31,
                0x11 => 0x30,
                0x12 => 0x31,
                _ => unreachable!(),
            };

        if inc_overflow(&mut self.day, days_in_month, 0x01) {
            if inc_overflow(&mut self.month, 0x12, 0x01) {
                inc_overflow(&mut self.year, 0x99, 0x00);
            }
        }
    }
}

impl fmt::Debug for Rtc {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}.{} {}:{}:{}",
               self.year, self.month, self.day,
               self.hours,
               self.minutes,
               self.seconds)
    }
}


/// A single packed BCD value in the range 0-99 (2 digits, 4bits per
/// digit).
#[derive(RustcDecodable, RustcEncodable)]
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Bcd(u8);

impl Bcd {
    /// Build a `Bcd` from an `u8` in BCD format. Returns `None` if
    /// the value provided is not valid BCD.
    pub fn from_bcd(b: u8) -> Option<Bcd> {
        if b <= 0x99 && (b & 0xf) <= 0x9 {
            Some(Bcd(b))
        } else {
            None
        }
    }

    /// Build a `Bcd` from a binary `u8`. Returns `None` if the value
    /// is greater than 0x99.
    pub fn from_binary(b: u8) -> Option<Bcd> {
        if b > 99 {
            None
        } else {
            Some(Bcd(((b / 10) << 4) | (b % 10)))
        }
    }

    /// Return a BCD equal to 0
    pub fn zero() -> Bcd {
        Bcd(0)
    }

    /// Return a BCD equal to 1
    pub fn one() -> Bcd {
        Bcd(1)
    }

    /// Returns the BCD as an u8
    pub fn bcd(self) -> u8 {
        self.0
    }

    /// Returns the BCD value plus one or None if the value is 99.
    pub fn next(self) -> Option<Bcd> {
        let b = self.bcd();

        if b & 0xf < 9 {
            Some(Bcd(b + 1))
        } else if b < 0x99 {
            Some(Bcd((b & 0xf0) + 0x10))
        } else {
            None
        }
    }
}

impl fmt::Display for Bcd {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:02x}", self.0)
    }
}

const MASTER_DIVIDER: u32 = MASTER_CLOCK_HZ / 2;
