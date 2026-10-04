//! Read Aloud: Adobe Acrobat source (spec 3.3).
//!
//! Acrobat's page view has no usable UIA TextPattern, but its own accessibility DOM
//! (`IPDDomNode`, reached through MSAA + `IServiceProvider::QueryService`) gives every
//! Word node's text, its box in physical screen pixels and `ScrollTo`. COM calls only:
//! nothing here sends input, focus or scroll messages to Acrobat. Every call runs on the
//! `ra-uia` worker thread (COM initialised MTA, per-monitor DPI aware). Never panics.
//!
//! Spike facts this code relies on (2026-10-02, Acrobat Reader 64-bit):
//! - only the *visible* `AVL_AVView` page view (window text `AVPageView`) has a DOM; a
//!   hidden background tab's view does not;
//! - `AccessibleObjectFromWindow` on it answers with a client wrapper (role 10) whose
//!   child is the document (role 15); only the document answers `QueryService`;
//! - the page view belongs to the `--type=renderer` child process;
//! - `GetLocation` is (0,0,0,0) off screen, and a container's box is the union of its
//!   on-screen parts, so an off-screen container has no on-screen descendants.

use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::{interface, IUnknown, IUnknown_Vtbl, Interface, BSTR, GUID, HRESULT, VARIANT};
use windows::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, POINT, RECT};
use windows::Win32::System::Com::{IDispatch, IServiceProvider};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{AccessibleChildren, AccessibleObjectFromWindow, IAccessible};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GetAncestor, GetClassNameW, GetWindowRect, GetWindowTextW,
    GetWindowThreadProcessId, IsWindow, IsWindowVisible, WindowFromPoint, GA_PARENT, GA_ROOT,
    OBJID_CLIENT,
};

use crate::ra_struct::{mark, Brk};
use crate::ra_uia::Rect;

/// Error text when the point is not on an Acrobat word (the caller may try OCR next).
pub const NO_TEXT_ACRO: &str = "no Acrobat text under the point";
/// Error text when `pick` ran out of time before it found the clicked word. Not "no
/// text": the caller must not fall through to OCR (Acrobat is just slow right now).
pub const ACRO_TIMEOUT: &str = "Acrobat lookup timed out";
/// Error text prefix when the DOM could not be reached (the caller may try OCR next).
pub const NO_DOM: &str = "Acrobat DOM unavailable";
/// Tokens returned by `pick`. Smaller than the UIA source's 200: Acrobat costs 1.3-4 ms
/// per word (3 COM calls), so 200 words held speech back by 0.3-0.8 s; 60 still covers
/// the 12-word first chunk and the next one, and `more` fetches the rest while the
/// first chunk is synthesised.
const FIRST_BATCH: usize = 60;
/// Tokens per `more` call (same as the UIA source).
const MORE_BATCH: usize = 400;
/// One `more` call returns after about this long (a short batch), so the worker can
/// draw the highlight between slices: 400 words cost 0.5-1.6 s in Acrobat.
const MORE_SLICE: Duration = Duration::from_millis(100);
/// Words walked per read, in total (seek + reads).
const MAX_WORDS: usize = 20_000;
/// Time budget for one `pick` / `more` call (the caller waits 5 s).
const CALL_BUDGET: Duration = Duration::from_secs(3);
/// Time budget for one `hover` (the caller waits 50 ms).
const HOVER_BUDGET: Duration = Duration::from_millis(50);
/// A page view whose DOM could not be reached is retried for hover at most this often.
const HOVER_RETRY: Duration = Duration::from_secs(1);
/// Window class of Acrobat's views; the page view's window text is `AVPageView`.
const VIEW_CLASS: &str = "AVL_AVView";
const PAGE_VIEW_TEXT: &str = "AVPageView";
/// MSAA `ROLE_SYSTEM_DOCUMENT`.
const ROLE_DOCUMENT: i32 = 15;
/// Max MSAA children looked at when unwrapping the client wrapper.
const MAX_ACC_KIDS: usize = 64;
/// Sanity cap on one DOM node's child count.
const MAX_KIDS: i32 = 100_000;

// ---------------------------------------------------------------------------------
// COM bindings (from Adobe's public AcrobatAccess.idl; vtable order must match it)
// ---------------------------------------------------------------------------------

/// `SID_GetPDDomNode`: the service id that hands out `IGetPDDomNode`.
const SID_GET_PD_DOM_NODE: GUID = GUID::from_u128(0xc0a1d5e9_1142_4cf3_b607_82fc3b96a4df);

/// `CPDDomNode_Word` in the IDL's `CPDDomNodeType` enum (Document = 1, Page = 2,
/// StructElement = 3, Text = 4, Word = 5, Char = 6, ...).
pub(crate) const WORD: i32 = 5;
/// `CPDDomNode_StructElement`: a structure element with children (Document, L, LI, TR).
const STRUCT_ELEMENT: i32 = 3;
/// `CPDDomNode_Text`: a run of words. A leaf structure element (P, H1, LBody, TD) is a
/// Text node; an untagged page is one Text node (named "P").
const TEXT_NODE: i32 = 4;

/// IDL method order of `IPDDomNode` (after IDispatch's 7). `com_bindings_match_the_idl`
/// checks the vtable against it (test-only: the check is the only reader).
#[cfg(test)]
pub(crate) const IPDDOMNODE_METHODS: [&str; 14] = [
    "GetParent",
    "GetType",
    "GetChildCount",
    "GetChild",
    "GetName",
    "GetValue",
    "IsSame",
    "GetTextContent",
    "GetLocation",
    "GetFontInfo",
    "GetFromID",
    "GetIAccessible",
    "ScrollTo",
    "GetTextInLines",
];

#[allow(non_snake_case, non_camel_case_types, clippy::too_many_arguments)]
mod com {
    use super::*;
    use std::ffi::c_void;

    /// `IGetPDDomNode : IUnknown`.
    #[interface("f9f2fe81-f764-4bd0-afa5-5de841ddb625")]
    pub unsafe trait IGetPDDomNode: IUnknown {
        pub fn get_PDDomNode(&self, varid: VARIANT, out: *mut Option<IPDDomNode>) -> HRESULT;
    }

    /// `IPDDomNode : IDispatch`. Declared on IUnknown with IDispatch's 4 slots spelled
    /// out (the same vtable; deriving from IDispatch would need the windows crate's
    /// `implement` feature). All 14 own methods are declared so the offsets are right.
    #[interface("5007373a-20d7-458f-9ffb-abc900e3a831")]
    pub unsafe trait IPDDomNode: IUnknown {
        // IDispatch (never called)
        pub fn GetTypeInfoCount(&self, n: *mut u32) -> HRESULT;
        pub fn GetTypeInfo(&self, i: u32, lcid: u32, out: *mut *mut c_void) -> HRESULT;
        pub fn GetIDsOfNames(
            &self,
            riid: *const GUID,
            names: *const *const u16,
            count: u32,
            lcid: u32,
            ids: *mut i32,
        ) -> HRESULT;
        pub fn Invoke(
            &self,
            id: i32,
            riid: *const GUID,
            lcid: u32,
            flags: u16,
            params: *mut c_void,
            result: *mut VARIANT,
            excep: *mut c_void,
            arg_err: *mut u32,
        ) -> HRESULT;
        // IPDDomNode
        pub fn GetParent(&self, out: *mut Option<IPDDomNode>) -> HRESULT;
        pub fn GetType(&self, out: *mut i32) -> HRESULT;
        pub fn GetChildCount(&self, out: *mut i32) -> HRESULT;
        pub fn GetChild(&self, index: i32, out: *mut Option<IPDDomNode>) -> HRESULT;
        pub fn GetName(&self, out: *mut BSTR) -> HRESULT;
        pub fn GetValue(&self, out: *mut BSTR) -> HRESULT;
        pub fn IsSame(&self, node: *mut c_void, out: *mut BOOL) -> HRESULT;
        pub fn GetTextContent(&self, out: *mut BSTR) -> HRESULT;
        pub fn GetLocation(&self, x: *mut i32, y: *mut i32, w: *mut i32, h: *mut i32) -> HRESULT;
        pub fn GetFontInfo(
            &self,
            status: *mut i32,
            name: *mut BSTR,
            size: *mut f32,
            flags: *mut i32,
            red: *mut f32,
            green: *mut f32,
            blue: *mut f32,
        ) -> HRESULT;
        pub fn GetFromID(&self, id: *mut c_void, out: *mut Option<IPDDomNode>) -> HRESULT;
        pub fn GetIAccessible(&self, out: *mut Option<IAccessible>) -> HRESULT;
        pub fn ScrollTo(&self) -> HRESULT;
        pub fn GetTextInLines(&self, visible_only: BOOL, out: *mut Option<IPDDomNode>) -> HRESULT;
    }

    /// `IPDDomElement : IPDDomNode` (a structure element; from the type library in
    /// Acrobat's `plug_ins\Accessibility.api`): the 14 `IPDDomNode` methods, then these.
    #[interface("198f17ae-b921-4308-9543-288d426a5c2b")]
    pub unsafe trait IPDDomElement: IPDDomNode {
        pub fn GetTagName(&self, out: *mut BSTR) -> HRESULT;
        pub fn GetStdName(&self, out: *mut BSTR) -> HRESULT;
        pub fn GetID(&self, out: *mut BSTR) -> HRESULT;
        pub fn GetAttribute(&self, attr: *const u16, owner: *const u16, out: *mut BSTR) -> HRESULT;
    }

    /// `IPDDomNodeExt : IUnknown`. We only call `GetPageNum` (manual test); never
    /// `SetFocus` / `DoDefaultAction` (that would act on the user's document).
    #[interface("4A894040-247E-4aff-BB08-3489E9905235")]
    pub unsafe trait IPDDomNodeExt: IUnknown {
        pub fn GetState(&self, out: *mut i32) -> HRESULT;
        pub fn Navigate(&self, dir: i32, out: *mut Option<IPDDomNode>) -> HRESULT;
        pub fn SetFocus(&self) -> HRESULT;
        pub fn GetIndex(&self, out: *mut i32) -> HRESULT;
        pub fn GetPageNum(&self, first: *mut i32, last: *mut i32) -> HRESULT;
        pub fn DoDefaultAction(&self) -> HRESULT;
        pub fn ScrollToEx(&self, favor_left: BOOL, favor_top: BOOL) -> HRESULT;
        pub fn Relationship(&self, node: *mut c_void, out: *mut i32) -> HRESULT;
    }
}
use com::{IGetPDDomNode, IPDDomNode};

// ---------------------------------------------------------------------------------
// Pure logic: the DOM walk over a node trait (unit-tested with a fake tree)
// ---------------------------------------------------------------------------------

/// The node operations the walk needs. Children are fetched one at a time: a DOM call
/// costs ~1-2 ms across processes, and an untagged page is one Text node holding every
/// word of the page, so fetching a node's children all at once is too slow for hover.
pub(crate) trait Node: Sized {
    fn kind(&self) -> i32;
    fn child_count(&self) -> usize;
    /// `None` on an error (that child is skipped).
    fn child(&self, i: usize) -> Option<Self>;
    fn text(&self) -> String;
    /// A structure element's type ("P", "H1", "LI", ...: `IPDDomElement::GetStdName`,
    /// not `GetName`); "" otherwise or on an error.
    fn name(&self) -> String;
    /// A Text node's whole text, lines separated by "\r\n" (`GetTextContent`); "" on an
    /// error.
    fn content(&self) -> String;
    /// `None` = off screen ((0,0,0,0) or an empty box) or an error.
    fn location(&self) -> Option<Rect>;
}

/// One open container on the walk: the next child to fetch.
#[derive(Clone)]
struct Level<N> {
    node: N,
    next: usize,
    count: usize,
    kind: i32,
    /// Its structure name, read on first need (`Words::token` only; hover never reads it).
    name: Option<String>,
    /// No word inside it has been passed yet.
    fresh: bool,
    /// An untagged page's Text node: its line breaks, read on first need (`token`).
    /// `Some(None)`: not an untagged page.
    lines: Option<Option<Arc<Lines>>>,
    /// Where the next word is looked for in `lines.text`, and the line it is on.
    cursor: usize,
    line: usize,
}

/// An untagged page's text and, per line, whether a block ends after it.
struct Lines {
    text: String,
    ends: Vec<bool>,
}

/// How far ahead (bytes) a word is looked for in the page text before the match is
/// given up for that word (text and words disagree: no extra break, never a panic).
const LINE_LOOKAHEAD: usize = 256;

/// Depth-first document-order iterator over Word leaves; stops after `cap` words.
#[derive(Clone)]
pub(crate) struct Words<N: Node> {
    root: Option<N>,
    stack: Vec<Level<N>>,
    cap: usize,
    seen: usize,
    /// Stack levels from this index up held no word before the last word passed: that
    /// word starts them.
    starts_from: usize,
}

pub(crate) fn words<N: Node>(root: N, cap: usize) -> Words<N> {
    Words {
        root: Some(root),
        stack: Vec::new(),
        cap,
        seen: 0,
        starts_from: 0,
    }
}

/// The marker a structure element's first word gets (`GetStdName`, any case).
pub(crate) fn brk_of(name: &str) -> Brk {
    let n = name.to_ascii_uppercase();
    match n.as_str() {
        "H" => Brk::Heading(0),
        "LI" => Brk::ListItem,
        "TR" => Brk::TableRow,
        "TD" | "TH" => Brk::TableCell,
        "CAPTION" => Brk::Caption,
        "BLOCKQUOTE" => Brk::Quote,
        _ => match n.strip_prefix('H').and_then(|d| d.parse::<u8>().ok()) {
            Some(l @ 1..=6) if n.len() == 2 => Brk::Heading(l),
            _ => Brk::None,
        },
    }
}

/// When two elements start at the same word: the outer row beats its cell, a heading
/// beats everything. (PDF `Code` is inline in most PDFs: no marker.)
fn brk_rank(b: Brk) -> u8 {
    match b {
        b if b.is_heading() => 7,
        Brk::TableRow => 6,
        Brk::TableCell => 5,
        Brk::ListItem => 4,
        Brk::Caption => 3,
        Brk::Quote => 2,
        _ => 0,
    }
}

/// Inline structure types: their end is not the end of a paragraph. Everything else
/// (and a name that could not be read) is a block. A list label is inline so "1." is
/// read with its item.
pub(crate) fn is_inline(name: &str) -> bool {
    const INLINE: [&str; 19] = [
        "Span", "Link", "Annot", "Quote", "Note", "Reference", "BibEntry", "Lbl", "Ruby", "RB",
        "RT", "RP", "Warichu", "WT", "WP", "Form", "Em", "Strong", "Code",
    ];
    INLINE.iter().any(|i| i.eq_ignore_ascii_case(name))
}

impl<N: Node> Iterator for Words<N> {
    type Item = N;
    fn next(&mut self) -> Option<N> {
        while self.seen < self.cap {
            let n = self.next_node()?;
            let kind = n.kind();
            if kind == WORD {
                self.passed();
                return Some(n);
            }
            self.open(n, kind);
        }
        None
    }
}

impl<N: Node> Words<N> {
    /// The next node in document order at the current depth (popping finished levels).
    fn next_node(&mut self) -> Option<N> {
        if let Some(r) = self.root.take() {
            return Some(r);
        }
        loop {
            let top = self.stack.last_mut()?;
            if top.next >= top.count {
                self.stack.pop();
                continue;
            }
            let i = top.next;
            top.next += 1;
            if let Some(c) = top.node.child(i) {
                return Some(c);
            }
        }
    }

    /// Walk into a container (its children come next).
    fn open(&mut self, n: N, kind: i32) {
        let count = n.child_count();
        if count > 0 {
            self.stack.push(Level { node: n, next: 0, count, kind, name: None, fresh: true, lines: None, cursor: 0, line: 0 });
        }
    }

    /// A word was passed: count it and note which open containers it starts. The fresh
    /// levels are always the top of the stack (a level opened inside a fresh one is fresh).
    fn passed(&mut self) {
        self.seen += 1;
        let mut i = self.stack.len();
        while i > 0 && self.stack.get(i - 1).is_some_and(|l| l.fresh) {
            i -= 1;
            if let Some(l) = self.stack.get_mut(i) {
                l.fresh = false;
            }
        }
        self.starts_from = i;
    }

    /// Structure name of stack level `i` ("" for a page, the document node, or a read
    /// error), read once per level.
    fn level_name(&mut self, i: usize) -> &str {
        let Some(l) = self.stack.get_mut(i) else { return "" };
        if l.name.is_none() {
            let named = l.kind == STRUCT_ELEMENT || l.kind == TEXT_NODE;
            l.name = Some(if named { l.node.name() } else { String::new() });
        }
        l.name.as_deref().unwrap_or("")
    }

    /// The token for `n`, the word this walk passed last (call it before walking on):
    /// its text, prefixed with the marker of the structure it starts (heading, list
    /// item, table row or cell, caption, quote), and ending in "\n" instead of its
    /// trailing space when it is the last word of a block (natural-reading spec §5).
    pub(crate) fn token(&mut self, n: &N) -> String {
        let mut brk = Brk::None;
        for i in self.starts_from..self.stack.len() {
            let b = brk_of(self.level_name(i));
            if brk_rank(b) > brk_rank(brk) {
                brk = b;
            }
        }
        let mut t = n.text();
        let line_end = self.ends_untagged_line(&t);
        if line_end == Some(false) && !t.ends_with(char::is_whitespace) {
            // a soft wrap: Acrobat gives the line's last word no trailing space
            t.push(' ');
        }
        if self.ends_block() || line_end == Some(true) {
            let keep = t.trim_end().len();
            t.truncate(keep);
            t.push('\n');
        }
        if brk.is_break() {
            t.insert_str(0, &mark(brk));
        }
        t
    }

    /// Untagged PDFs (task 5b): `Some(true)` when the word passed last, with text `word`,
    /// ends a line of its page that `ra_text::line_breaks` ends a block after,
    /// `Some(false)` when it ends a soft-wrapped line, `None` otherwise. Only for a Text
    /// node right under an untagged page group (named "StructTreeRoot"); its text is read
    /// once, and words are matched to it in order. A word not found close ahead: `None`.
    fn ends_untagged_line(&mut self, word: &str) -> Option<bool> {
        let top = self.stack.len().checked_sub(1)?;
        let untagged_page = |s: &mut Self| {
            s.stack.get(top).is_some_and(|l| l.kind == TEXT_NODE)
                && top.checked_sub(1).is_some_and(|p| s.level_name(p) == "StructTreeRoot")
        };
        let known = self.stack.get(top).and_then(|l| l.lines.clone());
        let lines = match known {
            Some(v) => v,
            None => {
                let v = untagged_page(self).then(|| {
                    let text = self.stack.get(top).map(|l| l.node.content()).unwrap_or_default();
                    let ends = crate::ra_text::line_breaks(&text);
                    Arc::new(Lines { text, ends })
                });
                if let Some(l) = self.stack.get_mut(top) {
                    l.lines = Some(v.clone());
                }
                v
            }
        };
        let (Some(lines), Some(l)) = (lines, self.stack.get_mut(top)) else { return None };
        let w = word.trim();
        let rest = lines.text.get(l.cursor..)?;
        if w.is_empty() {
            return None;
        }
        let pos = rest.find(w).filter(|&p| p <= LINE_LOOKAHEAD)?;
        let skipped = rest.get(..pos).unwrap_or("");
        l.line = l.line.saturating_add(skipped.matches('\n').count());
        let end = l.cursor.saturating_add(pos).saturating_add(w.len());
        l.cursor = end;
        let tail = lines.text.get(end..).unwrap_or("");
        let gap: &str = tail.get(..tail.len() - tail.trim_start().len()).unwrap_or("");
        let line_ends = gap.contains('\n') || tail.is_empty();
        line_ends.then(|| lines.ends.get(l.line).copied().unwrap_or(false))
    }

    /// The word passed last ends a block: every container from its parent out to the
    /// nearest block-level one has no children left (inline ones are looked through).
    fn ends_block(&mut self) -> bool {
        for i in (0..self.stack.len()).rev() {
            let more = self.stack.get(i).is_some_and(|l| l.next < l.count);
            if more {
                return false;
            }
            if !is_inline(self.level_name(i)) {
                return true;
            }
        }
        true
    }

    /// Next on-screen word and its box, in document order. Skips (without walking into)
    /// containers that are off screen: Acrobat gives a container the union of its
    /// on-screen parts, so such a subtree has no on-screen words. Off-screen words
    /// count toward the cap; skipped subtrees do not.
    pub(crate) fn on_screen_word(&mut self) -> Option<(N, Rect)> {
        self.on_screen_word_in(|_| true)
    }

    /// `on_screen_word`, also skipping containers whose box fails `keep`.
    fn on_screen_word_in(&mut self, keep: impl Fn(&Rect) -> bool) -> Option<(N, Rect)> {
        while self.seen < self.cap {
            let n = self.next_node()?;
            let loc = n.location();
            let kind = n.kind();
            if kind == WORD {
                self.passed();
                if let Some(r) = loc {
                    return Some((n, r));
                }
            } else if loc.as_ref().is_some_and(&keep) {
                self.open(n, kind);
            }
        }
        None
    }
}

fn hit(r: &Rect, x: f64, y: f64) -> bool {
    crate::ra_uia::contains(std::slice::from_ref(r), x, y)
}

/// Index (in the iterator's order) of the first word whose location contains (x, y).
/// Test-only: it pins down the hit rule `seek` uses (`hit`).
#[cfg(test)]
pub(crate) fn word_at<N: Node>(ws: &[N], x: f64, y: f64) -> Option<usize> {
    ws.iter()
        .position(|n| n.location().is_some_and(|r| hit(&r, x, y)))
}

/// A word that starts reading when the click hits no word: below the point, or on the
/// point's line and to its right.
pub(crate) fn after_point(r: &Rect, x: f64, y: f64) -> bool {
    r.y > y || (y >= r.y && y <= r.y + r.h && r.x > x)
}

/// Move `ws` to the word under (x, y) and return it; the iterator then continues
/// right after it. No word under the point: the first on-screen word below or after
/// the point (the iterator is rewound to just after that word). Walks on-screen words
/// only, and stops at the deadline (then the fallback, if one was seen, is used).
pub(crate) fn seek<N: Node + Clone>(
    ws: &mut Words<N>,
    x: f64,
    y: f64,
    deadline: Instant,
) -> Option<N> {
    let mut fallback: Option<(N, Words<N>)> = None;
    while Instant::now() < deadline {
        let Some((n, r)) = ws.on_screen_word() else { break };
        if hit(&r, x, y) {
            return Some(n);
        }
        if fallback.is_none() && after_point(&r, x, y) {
            fallback = Some((n, ws.clone()));
        }
    }
    let (n, state) = fallback?;
    let seen = ws.seen; // the work done stays counted against the cap
    *ws = state;
    ws.seen = seen;
    Some(n)
}

/// `pick`'s error when `seek` found no word: out of time, or truly nothing there.
pub(crate) fn seek_error(timed_out: bool) -> &'static str {
    if timed_out {
        ACRO_TIMEOUT
    } else {
        NO_TEXT_ACRO
    }
}

/// Words handed out so far, and the walk that continues after them.
pub(crate) struct Walk<N: Node> {
    walk: Words<N>,
    /// Source index `i` = `nodes[i]`.
    nodes: Vec<N>,
    /// The walk reached the end of the document (or the word cap).
    ended: bool,
}

/// `Walk::mark`: where the walk was, to undo a batch the caller never got.
pub(crate) struct WalkMark<N: Node> {
    walk: Words<N>,
    len: usize,
    ended: bool,
}

impl<N: Node + Clone> Walk<N> {
    pub(crate) fn new(walk: Words<N>, nodes: Vec<N>) -> Walk<N> {
        Walk { walk, nodes, ended: false }
    }

    /// Up to `n` more tokens, stopping early at the deadline or the end.
    pub(crate) fn take(&mut self, n: usize, deadline: Instant) -> Vec<String> {
        let mut out = Vec::new();
        while out.len() < n && !self.ended && Instant::now() < deadline {
            match self.walk.next() {
                Some(w) => {
                    out.push(self.walk.token(&w));
                    self.nodes.push(w);
                }
                None => self.ended = true,
            }
        }
        out
    }

    /// `more`'s batch (pure over the node trait): ≤ `MORE_BATCH` tokens, returning once
    /// `slice_end` passes (after at least one word) so the single worker thread can
    /// serve the Rects/Hover requests queued meanwhile. A short batch is not the end;
    /// `Ok(empty)` is (end of document or word cap). `Err` = `hard_end` passed before
    /// any word.
    pub(crate) fn more_sliced(&mut self, slice_end: Instant, hard_end: Instant) -> Result<Vec<String>, String> {
        // the first word may take up to the hard budget (Acrobat stalls now and then)
        let mut out = self.take(1, hard_end);
        if !out.is_empty() {
            out.extend(self.take(MORE_BATCH.saturating_sub(1), slice_end));
        }
        if out.is_empty() && !self.ended {
            return Err("Acrobat read timed out".into());
        }
        Ok(out)
    }

    pub(crate) fn mark(&self) -> WalkMark<N> {
        WalkMark { walk: self.walk.clone(), len: self.nodes.len(), ended: self.ended }
    }

    pub(crate) fn rollback(&mut self, m: WalkMark<N>) {
        self.walk = m.walk;
        self.nodes.truncate(m.len);
        self.ended = m.ended;
    }
}

/// Hover answer: `Some([box])` for the on-screen word under the point, `Some([])` when
/// there is none, `None` when the deadline passed first (no answer). Hover itself uses
/// `hover_seen`; this is the plain walk it builds on (tests and the live checks).
#[cfg(test)]
pub(crate) fn hover_rects<N: Node>(
    ws: &mut Words<N>,
    x: f64,
    y: f64,
    deadline: Instant,
) -> Option<Vec<Rect>> {
    hover_rects_into(ws, x, y, deadline, &mut |_, _| {})
}

/// `hover_rects`, reporting every on-screen word box it passes to `seen`.
fn hover_rects_into<N: Node>(
    ws: &mut Words<N>,
    x: f64,
    y: f64,
    deadline: Instant,
    seen: &mut dyn FnMut(&N, &Rect),
) -> Option<Vec<Rect>> {
    // A container's box is the union of its on-screen parts: if it does not contain
    // the point, no word inside it does.
    while Instant::now() < deadline {
        match ws.on_screen_word_in(|b| hit(b, x, y)) {
            None => return Some(Vec::new()),
            Some((n, r)) => {
                seen(&n, &r);
                if hit(&r, x, y) {
                    return Some(vec![r]);
                }
            }
        }
    }
    None
}

/// Most word boxes a hover view remembers.
const SEEN_MAX: usize = 4000;

/// Word boxes earlier hovers on this page view walked past: (node, box when seen).
pub(crate) type Seen<N> = Vec<(N, Rect)>;

/// Hover with memory: a remembered box under the point is answered after ONE location
/// check of its node (a scroll or zoom moves it; then everything remembered is
/// forgotten), instead of walking the DOM from the top (one cross-process call per
/// node). Otherwise `hover_rects` from `root`, remembering every box it passes.
pub(crate) fn hover_seen<N: Node + Clone>(
    seen: &mut Seen<N>,
    root: N,
    x: f64,
    y: f64,
    deadline: Instant,
) -> Option<Vec<Rect>> {
    if let Some((n, r)) = seen.iter().find(|(_, r)| hit(r, x, y)) {
        match n.location() {
            Some(now) if now == *r => return Some(vec![now]),
            _ => seen.clear(),
        }
    }
    hover_rects_into(&mut words(root, MAX_WORDS), x, y, deadline, &mut |n, r| {
        if seen.len() < SEEN_MAX && !seen.iter().any(|(_, s)| s == r) {
            seen.push((n.clone(), r.clone()));
        }
    })
}

/// Pick the visible page view among AVL_AVView candidates: (hwnd, visible, area).
/// A hidden background tab's view has no DOM, however big it is.
pub(crate) fn visible_page_view(c: &[(isize, bool, i64)]) -> Option<isize> {
    c.iter()
        .filter(|(_, vis, area)| *vis && *area > 0)
        .max_by_key(|(_, _, area)| *area)
        .map(|(h, _, _)| *h)
}

/// The page view under the point: visible, non-empty, rect containing (x, y); the
/// largest if several (split view).
pub(crate) fn page_view_for(c: &[(isize, bool, Rect)], x: f64, y: f64) -> Option<isize> {
    let under: Vec<(isize, bool, i64)> = c
        .iter()
        .filter(|(_, _, r)| hit(r, x, y))
        .map(|(h, vis, r)| (*h, *vis, (r.w.max(0.0) * r.h.max(0.0)) as i64))
        .collect();
    visible_page_view(&under)
}

/// True for the image path of Acrobat or the 32-bit Reader.
pub(crate) fn is_acrobat_image(path: &str) -> bool {
    let name = path.rsplit(['\\', '/']).next().unwrap_or("");
    name.eq_ignore_ascii_case("Acrobat.exe") || name.eq_ignore_ascii_case("AcroRd32.exe")
}

// ---------------------------------------------------------------------------------
// The real DOM
// ---------------------------------------------------------------------------------

/// `GetLocation`: `Ok(None)` = off screen (Acrobat answers (0,0,0,0)); `Err` = COM error.
fn location_of(n: &IPDDomNode) -> windows::core::Result<Option<Rect>> {
    let (mut x, mut y, mut w, mut h) = (0i32, 0i32, 0i32, 0i32);
    unsafe { n.GetLocation(&mut x, &mut y, &mut w, &mut h) }.ok()?;
    Ok((w > 0 && h > 0).then_some(Rect {
        x: x as f64,
        y: y as f64,
        w: w as f64,
        h: h as f64,
    }))
}

impl Node for IPDDomNode {
    fn kind(&self) -> i32 {
        let mut t = -1;
        match unsafe { self.GetType(&mut t) }.ok() {
            Ok(()) => t,
            Err(e) => {
                log::debug!("read-aloud: acro GetType {e}");
                -1
            }
        }
    }

    fn child_count(&self) -> usize {
        let mut n = 0;
        match unsafe { self.GetChildCount(&mut n) }.ok() {
            Ok(()) => n.clamp(0, MAX_KIDS) as usize,
            Err(e) => {
                log::debug!("read-aloud: acro GetChildCount {e}");
                0
            }
        }
    }

    fn child(&self, i: usize) -> Option<Self> {
        let mut c = None;
        match unsafe { self.GetChild(i as i32, &mut c) }.ok() {
            Ok(()) => c,
            Err(e) => {
                log::debug!("read-aloud: acro GetChild({i}) {e}");
                None
            }
        }
    }

    fn text(&self) -> String {
        let mut b = BSTR::new();
        match unsafe { self.GetTextContent(&mut b) }.ok() {
            Ok(()) => b.to_string(),
            Err(e) => {
                log::debug!("read-aloud: acro GetTextContent {e}");
                String::new()
            }
        }
    }

    fn content(&self) -> String {
        self.text()
    }

    /// `IPDDomElement::GetStdName` (the standard type after the role map), else
    /// `GetTagName`. `IPDDomNode::GetName` answers "" (or the text) here, so it is not used.
    fn name(&self) -> String {
        let Ok(el) = self.cast::<com::IPDDomElement>() else { return String::new() };
        let read = |std: bool| {
            let mut b = BSTR::new();
            let r = unsafe { if std { el.GetStdName(&mut b) } else { el.GetTagName(&mut b) } }.ok();
            match r {
                Ok(()) => b.to_string(),
                Err(e) => {
                    log::debug!("read-aloud: acro GetStdName/GetTagName {e}");
                    String::new()
                }
            }
        };
        let std = read(true);
        if std.is_empty() { read(false) } else { std }
    }

    fn location(&self) -> Option<Rect> {
        location_of(self).unwrap_or_else(|e| {
            log::debug!("read-aloud: acro GetLocation {e}");
            None
        })
    }
}

fn class_of(h: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut buf) }.clamp(0, buf.len() as i32) as usize;
    String::from_utf16_lossy(&buf[..n])
}

fn text_of(h: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetWindowTextW(h, &mut buf) }.clamp(0, buf.len() as i32) as usize;
    String::from_utf16_lossy(&buf[..n])
}

fn pid_of(h: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    pid
}

fn process_image(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            h,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(h);
        ok.ok()?;
        Some(String::from_utf16_lossy(&buf[..(len as usize).min(buf.len())]))
    }
}

fn root_at(x: i32, y: i32) -> Option<HWND> {
    let w = unsafe { WindowFromPoint(POINT { x, y }) };
    if w.is_invalid() {
        return None;
    }
    let root = unsafe { GetAncestor(w, GA_ROOT) };
    (!root.is_invalid()).then_some(root)
}

fn window_rect(h: HWND) -> Rect {
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(h, &mut r);
    }
    Rect {
        x: r.left as f64,
        y: r.top as f64,
        w: (r.right - r.left) as f64,
        h: (r.bottom - r.top) as f64,
    }
}

/// A window chain (the window under the point, then its ancestors: class, text) is
/// inside an Acrobat page view: an `AVL_AVView` whose text is `AVPageView`. Acrobat
/// uses `AVL_AVView` for scroll bars, toolbars, thumbnails and side panes too; those
/// are ordinary windows (UIA, the app-chrome gate, click checks). Pure, so it is
/// unit-tested.
pub(crate) fn is_page_chain(chain: &[(&str, &str)]) -> bool {
    chain.iter().any(|&(class, text)| class == VIEW_CLASS && text == PAGE_VIEW_TEXT)
}

/// The window under the point (or an ancestor) is an Acrobat page view and its
/// process image is `Acrobat.exe` or `AcroRd32.exe`.
pub fn is_acrobat_page(hwnd: isize) -> bool {
    let start = HWND(hwnd as *mut _);
    let mut chain: Vec<(String, String)> = Vec::new();
    let mut h = start;
    for _ in 0..64 {
        if h.is_invalid() {
            break;
        }
        let class = class_of(h);
        // only a view's text matters (and is worth the call)
        let text = if class == VIEW_CLASS { text_of(h) } else { String::new() };
        chain.push((class, text));
        h = unsafe { GetAncestor(h, GA_PARENT) };
    }
    let chain: Vec<(&str, &str)> = chain.iter().map(|(c, t)| (c.as_str(), t.as_str())).collect();
    is_page_chain(&chain) && process_image(pid_of(start)).is_some_and(|p| is_acrobat_image(&p))
}

/// All Acrobat page views under the frame: (hwnd, visible, screen rect).
fn page_views(frame: HWND) -> Vec<(isize, bool, Rect)> {
    unsafe extern "system" fn cb(h: HWND, l: LPARAM) -> BOOL {
        let v = &mut *(l.0 as *mut Vec<HWND>);
        v.push(h);
        BOOL(1)
    }
    let mut all: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumChildWindows(frame, Some(cb), LPARAM(&mut all as *mut Vec<HWND> as isize));
    }
    all.into_iter()
        .filter(|&h| class_of(h) == VIEW_CLASS && text_of(h) == PAGE_VIEW_TEXT)
        .map(|h| (h.0 as isize, unsafe { IsWindowVisible(h) }.as_bool(), window_rect(h)))
        .collect()
}

fn role_of(acc: &IAccessible) -> i32 {
    unsafe { acc.get_accRole(&VARIANT::from(0i32)) }
        .ok()
        .and_then(|v| i32::try_from(&v).ok())
        .unwrap_or(0)
}

/// The window's MSAA object may be a client wrapper (role 10) around the document
/// (role 15); only the document answers `QueryService`.
fn document_acc(acc: IAccessible) -> IAccessible {
    if role_of(&acc) == ROLE_DOCUMENT {
        return acc;
    }
    let n = unsafe { acc.accChildCount() }.unwrap_or(0).clamp(0, MAX_ACC_KIDS as i32) as usize;
    let mut kids: Vec<VARIANT> = (0..n).map(|_| VARIANT::default()).collect();
    let mut got = 0i32;
    if n > 0 && unsafe { AccessibleChildren(&acc, 0, &mut kids, &mut got) }.is_ok() {
        for v in kids.iter().take(got.clamp(0, n as i32) as usize) {
            let raw = v.as_raw();
            // VT_DISPATCH = 9: a child object (VT_I4 would be a simple child id)
            if unsafe { raw.Anonymous.Anonymous.vt } != 9 {
                continue;
            }
            let p = unsafe { raw.Anonymous.Anonymous.Anonymous.pdispVal };
            let Some(d) = (unsafe { IDispatch::from_raw_borrowed(&p) }) else {
                continue;
            };
            if let Ok(c) = d.cast::<IAccessible>() {
                if role_of(&c) == ROLE_DOCUMENT {
                    return c;
                }
            }
        }
    }
    acc
}

/// MSAA page view → document → `QueryService(SID_GetPDDomNode)` → the DOM root.
fn dom_root(page_view: isize) -> Result<IPDDomNode, String> {
    let fail = |what: &str, e: windows::core::Error| {
        log::debug!("read-aloud: acro {what}: {e}");
        format!("{NO_DOM} ({what})")
    };
    unsafe {
        let mut p = std::ptr::null_mut();
        AccessibleObjectFromWindow(
            HWND(page_view as *mut _),
            OBJID_CLIENT.0 as u32,
            &IAccessible::IID,
            &mut p,
        )
        .map_err(|e| fail("AccessibleObjectFromWindow", e))?;
        if p.is_null() {
            return Err(format!("{NO_DOM} (no MSAA object)"));
        }
        let acc = IAccessible::from_raw(p);
        let sp: IServiceProvider = document_acc(acc)
            .cast()
            .map_err(|e| fail("IServiceProvider", e))?;
        let g: IGetPDDomNode = sp
            .QueryService(&SID_GET_PD_DOM_NODE)
            .map_err(|e| fail("QueryService", e))?;
        let mut root = None;
        g.get_PDDomNode(VARIANT::from(0i32), &mut root)
            .ok()
            .map_err(|e| fail("get_PDDomNode", e))?;
        root.ok_or_else(|| format!("{NO_DOM} (no root)"))
    }
}

/// Frame and page view under the point.
fn view_at(x: i32, y: i32) -> Option<(HWND, isize)> {
    let frame = root_at(x, y)?;
    let pv = page_view_for(&page_views(frame), x as f64, y as f64)?;
    Some((frame, pv))
}

/// The current Acrobat read: Word nodes from the clicked word on (index = source index).
pub struct AcroRead {
    frame: isize,
    /// Lazy document-order walk; continues after the last word handed out.
    w: Walk<IPDDomNode>,
}

/// `AcroRead::mark`.
pub struct AcroMark(WalkMark<IPDDomNode>);

impl AcroRead {
    /// Tokens (≤ 60) from the word under (x, y) on. No word exactly there: from the
    /// first on-screen word below or after the point. `Err(NO_TEXT_ACRO)` when there is
    /// no page view or no on-screen word, `Err(ACRO_TIMEOUT)` when time ran out first,
    /// and `Err(NO_DOM ...)` when the DOM was unreachable.
    pub fn pick(x: i32, y: i32) -> Result<(AcroRead, Vec<String>), String> {
        let (frame, pv) = view_at(x, y).ok_or(NO_TEXT_ACRO)?;
        Self::pick_in(frame, pv, x, y)
    }

    /// `pick` on a known frame and page view (the window lookup already done).
    fn pick_in(frame: HWND, pv: isize, x: i32, y: i32) -> Result<(AcroRead, Vec<String>), String> {
        let deadline = Instant::now() + CALL_BUDGET;
        let mut walk = words(dom_root(pv)?, MAX_WORDS);
        let first = seek(&mut walk, x as f64, y as f64, deadline)
            .ok_or_else(|| seek_error(Instant::now() >= deadline))?;
        let mut out = vec![walk.token(&first)];
        let mut read = AcroRead {
            frame: frame.0 as isize,
            w: Walk::new(walk, vec![first]),
        };
        out.extend(read.w.take(FIRST_BATCH - 1, deadline));
        Ok((read, out))
    }

    /// Where `more` is now; `rollback` undoes the batches after it.
    pub fn mark(&self) -> AcroMark {
        AcroMark(self.w.mark())
    }

    pub fn rollback(&mut self, m: AcroMark) {
        self.w.rollback(m.0);
    }

    /// Next ≤ 400 tokens, or what ~100 ms of walking found (`Walk::more_sliced`); source
    /// indices continue. `Ok(empty)` = end of the document (or the 20 000-word cap);
    /// a short batch is not the end; `Err` = 3 s passed before any word.
    pub fn more(&mut self) -> Result<Vec<String>, String> {
        let now = Instant::now();
        self.w.more_sliced(now + MORE_SLICE, now + CALL_BUDGET)
    }

    /// Fresh boxes of the words at source indices `src`. `Some(empty)` = a word is off
    /// screen; `None` = unknown index or a COM error (no answer, not "off screen").
    pub fn rects(&self, src: &[usize]) -> Option<Vec<Rect>> {
        let mut out = Vec::with_capacity(src.len());
        for &i in src {
            match location_of(self.w.nodes.get(i)?) {
                Ok(Some(r)) => out.push(r),
                Ok(None) => return Some(Vec::new()),
                Err(e) => {
                    log::debug!("read-aloud: acro rects GetLocation {e}");
                    return None;
                }
            }
        }
        Some(out)
    }

    /// Scroll Acrobat so word `idx` is on screen (`IPDDomNode::ScrollTo`, no input).
    pub fn scroll_to(&self, idx: usize) {
        if let Some(n) = self.w.nodes.get(idx) {
            if let Err(e) = unsafe { n.ScrollTo() }.ok() {
                log::debug!("read-aloud: acro ScrollTo {e}");
            }
        }
    }

    /// The top-level Acrobat frame of this read (for `source_hwnd`).
    pub fn hwnd(&self) -> isize {
        self.frame
    }
}

/// The DOM root of the page view last hovered, kept between hovers: attaching costs
/// 4-7 ms, with 270-480 ms spikes right after Acrobat loads, and the worker is blocked
/// meanwhile. The owner drops it when pick mode ends.
pub struct HoverView {
    pv: isize,
    /// `None`: the DOM could not be reached (retried after `HOVER_RETRY`).
    root: Option<IPDDomNode>,
    at: Instant,
    /// Word boxes earlier hovers walked past (`hover_seen`).
    seen: Seen<IPDDomNode>,
}

/// What hover found in an Acrobat window.
#[derive(Debug, PartialEq)]
pub enum Hover {
    /// The word box under the point; empty when there is no word there.
    Rects(Vec<Rect>),
    /// Over the 50 ms budget: no answer.
    NoAnswer,
    /// The page view's DOM is unreachable (the caller may use OCR instead).
    NoDom,
}

/// The cached view still serves a point: same frame under the point, still visible
/// (a tab switch hides it), and the point is inside it (split view). Pure.
pub(crate) fn view_still_valid(same_frame: bool, visible: bool, view: &Rect, x: f64, y: f64) -> bool {
    same_frame && visible && hit(view, x, y)
}

fn cached_view_serves(c: &HoverView, x: i32, y: i32) -> bool {
    let pv = HWND(c.pv as *mut _);
    if !unsafe { IsWindow(pv) }.as_bool() {
        return false;
    }
    let frame = unsafe { GetAncestor(pv, GA_ROOT) };
    let visible = unsafe { IsWindowVisible(pv) }.as_bool();
    view_still_valid(root_at(x, y) == Some(frame), visible, &window_rect(pv), x as f64, y as f64)
}

/// Hover in pick mode: the box of the on-screen word under the point. Re-attaches to
/// the DOM only when the page view under the point changed.
pub fn hover_cached(cache: &mut Option<HoverView>, x: i32, y: i32) -> Hover {
    let deadline = Instant::now() + HOVER_BUDGET;
    let fresh = match cache.as_ref() {
        Some(c) if cached_view_serves(c, x, y) => c.root.is_some() || c.at.elapsed() < HOVER_RETRY,
        _ => false,
    };
    if !fresh {
        *cache = None;
        let Some((_, pv)) = view_at(x, y) else {
            return Hover::Rects(Vec::new());
        };
        *cache = Some(HoverView { pv, root: dom_root(pv).ok(), at: Instant::now(), seen: Vec::new() });
    }
    let Some(c) = cache.as_mut() else { return Hover::NoDom };
    let Some(root) = c.root.clone() else {
        return Hover::NoDom;
    };
    match hover_seen(&mut c.seen, root, x as f64, y as f64, deadline) {
        Some(v) => Hover::Rects(v),
        None => Hover::NoAnswer,
    }
}

/// `hover` on a known page view (no cache); manual test only.
#[cfg(test)]
fn hover_in(pv: isize, x: i32, y: i32, deadline: Instant) -> Option<Vec<Rect>> {
    let mut ws = words(dom_root(pv).ok()?, MAX_WORDS);
    hover_rects(&mut ws, x as f64, y as f64, deadline)
}

/// First and last page (as Acrobat numbers them) of a node; manual test only.
#[cfg(test)]
fn page_of(n: &IPDDomNode) -> Option<(i32, i32)> {
    let ext: com::IPDDomNodeExt = n.cast().ok()?;
    let (mut a, mut b) = (0, 0);
    unsafe { ext.GetPageNum(&mut a, &mut b) }.ok().ok()?;
    Some((a, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake DOM node: (kind, text, location, children).
    #[derive(Clone, Debug)]
    struct F(i32, &'static str, Option<(f64, f64, f64, f64)>, Vec<F>);

    impl Node for F {
        fn kind(&self) -> i32 {
            self.0
        }
        fn child_count(&self) -> usize {
            self.3.len()
        }
        fn child(&self, i: usize) -> Option<F> {
            FETCHES.with(|c| c.set(c.get() + 1));
            self.3.get(i).cloned()
        }
        fn text(&self) -> String {
            self.1.to_string()
        }
        /// A container's text field is "name" or "name|content" (tests only).
        fn name(&self) -> String {
            NAMES.with(|c| c.set(c.get() + 1));
            if self.0 == WORD { String::new() } else { self.1.split('|').next().unwrap_or("").to_string() }
        }
        fn content(&self) -> String {
            CONTENTS.with(|c| c.set(c.get() + 1));
            self.1.split_once('|').map_or("", |(_, c)| c).to_string()
        }
        fn location(&self) -> Option<Rect> {
            self.2.map(|(x, y, w, h)| Rect { x, y, w, h })
        }
    }

    thread_local! {
        /// Child fetches made by the fake DOM on this test thread.
        static FETCHES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        /// Structure-name reads made by the fake DOM on this test thread.
        static NAMES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        /// Whole-text reads of Text nodes made by the fake DOM on this test thread.
        static CONTENTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    fn contents() -> usize {
        CONTENTS.with(|c| c.get())
    }
    fn names() -> usize {
        NAMES.with(|c| c.get())
    }
    fn fetches() -> usize {
        FETCHES.with(|c| c.get())
    }

    const DOC: i32 = 1;
    const STRUCT: i32 = 3;
    const TEXT: i32 = 4;

    fn w(t: &'static str, loc: Option<(f64, f64, f64, f64)>) -> F {
        F(WORD, t, loc, vec![])
    }

    /// Doc > [Struct > [Text > [a, b], Struct > [Text > [c]]], Text > [d, e]]
    fn tree() -> F {
        F(
            DOC,
            "",
            None,
            vec![
                F(
                    STRUCT,
                    "",
                    None,
                    vec![
                        F(TEXT, "", None, vec![w("a ", None), w("b ", Some((10.0, 10.0, 20.0, 10.0)))]),
                        F(STRUCT, "", None, vec![F(TEXT, "", None, vec![w("c ", Some((40.0, 10.0, 20.0, 10.0)))])]),
                    ],
                ),
                F(TEXT, "", None, vec![w("d ", Some((10.0, 30.0, 20.0, 10.0))), w("e", None)]),
            ],
        )
    }

    fn texts(v: impl Iterator<Item = F>) -> Vec<String> {
        v.map(|n| n.text()).collect()
    }

    #[test]
    fn words_come_out_in_document_order() {
        assert_eq!(texts(words(tree(), 100)), ["a ", "b ", "c ", "d ", "e"]);
    }

    #[test]
    fn a_word_root_is_itself_and_words_are_leaves() {
        // a Word's own children (Char nodes) are never walked into
        let word_with_chars = F(WORD, "hi ", None, vec![F(6, "h", None, vec![])]);
        assert_eq!(texts(words(word_with_chars, 10)), ["hi "]);
        assert_eq!(texts(words(F(DOC, "", None, vec![]), 10)), Vec::<String>::new());
    }

    #[test]
    fn the_cap_stops_the_walk() {
        assert_eq!(texts(words(tree(), 3)), ["a ", "b ", "c "]);
        assert_eq!(texts(words(tree(), 0)), Vec::<String>::new());
    }

    #[test]
    fn word_at_finds_the_word_under_the_point() {
        let ws: Vec<F> = words(tree(), 100).collect();
        assert_eq!(word_at(&ws, 45.0, 15.0), Some(2)); // "c "
        assert_eq!(word_at(&ws, 10.0, 10.0), Some(1)); // edge of "b "
        assert_eq!(word_at(&ws, 35.0, 15.0), None); // gap between words
    }

    #[test]
    fn word_at_never_matches_an_off_screen_word() {
        // Acrobat reports (0,0,0,0) off screen: such a word must not match (0, 0)
        let ws = vec![w("off ", None), w("on ", Some((0.0, 0.0, 5.0, 5.0)))];
        assert_eq!(word_at(&ws, 0.0, 0.0), Some(1));
        assert_eq!(word_at(&[w("off ", None)], 0.0, 0.0), None);
    }

    #[test]
    fn visible_page_view_ignores_hidden_views_and_takes_the_largest() {
        // a hidden background tab's view is the biggest, but has no DOM
        let c = [(1, false, 9_000_000), (2, true, 500_000), (3, true, 1_400_000)];
        assert_eq!(visible_page_view(&c), Some(3));
        assert_eq!(visible_page_view(&[(1, false, 10)]), None);
        assert_eq!(visible_page_view(&[(4, true, 0)]), None, "zero-size view");
        assert_eq!(visible_page_view(&[]), None);
    }

    // ---- on-screen walk, seek, hover ----

    fn r(x: f64, y: f64, w: f64, h: f64) -> Option<(f64, f64, f64, f64)> {
        Some((x, y, w, h))
    }

    /// Like Acrobat: containers carry the union box of their on-screen parts, or None.
    /// Page 1 is scrolled away (off screen); page 2 is on screen, two lines.
    fn screen() -> F {
        F(
            DOC,
            "",
            r(0.0, 0.0, 500.0, 100.0),
            vec![
                F(STRUCT, "", None, vec![F(TEXT, "", None, vec![w("gone ", None), w("away ", None)])]),
                F(
                    STRUCT,
                    "",
                    r(0.0, 0.0, 500.0, 100.0),
                    vec![
                        // a paragraph that starts off screen and ends on it
                        F(TEXT, "", r(0.0, 0.0, 300.0, 20.0), vec![w("half ", None), w("one ", r(0.0, 0.0, 50.0, 20.0)), w("two ", r(60.0, 0.0, 50.0, 20.0))]),
                        F(TEXT, "", r(0.0, 50.0, 300.0, 20.0), vec![w("three ", r(0.0, 50.0, 50.0, 20.0)), w("four", r(60.0, 50.0, 50.0, 20.0))]),
                    ],
                ),
            ],
        )
    }

    fn far() -> Instant {
        Instant::now() + Duration::from_secs(60)
    }

    #[test]
    fn the_on_screen_walk_skips_off_screen_subtrees() {
        let mut ws = words(screen(), 100);
        let mut got = Vec::new();
        while let Some((n, _)) = ws.on_screen_word() {
            got.push(n.text());
        }
        assert_eq!(got, ["one ", "two ", "three ", "four"]);
        // "gone"/"away" were never visited (pruned); "half" was (counted, not returned)
        assert_eq!(ws.seen, 5);
    }

    #[test]
    fn seek_starts_at_the_clicked_word_and_reads_on_in_document_order() {
        let mut ws = words(screen(), 100);
        let first = seek(&mut ws, 70.0, 10.0, far()).map(|n| n.text());
        assert_eq!(first.as_deref(), Some("two "));
        assert_eq!(texts(ws), ["three ", "four"]);
    }

    #[test]
    fn seek_without_a_hit_starts_at_the_first_word_below_or_after_the_point() {
        // between the lines: the next line's first word
        let mut ws = words(screen(), 100);
        assert_eq!(seek(&mut ws, 200.0, 35.0, far()).map(|n| n.text()).as_deref(), Some("three "));
        assert_eq!(texts(ws), ["four"]);
        // in the gap between two words of a line: the word after it
        let mut ws = words(screen(), 100);
        assert_eq!(seek(&mut ws, 55.0, 10.0, far()).map(|n| n.text()).as_deref(), Some("two "));
        assert_eq!(texts(ws), ["three ", "four"]);
        // below everything: nothing to read
        let mut ws = words(screen(), 100);
        assert!(seek(&mut ws, 10.0, 500.0, far()).is_none());
    }

    #[test]
    fn seek_gives_up_at_the_deadline() {
        let mut ws = words(screen(), 100);
        assert!(seek(&mut ws, 70.0, 10.0, Instant::now()).is_none());
    }

    #[test]
    fn a_seek_that_ran_out_of_time_is_a_timeout_not_no_text() {
        // only "no words" may fall through to OCR; a timeout must not (Task 6)
        assert_eq!(seek_error(true), ACRO_TIMEOUT);
        assert_eq!(seek_error(false), NO_TEXT_ACRO);
        assert_ne!(ACRO_TIMEOUT, NO_TEXT_ACRO);
    }

    #[test]
    fn a_rolled_back_batch_is_read_again() {
        // tree()'s blocks end at "b", "c" and "e" (their last word ends in a line break)
        let mut rd = Walk::new(words(tree(), 100), Vec::new());
        assert_eq!(rd.take(2, far()), ["a ", "b
"]);
        let m = rd.mark();
        assert_eq!(rd.take(10, far()), ["c
", "d ", "e
"]);
        assert!(rd.ended);
        rd.rollback(m);
        assert!(!rd.ended, "the end was not delivered");
        assert_eq!(rd.nodes.len(), 2, "source indices are back where the caller is");
        assert_eq!(rd.take(10, far()), ["c
", "d ", "e
"]);
        assert_eq!(rd.nodes.len(), 5);
    }

    #[test]
    fn more_is_time_sliced_and_only_empty_is_the_end() {
        let past = Instant::now();
        let mut rd = Walk::new(words(tree(), 100), Vec::new());
        // the slice is already over: one word (progress), then return
        assert_eq!(rd.more_sliced(past, far()), Ok(vec!["a ".to_string()]));
        assert!(!rd.ended, "a short batch is not the end");
        // a slice that is over, rolled back like an undelivered reply
        let m = rd.mark();
        assert_eq!(rd.more_sliced(past, far()), Ok(vec!["b
".to_string()]));
        rd.rollback(m);
        assert_eq!(rd.nodes.len(), 1);
        // a long slice: the rest (capped at MORE_BATCH)
        assert_eq!(rd.more_sliced(far(), far()), Ok(vec!["b
".to_string(), "c
".into(), "d ".into(), "e
".into()]));
        assert_eq!(rd.more_sliced(past, far()), Ok(vec![]), "only empty is the end");
        assert!(rd.ended);
        // the hard budget ran out before any word: an error, not the end
        let mut rd = Walk::new(words(tree(), 100), Vec::new());
        assert!(rd.more_sliced(past, past).is_err());
        assert!(!rd.ended);
        assert_eq!(rd.nodes.len(), 0);
        // a big page: one slice never returns more than MORE_BATCH
        let mut rd = Walk::new(words(big_page(MORE_BATCH + 50), MAX_WORDS), Vec::new());
        assert_eq!(rd.more_sliced(far(), far()).map(|v| v.len()), Ok(MORE_BATCH));
        assert_eq!(rd.more_sliced(far(), far()).map(|v| v.len()), Ok(50));
    }

    #[test]
    fn hover_cache_is_reused_only_for_the_same_visible_view_under_the_point() {
        let v = Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
        assert!(view_still_valid(true, true, &v, 50.0, 50.0));
        assert!(!view_still_valid(false, true, &v, 50.0, 50.0), "another frame is under the point");
        assert!(!view_still_valid(true, false, &v, 50.0, 50.0), "the view was hidden (tab switch)");
        assert!(!view_still_valid(true, true, &v, 150.0, 50.0), "the point left the view (split view)");
    }

    #[test]
    fn after_point_is_below_or_right_on_the_same_line() {
        let b = Rect { x: 100.0, y: 100.0, w: 50.0, h: 20.0 };
        assert!(after_point(&b, 0.0, 50.0), "below the point");
        assert!(after_point(&b, 90.0, 110.0), "same line, to the right");
        assert!(!after_point(&b, 160.0, 110.0), "same line, to the left");
        assert!(!after_point(&b, 0.0, 130.0), "above the point");
    }

    #[test]
    fn hover_finds_the_word_box_or_says_none_is_there() {
        let b = |x, y| hover_rects(&mut words(screen(), 100), x, y, far());
        assert_eq!(b(70.0, 10.0), Some(vec![Rect { x: 60.0, y: 0.0, w: 50.0, h: 20.0 }]));
        assert_eq!(b(200.0, 35.0), Some(vec![]), "no word there");
        assert_eq!(hover_rects(&mut words(screen(), 100), 70.0, 10.0, Instant::now()), None, "out of time");
    }

    #[test]
    fn hover_remembers_boxes_and_rechecks_them_instead_of_walking() {
        let mut seen: Seen<F> = Vec::new();
        let two = Rect { x: 60.0, y: 0.0, w: 50.0, h: 20.0 };
        assert_eq!(hover_seen(&mut seen, screen(), 70.0, 10.0, far()), Some(vec![two.clone()]));
        assert!(seen.iter().any(|(n, _)| n.text() == "one "), "boxes walked past are kept");
        // "one" again, and "two" again: answered from memory, no DOM walk
        let f0 = fetches();
        assert_eq!(hover_seen(&mut seen, screen(), 20.0, 10.0, far()), Some(vec![Rect { x: 0.0, y: 0.0, w: 50.0, h: 20.0 }]));
        assert_eq!(hover_seen(&mut seen, screen(), 70.0, 10.0, far()), Some(vec![two]));
        assert_eq!(fetches(), f0, "remembered words need no walk");
        // no word under the point: walked as before, the answer stays "none"
        assert_eq!(hover_seen(&mut seen, screen(), 200.0, 35.0, far()), Some(vec![]));
        // out of time with nothing remembered there: no answer
        assert_eq!(hover_seen(&mut seen, screen(), 20.0, 60.0, Instant::now()), None);
    }

    #[test]
    fn a_moved_word_is_never_answered_from_memory() {
        // remembered at (300, 300) but the node now says (0, 0): the page scrolled
        let mut seen: Seen<F> = vec![(w("one ", r(0.0, 0.0, 50.0, 20.0)), Rect { x: 300.0, y: 300.0, w: 50.0, h: 20.0 })];
        assert_eq!(hover_seen(&mut seen, screen(), 310.0, 310.0, far()), Some(vec![]), "nothing really there");
        assert!(seen.iter().all(|(_, b)| b.x < 300.0), "the stale memory was dropped");
        // an off-screen node (no location now) is not answered either
        let mut seen: Seen<F> = vec![(w("gone ", None), Rect { x: 300.0, y: 300.0, w: 50.0, h: 20.0 })];
        assert_eq!(hover_seen(&mut seen, screen(), 310.0, 310.0, far()), Some(vec![]));
    }

    #[test]
    fn the_page_view_under_the_point_is_chosen() {
        let rc = |x, w| Rect { x, y: 0.0, w, h: 100.0 };
        // split view: two visible page views side by side, plus a hidden tab's view
        let c = [(1, true, rc(0.0, 100.0)), (2, true, rc(100.0, 100.0)), (3, false, rc(0.0, 300.0))];
        assert_eq!(page_view_for(&c, 150.0, 50.0), Some(2));
        assert_eq!(page_view_for(&c, 50.0, 50.0), Some(1));
        assert_eq!(page_view_for(&c, 250.0, 50.0), None, "only the hidden view is there");
        assert_eq!(page_view_for(&c, 50.0, 500.0), None, "outside every view");
    }

    #[test]
    fn only_a_page_view_chain_is_an_acrobat_page() {
        const FRAME: (&str, &str) = ("AcrobatSDIWindow", "doc.pdf - Adobe Acrobat");
        // the page itself, and a child window inside it
        assert!(is_page_chain(&[("AVL_AVView", "AVPageView"), ("AVL_AVView", "AVSplitterView"), FRAME]));
        assert!(is_page_chain(&[("AVL_AVPageChild", ""), ("AVL_AVView", "AVPageView"), FRAME]));
        // other AVL_AVView panes: scroll bars, toolbars, thumbnails, side panes
        for pane in ["AVScrollView", "AVToolBarView", "AVThumbnailView", "AVSplitterView", "AVTabView", ""] {
            assert!(!is_page_chain(&[("AVL_AVView", pane), ("AVL_AVView", "AVDocView"), FRAME]), "{pane}");
        }
        assert!(!is_page_chain(&[("ScrollBar", ""), ("AVL_AVView", "AVScrollView"), FRAME]));
        // a window titled like a page view but of another class
        assert!(!is_page_chain(&[("Static", "AVPageView"), FRAME]));
        assert!(!is_page_chain(&[]));
    }

    #[test]
    fn acrobat_images_are_recognised() {
        assert!(is_acrobat_image(r"C:\Program Files\Adobe\Acrobat DC\Acrobat\Acrobat.exe"));
        assert!(is_acrobat_image(r"C:\Program Files (x86)\Adobe\Reader\AcroRd32.exe"));
        assert!(is_acrobat_image("acrobat.EXE"));
        assert!(!is_acrobat_image(r"C:\Windows\notepad.exe"));
        assert!(!is_acrobat_image(r"C:\x\NotAcrobat.exe"));
        assert!(!is_acrobat_image(""));
    }

    /// An untagged page: one Text node holding every word of the page.
    fn big_page(n: usize) -> F {
        let ws = (0..n)
            .map(|i| w("w ", r(10.0 * i as f64, 0.0, 8.0, 10.0)))
            .collect();
        F(DOC, "", r(0.0, 0.0, 10.0 * n as f64, 10.0), vec![F(TEXT, "", r(0.0, 0.0, 10.0 * n as f64, 10.0), ws)])
    }

    #[test]
    fn children_are_fetched_one_at_a_time() {
        let before = fetches();
        let mut ws = words(big_page(500), MAX_WORDS);
        assert!(ws.next().is_some());
        // Doc -> Text -> first word: two fetches, not 500
        assert_eq!(fetches() - before, 2);
        let before = fetches();
        let mut ws = words(big_page(500), MAX_WORDS);
        assert!(seek(&mut ws, 25.0, 5.0, far()).is_some()); // 3rd word
        assert_eq!(fetches() - before, 4);
    }

    #[test]
    fn hover_skips_containers_whose_box_misses_the_point() {
        // two paragraphs on screen; the point is in the second one
        let para = |y: f64, t: &'static str| {
            F(TEXT, "", r(0.0, y, 100.0, 10.0), (0..50).map(|i| w(t, r(2.0 * i as f64, y, 2.0, 10.0))).collect())
        };
        let doc = F(DOC, "", r(0.0, 0.0, 100.0, 40.0), vec![para(0.0, "a "), para(30.0, "b ")]);
        let before = fetches();
        let got = hover_rects(&mut words(doc, MAX_WORDS), 1.0, 35.0, far());
        assert_eq!(got, Some(vec![Rect { x: 0.0, y: 30.0, w: 2.0, h: 10.0 }]));
        // both paragraphs fetched, plus one word of the second; none of the first's words
        assert_eq!(fetches() - before, 3);
    }

    // ---- structure: paragraph ends and markers (natural-reading spec §5) ----

    use crate::ra_struct::{mark, Brk};

    fn m(b: Brk, t: &str) -> String {
        format!("{}{t}", mark(b))
    }

    /// Every token of the document, the way `more` hands them out.
    fn read_all(doc: F) -> Vec<String> {
        Walk::new(words(doc, MAX_WORDS), Vec::new()).take(MAX_WORDS, far())
    }

    #[test]
    fn the_last_word_of_each_paragraph_ends_with_a_line_break() {
        // like Acrobat: a tagged P is a Text node named "P" under the Document element
        let doc = F(DOC, "StructTreeRoot", None, vec![F(STRUCT, "Document", None, vec![
            F(TEXT, "P", None, vec![w("One ", None), w("two.", None)]),
            F(TEXT, "P", None, vec![w("Three ", None), w("four. ", None)]),
        ])]);
        assert_eq!(read_all(doc), ["One ", "two.\n", "Three ", "four.\n"]);
    }

    #[test]
    fn a_heading_marks_its_first_word() {
        let doc = F(DOC, "StructTreeRoot", None, vec![F(STRUCT, "Document", None, vec![
            F(TEXT, "H1", None, vec![w("Big ", None), w("Title", None)]),
            F(TEXT, "P", None, vec![w("Body ", None), w("text.", None)]),
            F(STRUCT, "Sect", None, vec![F(TEXT, "H", None, vec![w("Plain", None)]), F(TEXT, "h3", None, vec![w("Small", None)])]),
        ])]);
        assert_eq!(
            read_all(doc),
            [m(Brk::Heading(1), "Big "), "Title\n".into(), "Body ".into(), "text.\n".into(), m(Brk::Heading(0), "Plain\n"), m(Brk::Heading(3), "Small\n")]
        );
    }

    #[test]
    fn an_untagged_doc_gets_only_line_breaks() {
        // untagged, Acrobat makes one "P" Text node per page under a nameless group
        let page = |a: &'static str, b: &'static str| F(STRUCT, "StructTreeRoot", None, vec![F(TEXT, "P", None, vec![w(a, None), w(b, None)])]);
        let doc = F(DOC, "StructTreeRoot", None, vec![page("a ", "b "), page("c ", "d")]);
        assert_eq!(read_all(doc), ["a ", "b\n", "c ", "d\n"]);
        // a node whose name could not be read is still a block
        let doc = F(DOC, "", None, vec![F(TEXT, "", None, vec![w("x ", None)]), F(TEXT, "", None, vec![w("y ", None)])]);
        assert_eq!(read_all(doc), ["x\n", "y\n"]);
    }

    #[test]
    fn list_items_and_table_rows_and_cells_are_marked() {
        let item = |a: &'static str, b: &'static str| {
            F(STRUCT, "LI", None, vec![F(TEXT, "Lbl", None, vec![w(a, None)]), F(TEXT, "LBody", None, vec![w(b, None)])])
        };
        let row = |a: &'static str, b: &'static str| {
            F(STRUCT, "TR", None, vec![F(TEXT, "TD", None, vec![w(a, None)]), F(TEXT, "TH", None, vec![w(b, None)])])
        };
        let doc = F(DOC, "", None, vec![
            F(STRUCT, "L", None, vec![item("1. ", "Milk "), item("2. ", "Eggs")]),
            F(STRUCT, "Table", None, vec![row("a ", "b "), row("c ", "d ")]),
            F(TEXT, "Caption", None, vec![w("Fig ", None), w("one", None)]),
            F(TEXT, "BlockQuote", None, vec![w("Said ", None)]),
            F(TEXT, "Code", None, vec![w("x=1 ", None)]),
            F(TEXT, "P", None, vec![w("end", None)]),
        ]);
        assert_eq!(
            read_all(doc),
            [
                // the label stays with its item: no break after "1."
                m(Brk::ListItem, "1. "), "Milk\n".into(), m(Brk::ListItem, "2. "), "Eggs\n".into(),
                m(Brk::TableRow, "a\n"), m(Brk::TableCell, "b\n"), m(Brk::TableRow, "c\n"), m(Brk::TableCell, "d\n"),
                m(Brk::Caption, "Fig "), "one\n".into(),
                m(Brk::Quote, "Said\n"),
                // Code is inline in most PDFs: no marker; here it still ends with its parent
                "x=1 ".into(),
                "end\n".into(),
            ]
        );
    }

    #[test]
    fn inline_elements_do_not_end_the_paragraph() {
        let doc = F(DOC, "", None, vec![
            F(STRUCT, "P", None, vec![
                F(TEXT, "Span", None, vec![w("See ", None), w("the ", None)]),
                F(TEXT, "Link", None, vec![w("site ", None)]),
                F(TEXT, "Span", None, vec![w("now. ", None)]),
            ]),
            F(TEXT, "P", None, vec![w("Next", None)]),
        ]);
        assert_eq!(read_all(doc), ["See ", "the ", "site ", "now.\n", "Next\n"]);
    }

    #[test]
    fn a_picked_word_gets_its_marker_only_at_the_start_of_its_block() {
        let doc = || F(DOC, "", r(0.0, 0.0, 500.0, 100.0), vec![
            F(TEXT, "H2", r(0.0, 0.0, 300.0, 20.0), vec![w("Head ", r(0.0, 0.0, 50.0, 20.0)), w("line", r(60.0, 0.0, 50.0, 20.0))]),
            F(TEXT, "P", r(0.0, 50.0, 300.0, 20.0), vec![w("body ", r(0.0, 50.0, 50.0, 20.0)), w("end", r(60.0, 50.0, 50.0, 20.0))]),
        ]);
        let pick = |x, y| {
            let mut ws = words(doc(), 100);
            let first = seek(&mut ws, x, y, far()).expect("a word");
            let mut out = vec![ws.token(&first)];
            out.extend(Walk::new(ws, vec![first]).take(10, far()));
            out
        };
        assert_eq!(pick(10.0, 10.0), [m(Brk::Heading(2), "Head "), "line\n".into(), "body ".into(), "end\n".into()]);
        assert_eq!(pick(70.0, 10.0), ["line\n", "body ", "end\n"]);
        // the fallback (no word under the point) rewinds to the word's own state
        assert_eq!(pick(200.0, 35.0), ["body ", "end\n"]);
    }

    #[test]
    fn structure_costs_one_name_read_per_container_and_none_on_hover() {
        // 20 paragraphs of 5 words: 21 named containers (Document + 20 P), 100 words
        let doc = || F(DOC, "", r(0.0, 0.0, 500.0, 500.0), vec![F(STRUCT, "Document", r(0.0, 0.0, 500.0, 500.0),
            (0..20).map(|p| F(TEXT, "P", r(0.0, 20.0 * p as f64, 100.0, 10.0),
                (0..5).map(|i| w("w ", r(20.0 * i as f64, 20.0 * p as f64, 10.0, 10.0))).collect())).collect())]);
        let before = names();
        let toks = read_all(doc());
        assert_eq!(toks.len(), 100);
        assert_eq!(toks.iter().filter(|t| t.ends_with('\n')).count(), 20);
        assert!(names() - before <= 21, "name reads: {}", names() - before);
        let before = names();
        assert!(hover_rects(&mut words(doc(), MAX_WORDS), 25.0, 205.0, far()).is_some());
        let mut seen = Vec::new();
        assert!(hover_seen(&mut seen, doc(), 25.0, 305.0, far()).is_some());
        assert_eq!(names(), before, "hover reads no names");
    }

    #[test]
    fn rollback_and_slices_keep_one_token_per_word() {
        let doc = F(DOC, "", None, vec![
            F(TEXT, "H1", None, vec![w("A ", None), w("B ", None)]),
            F(TEXT, "P", None, vec![w("c ", None), w("d ", None)]),
        ]);
        let mut rd = Walk::new(words(doc, 100), Vec::new());
        assert_eq!(rd.take(1, far()), [m(Brk::Heading(1), "A ")]);
        let mk = rd.mark();
        assert_eq!(rd.take(10, far()), ["B\n", "c ", "d\n"]);
        rd.rollback(mk);
        assert_eq!(rd.take(10, far()), ["B\n", "c ", "d\n"]);
        assert_eq!(rd.nodes.len(), 4, "one node per token");
    }

    #[test]
    fn normalize_reads_the_markers_and_paragraph_ends() {
        let doc = F(DOC, "", None, vec![
            F(TEXT, "H2", None, vec![w("Big ", None), w("news", None)]),
            F(TEXT, "P", None, vec![w("It ", None), w("rained.", None)]),
            F(STRUCT, "L", None, vec![F(STRUCT, "LI", None, vec![F(TEXT, "LBody", None, vec![w("Milk", None)])])]),
        ]);
        let ws = crate::ra_text::normalize(&read_all(doc));
        let got: Vec<(&str, Brk, Vec<usize>)> = ws.iter().map(|w| (w.text.as_str(), w.brk, w.src.clone())).collect();
        assert_eq!(
            got,
            [
                ("Big", Brk::Heading(2), vec![0]),
                ("news", Brk::None, vec![1]),
                ("It", Brk::Paragraph, vec![2]),
                ("rained.", Brk::None, vec![3]),
                ("Milk", Brk::ListItem, vec![4]),
            ]
        );
    }

    #[test]
    fn structure_names_map_to_breaks() {
        assert_eq!(brk_of("H1"), Brk::Heading(1));
        assert_eq!(brk_of("h6"), Brk::Heading(6));
        assert_eq!(brk_of("H7"), Brk::None);
        assert_eq!(brk_of("H"), Brk::Heading(0));
        assert_eq!(brk_of("LI"), Brk::ListItem);
        assert_eq!(brk_of("TR"), Brk::TableRow);
        assert_eq!(brk_of("TD"), Brk::TableCell);
        assert_eq!(brk_of("TH"), Brk::TableCell);
        assert_eq!(brk_of("Caption"), Brk::Caption);
        assert_eq!(brk_of("BlockQuote"), Brk::Quote);
        assert_eq!(brk_of("Code"), Brk::None, "inline in most PDFs");
        for n in ["P", "", "Document", "StructTreeRoot", "L", "Table", "Span", "Lbl", "LBody", "Hx", "H10", "\u{E0F0}"] {
            assert_eq!(brk_of(n), Brk::None, "{n:?}");
        }
        assert!(is_inline("Span") && is_inline("link") && is_inline("Lbl") && is_inline("Quote"));
        assert!(!is_inline("P") && !is_inline("") && is_inline("Code") && !is_inline("BlockQuote"));
    }

    // ---- untagged PDFs: line ends from the page text (task 5b) ----

    /// Words of `lines`, Acrobat-style: a space after each word except a line's last.
    fn line_words(lines: &[&str]) -> Vec<F> {
        let mut out = Vec::new();
        for l in lines {
            let ws: Vec<&str> = l.split_whitespace().collect();
            for (i, t) in ws.iter().enumerate() {
                let t: &'static str = Box::leak(if i + 1 == ws.len() { t.to_string() } else { format!("{t} ") }.into_boxed_str());
                out.push(w(t, None));
            }
        }
        out
    }

    /// An untagged page as Acrobat gives it: Doc > nameless group > Text "P".
    fn untagged(lines: &[&str]) -> F {
        let content: &'static str = Box::leak(format!("P|{}", lines.join("\r\n")).into_boxed_str());
        F(DOC, "StructTreeRoot", None, vec![F(STRUCT, "StructTreeRoot", None, vec![F(TEXT, content, None, line_words(lines))])])
    }

    const PAGE: [&str; 7] = [
        "Budget Report 2026",
        "The committee met on Tuesday to discuss the budget for the coming",
        "year and agreed that spending on roads should rise by ten percent.",
        "Members also asked the treasurer to report on the new parking fees",
        "before the end of the month.",
        "\u{2022} Milk",
        "\u{2022} Eggs",
    ];

    #[test]
    fn an_untagged_page_breaks_at_paragraph_like_line_ends_only() {
        let toks = read_all(untagged(&PAGE));
        let ends: Vec<&str> = toks.iter().filter(|t| t.ends_with('\n')).map(|t| t.as_str()).collect();
        assert_eq!(ends, ["2026\n", "month.\n", "Milk\n", "Eggs\n"]);
        // the soft-wrapped lines stay running text
        // (and get the space Acrobat leaves off a line's last word)
        assert!(toks.iter().any(|t| t == "coming "), "{toks:?}");
        assert!(toks.iter().any(|t| t == "percent. "), "{toks:?}");
        assert!(toks.iter().any(|t| t == "fees "), "{toks:?}");
        assert_eq!(toks.len(), PAGE.iter().map(|l| l.split_whitespace().count()).sum::<usize>(), "one token per word");
    }

    #[test]
    fn untagged_line_ends_feed_the_raw_text_rules() {
        let ws = crate::ra_text::normalize(&read_all(untagged(&PAGE)));
        let brk = |t: &str| ws.iter().find(|w| w.text == t).map(|w| w.brk);
        assert_eq!(brk("The"), Some(Brk::AfterHeading));
        assert_eq!(brk("year"), Some(Brk::None), "a soft wrap is no break");
        assert_eq!(brk("Members"), Some(Brk::None), "a full-width sentence end is no paragraph");
        assert_eq!(brk("Milk"), Some(Brk::ListItem));
        assert_eq!(brk("Eggs"), Some(Brk::ListItem));
        let src: Vec<usize> = ws.iter().flat_map(|w| w.src.clone()).collect();
        assert_eq!(src, (0..src.len()).collect::<Vec<_>>(), "every token kept, in order");
    }

    #[test]
    fn tagged_text_is_never_split_at_its_lines() {
        let content: &'static str = Box::leak(format!("P|{}", PAGE.join("\r\n")).into_boxed_str());
        let doc = F(DOC, "StructTreeRoot", None, vec![F(STRUCT, "Document", None, vec![F(TEXT, content, None, line_words(&PAGE))])]);
        let before = contents();
        let toks = read_all(doc);
        assert_eq!(toks.iter().filter(|t| t.ends_with('\n')).count(), 1, "only the block's end");
        assert_eq!(contents(), before, "a tagged block's text is never fetched");
    }

    #[test]
    fn untagged_text_that_does_not_match_the_words_adds_no_breaks() {
        // page text that has nothing to do with the words (or is empty): no extra breaks
        for content in ["P|Totally\r\ndifferent\r\nwords here", "P|", "P"] {
            let doc = F(DOC, "StructTreeRoot", None, vec![F(STRUCT, "StructTreeRoot", None, vec![F(TEXT, content, None, line_words(&PAGE))])]);
            let toks = read_all(doc);
            assert_eq!(toks.iter().filter(|t| t.ends_with('\n')).count(), 1, "{content:?}");
        }
    }

    #[test]
    fn untagged_lines_cost_one_text_read_per_page_and_none_on_hover() {
        let page = || {
            let mut d = untagged(&PAGE);
            // give it boxes so hover walks it
            fn boxed(f: &mut F) {
                f.2 = Some((0.0, 0.0, 10.0, 10.0));
                f.3.iter_mut().for_each(boxed);
            }
            boxed(&mut d);
            d
        };
        let before = contents();
        let mut rd = Walk::new(words(page(), MAX_WORDS), Vec::new());
        let past = Instant::now();
        while !rd.more_sliced(past, far()).unwrap_or_default().is_empty() {}
        assert_eq!(contents() - before, 1, "one read for the page, kept across slices");
        let before = contents();
        assert!(hover_rects(&mut words(page(), MAX_WORDS), 5.0, 5.0, far()).is_some());
        let mut seen = Vec::new();
        assert!(hover_seen(&mut seen, page(), 5.0, 5.0, far()).is_some());
        assert_eq!(contents(), before, "hover reads no page text");
    }

    #[test]
    fn untagged_line_ends_survive_a_rollback() {
        let mut rd = Walk::new(words(untagged(&PAGE), MAX_WORDS), Vec::new());
        let a = rd.take(5, far());
        let mk = rd.mark();
        let b = rd.take(MAX_WORDS, far());
        rd.rollback(mk);
        assert_eq!(rd.take(MAX_WORDS, far()), b);
        assert_eq!(a[2], "2026\n");
    }

    #[test]
    fn com_bindings_match_the_idl() {
        use std::mem::{offset_of, size_of};
        use windows::Win32::System::Com::IDispatch_Vtbl;
        let ptr = size_of::<usize>();
        let base = size_of::<IDispatch_Vtbl>();
        assert_eq!(base, 7 * ptr, "IDispatch = IUnknown(3) + 4");
        // IDispatch's slots sit where IDispatch_Vtbl has them
        assert_eq!(offset_of!(com::IPDDomNode_Vtbl, GetTypeInfoCount), offset_of!(IDispatch_Vtbl, GetTypeInfoCount));
        assert_eq!(offset_of!(com::IPDDomNode_Vtbl, Invoke), offset_of!(IDispatch_Vtbl, Invoke));
        // every IPDDomNode method, in IDL order, at its slot
        let mut slots = Vec::new();
        macro_rules! slots {
            ($($m:ident),*) => {$(
                slots.push((stringify!($m), (offset_of!(com::IPDDomNode_Vtbl, $m) - base) / ptr));
            )*};
        }
        slots!(
            GetParent, GetType, GetChildCount, GetChild, GetName, GetValue, IsSame,
            GetTextContent, GetLocation, GetFontInfo, GetFromID, GetIAccessible, ScrollTo,
            GetTextInLines
        );
        let names: Vec<&str> = slots.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, IPDDOMNODE_METHODS);
        for (i, (name, slot)) in slots.iter().enumerate() {
            assert_eq!(*slot, i, "{name}");
        }
        assert_eq!(IPDDOMNODE_METHODS.len(), 14);
        assert_eq!(size_of::<com::IPDDomNode_Vtbl>(), base + 14 * ptr);
        // IPDDomElement : IPDDomNode adds 4 (type library order)
        let node = size_of::<com::IPDDomNode_Vtbl>();
        assert_eq!(offset_of!(com::IPDDomElement_Vtbl, GetTagName), node);
        assert_eq!(offset_of!(com::IPDDomElement_Vtbl, GetStdName), node + ptr);
        assert_eq!(offset_of!(com::IPDDomElement_Vtbl, GetID), node + 2 * ptr);
        assert_eq!(offset_of!(com::IPDDomElement_Vtbl, GetAttribute), node + 3 * ptr);
        assert_eq!(size_of::<com::IPDDomElement_Vtbl>(), node + 4 * ptr);
        assert_eq!(format!("{:?}", com::IPDDomElement::IID), "198F17AE-B921-4308-9543-288D426A5C2B");
        // IUnknown-based ones
        let unk = size_of::<IUnknown_Vtbl>();
        assert_eq!(offset_of!(com::IGetPDDomNode_Vtbl, get_PDDomNode), unk);
        assert_eq!(size_of::<com::IGetPDDomNode_Vtbl>(), unk + ptr);
        assert_eq!(offset_of!(com::IPDDomNodeExt_Vtbl, GetPageNum), unk + 4 * ptr);
        assert_eq!(size_of::<com::IPDDomNodeExt_Vtbl>(), unk + 8 * ptr);
        // GUIDs
        assert_eq!(format!("{:?}", IPDDomNode::IID), "5007373A-20D7-458F-9FFB-ABC900E3A831");
        assert_eq!(format!("{:?}", IGetPDDomNode::IID), "F9F2FE81-F764-4BD0-AFA5-5DE841DDB625");
        assert_eq!(format!("{:?}", com::IPDDomNodeExt::IID), "4A894040-247E-4AFF-BB08-3489E9905235");
        assert_eq!(format!("{:?}", SID_GET_PD_DOM_NODE), "C0A1D5E9-1142-4CF3-B607-82FC3B96A4DF");
    }
}

/// Manual check against a real Acrobat (spec 3.6). Launches Acrobat on a generated PDF
/// only when no Acrobat / Reader is running, never sends input, and closes only the
/// process tree it started. Run: cargo test acro_live -- --ignored --nocapture
#[cfg(test)]
mod live {
    use super::*;
    use std::process::{Child, Command};
    use windows::Win32::Foundation::WPARAM;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    use windows::Win32::UI::HiDpi::{
        SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, IsIconic, PostMessageW, SetWindowPos, ShowWindow, HWND_TOPMOST,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNOACTIVATE, WM_CLOSE,
    };

    const ACROBAT: &str = r"C:\Program Files\Adobe\Acrobat DC\Acrobat\Acrobat.exe";

    fn running(image: &str) -> bool {
        let out = Command::new("tasklist")
            .args(["/FI", &format!("IMAGENAME eq {image}"), "/FO", "CSV", "/NH"])
            .output()
            .expect("tasklist");
        String::from_utf8_lossy(&out.stdout)
            .to_ascii_lowercase()
            .contains(&image.to_ascii_lowercase())
    }

    /// Visible Acrobat frames. No Acrobat ran before the launch (checked), so these
    /// belong to the process tree this test started.
    fn acrobat_frames() -> Vec<HWND> {
        unsafe extern "system" fn cb(h: HWND, l: LPARAM) -> BOOL {
            let v = &mut *(l.0 as *mut Vec<HWND>);
            if IsWindowVisible(h).as_bool() && class_of(h) == "AcrobatSDIWindow" {
                v.push(h);
            }
            BOOL(1)
        }
        let mut v: Vec<HWND> = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(cb), LPARAM(&mut v as *mut Vec<HWND> as isize));
        }
        v
    }

    /// Closes what the test launched: WM_CLOSE to our frame, and if the launched
    /// process has not exited 3 s later (e.g. the "close all tabs?" prompt), kill its
    /// process tree. Never touches any other process. Deletes the PDF.
    struct Launched {
        child: Child,
        frame: Option<HWND>,
        pdf: std::path::PathBuf,
    }
    impl Drop for Launched {
        fn drop(&mut self) {
            if let Some(f) = self.frame {
                unsafe {
                    let _ = PostMessageW(f, WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            let t0 = Instant::now();
            let mut exited = false;
            while t0.elapsed() < Duration::from_secs(3) {
                if matches!(self.child.try_wait(), Ok(Some(_))) {
                    exited = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if exited {
                println!("Acrobat closed on WM_CLOSE");
            } else {
                let pid = self.child.id().to_string();
                let r = Command::new("taskkill").args(["/PID", &pid, "/T", "/F"]).output();
                println!("closed by taskkill /T of pid {pid}: {:?}", r.map(|o| o.status));
                let _ = self.child.wait();
            }
            std::thread::sleep(Duration::from_millis(500));
            let _ = std::fs::remove_file(&self.pdf);
        }
    }

    fn center(r: &Rect) -> (i32, i32) {
        ((r.x + r.w / 2.0) as i32, (r.y + r.h / 2.0) as i32)
    }

    /// Task 8 bench: hover latency over a dense page (~440 words), the cost of walking
    /// every on-screen word box, and pick at the bottom of the page. Same launch and
    /// cleanup rules as `acro_live`. Run in release:
    /// cargo test --release acro_bench -- --ignored --nocapture --test-threads=1
    #[test]
    #[ignore]
    fn acro_bench() {
        use crate::ra_perf::{ms, pct, thread_cpu};
        if running("Acrobat.exe") || running("AcroRd32.exe") {
            println!("skipped: Acrobat already running");
            return;
        }
        unsafe {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let pdf = std::env::temp_dir().join(format!("ra_acro_bench_{}.pdf", std::process::id()));
        let tool = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("tools").join("read-aloud").join("mkpdf.py");
        let mut py = Command::new("py");
        py.arg(&tool).arg(&pdf).arg("--dense");
        if std::env::var_os("RA_ACRO_UNTAGGED").is_some() {
            py.arg("--untagged");
        }
        let made = py.output().expect("run py mkpdf.py");
        assert!(made.status.success() && pdf.exists(), "mkpdf failed: {made:?}");
        let child = Command::new(ACROBAT).arg(&pdf).spawn().expect("launch Acrobat");
        let mut guard = Launched { child, frame: None, pdf: pdf.clone() };
        let t0 = Instant::now();
        let frame = loop {
            if let Some(f) = acrobat_frames().into_iter().next() {
                break f;
            }
            assert!(t0.elapsed() < Duration::from_secs(40), "no Acrobat frame in 40 s");
            std::thread::sleep(Duration::from_millis(250));
        };
        guard.frame = Some(frame);
        let raise = || unsafe {
            if IsIconic(frame).as_bool() {
                let _ = ShowWindow(frame, SW_SHOWNOACTIVATE);
            }
            let _ = SetWindowPos(frame, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        };
        // every on-screen word box, once the document is ready
        let (pv, boxes) = loop {
            raise();
            std::thread::sleep(Duration::from_millis(300));
            let pv = page_views(frame).into_iter().filter(|(_, vis, r)| *vis && r.w > 0.0 && r.h > 0.0).map(|(h, _, _)| h).next();
            let found = pv.and_then(|pv| {
                let mut ws = words(dom_root(pv).ok()?, MAX_WORDS);
                let v: Vec<Rect> = std::iter::from_fn(|| ws.on_screen_word().map(|(_, r)| r)).collect();
                (v.len() > 100).then_some((pv, v))
            });
            if let Some(x) = found {
                break x;
            }
            assert!(t0.elapsed() < Duration::from_secs(40), "no dense page in 40 s");
            std::thread::sleep(Duration::from_millis(500));
        };
        std::thread::sleep(Duration::from_millis(1500)); // past the post-load stalls
        let mut walk = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let mut ws = words(dom_root(pv).unwrap(), MAX_WORDS);
            let n = std::iter::from_fn(|| ws.on_screen_word()).count();
            walk.push(ms(t.elapsed()));
            assert_eq!(n, boxes.len());
        }
        println!("dense page: {} on-screen words; full on-screen walk (attach + every box) {}", boxes.len(), pct(&walk));
        let centre = |r: &Rect| ((r.x + r.w / 2.0) as i32, (r.y + r.h / 2.0) as i32);
        // The DOM part of hover (no window checks, so it runs even when a window of the
        // user's covers the page): the plain walk from the root vs `hover_seen`, same
        // points, same attached root.
        {
            let root = dom_root(pv).expect("DOM root");
            let all: Vec<(f64, f64)> = boxes.iter().step_by((boxes.len() / 60).max(1)).map(|r| (r.x + r.w / 2.0, r.y + r.h / 2.0)).collect();
            let run = |f: &mut dyn FnMut(f64, f64) -> Option<Vec<Rect>>| {
                let (mut t_ok, mut none, c0) = (Vec::new(), 0, thread_cpu());
                for _ in 0..2 {
                    for &(x, y) in &all {
                        let t = Instant::now();
                        match f(x, y) {
                            Some(v) if v.len() == 1 && hit(&v[0], x, y) => t_ok.push(ms(t.elapsed())),
                            Some(v) => panic!("wrong answer at ({x},{y}): {v:?}"),
                            None => none += 1,
                        }
                    }
                }
                format!("answered {} ({}), no answer {none}, CPU {:.0} ms", t_ok.len(), pct(&t_ok), ms(thread_cpu() - c0))
            };
            let plain = run(&mut |x, y| hover_rects(&mut words(root.clone(), MAX_WORDS), x, y, Instant::now() + HOVER_BUDGET));
            let mut seen = Vec::new();
            let remembered = run(&mut |x, y| hover_seen(&mut seen, root.clone(), x, y, Instant::now() + HOVER_BUDGET));
            println!("DOM hover over {} words x2, plain walk: {plain}", all.len());
            println!("DOM hover over {} words x2, hover_seen: {remembered}; {} boxes remembered at the end", all.len(), seen.len());
        }
        // only points where our Acrobat is really on top (a window of the user's may cover part)
        let pts: Vec<(i32, i32)> = boxes
            .iter()
            .step_by((boxes.len() / 60).max(1))
            .map(centre)
            .filter(|&(x, y)| root_at(x, y) == Some(frame))
            .collect();
        let (lx, ly) = centre(boxes.last().unwrap());
        if pts.len() < 10 || root_at(lx, ly) != Some(frame) {
            println!("NOTE: the page is covered by another window ({} points free); point-based hover skipped", pts.len());
        } else {
            let mut cache = None;
            let (mut t_ok, mut none, mut wrong) = (Vec::new(), 0, 0);
            let c0 = thread_cpu();
            for _ in 0..2 {
                for (i, &(x, y)) in pts.iter().enumerate() {
                    let t = Instant::now();
                    let h = hover_cached(&mut cache, x, y);
                    let d = ms(t.elapsed());
                    match h {
                        Hover::Rects(v) if v.len() == 1 && hit(&v[0], x as f64, y as f64) => t_ok.push(d),
                        Hover::NoAnswer => none += 1,
                        other => {
                            wrong += 1;
                            println!("hover {i} at ({x},{y}): {other:?}");
                        }
                    }
                }
            }
            println!(
                "hover over {} points x2: answered {} ({}), no answer (>50 ms) {none}, wrong {wrong}; worker CPU {:.0} ms",
                pts.len(),
                t_ok.len(),
                pct(&t_ok),
                ms(thread_cpu() - c0)
            );
            let (bx, by) = centre(boxes.last().unwrap());
            let mut picks = Vec::new();
            for _ in 0..3 {
                let t = Instant::now();
                let r = AcroRead::pick(bx, by);
                picks.push(ms(t.elapsed()));
                assert!(r.is_ok(), "pick at the last word: {:?}", r.err());
            }
            println!("pick at the page's last word: {}", pct(&picks));
        }
        drop(guard);
    }

    /// Structure markers against a real Acrobat (natural-reading spec §5): prints the
    /// DOM's containers (type, `GetStdName`) and the tokens of a full read of a PDF
    /// tagged H1, P, list, then checks the markers and paragraph ends. Same launch and
    /// cleanup rules as `acro_live`. RA_ACRO_UNTAGGED=1 checks the untagged fallback on
    /// `mkpdf.py --prose` (task 5b): no markers, a break only at paragraph-like line ends.
    /// cargo test acro_struct_live -- --ignored --nocapture
    #[test]
    #[ignore]
    fn acro_struct_live() {
        use crate::ra_struct::strip_mark;
        if running("Acrobat.exe") || running("AcroRd32.exe") {
            println!("skipped: Acrobat already running");
            return;
        }
        unsafe {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let untagged = std::env::var_os("RA_ACRO_UNTAGGED").is_some();
        let pdf = std::env::temp_dir().join(format!("ra_acro_struct_{}.pdf", std::process::id()));
        let tool = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("tools").join("read-aloud").join("mkpdf.py");
        let mut py = Command::new("py");
        if untagged {
            py.arg(&tool).arg(&pdf).arg("--prose").arg("--untagged");
        } else {
            py.arg(&tool).arg(&pdf).arg("--struct");
        }
        let made = py.output().expect("run py mkpdf.py");
        assert!(made.status.success() && pdf.exists(), "mkpdf failed: {made:?}");
        let child = Command::new(ACROBAT).arg(&pdf).spawn().expect("launch Acrobat");
        let mut guard = Launched { child, frame: None, pdf: pdf.clone() };
        let t0 = Instant::now();
        let frame = loop {
            if let Some(f) = acrobat_frames().into_iter().next() {
                break f;
            }
            assert!(t0.elapsed() < Duration::from_secs(40), "no Acrobat frame in 40 s");
            std::thread::sleep(Duration::from_millis(250));
        };
        guard.frame = Some(frame);
        let root = loop {
            std::thread::sleep(Duration::from_millis(500));
            let pv = page_views(frame).into_iter().filter(|(_, vis, r)| *vis && r.w > 0.0 && r.h > 0.0).map(|(h, _, _)| h).next();
            let found = pv.and_then(|pv| dom_root(pv).ok()).filter(|r| words(r.clone(), 5).count() >= 3);
            if let Some(r) = found {
                break r;
            }
            assert!(t0.elapsed() < Duration::from_secs(40), "no DOM words in 40 s");
        };
        fn dump(n: &IPDDomNode, depth: usize) {
            let k = n.kind();
            if k == WORD || depth > 4 {
                return;
            }
            println!("{}type {k} name {:?} kids {}", "  ".repeat(depth), n.name(), n.child_count());
            for i in 0..n.child_count().min(12) {
                if let Some(c) = n.child(i) {
                    dump(&c, depth + 1);
                }
            }
        }
        dump(&root, 0);
        // the cost of the one extra call per untagged page: its whole text
        if let Some(text) = root.child(0).and_then(|g| g.child(0)).filter(|_| untagged) {
            let t = Instant::now();
            let c = text.content();
            println!("page text read in {:.1} ms: {c:?}", t.elapsed().as_secs_f64() * 1000.0);
        }
        let mut rd = Walk::new(words(root, MAX_WORDS), Vec::new());
        let t = Instant::now();
        let toks = rd.take(MAX_WORDS, Instant::now() + CALL_BUDGET);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let marked: Vec<(Brk, &str)> = toks.iter().filter_map(|t| match strip_mark(t) {
            (Some(b), rest) => Some((b, rest)),
            _ => None,
        }).collect();
        let ends: Vec<&String> = toks.iter().filter(|t| t.ends_with('\n')).collect();
        println!("{} tokens in {ms:.0} ms; markers {marked:?}; {} paragraph ends: {ends:?}", toks.len(), ends.len());
        assert!(rd.ended, "the whole test document was read");
        assert_eq!(rd.nodes.len(), toks.len(), "one token per word");
        if untagged {
            // Acrobat infers list items on an untagged page (LI > Lbl "•" / "1.", LBody)
            assert!(marked.iter().all(|(b, _)| *b == Brk::ListItem), "only inferred list items");
            let items: Vec<&str> = marked.iter().map(|(_, t)| *t).collect();
            assert_eq!(items, ["\u{2022}", "\u{2022}", "1.", "2."]);
            let ends: Vec<&str> = ends.iter().map(|s| s.as_str()).collect();
            assert_eq!(ends, ["2026
", "agreed.
", "ends.
", "bread
", "Eggs
", "box
", "guide
", "tango.
", "time.
"]);
            assert!(toks.iter().any(|t| t == "for "), "the soft wrap stays running text: {toks:?}");
        } else {
            assert_eq!(marked, [(Brk::Heading(1), "Page "), (Brk::ListItem, "The "), (Brk::ListItem, "Alpha ")]);
            assert_eq!(ends.len(), 12, "every line of the test PDF is its own block");
            assert_eq!(ends.first().map(|s| s.as_str()), Some("vote.\n"));
        }
        drop(guard);
    }

    #[test]
    #[ignore]
    fn acro_live() {
        if running("Acrobat.exe") || running("AcroRd32.exe") {
            println!("skipped: Acrobat already running");
            return;
        }
        unsafe {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let pdf = std::env::temp_dir().join(format!("ra_acro_live_{}.pdf", std::process::id()));
        let tool = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("tools")
            .join("read-aloud")
            .join("mkpdf.py");
        // RA_ACRO_UNTAGGED=1 runs the same checks on an untagged PDF.
        let mut py = Command::new("py");
        py.arg(&tool).arg(&pdf);
        if std::env::var_os("RA_ACRO_UNTAGGED").is_some() {
            py.arg("--untagged");
        }
        let made = py.output().expect("run py mkpdf.py");
        assert!(made.status.success() && pdf.exists(), "mkpdf failed: {made:?}");

        let child = Command::new(ACROBAT).arg(&pdf).spawn().expect("launch Acrobat");
        let mut guard = Launched { child, frame: None, pdf: pdf.clone() };
        let t0 = Instant::now();
        let (frame, pv) = loop {
            let found = acrobat_frames().into_iter().find_map(|f| {
                let v: Vec<(isize, bool, i64)> = page_views(f)
                    .iter()
                    .map(|(h, vis, r)| (*h, *vis, (r.w * r.h) as i64))
                    .collect();
                Some((f, visible_page_view(&v)?))
            });
            if let Some(x) = found {
                break x;
            }
            assert!(t0.elapsed() < Duration::from_secs(40), "no AVL_AVView page view in 40 s");
            std::thread::sleep(Duration::from_millis(250));
        };
        guard.frame = Some(frame);
        println!("page view after {} ms", t0.elapsed().as_millis());
        // Our own window only: restore it if it came up minimised, and keep it on top
        // without activating it (no focus change, no input). Re-applied while waiting:
        // Acrobat re-orders its frame while it starts up.
        let raise = || unsafe {
            if IsIconic(frame).as_bool() {
                let _ = ShowWindow(frame, SW_SHOWNOACTIVATE);
            }
            let r = SetWindowPos(
                frame,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            if let Err(e) = r {
                println!("SetWindowPos: {e}");
            }
        };
        // The 3rd on-screen word. The document may still be loading (or the first
        // view may be the start page), so re-find the page view and retry for 40 s.
        let (pv, word, wr) = loop {
            raise();
            std::thread::sleep(Duration::from_millis(300));
            let pv = page_views(frame)
                .into_iter()
                .filter(|(_, vis, r)| *vis && r.w > 0.0 && r.h > 0.0)
                .map(|(h, _, _)| h)
                .next()
                .unwrap_or(pv);
            let found = dom_root(pv).ok().and_then(|root| {
                let mut ws = words(root, MAX_WORDS);
                (0..3).filter_map(|_| ws.on_screen_word()).nth(2)
            });
            if let Some((n, r)) = found {
                break (pv, n.text(), r);
            }
            assert!(t0.elapsed() < Duration::from_secs(40), "no on-screen word in 40 s");
            std::thread::sleep(Duration::from_millis(500));
        };
        println!("document ready after {} ms", t0.elapsed().as_millis());
        let (cx, cy) = center(&wr);
        println!("target word {word:?} at {wr:?}");
        assert!(is_acrobat_page(pv), "is_acrobat_page(page view)");

        // Acrobat ignores HWND_TOPMOST, so a foreground window of the user's (never
        // touched) may cover it. Uncovered: the full point-based path. Covered: the
        // same code after the window lookup, and the point-based pick must refuse
        // (it never reads text hidden under another window).
        let covered = root_at(cx, cy) != Some(frame);

        // Timing breakdown: attaching to the DOM, then seeking the word (on-screen walk).
        for _ in 0..3 {
            let t = Instant::now();
            let root = dom_root(pv).expect("DOM root");
            let attach = t.elapsed().as_secs_f64() * 1000.0;
            let mut ws = words(root, MAX_WORDS);
            let found = seek(&mut ws, cx as f64, cy as f64, Instant::now() + CALL_BUDGET);
            assert_eq!(found.map(|n| n.text()), Some(word.clone()));
            let total = t.elapsed().as_secs_f64() * 1000.0;
            println!("attach {attach:.1} ms, seek {:.1} ms", total - attach);
        }
        // Hover has a 50 ms budget; `None` (no answer in time) is allowed by its contract
        // (Acrobat stalls now and then, mostly right after loading), so take 10 tries
        // and record every time.
        let mut hover_ms = Vec::new();
        let mut answers = Vec::new();
        let mut cache = None;
        for _ in 0..10 {
            let t = Instant::now();
            let hv = if covered {
                hover_in(pv, cx, cy, Instant::now() + HOVER_BUDGET)
            } else {
                match hover_cached(&mut cache, cx, cy) {
                    Hover::Rects(v) => Some(v),
                    other => {
                        println!("hover: {other:?}");
                        None
                    }
                }
            };
            hover_ms.push(format!("{:.0}", t.elapsed().as_secs_f64() * 1000.0));
            answers.push(hv);
        }
        let hits = answers.iter().filter(|a| a.as_ref() == Some(&vec![wr.clone()])).count();
        let misses = answers.iter().filter(|a| a.is_none()).count();
        assert_eq!(hits + misses, 10, "every answer is the word's box or no answer");
        assert!(hits >= 5, "hover answered in time at least half the time: {hover_ms:?}");
        let hover_ms = format!("{} / 10 in time, ms {}", hits, hover_ms.join(" "));
        // The worker's cache hit: the page view's DOM root is already attached.
        let root = dom_root(pv).expect("DOM root");
        let mut cached = Vec::new();
        for _ in 0..10 {
            let t = Instant::now();
            let hv = hover_rects(&mut words(root.clone(), MAX_WORDS), cx as f64, cy as f64, t + HOVER_BUDGET);
            cached.push(format!("{:.0}", t.elapsed().as_secs_f64() * 1000.0));
            assert!(hv.is_none() || hv == Some(vec![wr.clone()]), "cached hover: the word or no answer");
        }
        let hover_ms = format!("{hover_ms}; cached root ms {}", cached.join(" "));
        let gap = hover_in(pv, cx, cy - 200, Instant::now() + Duration::from_secs(1));
        assert_eq!(gap, Some(vec![]), "no word 200 px above the first line");

        let t = Instant::now();
        let picked = if covered {
            println!("NOTE: the word is covered by another window; window lookup skipped");
            assert!(AcroRead::pick(cx, cy).is_err(), "a covered point is not read");
            AcroRead::pick_in(frame, pv, cx, cy)
        } else {
            let under = unsafe { WindowFromPoint(POINT { x: cx, y: cy }) };
            assert!(is_acrobat_page(under.0 as isize), "is_acrobat_page(point)");
            AcroRead::pick(cx, cy)
        };
        let (mut rd, toks) = picked.expect("pick");
        let pick_ms = t.elapsed().as_secs_f64() * 1000.0;
        // once more, warm (the first pick follows the document load)
        let t = Instant::now();
        let again = if covered {
            AcroRead::pick_in(frame, pv, cx, cy)
        } else {
            AcroRead::pick(cx, cy)
        };
        let pick2_ms = t.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(again.map(|(_, t)| t), Ok(toks.clone()), "same tokens again");
        assert_eq!(toks.first(), Some(&word), "first token is the clicked word");
        assert_eq!(rd.hwnd(), frame.0 as isize);
        println!("pick: {} tokens: {:?}...", toks.len(), &toks[..toks.len().min(8)]);

        let n = toks.len().min(20);
        let t = Instant::now();
        for i in 0..n {
            assert!(rd.rects(&[i]).is_some(), "rects({i}) answered");
        }
        let rect_ms = t.elapsed().as_secs_f64() * 1000.0 / n as f64;

        // read on to page 3's marker word
        if toks.len() < FIRST_BATCH {
            // the whole test document fits in the first batch: more() says "end"
            assert_eq!(rd.more(), Ok(vec![]), "end of document");
        }
        let mut all = toks.clone();
        let t = Instant::now();
        let idx = loop {
            if let Some(i) = all.iter().position(|s| s.contains("ZEBRAFINCH")) {
                break i;
            }
            let more = rd.more().expect("more");
            assert!(!more.is_empty(), "document ended before ZEBRAFINCH");
            all.extend(more);
        };
        let more_ms = t.elapsed().as_secs_f64() * 1000.0;
        let page = page_of(&rd.w.nodes[idx]);
        let before = rd.rects(&[idx]);
        println!(
            "ZEBRAFINCH = token {idx} of {}, page {page:?}, rects before scroll {before:?}",
            all.len()
        );
        assert_eq!(before, Some(vec![]), "the page 3 word starts off screen");

        let t = Instant::now();
        rd.scroll_to(idx);
        let scroll_call_ms = t.elapsed().as_secs_f64() * 1000.0;
        let on = loop {
            match rd.rects(&[idx]) {
                Some(v) if !v.is_empty() => break Some(v),
                _ if t.elapsed() > Duration::from_secs(1) => break None,
                _ => std::thread::sleep(Duration::from_millis(20)),
            }
        };
        let scroll_ms = t.elapsed().as_secs_f64() * 1000.0;
        println!("after scroll_to: {on:?}");
        assert!(on.is_some(), "ScrollTo brought the word on screen within 1 s");
        assert_eq!(rd.rects(&[0]), Some(vec![]), "the page 1 word scrolled off screen");

        println!(
            "TIMINGS: hover {hover_ms}, pick {pick_ms:.1} ms (again {pick2_ms:.1} ms; {} tokens), \
             rects {rect_ms:.2} ms/word, more to page 3 {more_ms:.1} ms, \
             ScrollTo call {scroll_call_ms:.1} ms, on screen after {scroll_ms:.0} ms",
            toks.len()
        );
        drop(guard);
    }
}
