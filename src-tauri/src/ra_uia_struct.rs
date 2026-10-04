//! Read Aloud: page structure from UI Automation (natural-reading spec §5).
//!
//! At a block start only, the token's enclosing element (Chromium) or its text style
//! (Word and other UIA apps) says what kind of block starts there; the caller prefixes
//! the raw token with `ra_struct::mark`. Unknown structure gives no marker (the raw
//! text rules in `ra_text` handle it). Never panics.

use std::time::{Duration, Instant};

use windows::core::Interface;
use windows::Win32::UI::Accessibility::*;

use crate::ra_struct::Brk;

/// How a read finds block structure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Mode {
    /// No structure reads (no cost).
    #[default]
    Off,
    /// Chromium: walk up from the token's enclosing element.
    Elements,
    /// Word and other UIA apps: the token's StyleId text attribute. Turned off at the
    /// first block start whose app does not support it (Notepad).
    Style,
}

/// True when `cur` may start a block: the first token (`prev` = None), a token after
/// one with a line break, and, when `glued_rule` (Chromium: its word ranges carry no
/// line breaks; a block ends with a token that has no trailing space), a token glued
/// to the previous one that opens with a letter, digit, bullet, embedded object or
/// opening quote / bracket. Candidates only: the caller checks the element.
pub(super) fn block_start(prev: Option<&str>, cur: &str, glued_rule: bool) -> bool {
    let Some(prev) = prev else {
        return true;
    };
    if prev.contains(['\r', '\n']) {
        return true;
    }
    if !glued_rule || prev.is_empty() || prev.ends_with(char::is_whitespace) {
        return false;
    }
    cur.chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || "•◦▪‣·\u{FFFC}\"“‘([".contains(c))
}

/// The UIA properties of one element that the structure walk reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ElInfo {
    /// Control type id.
    pub ct: i32,
    /// AriaRole, lower case.
    pub role: String,
    /// LocalizedControlType, lower case (only "blockquote" is used: Chromium gives a
    /// blockquote no role of its own).
    pub lct: String,
    /// HeadingLevel property id (`HeadingLevel_None` .. `HeadingLevel9`), 0 = unknown.
    pub heading: i32,
}

/// What an ancestor of a block-start token says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// This block kind starts here (when the child is the element's first child).
    Mark(Brk),
    /// Like `Mark`, but the element itself must also be its parent's first child (an
    /// inline element: `code` inside a sentence is not a code block).
    Block(Brk),
    /// A table cell: `TableRow` when the cell is the row's first cell, else `TableCell`.
    Cell,
    /// Nothing yet: look at the parent.
    Up,
    /// A container (document, landmark, list, table row, dialog, ...): no marker.
    Stop,
}

/// Roles that end the walk without a marker.
const STOP_ROLES: &[&str] = &[
    "document", "article", "main", "region", "navigation", "banner", "contentinfo",
    "complementary", "search", "form", "list", "row", "rowgroup", "dialog", "alertdialog",
    "figure", "application", "menu", "menubar", "toolbar", "tablist", "listbox", "tree",
];

/// What element `el` (the parent of `child`, the element the walk came from) means.
pub(super) fn classify(el: &ElInfo, child: &ElInfo) -> Step {
    let role = el.role.as_str();
    if (HeadingLevel1.0..=HeadingLevel9.0).contains(&el.heading) {
        let level = (el.heading - HeadingLevel_None.0).clamp(1, 6) as u8;
        return Step::Mark(Brk::Heading(level));
    }
    if role == "heading" {
        return Step::Mark(Brk::Heading(0));
    }
    if role == "listitem" || (role.is_empty() && el.ct == UIA_ListItemControlTypeId.0) {
        return Step::Mark(Brk::ListItem);
    }
    if matches!(role, "cell" | "gridcell" | "columnheader" | "rowheader") {
        return Step::Cell;
    }
    if role == "blockquote" || el.lct == "blockquote" {
        return Step::Mark(Brk::Quote);
    }
    if role == "code" {
        return Step::Block(Brk::Code);
    }
    if matches!(role, "caption" | "figcaption") || (role == "description" && el.ct == UIA_GroupControlTypeId.0) {
        return Step::Mark(Brk::Caption);
    }
    if matches!(role, "table" | "grid" | "treegrid") || el.ct == UIA_TableControlTypeId.0 {
        // the table's own text that is not in a row: its caption
        let in_rows = matches!(child.role.as_str(), "row" | "rowgroup" | "cell" | "gridcell" | "columnheader" | "rowheader");
        return if in_rows || child.role.is_empty() && child.ct == 0 { Step::Stop } else { Step::Mark(Brk::Caption) };
    }
    if el.ct == UIA_DocumentControlTypeId.0
        || el.ct == UIA_WindowControlTypeId.0
        || el.ct == UIA_PaneControlTypeId.0
        || el.ct == UIA_ListControlTypeId.0
        || STOP_ROLES.contains(&role)
    {
        return Step::Stop;
    }
    Step::Up
}

/// The walk reached a container: when it came up through a generic block (a `Group`:
/// Chromium's `<p>` / `<div>`), the token starts a paragraph (Chromium word ranges
/// carry no line breaks, so the raw-text rules cannot see it). `cts`: the control
/// types below the container, enclosing element first. Also returns the index of the
/// outermost such block (the elements below it must be first children).
pub(super) fn plain_block(cts: &[i32]) -> Option<(Brk, usize)> {
    let top = cts.iter().rposition(|&c| c == UIA_GroupControlTypeId.0)?;
    Some((Brk::Paragraph, top))
}

/// The block kind of a Word-style paragraph (StyleId text attribute), if any.
pub(super) fn style_brk(style: i32) -> Option<Brk> {
    let id = UIA_STYLE_ID(style);
    if id == StyleId_Title {
        return Some(Brk::Heading(1));
    }
    if (StyleId_Heading1.0..=StyleId_Heading9.0).contains(&style) {
        return Some(Brk::Heading((style - StyleId_Heading1.0 + 1).clamp(1, 6) as u8));
    }
    if id == StyleId_BulletedList || id == StyleId_NumberedList {
        return Some(Brk::ListItem);
    }
    if id == StyleId_Quote {
        return Some(Brk::Quote);
    }
    None
}

/// True when the element's text (`name`) starts with this token: the token is the
/// element's first word, not a word in its middle. An embedded object (U+FFFC) is an
/// element of its own.
pub(super) fn leaf_starts_with(name: &str, tok: &str) -> bool {
    let t = tok.trim();
    if t.is_empty() {
        return false;
    }
    t == "\u{FFFC}" || name.trim_start().starts_with(t)
}

/// Extra time allowed for structure reads in a batch of `n` tokens (spec §5: 150 ms
/// per 400-token batch).
pub(super) fn budget(n: usize) -> Duration {
    Duration::from_micros((n as u64).saturating_mul(150_000) / 400)
}

/// Max ancestors walked above the enclosing element.
const MAX_UP: usize = 4;

/// Structure reads for one UIA read: the mode plus the COM helpers they use.
#[derive(Default)]
pub(super) struct Reader {
    pub mode: Mode,
    walker: Option<IUIAutomationTreeWalker>,
    cache: Option<IUIAutomationCacheRequest>,
    /// UIA's "not supported" attribute value (Style mode).
    not_supported: Option<windows::core::IUnknown>,
}

impl Reader {
    /// A reader for a read from `uia`: element walk on Chromium, StyleId elsewhere.
    pub fn new(uia: &IUIAutomation, chromium: bool) -> Reader {
        if !chromium {
            return Reader {
                mode: Mode::Style,
                not_supported: unsafe { uia.ReservedNotSupportedValue() }.ok(),
                ..Default::default()
            };
        }
        let made = (|| -> windows::core::Result<(IUIAutomationTreeWalker, IUIAutomationCacheRequest)> {
            let walker = unsafe { uia.RawViewWalker() }?;
            let cr = unsafe { uia.CreateCacheRequest() }?;
            for p in [
                UIA_ControlTypePropertyId,
                UIA_AriaRolePropertyId,
                UIA_LocalizedControlTypePropertyId,
                UIA_HeadingLevelPropertyId,
                UIA_NamePropertyId,
            ] {
                unsafe { cr.AddProperty(p) }?;
            }
            Ok((walker, cr))
        })();
        match made {
            Ok((w, c)) => Reader { mode: Mode::Elements, walker: Some(w), cache: Some(c), not_supported: None },
            Err(e) => {
                log::debug!("read-aloud: structure reads off: {e}");
                Reader::default()
            }
        }
    }

    /// The block kind starting at word range `r` (text `tok`), already known to be a
    /// block-start candidate. `None` when unknown or on any UIA error.
    pub fn brk_at(&mut self, r: &IUIAutomationTextRange, tok: &str) -> Option<Brk> {
        match self.mode {
            Mode::Off => None,
            Mode::Style => {
                let v = unsafe { r.GetAttributeValue(UIA_StyleIdAttributeId) }.ok()?;
                if let Ok(style) = i32::try_from(&v) {
                    return style_brk(style);
                }
                // not supported (Notepad): stop paying for it this read; mixed: unknown
                let unk = windows::core::IUnknown::try_from(&v).ok();
                if unk.is_some() && unk == self.not_supported {
                    self.mode = Mode::Off;
                }
                None
            }
            Mode::Elements => self.walk(r, tok),
        }
    }

    /// Chromium: from the token's enclosing element up to the first block ancestor;
    /// every step must be its parent's first child (the token starts that block).
    fn walk(&self, r: &IUIAutomationTextRange, tok: &str) -> Option<Brk> {
        let (walker, cr) = (self.walker.as_ref()?, self.cache.as_ref()?);
        let r3 = r.cast::<IUIAutomationTextRange3>().ok()?;
        let leaf = unsafe { r3.GetEnclosingElementBuildCache(cr) }.ok()?;
        let name = unsafe { leaf.CachedName() }.map(|b| b.to_string()).unwrap_or_default();
        if !leaf_starts_with(&name, tok) {
            return None;
        }
        let mut el = leaf;
        let mut info = cached_info(&el);
        // the elements below `el` on the way up; each must be its parent's first child,
        // checked only once a block kind is found (plain paragraphs cost no checks)
        let mut below: Vec<IUIAutomationElement> = Vec::new();
        let mut below_cts: Vec<i32> = Vec::new();
        let mut child_info = ElInfo::default();
        for _ in 0..=MAX_UP {
            let step = classify(&info, &child_info);
            let first_of = |e: &IUIAutomationElement| first_child(walker, e);
            let starts_here = || below.iter().all(first_of);
            match step {
                // a container: the token starts the plain block (<p>, <div>) below it
                // when it starts each element below that block
                Step::Stop => {
                    let (b, top) = plain_block(&below_cts)?;
                    return below.get(..top)?.iter().all(first_of).then_some(b);
                }
                Step::Mark(b) => return starts_here().then_some(b),
                Step::Block(b) => return (starts_here() && first_of(&el)).then_some(b),
                Step::Cell => {
                    if !starts_here() {
                        return None;
                    }
                    return Some(if first_of(&el) { Brk::TableRow } else { Brk::TableCell });
                }
                Step::Up => {}
            }
            let parent = unsafe { walker.GetParentElementBuildCache(&el, cr) }.ok()?;
            let pinfo = cached_info(&parent);
            below.push(el);
            below_cts.push(info.ct);
            el = parent;
            child_info = std::mem::replace(&mut info, pinfo);
        }
        None
    }
}

/// The cached structure properties of `el`.
fn cached_info(el: &IUIAutomationElement) -> ElInfo {
    let heading = unsafe { el.GetCachedPropertyValue(UIA_HeadingLevelPropertyId) }
        .ok()
        .and_then(|v| i32::try_from(&v).ok())
        .unwrap_or(0);
    ElInfo {
        ct: unsafe { el.CachedControlType() }.map(|c| c.0).unwrap_or(0),
        role: unsafe { el.CachedAriaRole() }.map(|b| b.to_string().to_lowercase()).unwrap_or_default(),
        lct: unsafe { el.CachedLocalizedControlType() }
            .map(|b| b.to_string().to_lowercase())
            .unwrap_or_default(),
        heading,
    }
}

/// True when `el` has no previous sibling (a failed query counts as none: the other
/// checks still have to pass).
fn first_child(walker: &IUIAutomationTreeWalker, el: &IUIAutomationElement) -> bool {
    unsafe { walker.GetPreviousSiblingElement(el) }.is_err()
}

/// Structure reads for one batch: candidates only, and none once the batch has spent
/// its budget (`budget(n)`).
pub(super) struct Batch {
    spent: Duration,
    limit: Duration,
    /// Previous token's text (`None` before the read's first token).
    prev: Option<String>,
    /// How many tokens got a marker / how many candidates were checked.
    pub marked: usize,
    pub checked: usize,
    pub over: bool,
}

impl Batch {
    /// A batch of `n` tokens after the token `prev` (`None`: the read's first token).
    pub fn new(n: usize, prev: Option<String>) -> Batch {
        Batch { spent: Duration::ZERO, limit: budget(n), prev, marked: 0, checked: 0, over: false }
    }

    /// `tok` with its structure marker when it starts a known block. Remembers `tok`
    /// as the previous token.
    pub fn token(&mut self, reader: &mut Reader, r: &IUIAutomationTextRange, tok: String) -> String {
        let candidate = reader.mode != Mode::Off
            && block_start(self.prev.as_deref(), &tok, reader.mode == Mode::Elements);
        let mut out = None;
        if candidate && !self.over {
            let t = Instant::now();
            let b = reader.brk_at(r, &tok);
            self.spent += t.elapsed();
            self.checked += 1;
            if self.spent >= self.limit {
                self.over = true;
            }
            if let Some(b) = b {
                self.marked += 1;
                out = Some(format!("{}{tok}", crate::ra_struct::mark(b)));
            }
        }
        let res = out.unwrap_or_else(|| tok.clone());
        self.prev = Some(tok);
        res
    }

    /// Time spent on structure reads so far.
    pub fn spent(&self) -> Duration {
        self.spent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(ct: UIA_CONTROLTYPE_ID, role: &str) -> ElInfo {
        ElInfo { ct: ct.0, role: role.into(), ..Default::default() }
    }

    fn text_leaf() -> ElInfo {
        el(UIA_TextControlTypeId, "description")
    }

    #[test]
    fn block_starts_are_the_first_token_line_breaks_and_glued_openers() {
        assert!(block_start(None, "Clicked ", false));
        assert!(block_start(Some("end.\r\n"), "Next", false));
        assert!(block_start(Some("end\n"), "Next", true));
        assert!(!block_start(Some("the "), "river", true), "running text");
        assert!(!block_start(Some("held"), "Lead ", false), "glued rule off (Word, Notepad)");
        // Chromium: a block ends with a token without a trailing space
        assert!(block_start(Some("held"), "Lead ", true));
        assert!(block_start(Some("."), "• ", true));
        assert!(block_start(Some("."), "1", true));
        assert!(block_start(Some("Year"), "Inspections", true));
        assert!(block_start(Some("."), "\u{FFFC}", true));
        assert!(block_start(Some("."), "“We ", true));
        // glued punctuation is never a block start
        assert!(!block_start(Some("decade"), ", ", true));
        assert!(!block_start(Some("spread"), ".", true));
        assert!(!block_start(Some("1"), ". ", true));
        assert!(!block_start(Some(""), "x", true), "an empty token says nothing");
    }

    #[test]
    fn headings_map_to_their_level() {
        let mut h = el(UIA_TextControlTypeId, "heading");
        h.heading = HeadingLevel2.0;
        assert_eq!(classify(&h, &text_leaf()), Step::Mark(Brk::Heading(2)));
        h.heading = HeadingLevel9.0;
        assert_eq!(classify(&h, &text_leaf()), Step::Mark(Brk::Heading(6)), "clamped to 6");
        h.heading = HeadingLevel_None.0;
        assert_eq!(classify(&h, &text_leaf()), Step::Mark(Brk::Heading(0)), "role only");
        h.heading = 0;
        assert_eq!(classify(&h, &text_leaf()), Step::Mark(Brk::Heading(0)));
    }

    #[test]
    fn edge_block_elements_map_to_markers() {
        let leaf = text_leaf();
        assert_eq!(classify(&el(UIA_ListItemControlTypeId, "listitem"), &leaf), Step::Mark(Brk::ListItem));
        assert_eq!(classify(&el(UIA_ListItemControlTypeId, ""), &leaf), Step::Mark(Brk::ListItem));
        assert_eq!(classify(&el(UIA_ListItemControlTypeId, "option"), &leaf), Step::Up, "a listbox option");
        for r in ["gridcell", "cell", "columnheader", "rowheader"] {
            assert_eq!(classify(&el(UIA_DataItemControlTypeId, r), &leaf), Step::Cell, "{r}");
        }
        let mut bq = el(UIA_GroupControlTypeId, "group");
        bq.lct = "blockquote".into();
        assert_eq!(classify(&bq, &el(UIA_GroupControlTypeId, "group")), Step::Mark(Brk::Quote));
        assert_eq!(classify(&el(UIA_GroupControlTypeId, "blockquote"), &leaf), Step::Mark(Brk::Quote));
        assert_eq!(classify(&el(UIA_TextControlTypeId, "code"), &leaf), Step::Block(Brk::Code));
        // figcaption (Edge: a Group with role "description"); table caption
        assert_eq!(classify(&el(UIA_GroupControlTypeId, "description"), &leaf), Step::Mark(Brk::Caption));
        assert_eq!(classify(&el(UIA_TextControlTypeId, "description"), &leaf), Step::Up, "a text leaf");
        assert_eq!(classify(&el(UIA_TextControlTypeId, "caption"), &leaf), Step::Mark(Brk::Caption));
        let table = el(UIA_TableControlTypeId, "grid");
        assert_eq!(classify(&table, &el(UIA_TextControlTypeId, "description")), Step::Mark(Brk::Caption));
        assert_eq!(classify(&table, &el(UIA_DataItemControlTypeId, "row")), Step::Stop);
    }

    #[test]
    fn plain_and_container_elements_walk_on_or_stop() {
        let leaf = text_leaf();
        assert_eq!(classify(&el(UIA_GroupControlTypeId, "group"), &leaf), Step::Up, "a <p> or <div>");
        assert_eq!(classify(&el(UIA_HyperlinkControlTypeId, "link"), &leaf), Step::Up);
        assert_eq!(classify(&el(UIA_DocumentControlTypeId, "document"), &leaf), Step::Stop);
        for r in ["article", "main", "navigation", "banner", "contentinfo", "complementary", "dialog", "list", "row", "figure"] {
            assert_eq!(classify(&el(UIA_GroupControlTypeId, r), &leaf), Step::Stop, "{r}");
        }
        assert_eq!(classify(&el(UIA_ListControlTypeId, ""), &leaf), Step::Stop);
        assert_eq!(classify(&ElInfo::default(), &ElInfo::default()), Step::Up, "unknown: look higher");
    }

    #[test]
    fn a_generic_block_under_a_container_is_a_paragraph() {
        let (text, group, link) = (UIA_TextControlTypeId.0, UIA_GroupControlTypeId.0, UIA_HyperlinkControlTypeId.0);
        assert_eq!(plain_block(&[text, group]), Some((Brk::Paragraph, 1)), "<p> in <article>");
        assert_eq!(plain_block(&[link, group]), Some((Brk::Paragraph, 1)), "a <p> starting with a link");
        assert_eq!(plain_block(&[text, group, group]), Some((Brk::Paragraph, 2)), "<div><p>: the outer block");
        assert_eq!(plain_block(&[link]), None, "a nav link right under the landmark");
        assert_eq!(plain_block(&[text, UIA_ButtonControlTypeId.0]), None, "a button");
        assert_eq!(plain_block(&[]), None);
    }

    #[test]
    fn word_styles_map_to_markers() {
        assert_eq!(style_brk(StyleId_Heading1.0), Some(Brk::Heading(1)));
        assert_eq!(style_brk(StyleId_Heading3.0), Some(Brk::Heading(3)));
        assert_eq!(style_brk(StyleId_Heading9.0), Some(Brk::Heading(6)));
        assert_eq!(style_brk(StyleId_Title.0), Some(Brk::Heading(1)));
        assert_eq!(style_brk(StyleId_BulletedList.0), Some(Brk::ListItem));
        assert_eq!(style_brk(StyleId_NumberedList.0), Some(Brk::ListItem));
        assert_eq!(style_brk(StyleId_Quote.0), Some(Brk::Quote));
        assert_eq!(style_brk(StyleId_Normal.0), None);
        assert_eq!(style_brk(StyleId_Custom.0), None);
        assert_eq!(style_brk(0), None);
        assert_eq!(style_brk(i32::MIN), None);
    }

    #[test]
    fn only_an_elements_first_word_starts_it() {
        assert!(leaf_starts_with("Bullet one about", "Bullet "));
        assert!(leaf_starts_with("1. ", "1"));
        assert!(leaf_starts_with("  Lead paragraph", "Lead "));
        assert!(!leaf_starts_with("Bullet costs $3.2B", "3"), "a glued piece mid-element");
        assert!(!leaf_starts_with("load = rated * 2.0\nfactor", "factor "));
        assert!(leaf_starts_with("A cracked arch", "\u{FFFC}"), "embedded object");
        assert!(!leaf_starts_with("", "x"));
        assert!(!leaf_starts_with("x", "  "));
    }

    #[test]
    fn the_budget_is_150_ms_per_400_tokens() {
        assert_eq!(budget(400), Duration::from_millis(150));
        assert_eq!(budget(200), Duration::from_millis(75));
        assert_eq!(budget(0), Duration::ZERO);
        let _ = budget(usize::MAX); // saturates, no panic
    }
}

#[cfg(test)]
mod with_ra_text {
    use crate::ra_struct::{mark, Brk};
    use crate::ra_text::normalize;

    /// Edge-shaped tokens (no line breaks; a block ends with a token without a
    /// trailing space) keep one word per source token and carry the markers.
    #[test]
    fn edge_tokens_with_markers_normalize() {
        let raw: Vec<String> = [
            format!("{}Headline ", mark(Brk::Heading(1))),
            "held".into(),
            "Lead ".into(),
            "text".into(),
            ".".into(),
            format!("{}• ", mark(Brk::ListItem)),
            "Bullet ".into(),
            "one".into(),
            ".".into(),
            format!("{}1", mark(Brk::ListItem)),
            ". ".into(),
            "First".into(),
            format!("{}Year", mark(Brk::TableRow)),
            format!("{}Count", mark(Brk::TableCell)),
        ]
        .to_vec();
        let words = normalize(&raw);
        for w in &words {
            println!("{:?} {:?} {:?}", w.text, w.src, w.brk);
        }
        let brk_of = |t: &str| words.iter().find(|w| w.text.contains(t)).map(|w| w.brk);
        assert_eq!(brk_of("Headline"), Some(Brk::Heading(1)));
        assert_eq!(brk_of("Year"), Some(Brk::TableRow));
        assert_eq!(brk_of("Count"), Some(Brk::TableCell));
        let srcs: Vec<usize> = words.iter().flat_map(|w| w.src.clone()).collect();
        let mut sorted = srcs.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(srcs.len(), sorted.len(), "no source token in two words");
    }
}
