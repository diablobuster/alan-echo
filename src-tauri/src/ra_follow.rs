//! Read Aloud — follow mode's "did the user scroll the page?" rule. Pure: time is
//! passed in, so every case is unit-tested.
//!
//! Round 3 (follow-off-diagnosis.md): word movement alone is NOT a user scroll. Chrome
//! pages reflow (ads, lazy images, embeds) and Chrome briefly answers "no rects" for
//! visible text. Follow turns OFF only when the spoken word moved (or left the screen)
//! AND there is scroll evidence:
//! - a mouse wheel over the source window (watch hook),
//! - a press-and-drag in the source window (scroll bar, drag-select autoscroll),
//! - the page's scroll offset changed (UIA ScrollPattern of the nearest scrollable
//!   ancestor), which also covers keyboard, touchpad and smooth scrolling,
//!   but never Echo's own scroll (queued, running, or settling).
//!
//! Movement without evidence is a reflow: follow stays on and the word is re-anchored.
//! "Off screen" needs two empty answers in a row. Echo's own scroll opens a grace that
//! starts when the scroll actually ran on the UIA worker and lasts until the page is
//! stable for two looks and 800 ms (capped at `GRACE_CAP`).

use std::fmt;
use std::time::{Duration, Instant};

use crate::ra_uia::{Rect, ScrollPos};

/// Where a word was last seen: its first rect's top-left relative to the source
/// window (`None` = off screen), and the source window's rect (x, y, w, h).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    pub word: usize,
    pub pos: Option<(f64, f64)>,
    pub win: (i32, i32, i32, i32),
}

/// `word`'s anchor from fresh screen rects and the source window rect (x, y, w, h).
pub fn anchor_of(word: usize, rects: &[Rect], win: (i32, i32, i32, i32)) -> Anchor {
    Anchor { word, pos: rects.first().map(|r| (r.x - win.0 as f64, r.y - win.1 as f64)), win }
}

/// Movement (px) of the same word inside the window that counts as "moved".
pub const SHIFT_PX: f64 = 3.0;
/// Change of the page's scroll offset (px) that counts as a scroll.
pub const SCROLL_PX: f64 = 2.0;
/// A wheel or drag this recent is evidence for a move seen now.
pub const INPUT_WINDOW: Duration = Duration::from_millis(1500);
/// Echo's own-scroll grace never lasts longer than this after the scroll ran.
pub const GRACE_CAP: Duration = Duration::from_millis(2500);
/// If nothing moved since our scroll ran, the grace holds at least this long (Chrome
/// lands accessibility location updates late, and smooth scrolls start late).
pub const GRACE_QUIET: Duration = Duration::from_millis(1000);
/// Stable looks in a row that end the grace...
pub const GRACE_STABLE: u32 = 2;
/// ...with no move for this long: Chrome lands a smooth scroll's location updates in
/// batches, so two quick looks can see the same offset while the page is still easing.
/// Measured on Edge (follow_reflow_manual): pauses of 500+ ms inside one smooth scroll.
pub const GRACE_SETTLE: Duration = Duration::from_millis(800);
/// A queued own scroll whose ack never came stops counting after this long.
pub const PENDING_CAP: Duration = Duration::from_secs(5);
/// Chrome updates the scroll offset before the words' rects: an offset change the
/// words have not followed yet keeps the old baseline this long (then it is taken as
/// scroll anchoring, which moves the offset but not the words).
pub const OFFSET_HOLD: Duration = Duration::from_millis(1000);
/// Inside Echo's own-scroll window, an offset change beyond the asked-for delta plus
/// this share of the viewport is the user's.
pub const OWN_SLACK: f64 = 0.25;
/// Content an own scroll reveals loads right after it lands (lazy images, blocks): an
/// unmeasured move beyond the asked-for delta counts as the user only after this long.
pub const LAZY_WINDOW: Duration = Duration::from_millis(700);
/// Lead scrolling (keep the spoken word on screen): when the word enters the bottom
/// `LEAD_BOTTOM` of the scroller viewport, scroll so it sits `LEAD_TARGET` from the top.
pub const LEAD_BOTTOM: f64 = 0.20;
pub const LEAD_TARGET: f64 = 0.30;
/// At most one lead scroll this often, unless the word is off screen.
pub const LEAD_EVERY: Duration = Duration::from_millis(1500);
/// Empty rect answers in a row before the word counts as off screen.
pub const EMPTIES: u32 = 2;

/// What proved the user scrolled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Evidence {
    Wheel,
    Drag,
    /// The page's scroll offset changed and Echo did not do it.
    ScrollOffset,
    /// The scroll offset moved against the direction of Echo's own settling scroll.
    AgainstOwnScroll,
    /// Inside Echo's own-scroll window the offset moved further than Echo asked for
    /// (|expected| + 25% of the viewport), against it, or after a scroll that asked for
    /// no movement: a PageDown on top of our scroll.
    BeyondOwnScroll,
    /// The scroll offset is unknown (Acrobat, OCR, no scrollable ancestor) and the word
    /// left the screen while Echo was not scrolling: the user scrolled it away.
    LeftScreen,
}

impl Evidence {
    fn name(self) -> &'static str {
        match self {
            Evidence::Wheel => "wheel",
            Evidence::Drag => "drag",
            Evidence::ScrollOffset => "scroll_offset",
            Evidence::AgainstOwnScroll => "against_own_scroll",
            Evidence::BeyondOwnScroll => "beyond_own_scroll",
            Evidence::LeftScreen => "left_screen_no_offset",
        }
    }
}

/// Everything the follow-off log line needs.
#[derive(Debug, Clone, PartialEq)]
pub struct OffReport {
    /// "moved" (the word shifted) or "offscreen" (it left the screen).
    pub reason: &'static str,
    pub word: usize,
    /// The word's move in px (None when it left the screen).
    pub d: Option<(f64, f64)>,
    pub evidence: Evidence,
    pub since_wheel: Option<Duration>,
    pub since_drag: Option<Duration>,
    pub since_own: Option<Duration>,
    pub scroll_before: Option<ScrollPos>,
    pub scroll_after: Option<ScrollPos>,
}

fn ms(d: Option<Duration>) -> String {
    d.map_or("never".into(), |d| format!("{}ms", d.as_millis()))
}

fn pos(p: Option<ScrollPos>) -> String {
    p.map_or("?".into(), |p| format!("({:.0},{:.0})px/{:.2}%", p.x, p.y, p.pct))
}

impl fmt::Display for OffReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = self.d.map_or("offscreen".into(), |(x, y)| format!("({x:.0},{y:.0})px"));
        write!(
            f,
            "read-aloud: follow OFF reason={} evidence={} word={} move={} since_wheel={} since_drag={} since_own_scroll={} scroll={}->{}",
            self.reason,
            self.evidence.name(),
            self.word,
            d,
            ms(self.since_wheel),
            ms(self.since_drag),
            ms(self.since_own),
            pos(self.scroll_before),
            pos(self.scroll_after),
        )
    }
}

/// Why a move did not turn follow off (debug log).
#[derive(Debug, Clone, PartialEq)]
pub struct Ignored {
    /// "reflow" (no scroll evidence) or "own_scroll" (Echo's own scroll is settling).
    pub why: &'static str,
    pub word: usize,
    pub d: (f64, f64),
    pub scroll_before: Option<ScrollPos>,
    pub scroll_after: Option<ScrollPos>,
}

impl fmt::Display for Ignored {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "read-aloud: follow kept ({} ignored) word={} move=({:.0},{:.0})px scroll={}->{}",
            self.why,
            self.word,
            self.d.0,
            self.d.1,
            pos(self.scroll_before),
            pos(self.scroll_after),
        )
    }
}

/// What the controller does after a look at the spoken word.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Nothing,
    /// One empty answer: keep what is drawn and look again next tick.
    Wait,
    /// Follow is on and the word is off screen with no user scroll: scroll to it.
    Scroll,
    /// The word moved but follow stays on (debug log).
    Ignored(Ignored),
    /// The user scrolled: follow off (info log).
    Off(OffReport),
}

#[derive(Debug, Clone, Copy)]
struct Own {
    ran: Instant,
    /// Sign of our scroll's offset change per axis (0 = unknown / no change yet).
    dir: (f64, f64),
    moved: bool,
    stable: u32,
    /// The page settled after it: Echo may scroll again; only the tail is still ours.
    settled: bool,
    /// The offset when it ran, and the vertical delta it asked for (`None` = unknown,
    /// a scroll-into-view).
    start: Option<ScrollPos>,
    expected: Option<f64>,
    /// A look showed the offset where it asked (within the slack): from then on a move
    /// beyond it is the user's even when no word could be measured.
    landed: bool,
    /// The last time the page was seen moving (the ack time until then).
    last_move: Instant,
}

/// Follow-mode bookkeeping for one read.
#[derive(Debug, Default)]
pub struct Guard {
    anchor: Option<Anchor>,
    empty_word: Option<usize>,
    empties: u32,
    /// The page's scroll offset at the last look (the baseline for "it changed").
    scroll: Option<ScrollPos>,
    /// Since when the baseline is held for an offset change the words did not follow yet.
    held_since: Option<Instant>,
    wheel_at: Option<Instant>,
    drag_at: Option<Instant>,
    pending_at: Option<Instant>,
    own: Option<Own>,
    own_ran_at: Option<Instant>,
    /// The word Echo already scrolled to (each word is scrolled to once).
    scrolled_word: Option<usize>,
    /// The last lead scroll (`lead`).
    last_lead: Option<Instant>,
}

/// A word move `dw` (px, one axis) fits an offset change `ds` on that axis: the word
/// moved against the offset. Chrome's scroll anchoring changes the offset while the
/// words stay put, or (with a reflow at the same time) move WITH it or only a little
/// against it (Edge: word -114, offset +250). The word may move more than the offset (a
/// real scroll while content above the view shrinks: page down 442 px, offset +192),
/// but at least `FIT_SHARE` of it.
fn fits_scroll(dw: f64, ds: f64) -> bool {
    ds.abs() < SCROLL_PX || (dw.abs() >= SHIFT_PX && dw.signum() == -ds.signum() && dw.abs() >= FIT_SHARE * ds.abs())
}

/// A word move must be at least this share of the offset change to fit a scroll.
const FIT_SHARE: f64 = 0.6;

fn sign(v: f64) -> f64 {
    if v.abs() < SCROLL_PX {
        0.0
    } else {
        v.signum()
    }
}

impl Guard {
    /// Follow (re)armed by a pick, a jump or "Go to word": forget the old anchor,
    /// baseline and input (a wheel from before the button press is not a new scroll).
    /// An own scroll in flight keeps its grace.
    pub fn rearm(&mut self) {
        *self = Guard { pending_at: self.pending_at, own: self.own, own_ran_at: self.own_ran_at, ..Guard::default() };
    }

    pub fn wheel(&mut self, now: Instant) {
        self.wheel_at = Some(now);
    }

    pub fn drag(&mut self, now: Instant) {
        self.drag_at = Some(now);
    }

    /// Echo queued a scroll (it has not run yet).
    pub fn own_queued(&mut self, now: Instant) {
        self.pending_at = Some(now);
    }

    /// Echo's scroll ran on the UIA worker; `before`/`after` = the page's scroll offset
    /// right before and after the call (a smooth scroll may not have moved yet).
    /// `expected` = the vertical delta (px) it asked for, when known.
    pub fn own_ran(&mut self, now: Instant, before: Option<ScrollPos>, after: Option<ScrollPos>, expected: Option<f64>) {
        self.pending_at = None;
        let dir = match (before, after) {
            (Some(b), Some(a)) => (sign(a.x - b.x), sign(a.y - b.y)),
            _ => (0.0, 0.0),
        };
        let expected = expected.filter(|_| before.is_some());
        self.own = Some(Own {
            ran: now,
            dir,
            moved: dir != (0.0, 0.0),
            stable: 0,
            settled: false,
            last_move: now,
            start: before,
            expected,
            landed: false,
        });
        self.own_ran_at = Some(now);
        if after.is_some() {
            self.scroll = after; // our own change is never compared against
        }
    }

    /// Echo may scroll now: no own scroll queued, and the last one settled.
    pub fn may_scroll(&self, now: Instant) -> bool {
        let pending = self.pending_at.is_some_and(|t| now.saturating_duration_since(t) < PENDING_CAP);
        let settled = self.own.is_none_or(|o| o.settled || now.saturating_duration_since(o.ran) >= GRACE_CAP);
        !pending && settled
    }

    /// Lead scrolling: the vertical delta (px) that keeps the spoken word on screen, or
    /// `None` to leave the page alone. `word` = the spoken word's (top, height) on screen
    /// (`None` = off screen); `prev_y` = the top of the last word seen on screen (used
    /// when the spoken word is off screen); `view` = the scroller viewport (top, height).
    /// The word entering the bottom `LEAD_BOTTOM` scrolls it to `LEAD_TARGET` from the
    /// top, at most every `LEAD_EVERY`; a word off screen (or above the viewport) is
    /// scrolled to at once. Never while Echo's own scroll is in flight or follow is off.
    pub fn lead(
        &mut self,
        now: Instant,
        follow: bool,
        word: Option<(f64, f64)>,
        prev_y: Option<f64>,
        view: (f64, f64),
    ) -> Option<f64> {
        let (top, h) = view;
        if !follow || h <= 0.0 || !self.may_scroll(now) {
            return None;
        }
        let target = top + LEAD_TARGET * h;
        let (y, urgent) = match word {
            Some((y, _)) if y < top => (y, true),
            Some((y, wh)) if y + wh > top + (1.0 - LEAD_BOTTOM) * h => (y, false),
            Some(_) => return None,
            None => (prev_y?, true),
        };
        let throttled = self.last_lead.is_some_and(|t| now.saturating_duration_since(t) < LEAD_EVERY);
        if throttled && !urgent {
            return None;
        }
        self.last_lead = Some(now);
        Some(y - target)
    }

    /// The word whose anchor is held: when the spoken word changed, the caller re-measures
    /// this one in the same look (`look`'s `prev_again`), so movement is still measured on
    /// one word (at 2.2x the word changes every ~180 ms, more often than a refresh).
    pub fn anchor_word(&self) -> Option<usize> {
        self.anchor.map(|a| a.word)
    }

    /// `look` without a re-measured previous word.
    #[cfg(test)]
    pub fn observe(&mut self, now: Instant, follow: bool, obs: Anchor, scroll: Option<ScrollPos>) -> Decision {
        self.look(now, follow, obs, None, scroll)
    }

    /// A fresh look at the spoken word (`obs`), the previously anchored word measured
    /// again now (`prev_again`, when the word changed) and the page's scroll offset.
    pub fn look(
        &mut self,
        now: Instant,
        follow: bool,
        obs: Anchor,
        prev_again: Option<Anchor>,
        scroll: Option<ScrollPos>,
    ) -> Decision {
        // "Off screen" only counts on the second empty answer in a row; the first one
        // changes nothing (not even the baselines), the caller just looks again.
        if obs.pos.is_none() {
            if self.empty_word == Some(obs.word) {
                self.empties += 1;
            } else {
                self.empty_word = Some(obs.word);
                self.empties = 1;
            }
            if self.empties < EMPTIES {
                return Decision::Wait;
            }
        } else {
            self.empty_word = None;
            self.empties = 0;
        }

        let since = |t: Option<Instant>| t.map(|t| now.saturating_duration_since(t));
        let pending = since(self.pending_at).is_some_and(|d| d < PENDING_CAP);
        if !pending {
            self.pending_at = None;
        }
        let prev = self.anchor;
        let win_changed = prev.is_some_and(|p| p.win != obs.win);
        // the same word, then and now: the spoken word itself, or the previous one re-measured
        let pair = prev.filter(|_| !win_changed).and_then(|p| {
            if p.word == obs.word {
                Some((p, obs))
            } else {
                prev_again.filter(|a| a.word == p.word && a.win == p.win).map(|a| (p, a))
            }
        });
        let before = self.scroll;
        let dscroll = match (before, scroll) {
            (Some(a), Some(b)) if !win_changed => Some((b.x - a.x, b.y - a.y)),
            _ => None,
        };
        let scroll_moved = dscroll.is_some_and(|d| d.0.abs() >= SCROLL_PX || d.1.abs() >= SCROLL_PX);
        let shift = pair.and_then(|(a, b)| a.pos.zip(b.pos)).map(|(a, b)| (b.0 - a.0, b.1 - a.1));
        let word_moved = shift.is_some_and(|d| d.0.abs() >= SHIFT_PX || d.1.abs() >= SHIFT_PX);
        let went_off = pair.is_some_and(|(a, b)| a.pos.is_some() && b.pos.is_none());
        // the offset change explains the word's move (unknown move: assume it does)
        let fits = match (shift, dscroll) {
            (Some(w), Some(d)) => fits_scroll(w.0, d.0) && fits_scroll(w.1, d.1),
            _ => true,
        };

        // Echo's own scroll. Until it settles (GRACE_STABLE quiet looks and GRACE_SETTLE
        // with no move; after GRACE_QUIET if it never moved the page), every move that is
        // not against its direction is ours. Once settled, and until GRACE_CAP, an offset
        // change along its direction is still ours (a smooth scroll's late tail, measured
        // on Edge 2 s after the scroll ran); one against it is the user.
        let mut grace = false;
        let mut tail = false;
        let mut against = false;
        let mut beyond = false;
        if let Some(o) = self.own.as_mut() {
            let el = now.saturating_duration_since(o.ran);
            if el >= GRACE_CAP {
                self.own = None;
            } else if let (Some(e), Some(st), Some(sc)) = (o.expected, o.start, scroll) {
                // Echo asked for `e` px: a move within |e| + OWN_SLACK of the viewport, in
                // its direction, is ours; anything else is the user (also after a scroll
                // that asked for nothing).
                let cum = sc.y - st.y;
                let budget = e.abs() + OWN_SLACK * st.extent.max(sc.extent);
                let claimed = cum.abs() < SCROLL_PX
                    || (e.abs() >= SCROLL_PX && cum.signum() == e.signum() && cum.abs() <= budget);
                // only a word measured moving with the page tells a user scroll from lazy
                // content growing above the view (Chrome's scroll anchoring) meanwhile
                let measured = shift.is_some();
                let near = (cum - e).abs() <= OWN_SLACK * st.extent.max(sc.extent);
                // landed = the page really moved toward the target (a small or urgent lead
                // must not count as landed before anything moved)
                let toward = e.abs() < SCROLL_PX
                    || (cum.signum() == e.signum() && cum.abs() >= SCROLL_PX.max(0.5 * e.abs()));
                beyond = scroll_moved && !claimed && (measured || (o.landed && el >= LAZY_WINDOW));
                o.landed |= near && toward && !beyond;
                if o.settled {
                    tail = scroll_moved && !beyond;
                } else {
                    grace = !beyond;
                    if scroll_moved || word_moved || went_off {
                        o.moved = true;
                        o.stable = 0;
                        o.last_move = now;
                    } else if dscroll.is_some() || pair.is_some() {
                        o.stable += 1;
                    }
                    let quiet = now.saturating_duration_since(o.last_move) >= GRACE_SETTLE;
                    if o.stable >= GRACE_STABLE && quiet && (o.landed || el >= GRACE_QUIET) {
                        o.settled = true;
                    }
                }
            } else {
                // a scroll of unknown size (paragraph scroll-into-view) claims at most one
                // viewport from where it started
                if let (Some(st), Some(sc)) = (o.start, scroll) {
                    let cum = sc.y - st.y;
                    beyond = scroll_moved
                        && cum.abs() > st.extent.max(sc.extent)
                        && (shift.is_some() || el >= LAZY_WINDOW);
                }
                let mut along = false;
                if let Some(d) = dscroll.filter(|_| scroll_moved) {
                    let opposes = |dir: f64, v: f64| dir != 0.0 && v.abs() >= SCROLL_PX && v.signum() == -dir;
                    against = opposes(o.dir.0, d.0) || opposes(o.dir.1, d.1);
                    if o.dir == (0.0, 0.0) && !o.settled {
                        o.dir = (sign(d.0), sign(d.1)); // the first move after the ack is ours
                    }
                    along = !against && o.dir != (0.0, 0.0);
                }
                if o.settled {
                    tail = along && !beyond;
                } else {
                    grace = !beyond;
                    let comparable = dscroll.is_some() || pair.is_some();
                    if scroll_moved || word_moved || went_off {
                        o.moved = true;
                        o.stable = 0;
                        o.last_move = now;
                    } else if comparable {
                        o.stable += 1;
                    }
                    let quiet = now.saturating_duration_since(o.last_move) >= GRACE_SETTLE;
                    if o.stable >= GRACE_STABLE && quiet && (o.moved || el >= GRACE_QUIET) {
                        o.settled = true; // this look was quiet; Echo may scroll again
                    }
                }
            }
        }

        let recent = |t: Option<Instant>| since(t).is_some_and(|d| d <= INPUT_WINDOW);
        let evidence = if recent(self.wheel_at) {
            Some(Evidence::Wheel)
        } else if recent(self.drag_at) {
            Some(Evidence::Drag)
        } else if beyond && fits && !pending {
            Some(Evidence::BeyondOwnScroll)
        } else if scroll_moved && fits && against && !pending {
            Some(Evidence::AgainstOwnScroll)
        } else if scroll_moved && fits && !pending && !grace && !tail {
            Some(Evidence::ScrollOffset)
        } else if went_off && dscroll.is_none() && !pending && self.own.is_none_or(|o| o.settled) {
            // no offset to go by: the word leaving with Echo not scrolling is the user
            Some(Evidence::LeftScreen)
        } else {
            None
        };

        let decision = if !follow {
            Decision::Nothing
        } else if let (true, Some(ev)) = (word_moved || went_off, evidence) {
            Decision::Off(OffReport {
                reason: if shift.is_some() { "moved" } else { "offscreen" },
                word: obs.word,
                d: shift,
                evidence: ev,
                since_wheel: since(self.wheel_at),
                since_drag: since(self.drag_at),
                since_own: since(self.own_ran_at),
                scroll_before: before,
                scroll_after: scroll,
            })
        } else if obs.pos.is_none() {
            // off screen with no user scroll: reading ran off it, or a reflow pushed it
            // out. Scroll to it, once per word, never while our own scroll is settling.
            let settled = self.own.is_none_or(|o| o.settled);
            if !pending && settled && self.scrolled_word != Some(obs.word) {
                self.scrolled_word = Some(obs.word);
                Decision::Scroll
            } else {
                Decision::Nothing
            }
        } else if word_moved || went_off {
            Decision::Ignored(Ignored {
                why: if grace || pending || tail { "own_scroll" } else { "reflow" },
                word: obs.word,
                d: shift.unwrap_or_default(),
                scroll_before: before,
                scroll_after: scroll,
            })
        } else {
            Decision::Nothing
        };

        self.anchor = Some(obs);
        // the offset changed but the (visible) word did not move yet: its rects may lag
        let unfollowed = scroll_moved
            && !word_moved
            && !went_off
            && !grace
            && !tail
            && !pending
            && pair.is_some_and(|(a, b)| a.pos.is_some() && b.pos.is_some());
        let hold = unfollowed
            && !matches!(decision, Decision::Off(_))
            && since(self.held_since).is_none_or(|d| d < OFFSET_HOLD);
        if hold {
            self.held_since.get_or_insert(now);
        } else {
            self.held_since = None;
            if scroll.is_some() || win_changed {
                self.scroll = scroll;
            }
        }
        decision
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN: (i32, i32, i32, i32) = (0, 0, 1000, 800);

    fn at(word: usize, y: Option<f64>) -> Anchor {
        Anchor { word, pos: y.map(|y| (100.0, y)), win: WIN }
    }
    fn sp(y: f64) -> Option<ScrollPos> {
        Some(ScrollPos { x: 0.0, y, pct: y / 100.0, top: 100.0, extent: 800.0, view_pct: 20.0 })
    }
    fn ms(t0: Instant, n: u64) -> Instant {
        t0 + Duration::from_millis(n)
    }
    fn is_off(d: &Decision) -> bool {
        matches!(d, Decision::Off(_))
    }

    #[test]
    fn anchor_is_window_relative() {
        let r = [Rect { x: 110.0, y: 420.0, w: 30.0, h: 20.0 }];
        let win = (10, 20, 1000, 800);
        assert_eq!(anchor_of(3, &r, win), Anchor { word: 3, pos: Some((100.0, 400.0)), win });
        assert_eq!(anchor_of(3, &[], win).pos, None);
    }

    #[test]
    fn wheel_then_move_turns_follow_off() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        assert_eq!(g.observe(t0, true, at(5, Some(400.0)), None), Decision::Nothing);
        g.wheel(ms(t0, 100));
        let d = g.observe(ms(t0, 250), true, at(5, Some(300.0)), None);
        match d {
            Decision::Off(r) => {
                assert_eq!(r.evidence, Evidence::Wheel);
                assert_eq!(r.reason, "moved");
                assert_eq!(r.d, Some((0.0, -100.0)));
                assert_eq!(r.since_wheel, Some(Duration::from_millis(150)));
                let line = r.to_string();
                assert!(line.contains("evidence=wheel") && line.contains("since_wheel=150ms"), "{line}");
            }
            other => panic!("expected Off, got {other:?}"),
        }
    }

    #[test]
    fn an_old_wheel_is_not_evidence() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.wheel(t0);
        g.observe(ms(t0, 100), true, at(5, Some(400.0)), None);
        let d = g.observe(ms(t0, 2000), true, at(5, Some(300.0)), None);
        assert!(matches!(d, Decision::Ignored(Ignored { why: "reflow", .. })), "{d:?}");
    }

    #[test]
    fn scroll_offset_change_turns_follow_off() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.observe(t0, true, at(5, Some(400.0)), sp(1000.0));
        let d = g.observe(ms(t0, 250), true, at(5, Some(280.0)), sp(1120.0));
        match d {
            Decision::Off(r) => {
                assert_eq!(r.evidence, Evidence::ScrollOffset);
                assert_eq!(r.scroll_before, sp(1000.0));
                assert_eq!(r.scroll_after, sp(1120.0));
                let line = r.to_string();
                assert!(line.contains("scroll=(0,1000)px/10.00%->(0,1120)px/11.20%"), "{line}");
            }
            other => panic!("expected Off, got {other:?}"),
        }
    }

    #[test]
    fn keyboard_scroll_without_wheel_turns_follow_off() {
        // PgDn / arrows / Space: no mouse input at all, only the offset changes
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.observe(t0, true, at(9, Some(500.0)), sp(0.0));
        g.observe(ms(t0, 250), true, at(9, Some(500.0)), sp(0.0));
        let d = g.observe(ms(t0, 500), true, at(9, Some(-200.0)), sp(700.0));
        assert!(matches!(&d, Decision::Off(r) if r.evidence == Evidence::ScrollOffset), "{d:?}");
    }

    #[test]
    fn reflow_without_evidence_keeps_follow_on() {
        // an ad loads above the word: it moves, the scroll offset does not
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.observe(t0, true, at(5, Some(400.0)), sp(500.0));
        let d = g.observe(ms(t0, 250), true, at(5, Some(520.0)), sp(500.0));
        match &d {
            Decision::Ignored(i) => {
                assert_eq!(i.why, "reflow");
                assert!(i.to_string().contains("reflow ignored"), "{i}");
            }
            other => panic!("expected Ignored, got {other:?}"),
        }
        // and the word is re-anchored: no movement at the next look
        assert_eq!(g.observe(ms(t0, 500), true, at(5, Some(520.0)), sp(500.0)), Decision::Nothing);
    }

    #[test]
    fn reflow_when_scroll_position_is_unknown_keeps_follow_on() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.observe(t0, true, at(5, Some(400.0)), None);
        assert!(!is_off(&g.observe(ms(t0, 250), true, at(5, Some(250.0)), None)));
    }

    #[test]
    fn one_empty_answer_waits_two_count_as_off_screen() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.observe(t0, true, at(5, Some(400.0)), sp(0.0));
        g.wheel(ms(t0, 200));
        // a single empty answer (Chrome rebuilding its tree): keep follow on, look again
        assert_eq!(g.observe(ms(t0, 250), true, at(5, None), sp(0.0)), Decision::Wait);
        // the word is back: nothing moved
        assert_eq!(g.observe(ms(t0, 283), true, at(5, Some(400.0)), sp(0.0)), Decision::Nothing);
        // two empties in a row with a wheel: the user scrolled it away
        assert_eq!(g.observe(ms(t0, 300), true, at(5, None), sp(0.0)), Decision::Wait);
        let d = g.observe(ms(t0, 333), true, at(5, None), sp(0.0));
        assert!(matches!(&d, Decision::Off(r) if r.reason == "offscreen" && r.evidence == Evidence::Wheel), "{d:?}");
    }

    #[test]
    fn two_empties_without_evidence_scroll_back_to_the_word() {
        // reading ran off the screen, or a reflow pushed it out: follow scrolls
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.observe(t0, true, at(5, Some(700.0)), sp(0.0));
        assert_eq!(g.observe(ms(t0, 100), true, at(6, None), sp(0.0)), Decision::Wait);
        assert_eq!(g.observe(ms(t0, 133), true, at(6, None), sp(0.0)), Decision::Scroll);
    }

    #[test]
    fn a_single_empty_never_scrolls() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.observe(t0, true, at(5, Some(400.0)), sp(0.0));
        assert_eq!(g.observe(ms(t0, 100), true, at(6, None), sp(0.0)), Decision::Wait);
        assert_eq!(g.observe(ms(t0, 133), true, at(6, Some(420.0)), sp(0.0)), Decision::Nothing);
    }

    #[test]
    fn each_word_is_scrolled_to_once() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.observe(t0, true, at(6, None), None);
        assert_eq!(g.observe(ms(t0, 33), true, at(6, None), None), Decision::Scroll);
        g.own_queued(ms(t0, 33));
        g.own_ran(ms(t0, 40), None, None, None);
        // still off screen after the grace ran out: leave it (never fight every tick)
        assert_eq!(g.observe(ms(t0, 3000), true, at(6, None), None), Decision::Nothing);
        assert_eq!(g.observe(ms(t0, 3250), true, at(6, None), None), Decision::Nothing);
    }

    #[test]
    fn own_scroll_with_a_late_landing_keeps_follow_on() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.observe(t0, true, at(5, Some(750.0)), sp(1000.0));
        g.observe(ms(t0, 100), true, at(6, None), sp(1000.0));
        assert_eq!(g.observe(ms(t0, 133), true, at(6, None), sp(1000.0)), Decision::Scroll);
        g.own_queued(ms(t0, 133));
        // the request waited behind a long `more()`: the page moves before the ack
        assert!(!is_off(&g.observe(ms(t0, 400), true, at(6, None), sp(1000.0))));
        // it ran 1.5 s after it was queued; the smooth scroll has not started yet
        g.own_ran(ms(t0, 1633), sp(1000.0), sp(1000.0), None);
        // two quiet looks right after the ack must not end the grace
        assert!(!is_off(&g.observe(ms(t0, 1666), true, at(6, None), sp(1000.0))));
        assert!(!is_off(&g.observe(ms(t0, 1700), true, at(6, None), sp(1000.0))));
        // the page lands 600 ms after the ack, then keeps easing for a bit
        let d = g.observe(ms(t0, 2233), true, at(6, Some(400.0)), sp(1400.0));
        assert!(!is_off(&d), "{d:?}");
        let d = g.observe(ms(t0, 2483), true, at(6, Some(380.0)), sp(1420.0));
        assert!(!is_off(&d), "{d:?}");
        // settled: stable looks for GRACE_SETTLE end the grace
        for t in [2733, 2983, 3233, 3333] {
            g.observe(ms(t0, t), true, at(6, Some(380.0)), sp(1420.0));
        }
        // a same-direction scroll inside the cap is taken for our tail; after the cap
        // (2.5 s after the scroll ran) a real scroll (keyboard) is the user's again
        let d = g.observe(ms(t0, 4200), true, at(6, Some(100.0)), sp(1700.0));
        assert!(matches!(&d, Decision::Off(r) if r.evidence == Evidence::ScrollOffset), "{d:?}");
    }

    #[test]
    fn own_scroll_grace_is_capped() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.observe(t0, true, at(5, Some(400.0)), sp(0.0));
        g.own_queued(t0);
        g.own_ran(ms(t0, 10), sp(0.0), sp(0.0), None);
        // a page that never settles (an animation) cannot hold the grace forever
        let mut y = 0.0;
        let mut last = Decision::Nothing;
        for i in 1..=12 {
            y += 50.0;
            last = g.observe(ms(t0, 10 + i * 250), true, at(5, Some(400.0 - y)), sp(y));
        }
        assert!(is_off(&last), "{last:?}");
    }

    #[test]
    fn scrolling_against_our_own_scroll_is_the_user() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.observe(t0, true, at(5, Some(400.0)), sp(1000.0));
        g.own_queued(t0);
        g.own_ran(ms(t0, 20), sp(1000.0), sp(1300.0), None); // we scrolled down 300 px
        // our scroll keeps easing down: ours
        assert!(!is_off(&g.observe(ms(t0, 120), true, at(5, Some(80.0)), sp(1320.0))));
        // the page goes back up during the grace: the user pressed PgUp
        let d = g.observe(ms(t0, 370), true, at(5, Some(500.0)), sp(900.0));
        assert!(matches!(&d, Decision::Off(r) if r.evidence == Evidence::AgainstOwnScroll), "{d:?}");
    }

    #[test]
    fn wheel_during_our_own_scroll_still_counts() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.observe(t0, true, at(5, Some(400.0)), sp(1000.0));
        g.own_queued(t0);
        g.own_ran(ms(t0, 20), sp(1000.0), sp(1300.0), None);
        g.wheel(ms(t0, 200));
        let d = g.observe(ms(t0, 300), true, at(5, Some(50.0)), sp(1350.0));
        assert!(matches!(&d, Decision::Off(r) if r.evidence == Evidence::Wheel), "{d:?}");
    }

    #[test]
    fn a_scroll_bar_drag_is_evidence() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.observe(t0, true, at(5, Some(400.0)), None);
        g.drag(ms(t0, 100));
        let d = g.observe(ms(t0, 250), true, at(5, Some(100.0)), None);
        assert!(matches!(&d, Decision::Off(r) if r.evidence == Evidence::Drag), "{d:?}");
    }

    #[test]
    fn window_move_or_resize_is_never_a_scroll() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.observe(t0, true, at(5, Some(400.0)), sp(0.0));
        g.wheel(ms(t0, 100));
        let moved = Anchor { word: 5, pos: Some((100.0, 300.0)), win: (0, 0, 1000, 700) };
        // the scroller got shorter too, so its offset in px changed: still not a scroll
        assert!(!is_off(&g.observe(ms(t0, 250), true, moved, sp(40.0))));
    }

    #[test]
    fn follow_off_or_other_words_never_decide() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.observe(t0, false, at(5, Some(400.0)), sp(0.0));
        g.wheel(ms(t0, 100));
        assert_eq!(g.observe(ms(t0, 250), false, at(5, Some(100.0)), sp(300.0)), Decision::Nothing);
        assert_eq!(g.observe(ms(t0, 260), false, at(6, None), sp(300.0)), Decision::Wait);
        assert_eq!(g.observe(ms(t0, 290), false, at(6, None), sp(300.0)), Decision::Nothing);
        // a different word at another place is reading, not a move
        let mut g = Guard::default();
        g.observe(t0, true, at(5, Some(400.0)), sp(0.0));
        assert_eq!(g.observe(ms(t0, 100), true, at(6, Some(430.0)), sp(0.0)), Decision::Nothing);
    }

    #[test]
    fn jitter_is_not_a_move() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.observe(t0, true, at(5, Some(400.0)), sp(0.0));
        g.wheel(ms(t0, 100));
        assert_eq!(g.observe(ms(t0, 250), true, at(5, Some(401.5)), sp(0.5)), Decision::Nothing);
    }

    #[test]
    fn rearm_forgets_old_input_and_anchor() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.observe(t0, true, at(5, Some(400.0)), sp(0.0));
        g.wheel(ms(t0, 100));
        g.rearm(); // "Go to word": the old anchor and baseline are gone
        assert_eq!(g.observe(ms(t0, 200), true, at(5, Some(100.0)), sp(300.0)), Decision::Nothing);
        // the wheel before the button press is forgotten too
        let mut g = Guard::default();
        g.wheel(t0);
        g.rearm();
        g.observe(ms(t0, 100), true, at(5, Some(400.0)), None);
        assert!(!is_off(&g.observe(ms(t0, 350), true, at(5, Some(100.0)), None)));
    }

    #[test]
    fn the_previous_word_measured_again_shows_the_move() {
        // reading moved on to word 6; word 5, measured again, moved with a wheel: off
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(400.0)), None, None);
        assert_eq!(g.anchor_word(), Some(5));
        g.wheel(ms(t0, 50));
        let d = g.look(ms(t0, 180), true, at(6, Some(250.0)), Some(at(5, Some(250.0))), None);
        assert!(matches!(&d, Decision::Off(r) if r.reason == "moved" && r.d == Some((0.0, -150.0))), "{d:?}");
        // not moved: reading just went on
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(400.0)), None, None);
        g.wheel(ms(t0, 50));
        let d = g.look(ms(t0, 180), true, at(6, Some(400.0)), Some(at(5, Some(400.0))), None);
        assert_eq!(d, Decision::Nothing);
    }

    #[test]
    fn scrolled_away_needs_the_page_to_move_and_evidence() {
        // the new word is off screen and the previous one left too, with PgDn: off
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(400.0)), None, sp(0.0));
        g.look(ms(t0, 180), true, at(6, None), Some(at(5, None)), sp(900.0));
        let d = g.look(ms(t0, 213), true, at(6, None), Some(at(5, None)), sp(900.0));
        assert!(is_off(&d), "{d:?}");
    }

    #[test]
    fn scroll_anchoring_is_not_a_user_scroll() {
        // Chrome adjusts the offset when content loads above the viewport; the words
        // stay put: no move, so no follow off, and reading off the bottom still scrolls
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(700.0)), None, sp(0.0));
        assert_eq!(g.look(ms(t0, 180), true, at(6, Some(740.0)), Some(at(5, Some(700.0))), sp(120.0)), Decision::Nothing);
        assert_eq!(g.look(ms(t0, 360), true, at(7, None), Some(at(6, Some(740.0))), sp(240.0)), Decision::Wait);
        assert_eq!(g.look(ms(t0, 393), true, at(7, None), Some(at(6, Some(740.0))), sp(240.0)), Decision::Scroll);
    }

    #[test]
    fn own_smooth_scroll_with_batched_location_updates_keeps_follow_on() {
        // Measured on Edge (follow_reflow_manual): Chrome lands a smooth scroll's
        // location updates in batches, so looks 83 ms apart (12 words/s) see the same
        // offset twice in a row while the page is still easing. That is not "settled".
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(720.0)), None, sp(273.0));
        g.own_queued(t0);
        g.own_ran(ms(t0, 2), sp(273.0), sp(273.0), None);
        let mut t = 2;
        let mut y = 273.0;
        let mut word = 5;
        for batch in [389.0, 0.0, 0.0, 0.0, 96.0, 0.0, 0.0, 40.0] {
            t += 83;
            word += 1;
            y += batch;
            let prev = Some(at(word - 1, Some(720.0 - (y - 273.0))));
            let d = g.look(ms(t0, t), true, at(word, Some(720.0 - (y - 273.0))), prev, sp(y));
            assert!(!is_off(&d), "t={t} {d:?}");
        }
    }

    #[test]
    fn anchoring_plus_reflow_is_not_a_user_scroll() {
        // Measured on Edge: an ad grew above the view (Chrome's scroll anchoring added
        // 250 px to the offset) while a block was inserted above the word (it moved
        // DOWN 138 px). A real scroll moves the word against the offset, by about as much.
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(400.0)), None, sp(758.0));
        let d = g.look(ms(t0, 83), true, at(6, Some(540.0)), Some(at(5, Some(538.0))), sp(1008.0));
        assert!(matches!(&d, Decision::Ignored(Ignored { why: "reflow", .. })), "{d:?}");
        // a real scroll of the same size: the word moves up by as much
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(400.0)), None, sp(758.0));
        let d = g.look(ms(t0, 83), true, at(6, Some(152.0)), Some(at(5, Some(150.0))), sp(1008.0));
        assert!(is_off(&d), "{d:?}");
        // a scroll with a small reflow mixed in still counts
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(400.0)), None, sp(758.0));
        let d = g.look(ms(t0, 83), true, at(5, Some(190.0)), None, sp(1008.0));
        assert!(is_off(&d), "{d:?}");
        // measured on Edge: a 442 px page down while an ad above the view shrank by 250
        // (the offset only rose 192): the word moved against the offset, so it counts
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(585.0)), None, sp(886.0));
        let d = g.look(ms(t0, 100), true, at(5, Some(143.0)), None, sp(1078.0));
        assert!(matches!(&d, Decision::Off(r) if r.evidence == Evidence::ScrollOffset), "{d:?}");
    }

    #[test]
    fn own_smooth_scroll_tail_after_a_long_pause_is_still_ours() {
        // Measured on Edge: 348 -> 416 at ~300 ms, nothing for 600 ms, then +19 at 925 ms
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(700.0)), None, sp(0.0));
        g.own_queued(t0);
        g.own_ran(ms(t0, 1), sp(0.0), sp(0.0), None);
        assert!(!is_off(&g.look(ms(t0, 300), true, at(5, Some(284.0)), None, sp(416.0))));
        for t in [383, 466, 550, 633, 716, 800, 883] {
            assert!(!is_off(&g.look(ms(t0, t), true, at(5, Some(284.0)), None, sp(416.0))));
        }
        let d = g.look(ms(t0, 925), true, at(5, Some(265.0)), None, sp(435.0));
        assert!(!is_off(&d), "{d:?}");
    }

    #[test]
    fn a_late_same_direction_tail_within_the_cap_is_ours() {
        // Measured on Edge: the page settled, then moved +96 px more in our scroll's
        // direction 2010 ms after our scroll ran (a smooth scroll retargeted by a reflow)
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(700.0)), None, sp(500.0));
        g.own_queued(t0);
        g.own_ran(ms(t0, 1), sp(500.0), sp(500.0), None);
        assert!(!is_off(&g.look(ms(t0, 300), true, at(5, Some(288.0)), None, sp(912.0))));
        for t in [550, 800, 1050, 1300, 1550, 1800] {
            assert!(!is_off(&g.look(ms(t0, t), true, at(5, Some(288.0)), None, sp(912.0))));
        }
        let d = g.look(ms(t0, 2010), true, at(5, Some(192.0)), None, sp(1008.0));
        assert!(!is_off(&d), "{d:?}");
        // after the cap, the same move is the user's
        let d = g.look(ms(t0, 2600), true, at(5, Some(96.0)), None, sp(1104.0));
        assert!(is_off(&d), "{d:?}");
    }

    #[test]
    fn after_settling_echo_may_scroll_again_but_a_move_against_it_is_the_user() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(700.0)), None, sp(500.0));
        g.own_queued(t0);
        g.own_ran(ms(t0, 1), sp(500.0), sp(500.0), None);
        g.look(ms(t0, 300), true, at(5, Some(288.0)), None, sp(912.0));
        for t in [550, 800, 1050, 1300] {
            g.look(ms(t0, t), true, at(5, Some(288.0)), None, sp(912.0));
        }
        // settled: reading runs off the screen again -> Echo scrolls
        assert_eq!(g.look(ms(t0, 1400), true, at(9, None), None, sp(912.0)), Decision::Wait);
        assert_eq!(g.look(ms(t0, 1433), true, at(9, None), None, sp(912.0)), Decision::Scroll);
        // a fresh guard: settled, then the page goes back up (PgUp) inside the cap
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(700.0)), None, sp(500.0));
        g.own_queued(t0);
        g.own_ran(ms(t0, 1), sp(500.0), sp(500.0), None);
        g.look(ms(t0, 300), true, at(5, Some(288.0)), None, sp(912.0));
        for t in [550, 800, 1050, 1300] {
            g.look(ms(t0, t), true, at(5, Some(288.0)), None, sp(912.0));
        }
        let d = g.look(ms(t0, 1500), true, at(5, Some(688.0)), None, sp(512.0));
        assert!(matches!(&d, Decision::Off(r) if r.evidence == Evidence::AgainstOwnScroll), "{d:?}");
    }

    #[test]
    fn rects_lagging_behind_the_scroll_offset_still_count() {
        // Measured on Edge: after a page down the offset updates first and the word's
        // rects a look or two later. The offset change must not be used up meanwhile.
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(400.0)), None, sp(1000.0));
        assert_eq!(g.look(ms(t0, 100), true, at(5, Some(400.0)), None, sp(1800.0)), Decision::Nothing);
        assert_eq!(g.look(ms(t0, 200), true, at(5, Some(400.0)), None, sp(1800.0)), Decision::Nothing);
        let d = g.look(ms(t0, 300), true, at(5, Some(-400.0)), None, sp(1800.0));
        assert!(matches!(&d, Decision::Off(r) if r.evidence == Evidence::ScrollOffset), "{d:?}");
        // same, but the word leaves the screen
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(400.0)), None, sp(1000.0));
        g.look(ms(t0, 100), true, at(5, Some(400.0)), None, sp(1800.0));
        assert_eq!(g.look(ms(t0, 200), true, at(5, None), None, sp(1800.0)), Decision::Wait);
        let d = g.look(ms(t0, 233), true, at(5, None), None, sp(1800.0));
        assert!(matches!(&d, Decision::Off(r) if r.evidence == Evidence::ScrollOffset), "{d:?}");
    }

    #[test]
    fn an_offset_change_the_words_never_follow_is_let_go() {
        // scroll anchoring: the offset moves, the words stay. After a second the change
        // is taken as the new baseline, so a later reflow is not mistaken for a scroll.
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(400.0)), None, sp(1000.0));
        for t in [100, 350, 600, 850, 1100, 1350] {
            assert_eq!(g.look(ms(t0, t), true, at(5, Some(400.0)), None, sp(1250.0)), Decision::Nothing);
        }
        let d = g.look(ms(t0, 1600), true, at(5, Some(150.0)), None, sp(1250.0));
        assert!(matches!(&d, Decision::Ignored(Ignored { why: "reflow", .. })), "{d:?}");
    }

    // ---- fix round 1 ----

    #[test]
    fn offset_unknown_and_the_word_leaves_the_screen_is_a_user_scroll() {
        // Acrobat / OCR / no scrollable ancestor: a keyboard scroll moves the word away and
        // nothing else can tell. Scrolling back would snap the page back on every word.
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(400.0)), None, None);
        assert_eq!(g.look(ms(t0, 250), true, at(5, None), None, None), Decision::Wait);
        let d = g.look(ms(t0, 283), true, at(5, None), None, None);
        assert!(matches!(&d, Decision::Off(r) if r.evidence == Evidence::LeftScreen), "{d:?}");
        // reading moves on to an off-screen word while the previous one is gone too
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(700.0)), None, None);
        g.look(ms(t0, 180), true, at(6, None), Some(at(5, None)), None);
        let d = g.look(ms(t0, 213), true, at(6, None), Some(at(5, None)), None);
        assert!(matches!(&d, Decision::Off(r) if r.evidence == Evidence::LeftScreen), "{d:?}");
    }

    #[test]
    fn offset_unknown_small_reflow_on_screen_keeps_follow_on() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(400.0)), None, None);
        let d = g.look(ms(t0, 250), true, at(5, Some(430.0)), None, None);
        assert!(matches!(&d, Decision::Ignored(Ignored { why: "reflow", .. })), "{d:?}");
        // reading running off the bottom with the previous word still on screen: scroll
        let d = g.look(ms(t0, 400), true, at(6, None), Some(at(5, Some(430.0))), None);
        assert_eq!(d, Decision::Wait);
        assert_eq!(g.look(ms(t0, 433), true, at(6, None), Some(at(5, Some(430.0))), None), Decision::Scroll);
    }

    #[test]
    fn offset_unknown_word_leaving_during_our_own_scroll_is_ours() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(700.0)), None, None);
        g.own_queued(t0);
        g.own_ran(ms(t0, 5), None, None, None);
        g.look(ms(t0, 100), true, at(5, None), None, None);
        assert!(!is_off(&g.look(ms(t0, 133), true, at(5, None), None, None)));
    }

    #[test]
    fn a_big_same_direction_page_down_inside_our_scroll_window_is_the_user() {
        // our lead scroll asked for +300 px; the user then pressed PageDown (+900 px)
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(600.0)), None, sp(1000.0));
        g.own_queued(t0);
        g.own_ran(ms(t0, 5), sp(1000.0), sp(1000.0), Some(300.0));
        let d = g.look(ms(t0, 100), true, at(5, Some(300.0)), None, sp(1300.0));
        assert!(!is_off(&d), "our own +300: {d:?}");
        let d = g.look(ms(t0, 350), true, at(5, Some(-600.0)), None, sp(2200.0));
        assert!(matches!(&d, Decision::Off(r) if r.evidence == Evidence::BeyondOwnScroll), "{d:?}");
    }

    #[test]
    fn a_small_overshoot_of_our_own_scroll_is_still_ours() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(600.0)), None, sp(1000.0));
        g.own_queued(t0);
        g.own_ran(ms(t0, 5), sp(1000.0), sp(1000.0), Some(300.0));
        // +300 plus a 120 px reflow-ish overshoot, within |expected| + 25% of 800
        let d = g.look(ms(t0, 100), true, at(5, Some(180.0)), None, sp(1420.0));
        assert!(!is_off(&d), "{d:?}");
    }

    #[test]
    fn our_scroll_that_did_not_move_claims_nothing() {
        // the scroll asked for nothing (the page was already at its end): the next move is
        // the user's even right after it
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(600.0)), None, sp(3200.0));
        g.own_queued(t0);
        g.own_ran(ms(t0, 5), sp(3200.0), sp(3200.0), Some(0.0));
        let d = g.look(ms(t0, 100), true, at(5, Some(1000.0)), None, sp(2800.0));
        assert!(matches!(&d, Decision::Off(r) if r.evidence == Evidence::BeyondOwnScroll), "{d:?}");
    }

    #[test]
    fn going_against_our_known_scroll_is_the_user() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(600.0)), None, sp(1000.0));
        g.own_queued(t0);
        g.own_ran(ms(t0, 5), sp(1000.0), sp(1000.0), Some(300.0));
        let d = g.look(ms(t0, 100), true, at(5, Some(900.0)), None, sp(700.0));
        assert!(matches!(&d, Decision::Off(r) if r.evidence == Evidence::BeyondOwnScroll), "{d:?}");
    }

    const VIEW: (f64, f64) = (100.0, 800.0); // viewport top, height (screen px)

    #[test]
    fn lead_keeps_the_word_off_the_bottom() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        // word in the middle: nothing
        assert_eq!(g.lead(t0, true, Some((500.0, 20.0)), None, VIEW), None);
        // word enters the bottom 20% (its bottom below 100 + 640): bring it to 30% (340)
        assert_eq!(g.lead(t0, true, Some((730.0, 20.0)), None, VIEW), Some(390.0));
        g.own_queued(t0);
        g.own_ran(ms(t0, 5), sp(0.0), sp(0.0), Some(390.0));
        // never while our own scroll is settling
        assert_eq!(g.lead(ms(t0, 300), true, Some((760.0, 20.0)), None, VIEW), None);
        // follow off: never
        let mut g = Guard::default();
        assert_eq!(g.lead(t0, false, Some((730.0, 20.0)), None, VIEW), None);
    }

    #[test]
    fn lead_scrolls_at_most_every_1_5_s_unless_off_screen() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        assert!(g.lead(t0, true, Some((730.0, 20.0)), None, VIEW).is_some());
        // (no own scroll registered: only the throttle applies)
        assert_eq!(g.lead(ms(t0, 1000), true, Some((735.0, 20.0)), None, VIEW), None);
        assert!(g.lead(ms(t0, 1600), true, Some((735.0, 20.0)), None, VIEW).is_some());
        // off screen: at once, placing the last visible word at 30%
        assert_eq!(g.lead(ms(t0, 1700), true, None, Some(880.0), VIEW), Some(540.0));
        // off screen with no word to go by: the caller falls back to scroll-into-view
        assert_eq!(g.lead(ms(t0, 1800), true, None, None, VIEW), None);
        // a word above the viewport counts as off screen too
        let mut g = Guard::default();
        assert_eq!(g.lead(t0, true, Some((40.0, 20.0)), None, VIEW), Some(-300.0));
    }

    #[test]
    fn lazy_content_growing_during_our_scroll_is_not_the_user() {
        // Measured on Edge (lab page): a 398 px lead scroll; lazy blocks grew above the
        // view meanwhile and the offset jumped +1082 px, and the words left the screen
        // with no measurable move. Without a word moving against the offset, it's ours.
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(74, Some(793.0)), None, sp(489.0));
        g.own_queued(t0);
        g.own_ran(ms(t0, 5), sp(489.0), sp(489.0), Some(398.0));
        assert_eq!(g.look(ms(t0, 130), true, at(75, None), Some(at(74, None)), sp(1571.0)), Decision::Wait);
        let d = g.look(ms(t0, 165), true, at(75, None), Some(at(74, None)), sp(1571.0));
        assert!(!is_off(&d), "{d:?}");
        let d = g.look(ms(t0, 600), true, at(76, None), Some(at(75, None)), sp(1991.0));
        assert!(!is_off(&d), "{d:?}");
    }

    #[test]
    fn a_small_reflow_against_an_anchoring_jump_is_not_a_scroll() {
        // Measured on Edge: the ad above the view grew (anchoring: offset +250) while a
        // reflow moved the word up 114 px. A scroll moves the word about as far as the
        // offset (at least 60% of it; the -442 / +192 case above still counts).
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(110, Some(500.0)), None, sp(676.0));
        let d = g.look(ms(t0, 150), true, at(110, Some(386.0)), None, sp(926.0));
        assert!(!is_off(&d), "{d:?}");
    }

    #[test]
    fn page_down_after_our_scroll_landed_is_the_user_even_unmeasured() {
        // our +300 landed; then PageDown takes the word (and the previous one) off the top
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(600.0)), None, sp(1000.0));
        g.own_queued(t0);
        g.own_ran(ms(t0, 5), sp(1000.0), sp(1000.0), Some(300.0));
        assert!(!is_off(&g.look(ms(t0, 120), true, at(6, Some(320.0)), Some(at(5, Some(300.0))), sp(1300.0))));
        assert_eq!(g.look(ms(t0, 900), true, at(7, None), Some(at(6, None)), sp(2000.0)), Decision::Wait);
        let d = g.look(ms(t0, 933), true, at(7, None), Some(at(6, None)), sp(2000.0));
        assert!(matches!(&d, Decision::Off(r) if r.evidence == Evidence::BeyondOwnScroll), "{d:?}");
    }

    #[test]
    fn lazy_content_jump_right_after_our_scroll_landed_is_not_the_user() {
        // Measured on Edge: our lead landed, then 269 ms after it ran content it revealed
        // loaded above the view (offset 1026 -> 1921) and the words left the screen
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(700.0)), None, sp(700.0));
        g.own_queued(t0);
        g.own_ran(ms(t0, 1), sp(700.0), sp(700.0), Some(326.0));
        assert!(!is_off(&g.look(ms(t0, 120), true, at(6, Some(400.0)), Some(at(5, Some(374.0))), sp(1026.0))));
        assert_eq!(g.look(ms(t0, 236), true, at(7, None), Some(at(6, None)), sp(1921.0)), Decision::Wait);
        let d = g.look(ms(t0, 270), true, at(7, None), Some(at(6, None)), sp(1921.0));
        assert!(!is_off(&d), "{d:?}");
    }

    // ---- fix round 2 ----

    /// An own scroll with the offset unknown (a paragraph scroll in Acrobat / no scroller)
    /// that moved the word and then settled.
    fn settled_unknown(t0: Instant) -> Guard {
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(700.0)), None, None);
        g.own_queued(t0);
        g.own_ran(ms(t0, 5), None, None, None);
        g.look(ms(t0, 100), true, at(5, Some(400.0)), None, None);
        for t in [350, 600, 850, 1000] {
            assert!(!is_off(&g.look(ms(t0, t), true, at(5, Some(400.0)), None, None)));
        }
        assert!(g.may_scroll(ms(t0, 1000)), "settled");
        g
    }

    #[test]
    fn user_scroll_after_our_scroll_settled_with_the_offset_unknown_is_off() {
        let t0 = Instant::now();
        let mut g = settled_unknown(t0);
        // 1 s after it settled (still inside the 2.5 s cap) the user scrolls the word away
        assert_eq!(g.look(ms(t0, 2000), true, at(5, None), None, None), Decision::Wait);
        let d = g.look(ms(t0, 2033), true, at(5, None), None, None);
        assert!(matches!(&d, Decision::Off(r) if r.evidence == Evidence::LeftScreen), "{d:?}");
    }

    #[test]
    fn repeated_page_downs_never_loop_into_snap_backs() {
        let t0 = Instant::now();
        let mut g = settled_unknown(t0);
        let mut follow = true;
        let mut scrolls = 0;
        // reading goes on while the user keeps paging down: every new word is off screen
        let mut t = 2000;
        for word in 5..20 {
            for _ in 0..2 {
                let prev = (word > 5).then(|| at(word - 1, None));
                match g.look(ms(t0, t), follow, at(word, None), prev, None) {
                    Decision::Off(_) => follow = false,
                    Decision::Scroll => scrolls += 1,
                    _ => {}
                }
                t += 40;
            }
            t += 300;
        }
        assert!(!follow, "the first page down turned follow off");
        assert_eq!(scrolls, 0, "Echo never scrolled back");
    }

    #[test]
    fn a_small_lead_does_not_count_as_landed_before_the_page_moved() {
        // +40 px asked; the page has not moved yet when content it revealed loads and
        // the words leave unmeasured: still ours (not "landed", so not the user)
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(700.0)), None, sp(1000.0));
        g.own_queued(t0);
        g.own_ran(ms(t0, 1), sp(1000.0), sp(1000.0), Some(40.0));
        for t in [100, 200, 300, 400, 500, 600, 700] {
            assert!(!is_off(&g.look(ms(t0, t), true, at(5, Some(700.0)), None, sp(1000.0))));
        }
        assert!(!g.may_scroll(ms(t0, 700)), "not settled before it landed (or 1 s passed)");
        assert_eq!(g.look(ms(t0, 800), true, at(6, None), Some(at(5, None)), sp(1900.0)), Decision::Wait);
        let d = g.look(ms(t0, 833), true, at(6, None), Some(at(5, None)), sp(1900.0));
        assert!(!is_off(&d), "{d:?}");
    }

    #[test]
    fn a_paragraph_scroll_of_unknown_size_claims_at_most_one_viewport() {
        let t0 = Instant::now();
        let mut g = Guard::default();
        g.look(t0, true, at(5, Some(600.0)), None, sp(1000.0));
        g.own_queued(t0);
        g.own_ran(ms(t0, 5), sp(1000.0), sp(1000.0), None);
        // our scroll-into-view moves 400 px: ours
        assert!(!is_off(&g.look(ms(t0, 100), true, at(5, Some(200.0)), None, sp(1400.0))));
        // then the user pages down past one viewport (800 px) from where we started
        let d = g.look(ms(t0, 300), true, at(5, Some(-700.0)), None, sp(2300.0));
        assert!(matches!(&d, Decision::Off(r) if r.evidence == Evidence::BeyondOwnScroll), "{d:?}");
    }
}
