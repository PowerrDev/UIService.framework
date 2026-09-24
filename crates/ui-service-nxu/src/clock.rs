//! Unix-epoch-seconds to menu bar clock text, entirely integer no_std math.
//!
//! No wall-clock formatting crate is pulled in for this: the conversion is
//! the well-known Howard Hinnant `civil_from_days` algorithm (proleptic
//! Gregorian, valid for the full `i64` day range), and the string is built
//! with `core::fmt::Write` over a fixed stack buffer since `alloc` is not
//! available in this freestanding target.

use core::fmt::Write;

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

struct FixedWriter<'a> {
    buffer: &'a mut [u8],
    len: usize,
}

impl<'a> FixedWriter<'a> {
    fn new(buffer: &'a mut [u8]) -> Self {
        Self { buffer, len: 0 }
    }

    /// Consumes the writer so the returned `&str` can borrow the caller's
    /// buffer directly instead of `self` (which would only live as long as
    /// this method call).
    fn finish(self) -> &'a str {
        let Self { buffer, len } = self;
        core::str::from_utf8(&buffer[..len]).unwrap_or("")
    }
}

impl Write for FixedWriter<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let available = self.buffer.len() - self.len;
        let copy_len = bytes.len().min(available);
        self.buffer[self.len..self.len + copy_len].copy_from_slice(&bytes[..copy_len]);
        self.len += copy_len;
        Ok(())
    }
}

/// Split days-since-epoch into a proleptic Gregorian (year, month, day),
/// month and day both 1-based. <https://howardhinnant.github.io/date_algorithms.html>
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { y + 1 } else { y };
    (year, month, day)
}

/// 1970-01-01 was a Thursday, so day 0 must land on index 4.
fn weekday_from_days(days: i64) -> usize {
    (((days % 7 + 7) % 7 + 4) % 7) as usize
}

/// Format `unix_seconds` as e.g. "Fri 11 Sep  14:32" into `buffer`, UTC.
///
/// This kernel has no timezone database, so the menu bar clock is UTC-only
/// for now -- see the RTC/ABI plumbing this was built on top of.
pub(crate) fn format(unix_seconds: u64, buffer: &mut [u8]) -> &str {
    let days = (unix_seconds / 86400) as i64;
    let seconds_of_day = unix_seconds % 86400;
    let hour = seconds_of_day / 3600;
    let minute = (seconds_of_day % 3600) / 60;

    let (_year, month, day) = civil_from_days(days);
    let weekday = WEEKDAYS[weekday_from_days(days)];
    let month_name = MONTHS[(month - 1) as usize];

    let mut writer = FixedWriter::new(buffer);
    let _ = write!(writer, "{weekday} {day} {month_name}  {hour:02}:{minute:02}");
    writer.finish()
}
