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

/// True when the element's text (`name`) starts with this token's text: a cheap
/// local pre-test only (`1` matches "1.1" twice); the caller then checks the
/// token's position. An embedded object (U+FFFC) is an element of its own.
pub(super) fn leaf_starts_with(name: &str, tok: &str) -> bool {
    let t = tok.trim();
    if t.is_empty() {
        return false;
    }
    t == "\u{FFFC}" || name.trim_start().starts_with(t)
}

/// Whether the token's position must be read to know it starts its enclosing element
/// (`leaf_starts_with` already passed). A text leaf's Name is its text: when the
/// previous token (glued to this one) followed by this token does not occur in it,
/// the token cannot be in its middle, so the two position reads are saved. Other
/// leaves (a link's Name may be an aria-label) and the read's first token are always
/// checked.
pub(super) fn needs_position_check(text_leaf: bool, name: &str, prev: Option<&str>, tok: &str) -> bool {
    let Some(prev) = prev else {
        return true;
    };
    // a line break in the text is not in the tokens (`<pre>` lines): always check
    !text_leaf || name.contains(['\r', '\n']) || name.contains(&format!("{}{}", prev.trim_start(), tok.trim()))
}

/// Extra time allowed for structure reads in a batch of `n` tokens (spec §5: 150 ms
/// per 400-token batch).
pub(super) fn budget(n: usize) -> Duration {
    Duration::from_micros((n as u64).saturating_mul(150_000) / 400)
}

/// Max ancestors walked above the enclosing element (news sites nest `<p>` 6-8 deep).
const MAX_UP: usize = 12;

/// What the structure walk asks of the element tree (UIA live, a fake in tests).
pub(super) trait Tree {
    type El;
    /// The parent of `el` and its properties (`None`: no parent, or an error).
    fn parent(&mut self, el: &Self::El) -> Option<(Self::El, ElInfo)>;
    /// `el`'s previous sibling: `Some(None)` when `el` is the first child,
    /// `Some(Some(info))` when it has one, `None` on an error (no marker: never guess).
    fn prev_sibling(&mut self, el: &Self::El) -> Option<Option<ElInfo>>;
    /// The batch's structure budget is spent: stop before the next read.
    fn out_of_time(&self) -> bool;
}

/// True for an element that is laid out as a block (it ends the text before the next
/// sibling): generic groups, headings, lists, tables, separators. Text leaves, links
/// and images are inline.
pub(super) fn is_block(el: &ElInfo) -> bool {
    [
        UIA_GroupControlTypeId.0,
        UIA_ListControlTypeId.0,
        UIA_TableControlTypeId.0,
        UIA_DataGridControlTypeId.0,
        UIA_SeparatorControlTypeId.0,
    ]
    .contains(&el.ct)
        || (HeadingLevel1.0..=HeadingLevel9.0).contains(&el.heading)
        || matches!(el.role.as_str(), "heading" | "list" | "table" | "grid" | "figure" | "blockquote" | "separator")
}

/// The block kind that starts at a token, walking up from its enclosing element
/// `leaf` (already known to start at the token). Each step up needs the element to be
/// its parent's first child (then the token starts the parent too). The first block
/// kind found wins. If the walk leaves the elements the token starts before finding
/// one, and it passed a generic block (`Group`: Chromium's `<p>`, `<div>`,
/// `<section>`), the token starts a `Paragraph` (Chromium word ranges carry no line
/// breaks, so the raw-text rules cannot see paragraphs), but only when the text before
/// it is a block of its own: Chromium also shows inline pieces (`<span title>`, a
/// focusable span, an inline-block div) as `Group`, and those sit after a text leaf.
/// Errors and an empty budget give no marker.
pub(super) fn walk<T: Tree>(t: &mut T, leaf: T::El, leaf_info: ElInfo) -> Option<Brk> {
    let group = UIA_GroupControlTypeId.0;
    let mut para = leaf_info.ct == group;
    let (mut el, mut info) = (leaf, leaf_info);
    // the enclosing element itself (Chromium: almost always a text leaf)
    match classify(&info, &ElInfo::default()) {
        Step::Mark(b) => return Some(b),
        Step::Stop => return None,
        Step::Block(b) => return first(t, &el)?.then_some(b),
        Step::Cell => return Some(if first(t, &el)? { Brk::TableRow } else { Brk::TableCell }),
        Step::Up => {}
    }
    for _ in 0..MAX_UP {
        if t.out_of_time() {
            return None;
        }
        let Some((parent, pinfo)) = t.parent(&el) else {
            return paragraph(t, &el, para);
        };
        let step = classify(&pinfo, &info);
        if step == Step::Stop {
            return paragraph(t, &el, para);
        }
        if t.out_of_time() {
            return None;
        }
        if let Some(prev) = t.prev_sibling(&el)? {
            // the token starts `el` but not its parent: nothing above starts here
            return (para && is_block(&prev)).then_some(Brk::Paragraph);
        }
        match step {
            Step::Mark(b) => return Some(b),
            // not a first child (inline `code` after text): judge by what precedes the element itself
            Step::Block(b) => return if first(t, &parent)? { Some(b) } else { paragraph(t, &parent, para) },
            Step::Cell => return Some(if first(t, &parent)? { Brk::TableRow } else { Brk::TableCell }),
            Step::Up | Step::Stop => {}
        }
        para |= pinfo.ct == group;
        el = parent;
        info = pinfo;
    }
    // deeper than MAX_UP: the token still starts every block passed
    paragraph(t, &el, para)
}

/// `Some(true)` when `el` is its parent's first child (`None` on an error or when out
/// of time).
fn first<T: Tree>(t: &mut T, el: &T::El) -> Option<bool> {
    if t.out_of_time() {
        return None;
    }
    t.prev_sibling(el).map(|p| p.is_none())
}

/// The walk ends at `el`, the outermost element the token starts: a `Paragraph` when
/// it passed a generic block and the element before `el` (if any) is a block.
fn paragraph<T: Tree>(t: &mut T, el: &T::El, para: bool) -> Option<Brk> {
    if !para || t.out_of_time() {
        return None;
    }
    match t.prev_sibling(el)? {
        None => Some(Brk::Paragraph),
        Some(prev) => is_block(&prev).then_some(Brk::Paragraph),
    }
}

/// The live element tree: cached parent and sibling reads.
struct UiaTree<'a> {
    walker: &'a IUIAutomationTreeWalker,
    cache: &'a IUIAutomationCacheRequest,
    deadline: Instant,
}

impl Tree for UiaTree<'_> {
    type El = IUIAutomationElement;

    fn parent(&mut self, el: &IUIAutomationElement) -> Option<(IUIAutomationElement, ElInfo)> {
        let p = unsafe { self.walker.GetParentElementBuildCache(el, self.cache) }.ok()?;
        let info = cached_info(&p);
        Some((p, info))
    }

    fn prev_sibling(&mut self, el: &IUIAutomationElement) -> Option<Option<ElInfo>> {
        match unsafe { self.walker.GetPreviousSiblingElementBuildCache(el, self.cache) } {
            Ok(p) => Some(Some(cached_info(&p))),
            // windows-rs reports a null result ("no sibling") as an error with S_OK
            Err(e) if e.code().is_ok() => Some(None),
            Err(_) => None,
        }
    }

    fn out_of_time(&self) -> bool {
        Instant::now() >= self.deadline
    }
}

/// Structure reads for one UIA read: the mode plus the COM helpers they use.
#[derive(Default)]
pub(super) struct Reader {
    pub mode: Mode,
    walker: Option<IUIAutomationTreeWalker>,
    cache: Option<IUIAutomationCacheRequest>,
    /// The read's TextPattern: the range of an element, to check it starts at a token.
    pattern: Option<IUIAutomationTextPattern>,
    /// UIA's "not supported" attribute value (Style mode).
    not_supported: Option<windows::core::IUnknown>,
}

impl Reader {
    /// A reader for a read from `uia` in TextPattern `pattern`: element walk on
    /// Chromium (needs the pattern), StyleId elsewhere.
    pub fn new(uia: &IUIAutomation, chromium: bool, pattern: Option<IUIAutomationTextPattern>) -> Reader {
        if !chromium {
            return Reader {
                mode: Mode::Style,
                not_supported: unsafe { uia.ReservedNotSupportedValue() }.ok(),
                ..Default::default()
            };
        }
        let Some(pattern) = pattern else {
            return Reader::default();
        };
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
            Ok((w, c)) => Reader {
                mode: Mode::Elements,
                walker: Some(w),
                cache: Some(c),
                pattern: Some(pattern),
                not_supported: None,
            },
            Err(e) => {
                log::debug!("read-aloud: structure reads off: {e}");
                Reader::default()
            }
        }
    }

    /// The block kind starting at word range `r` (text `tok`), already known to be a
    /// block-start candidate. `None` when unknown, on any UIA error, or when `deadline`
    /// passes (checked before each read).
    pub fn brk_at(
        &mut self,
        r: &IUIAutomationTextRange,
        tok: &str,
        prev: Option<&str>,
        deadline: Instant,
    ) -> Option<Brk> {
        if Instant::now() >= deadline {
            return None;
        }
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
            Mode::Elements => self.elements(r, tok, prev, deadline),
        }
    }

    /// Chromium: the token's enclosing element must start exactly at the token (its
    /// position, not just its text: "1.1" is `1` `.` `1` in one element), then `walk`.
    fn elements(&self, r: &IUIAutomationTextRange, tok: &str, prev: Option<&str>, deadline: Instant) -> Option<Brk> {
        let (walker, cache, pattern) = (self.walker.as_ref()?, self.cache.as_ref()?, self.pattern.as_ref()?);
        let r3 = r.cast::<IUIAutomationTextRange3>().ok()?;
        let leaf = unsafe { r3.GetEnclosingElementBuildCache(cache) }.ok()?;
        let name = unsafe { leaf.CachedName() }.map(|b| b.to_string()).unwrap_or_default();
        // cheap text test first (no cross-process call), then the position
        if !leaf_starts_with(&name, tok) || Instant::now() >= deadline {
            return None;
        }
        let info = cached_info(&leaf);
        if needs_position_check(info.ct == UIA_TextControlTypeId.0, &name, prev, tok) {
            let lr = unsafe { pattern.RangeFromChild(&leaf) }.ok()?;
            if Instant::now() >= deadline {
                return None;
            }
            let at = unsafe { lr.CompareEndpoints(TextPatternRangeEndpoint_Start, r, TextPatternRangeEndpoint_Start) }
                .ok()?;
            if at != 0 {
                return None;
            }
        }
        walk(&mut UiaTree { walker, cache, deadline }, leaf, info)
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

/// Structure reads for one batch: candidates only, and none once the batch has spent
/// its budget (`budget(n)`; a read never starts past it).
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

    /// A batch with no budget (manual tests: what the reads find, cost aside).
    #[cfg(test)]
    pub fn unlimited(prev: Option<String>) -> Batch {
        Batch { limit: Duration::from_secs(3600), ..Batch::new(0, prev) }
    }

    /// Counts `d` (a structure-only read outside `token`) against the budget.
    pub fn charge(&mut self, d: Duration) {
        self.spent += d;
        self.over |= self.spent >= self.limit;
    }

    /// `tok` with its structure marker when it starts a known block. Remembers `tok`
    /// as the previous token.
    pub fn token(&mut self, reader: &mut Reader, r: &IUIAutomationTextRange, tok: String) -> String {
        let candidate = reader.mode != Mode::Off
            && block_start(self.prev.as_deref(), &tok, reader.mode == Mode::Elements);
        let mut out = None;
        if candidate && !self.over {
            let t = Instant::now();
            let deadline = t + self.limit.saturating_sub(self.spent);
            let b = reader.brk_at(r, &tok, self.prev.as_deref(), deadline);
            self.charge(t.elapsed());
            self.checked += 1;
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

    /// A fake element tree: node i = (parent, properties, is the first child, the
    /// previous sibling's properties); `None` first = the sibling query fails.
    /// `reads` counts tree reads, and the budget runs out after `budget` reads.
    struct Fake {
        nodes: Vec<(Option<usize>, ElInfo, Option<bool>, ElInfo)>,
        reads: usize,
        budget: usize,
    }

    impl Fake {
        fn new() -> Fake {
            Fake { nodes: Vec::new(), reads: 0, budget: usize::MAX }
        }
        /// Adds a node under `parent`; `first` = no previous sibling (None: query fails).
        fn add(&mut self, parent: Option<usize>, info: ElInfo, first: Option<bool>) -> usize {
            self.nodes.push((parent, info, first, group()));
            self.nodes.len() - 1
        }
        /// Adds a node right after a text leaf (inline content).
        fn add_after_text(&mut self, parent: usize, info: ElInfo) -> usize {
            self.nodes.push((Some(parent), info, Some(false), text_leaf()));
            self.nodes.len() - 1
        }
        fn run(&mut self, leaf: usize) -> Option<Brk> {
            let info = self.nodes[leaf].1.clone();
            walk(self, leaf, info)
        }
    }

    impl Tree for Fake {
        type El = usize;
        fn parent(&mut self, el: &usize) -> Option<(usize, ElInfo)> {
            self.reads += 1;
            let p = self.nodes.get(*el)?.0?;
            Some((p, self.nodes.get(p)?.1.clone()))
        }
        fn prev_sibling(&mut self, el: &usize) -> Option<Option<ElInfo>> {
            self.reads += 1;
            let n = self.nodes.get(*el)?;
            Some(if n.2? { None } else { Some(n.3.clone()) })
        }
        fn out_of_time(&self) -> bool {
            self.reads >= self.budget
        }
    }

    fn group() -> ElInfo {
        el(UIA_GroupControlTypeId, "group")
    }

    /// document > article > div > div > section(group) > div > [p, p], each p with a
    /// text leaf; returns (tree, first p's leaf, second p's leaf).
    fn news_page() -> (Fake, usize, usize) {
        let mut t = Fake::new();
        let doc = t.add(None, el(UIA_DocumentControlTypeId, "document"), Some(true));
        let art = t.add(Some(doc), el(UIA_GroupControlTypeId, "article"), Some(false));
        let d1 = t.add(Some(art), group(), Some(false));
        let d2 = t.add(Some(d1), group(), Some(true));
        let sec = t.add(Some(d2), group(), Some(true));
        let d3 = t.add(Some(sec), group(), Some(true));
        let p1 = t.add(Some(d3), group(), Some(true));
        let l1 = t.add(Some(p1), text_leaf(), Some(true));
        let p2 = t.add(Some(d3), group(), Some(false));
        let l2 = t.add(Some(p2), text_leaf(), Some(true));
        (t, l1, l2)
    }

    #[test]
    fn paragraphs_deep_in_news_site_wrappers_are_marked() {
        let (mut t, l1, l2) = news_page();
        assert_eq!(t.run(l1), Some(Brk::Paragraph), "first <p>: walks up to the article");
        let deep = t.reads;
        t.reads = 0;
        assert_eq!(t.run(l2), Some(Brk::Paragraph), "a later <p> in the same wrapper");
        assert!(t.reads <= 4, "stops at the first parent it does not start: {} reads", t.reads);
        assert!(deep <= 2 * 7, "{deep} reads");
        // deeper than MAX_UP: still a paragraph (the token starts every block passed)
        let mut t = Fake::new();
        let mut at = t.add(None, group(), Some(true));
        for _ in 0..20 {
            at = t.add(Some(at), group(), Some(true));
        }
        let leaf = t.add(Some(at), text_leaf(), Some(true));
        assert_eq!(t.run(leaf), Some(Brk::Paragraph));
    }

    #[test]
    fn headings_lists_and_cells_inside_wrappers() {
        let (mut t, _, _) = news_page();
        let d3 = 5;
        let mut h = el(UIA_TextControlTypeId, "heading");
        h.heading = HeadingLevel2.0;
        let h2 = t.add(Some(d3), h, Some(false));
        let hl = t.add(Some(h2), text_leaf(), Some(true));
        assert_eq!(t.run(hl), Some(Brk::Heading(2)));
        let list = t.add(Some(d3), el(UIA_ListControlTypeId, "list"), Some(false));
        let li1 = t.add(Some(list), el(UIA_ListItemControlTypeId, "listitem"), Some(true));
        let li2 = t.add(Some(list), el(UIA_ListItemControlTypeId, "listitem"), Some(false));
        let b1 = t.add(Some(li1), text_leaf(), Some(true));
        let b2 = t.add(Some(li2), text_leaf(), Some(true));
        assert_eq!(t.run(b1), Some(Brk::ListItem));
        assert_eq!(t.run(b2), Some(Brk::ListItem), "every item, not just the first");
        let row = t.add(Some(d3), el(UIA_DataItemControlTypeId, "row"), Some(false));
        let c1 = t.add(Some(row), el(UIA_DataItemControlTypeId, "gridcell"), Some(true));
        let c2 = t.add(Some(row), el(UIA_DataItemControlTypeId, "gridcell"), Some(false));
        let cp = t.add(Some(c2), group(), Some(true)); // <td><p>
        let t1 = t.add(Some(c1), text_leaf(), Some(true));
        let t2 = t.add(Some(cp), text_leaf(), Some(true));
        assert_eq!(t.run(t1), Some(Brk::TableRow));
        assert_eq!(t.run(t2), Some(Brk::TableCell));
    }

    #[test]
    fn inline_elements_and_failed_queries_give_no_marker() {
        let (mut t, _, _) = news_page();
        let p = 6;
        // `(<a>link</a>)` / `(<code>x</code>)`: the inline element is not a first child
        let a = t.add(Some(p), el(UIA_HyperlinkControlTypeId, "link"), Some(false));
        assert_eq!(t.run(a), None, "a link mid-paragraph");
        let code = t.add(Some(p), el(UIA_TextControlTypeId, "code"), Some(false));
        let cl = t.add(Some(code), text_leaf(), Some(true));
        assert_eq!(t.run(cl), None, "inline code");
        // final review: `text <code><span-group>x</span></code>`: a Group first inside
        // inline code after text is still mid-sentence (the paragraph check looks
        // before the code element, not before the group inside it)
        let code2 = t.add_after_text(p, el(UIA_TextControlTypeId, "code"));
        let g = t.add(Some(code2), group(), Some(true));
        let gl = t.add(Some(g), text_leaf(), Some(true));
        assert_eq!(t.run(gl), None, "a group first inside inline code after text");
        // a nav link right under the landmark, a button in a dialog: no block passed
        let nav = t.add(None, el(UIA_GroupControlTypeId, "navigation"), Some(true));
        let link = t.add(Some(nav), el(UIA_HyperlinkControlTypeId, "link"), Some(false));
        assert_eq!(t.run(link), None);
        // a sibling query that fails is never "first child"
        let mut t = Fake::new();
        let art = t.add(None, el(UIA_GroupControlTypeId, "article"), Some(true));
        let li = t.add(Some(art), el(UIA_ListItemControlTypeId, "listitem"), Some(true));
        let leaf = t.add(Some(li), text_leaf(), None);
        assert_eq!(t.run(leaf), None);
        let p = t.add(Some(art), group(), Some(true));
        let pl = t.add(Some(p), text_leaf(), None);
        assert_eq!(t.run(pl), None, "no paragraph on an error either");
    }

    #[test]
    fn inline_groups_after_text_are_not_paragraphs() {
        let (mut t, _, _) = news_page();
        let p = 6;
        // `text (<span title tabindex>term</span>) more`: Chromium shows the span as a Group
        let span = t.add_after_text(p, group());
        let term = t.add(Some(span), text_leaf(), Some(true));
        assert_eq!(t.run(term), None, "a span mid-sentence");
        // an inline-block div after text
        let ib = t.add_after_text(p, group());
        let ibp = t.add(Some(ib), group(), Some(true));
        let ibl = t.add(Some(ibp), text_leaf(), Some(true));
        assert_eq!(t.run(ibl), None, "an inline-block div mid-sentence");
        // a block after a heading, a list or a separator is a paragraph
        let d3 = 5;
        for prev in [el(UIA_TextControlTypeId, "heading"), el(UIA_ListControlTypeId, "list"), el(UIA_SeparatorControlTypeId, "")] {
            t.nodes.push((Some(d3), group(), Some(false), prev.clone()));
            let blk = t.nodes.len() - 1;
            let l = t.add(Some(blk), text_leaf(), Some(true));
            assert_eq!(t.run(l), Some(Brk::Paragraph), "after {prev:?}");
        }
        // under a container (the walk stops): same rule
        let art = 1;
        let loose = t.add_after_text(art, group());
        let ll = t.add(Some(loose), text_leaf(), Some(true));
        assert_eq!(t.run(ll), None, "an inline group right in the article");
        assert!(is_block(&group()) && !is_block(&text_leaf()) && !is_block(&el(UIA_HyperlinkControlTypeId, "link")));
    }

    #[test]
    fn an_empty_budget_stops_the_walk() {
        let (mut t, l1, _) = news_page();
        t.budget = 3;
        assert_eq!(t.run(l1), None, "out of time before the answer: no marker");
        assert!(t.reads <= 3, "no read after the budget ran out: {}", t.reads);
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
    fn the_position_is_read_only_when_the_text_cannot_rule_it_out() {
        // "I think A.I. helps.": the second `I` follows `.` inside the leaf
        assert!(needs_position_check(true, "I think A.I. helps.", Some("."), "I"));
        assert!(needs_position_check(true, "1.1 Introduction", Some("."), "1 "));
        // "held" ends the heading; ".Lead" / "heldLead" is not in the paragraph
        assert!(!needs_position_check(true, "Lead paragraph a city.", Some("held"), "Lead "));
        assert!(!needs_position_check(true, "Deep para one here.", Some("."), "Deep "));
        assert!(needs_position_check(false, "Linked heading", Some("gamma"), "Linked "), "not a text leaf");
        assert!(needs_position_check(true, "Lead paragraph", None, "Lead "), "the read's first token");
        assert!(needs_position_check(true, "plain line one\nplain line two", Some("one"), "plain "), "pre lines");
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
            // a numbered heading: Edge splits "1.1"; only the first piece is marked
            format!("{}1", mark(Brk::Heading(2))),
            ".".into(),
            "1 ".into(),
            "Introduction".into(),
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
        let word_of = |src: usize| words.iter().find(|w| w.src.contains(&src));
        assert_eq!(word_of(5).map(|w| w.brk), Some(Brk::ListItem), "bullet item");
        assert_eq!(word_of(9).map(|w| w.brk), Some(Brk::ListItem), "numbered item");
        assert_eq!(word_of(6).map(|w| w.brk), Some(Brk::ListItem), "the bullet folds into its word");
        assert_eq!(word_of(7).map(|w| w.brk), Some(Brk::None), "the item text runs on");
        let n = word_of(14).expect("the heading number");
        assert_eq!(n.brk, Brk::Heading(2));
        assert!(n.src.contains(&15) && n.src.contains(&16), "1.1 stays one word: {n:?}");
        assert_eq!(word_of(17).map(|w| w.brk), Some(Brk::None));
        let srcs: Vec<usize> = words.iter().flat_map(|w| w.src.clone()).collect();
        let mut sorted = srcs.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(srcs.len(), sorted.len(), "no source token in two words");
    }
}
