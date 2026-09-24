//! Smooth scrolling.
//!
//! A wheel notch does not move the view; it moves a *target*, and the view
//! eases towards it on the runtime's per-frame clock (`App::tick`). A
//! trackpad's stream of small deltas therefore adds up to one fluid motion
//! instead of a staircase, and a single mouse-wheel notch glides rather than
//! jumps. The ease is an exponential approach (each frame closes a fixed
//! fraction of the remaining distance for the time that passed), which is what
//! Finder's feels like, and it is integer-only: the target is soft-float.
//!
//! Positions are in 1/16 pixel so a slow glide's last few pixels still move.
//! This module knows nothing about rows or windows: `content` gives it the
//! range (`max`, in pixels) and paints from `offset_px`.

/// Fixed-point scale of the stored offset and target.
const SUB: i32 = 16;

/// How long the view takes to close about two thirds of the distance to the
/// target. Shorter feels twitchy, longer feels like it is wading.
const EASE_TIME_CONSTANT_US: i64 = 90_000;

/// A frame after a long stall (the first tick, or a slow redraw) advances by
/// at most this much time, so an animation resumes rather than teleporting.
const MAX_STEP_US: i64 = 100_000;

/// The step assumed for the first tick, when there is no earlier frame to
/// measure from.
const FIRST_STEP_US: i64 = 16_667;

/// The overlay scroll indicator stays fully visible this long after the last
/// scroll, then fades out over `FADE_US`.
const INDICATOR_HOLD_US: i64 = 700_000;
const FADE_US: i64 = 300_000;

pub struct Scroller {
    /// Where the view is now, in 1/16 px from the top of the content.
    offset: i32,
    /// Where it is heading.
    target: i32,
    /// The clock of the last tick; 0 when no animation is running.
    last_tick_us: u64,
    /// How much longer the overlay indicator is shown, in microseconds.
    indicator_us: i64,
}

impl Scroller {
    pub const fn new() -> Self {
        Self { offset: 0, target: 0, last_tick_us: 0, indicator_us: 0 }
    }

    /// The whole-pixel offset to draw at.
    pub fn offset_px(&self) -> i32 {
        (self.offset + SUB / 2) / SUB
    }

    /// Whether a frame is due: the view has not reached its target, or the
    /// indicator is still showing (or fading).
    pub fn animating(&self) -> bool {
        self.offset != self.target || self.indicator_us > 0
    }

    /// Where the view is heading, in whole pixels.
    #[cfg(test)]
    pub fn target_px(&self) -> i32 {
        (self.target + SUB / 2) / SUB
    }

    /// Turn the wheel: `notches` positive is wheel up (towards the top), each
    /// worth `step_px`. `max_px` is the furthest the content can scroll.
    pub fn scroll_by(&mut self, notches: i32, step_px: i32, max_px: i32) {
        let moved = notches.saturating_mul(step_px).saturating_mul(SUB);
        self.target = self.target.saturating_sub(moved).clamp(0, max_px.max(0).saturating_mul(SUB));
        // Any turn shows the indicator afresh, even one that hits the end.
        self.indicator_us = INDICATOR_HOLD_US + FADE_US;
    }

    /// The content got shorter or taller (a resize, a new folder): keep the
    /// view and its target inside the new range.
    pub fn clamp(&mut self, max_px: i32) {
        let limit = max_px.max(0).saturating_mul(SUB);
        self.offset = self.offset.clamp(0, limit);
        self.target = self.target.clamp(0, limit);
    }

    /// Jump back to the top and hide the indicator: a different folder or view
    /// is a different piece of content.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Advance to `now_us`. Returns whether anything visible changed: the
    /// whole-pixel offset, or the indicator's opacity.
    pub fn tick(&mut self, now_us: u64, max_px: i32) -> bool {
        let step_us = if self.last_tick_us == 0 {
            FIRST_STEP_US
        } else {
            (now_us.saturating_sub(self.last_tick_us) as i64).clamp(0, MAX_STEP_US)
        };
        self.last_tick_us = now_us;

        let before = (self.offset_px(), self.indicator_alpha());

        self.clamp(max_px);

        let remaining = self.target - self.offset;
        if remaining != 0 {
            // Close `step / (tau + step)` of the gap: the rational stand-in for
            // `1 - e^(-step/tau)` that needs no floating point and never
            // overshoots, however long the step.
            let fraction_256 = (step_us * 256 / (EASE_TIME_CONSTANT_US + step_us)) as i32;
            let mut moved = (i64::from(remaining) * i64::from(fraction_256) / 256) as i32;
            // Within a pixel: arrive, rather than crawl the last sliver.
            if remaining.abs() <= SUB || moved == 0 {
                moved = remaining;
            }
            self.offset += moved;
        }

        self.indicator_us = (self.indicator_us - step_us).max(0);

        if !self.animating() {
            // Idle: the next scroll starts from a fresh clock.
            self.last_tick_us = 0;
        }

        before != (self.offset_px(), self.indicator_alpha())
    }

    /// The indicator's opacity, 0..=255.
    pub fn indicator_alpha(&self) -> u8 {
        if self.indicator_us >= FADE_US {
            255
        } else {
            (self.indicator_us.max(0) * 255 / FADE_US) as u8
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: i32 = 20;

    fn run(scroller: &mut Scroller, max: i32, frames: u64, frame_us: u64) -> u64 {
        let mut now = 1_000_000;
        for _ in 0..frames {
            now += frame_us;
            scroller.tick(now, max);
        }
        now
    }

    #[test]
    fn a_notch_glides_to_its_target_and_stops() {
        let mut scroller = Scroller::new();
        scroller.scroll_by(-2, STEP, 500); // wheel down two notches
        assert_eq!(scroller.target_px(), 40);
        assert_eq!(scroller.offset_px(), 0, "it does not jump");
        assert!(scroller.animating());

        run(&mut scroller, 500, 60, 16_667);
        assert_eq!(scroller.offset_px(), 40);
    }

    #[test]
    fn it_eases_out_each_frame_covers_less_than_the_last() {
        let mut scroller = Scroller::new();
        scroller.scroll_by(-10, STEP, 1000);
        let mut previous = scroller.offset_px();
        let mut previous_step = i32::MAX;
        let mut now = 1_000_000;
        for _ in 0..12 {
            now += 16_667;
            scroller.tick(now, 1000);
            let step = scroller.offset_px() - previous;
            assert!(step >= 0 && step <= previous_step, "step {step} after {previous_step}");
            previous_step = step;
            previous = scroller.offset_px();
        }
        assert!(previous > 100, "made real progress: {previous}");
        assert!(previous < 200, "and is not there yet: {previous}");
    }

    #[test]
    fn a_stream_of_small_deltas_adds_up() {
        let mut scroller = Scroller::new();
        for _ in 0..5 {
            scroller.scroll_by(-1, STEP, 500);
        }
        assert_eq!(scroller.target_px(), 100);
    }

    #[test]
    fn wheel_up_moves_towards_the_top_and_stops_there() {
        let mut scroller = Scroller::new();
        scroller.scroll_by(-3, STEP, 500);
        run(&mut scroller, 500, 60, 16_667);
        assert_eq!(scroller.offset_px(), 60);

        scroller.scroll_by(1, STEP, 500);
        assert_eq!(scroller.target_px(), 40);
        scroller.scroll_by(50, STEP, 500);
        assert_eq!(scroller.target_px(), 0, "cannot scroll above the start");
    }

    #[test]
    fn it_cannot_scroll_past_the_end_or_when_nothing_overflows() {
        let mut scroller = Scroller::new();
        scroller.scroll_by(-100, STEP, 150);
        assert_eq!(scroller.target_px(), 150);
        run(&mut scroller, 150, 200, 16_667);
        assert_eq!(scroller.offset_px(), 150);

        let mut short = Scroller::new();
        short.scroll_by(-5, STEP, 0);
        assert_eq!(short.target_px(), 0);
    }

    #[test]
    fn shrinking_the_content_pulls_the_view_back_in_range() {
        let mut scroller = Scroller::new();
        scroller.scroll_by(-20, STEP, 400);
        run(&mut scroller, 400, 100, 16_667);
        assert_eq!(scroller.offset_px(), 400);

        // A resize made the content shorter: max is now 100.
        scroller.tick(9_000_000, 100);
        assert_eq!(scroller.offset_px(), 100);
        assert_eq!(scroller.target_px(), 100);
    }

    #[test]
    fn a_long_stall_neither_overshoots_nor_freezes() {
        let mut scroller = Scroller::new();
        scroller.scroll_by(-4, STEP, 500);
        scroller.tick(1_000_000, 500);
        // Half a second passes between frames.
        scroller.tick(1_500_000, 500);
        let offset = scroller.offset_px();
        assert!(offset > 0 && offset <= 80, "{offset}");
    }

    #[test]
    fn the_animation_ends_and_the_next_scroll_starts_from_a_fresh_clock() {
        let mut scroller = Scroller::new();
        scroller.scroll_by(-1, STEP, 500);
        let end = run(&mut scroller, 500, 200, 16_667);
        assert!(!scroller.animating());

        // Much later: the gap since the last tick must not count as one huge frame.
        scroller.scroll_by(-1, STEP, 500);
        scroller.tick(end + 60_000_000, 500);
        assert!(scroller.offset_px() < 40, "{}", scroller.offset_px());
    }

    #[test]
    fn the_indicator_holds_then_fades_then_the_animation_ends() {
        let mut scroller = Scroller::new();
        assert_eq!(scroller.indicator_alpha(), 0);
        scroller.scroll_by(-1, STEP, 500);
        assert_eq!(scroller.indicator_alpha(), 255);

        // Ticks of 100 ms: hold for 700 ms...
        let mut now = 1_000_000;
        for _ in 0..6 {
            now += 100_000;
            scroller.tick(now, 500);
        }
        assert_eq!(scroller.indicator_alpha(), 255);

        // ...then fade over 300 ms (the first tick above was a nominal 16.7 ms
        // frame, so 700 ms of hold is spent after a further 2.x ticks)...
        for _ in 0..3 {
            now += 100_000;
            scroller.tick(now, 500);
        }
        let fading = scroller.indicator_alpha();
        assert!(fading > 0 && fading < 255, "{fading}");

        // ...and then nothing is left to animate.
        for _ in 0..5 {
            now += 100_000;
            scroller.tick(now, 500);
        }
        assert_eq!(scroller.indicator_alpha(), 0);
        assert!(!scroller.animating());
    }

    #[test]
    fn reset_returns_to_the_top_and_stops_animating() {
        let mut scroller = Scroller::new();
        scroller.scroll_by(-5, STEP, 500);
        run(&mut scroller, 500, 10, 16_667);
        scroller.reset();
        assert_eq!(scroller.offset_px(), 0);
        assert_eq!(scroller.target_px(), 0);
        assert!(!scroller.animating());
        assert_eq!(scroller.indicator_alpha(), 0);
    }

    #[test]
    fn tick_reports_only_visible_changes() {
        let mut scroller = Scroller::new();
        // Nothing scrolled: a tick changes nothing.
        assert!(!scroller.tick(1_000_000, 500));

        scroller.scroll_by(-3, STEP, 500);
        assert!(scroller.tick(1_016_667, 500), "the view moved");

        let end = run(&mut scroller, 500, 200, 16_667);
        assert!(!scroller.animating());
        assert!(!scroller.tick(end + 16_667, 500), "at rest, nothing changes");
    }
}
