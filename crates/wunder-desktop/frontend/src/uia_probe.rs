//! Element-level refinement for the screenshot selector.
//!
//! The window tree in [`crate::screen_capture`] only sees real HWNDs, so a
//! browser, an Electron app or a self-drawn chat client offers exactly one
//! rectangle. This probe asks Windows UI Automation for the element under the
//! pointer inside that window and walks down the control view until the
//! smallest element containing the point is found.
//!
//! Everything runs on one dedicated MTA thread: UIA calls cross into the target
//! process and can block for an unbounded time when that process is busy, so
//! they must never run on the selector's UI thread. Stale requests are dropped
//! before the call, the selector re-validates the answer against the pointer
//! generation, and a failure simply leaves the HWND rectangle in place.
//!
//! `windows-sys` exposes only the UIAutomationCore C entry points, not the
//! `IUIAutomation` client interface, which is why this module pulls in the
//! `windows` crate. Creation can fail on a machine without UIA; the caller then
//! keeps using the window tree.

#[cfg(windows)]
pub use imp::{spawn, Answer, Probe};

#[cfg(windows)]
mod imp {
    use std::sync::mpsc::{sync_channel, SyncSender};
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationCacheRequest, IUIAutomationElement,
        IUIAutomationTreeWalker, UIA_BoundingRectanglePropertyId, UIA_ControlTypePropertyId,
    };
    use std::sync::OnceLock;

    /// Deepest useful nesting of the control view. Web documents and panes sit
    /// a handful of levels below the frame; deeper walks only cost calls.
    const MAX_DEPTH: usize = 16;
    /// Siblings inspected per level. A list view can have hundreds; the element
    /// under the pointer is normally near the front of the control view.
    const MAX_SIBLINGS: usize = 32;
    /// Hard cap on elements inspected for one probe, so a pathological tree
    /// cannot turn a key press into a second-long walk.
    const MAX_NODES: usize = 160;
    /// Time budget for one walk. The selector already shows the window
    /// rectangle, so a late refinement is worse than a coarse one.
    const WALK_BUDGET: Duration = Duration::from_millis(280);
    /// Chromium builds its accessibility tree lazily on the first UIA query.
    /// When the first answer is still a near-full-window container, wait briefly
    /// and ask again; the second walk usually sees the real controls.
    const RETRY_DELAY: Duration = Duration::from_millis(90);
    /// Reuse a confident answer for the same frozen point, so pressing the
    /// detection key twice is instant instead of repeating the walk.
    const ANSWER_TTL: Duration = Duration::from_secs(2);

    // ControlType ids from UIAutomationCore. The crate exposes them as
    // constants, but only a handful matter here; numeric values keep the
    // candidate ranking readable without a long import list.
    const CT_IMAGE: i32 = 50006;
    const CT_TEXT: i32 = 50020;
    const CT_HEADER_ITEM: i32 = 50035;
    const CT_TREE_ITEM: i32 = 50024;
    const CT_DATA_ITEM: i32 = 50029;
    const CT_SPLIT_BUTTON: i32 = 50031;

    fn is_atomic_control(kind: i32) -> bool {
        matches!(
            kind,
            50000 // Button
                | 50002 // CheckBox
                | 50003 // ComboBox
                | 50004 // Edit
                | 50005 // Hyperlink
                | 50007 // ListItem
                | 50011 // MenuItem
                | 50013 // RadioButton
                | 50015 // Slider
                | 50016 // Spinner
                | 50019 // TabItem
        ) || kind == CT_HEADER_ITEM
            || kind == CT_TREE_ITEM
            || kind == CT_DATA_ITEM
            || kind == CT_SPLIT_BUTTON
    }

    /// Text and image leaves are useful end points, but descending into their
    /// children would split a paragraph or an icon into meaningless glyphs.
    fn terminal_by_size(kind: i32, area: i64, parent_area: i64) -> bool {
        (kind == CT_TEXT || kind == CT_IMAGE) && area * 100 < parent_area * 60
    }

    fn trace_on() -> bool {
        static ON: OnceLock<bool> = OnceLock::new();
        *ON.get_or_init(|| std::env::var_os("WUNDER_SHOT_DEBUG").is_some())
    }

    macro_rules! trace {
        ($($arg:tt)*) => {
            if trace_on() {
                eprintln!("[uia] {}", format_args!($($arg)*));
            }
        };
    }

    /// One refinement request. Coordinates are physical screen pixels.
    pub struct Request {
        hwnd: isize,
        x: i32,
        y: i32,
        epoch: u64,
        generation: i32,
    }

    /// A refined rectangle in physical screen pixels, tagged with the capture
    /// epoch and the pointer generation it was asked for.
    pub struct Answer {
        pub epoch: u64,
        pub generation: i32,
        pub rect: RECT,
    }

    pub struct Probe {
        sender: SyncSender<Request>,
    }

    impl Probe {
        pub fn request(&self, hwnd: isize, x: i32, y: i32, epoch: u64, generation: i32) {
            // A blocked target can stall the probe for seconds; a one-slot
            // queue keeps the newest position without growing or blocking the
            // selector's UI thread.
            let _ = self.sender.try_send(Request {
                hwnd,
                x,
                y,
                epoch,
                generation,
            });
        }
    }

    /// Start the probe thread. Returns `None` when the thread cannot be
    /// created; the selector then stays on window-level rectangles.
    pub fn spawn(on_answer: impl Fn(Answer) + Send + Sync + 'static) -> Option<Probe> {
        let (sender, receiver) = sync_channel::<Request>(1);
        let on_answer = Arc::new(on_answer);
        std::thread::Builder::new()
            .name("wunder-uia-probe".to_string())
            .spawn(move || {
                // UIA clients belong in an MTA; a failed init (or a machine
                // without UIAutomationCore) ends the thread and the feature
                // degrades to the window tree.
                if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
                    return;
                }
                let Ok(automation) = (unsafe {
                    CoCreateInstance::<_, IUIAutomation>(
                        &CUIAutomation,
                        None,
                        CLSCTX_INPROC_SERVER,
                    )
                }) else {
                    return;
                };
                let walker = unsafe { automation.ControlViewWalker() }.ok();
                // Batch the bounding rectangle into every traversal call: one
                // cross-process round trip per child instead of a separate
                // property fetch, which is the difference between a responsive
                // key press and a visible delay on deep Chromium trees.
                let cache = unsafe { automation.CreateCacheRequest() }
                    .ok()
                    .and_then(|request| {
                        unsafe {
                            request
                                .AddProperty(UIA_BoundingRectanglePropertyId)
                                .ok()?;
                            request.AddProperty(UIA_ControlTypePropertyId).ok()?;
                        }
                        Some(request)
                    });
                // A confident answer for a frozen point is reusable; a coarse
                // near-full-window answer is not, because the tree may have
                // finished building since the last press.
                let mut memo: Option<(isize, i32, i32, RECT, Instant)> = None;
                while let Ok(mut request) = receiver.recv() {
                    // Only the newest point is worth a cross-process call;
                    // earlier ones describe a place the user already left.
                    while let Ok(newer) = receiver.try_recv() {
                        request = newer;
                    }
                    if let Some((hwnd, x, y, rect, at)) = memo {
                        if hwnd == request.hwnd
                            && x == request.x
                            && y == request.y
                            && at.elapsed() < ANSWER_TTL
                        {
                            on_answer(Answer {
                                epoch: request.epoch,
                                generation: request.generation,
                                rect,
                            });
                            continue;
                        }
                    }
                    let Some(first) = locate(
                        &automation,
                        walker.as_ref(),
                        cache.as_ref(),
                        request.hwnd,
                        request.x,
                        request.y,
                    ) else {
                        continue;
                    };
                    let mut located = first;
                    if !located.confident {
                        // Give a lazily built accessibility tree one more
                        // chance; the window rectangle is already on screen,
                        // so this only delays the refinement.
                        std::thread::sleep(RETRY_DELAY);
                        if let Some(second) = locate(
                            &automation,
                            walker.as_ref(),
                            cache.as_ref(),
                            request.hwnd,
                            request.x,
                            request.y,
                        ) {
                            if second.confident || area(second.rect) < area(located.rect) {
                                located = second;
                            }
                        }
                    }
                    if !located.confident {
                        // A near-window slab is not a control. Keeping the
                        // window selection is more predictable than replacing
                        // it with a rectangle that merely trims the frame.
                        trace!("drop coarse answer rect={:?}", located.rect);
                        memo = None;
                        continue;
                    }
                    memo = Some((
                        request.hwnd,
                        request.x,
                        request.y,
                        located.rect,
                        Instant::now(),
                    ));
                    on_answer(Answer {
                        epoch: request.epoch,
                        generation: request.generation,
                        rect: located.rect,
                    });
                }
            })
            .ok()
            .map(|_| Probe { sender })
    }

    fn contains(rect: RECT, x: i32, y: i32) -> bool {
        x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
    }

    fn rect_of(element: &IUIAutomationElement) -> Option<RECT> {
        let rect = unsafe { element.CurrentBoundingRectangle() }.ok()?;
        (rect.right > rect.left && rect.bottom > rect.top).then_some(rect)
    }

    /// Rectangle already cached by the traversal call. Using the cached value
    /// avoids one cross-process property fetch per sibling.
    fn cached_rect(element: &IUIAutomationElement) -> Option<RECT> {
        let rect = unsafe { element.CachedBoundingRectangle() }.ok()?;
        (rect.right > rect.left && rect.bottom > rect.top).then_some(rect)
    }

    fn cached_kind(element: &IUIAutomationElement) -> i32 {
        unsafe { element.CachedControlType() }
            .map(|kind| kind.0 as i32)
            .unwrap_or(0)
    }

    struct Located {
        rect: RECT,
        /// The answer is meaningfully smaller than the window itself. A coarse
        /// near-full-window container is retried and never memoized.
        confident: bool,
    }

    /// Smallest meaningful control that contains the point, starting at the
    /// window itself. A rendering surface (CEF's `Intermediate D3D Window`,
    /// for example) covers the same pixels as the real UI container but has no
    /// children, so a plain "first containing child" walk would stop there
    /// forever. A leaf that covers nearly the whole parent is treated as such a
    /// surface; a genuinely small leaf (an icon or a paragraph) is allowed to
    /// win over a larger container, which is what makes the outline land on the
    /// control the user pointed at.
    ///
    /// ControlType matters more than area: a button must win over the text run
    /// inside it, and a list row over its icon. The walk therefore stops at
    /// atomic controls instead of drilling down to a glyph.
    fn locate(
        automation: &IUIAutomation,
        walker: Option<&IUIAutomationTreeWalker>,
        cache: Option<&IUIAutomationCacheRequest>,
        hwnd: isize,
        x: i32,
        y: i32,
    ) -> Option<Located> {
        let window = unsafe { automation.ElementFromHandle(HWND(hwnd)) }.ok()?;
        let root = rect_of(&window)?;
        let root_area = area(root);
        let root_kind = unsafe { window.CurrentControlType() }
            .map(|kind| kind.0 as i32)
            .unwrap_or(0);
        trace!("walk hwnd={hwnd} point=({x},{y}) root={:?} kind={root_kind}", root);
        let (Some(walker), Some(cache)) = (walker, cache) else {
            return Some(Located {
                rect: root,
                confident: false,
            });
        };
        let started = Instant::now();
        let mut best = root;
        let mut best_kind = root_kind;
        let mut node = window;
        let mut budget = MAX_NODES;
        let deadline = Instant::now() + WALK_BUDGET;
        for depth in 0..MAX_DEPTH {
            if budget == 0 || Instant::now() >= deadline {
                trace!("walk stop depth={depth} budget={budget} time={:?}", started.elapsed());
                break;
            }
            let parent_area = area(best);
            trace!("depth={depth} parent={best:?} kind={best_kind} area={parent_area}");
            let mut child = unsafe { walker.GetFirstChildElementBuildCache(&node, cache) }.ok();
            let mut chosen: Option<(IUIAutomationElement, RECT, bool, i32)> = None;
            let mut seen = 0;
            while let Some(current) = child {
                if seen >= MAX_SIBLINGS || budget == 0 || Instant::now() >= deadline {
                    break;
                }
                seen += 1;
                budget = budget.saturating_sub(1);
                if let Some(rect) = cached_rect(&current) {
                    if contains(rect, x, y) {
                        budget = budget.saturating_sub(1);
                        let kind = cached_kind(&current);
                        // Atomic controls are endpoints; asking the provider
                        // for children only to set `has_children=false` costs
                        // an extra cross-process call on every button/row.
                        let has_children = if is_atomic_control(kind) {
                            false
                        } else {
                            budget = budget.saturating_sub(1);
                            unsafe { walker.GetFirstChildElementBuildCache(&current, cache) }.is_ok()
                        };
                        trace!(
                            "  depth={depth} candidate={rect:?} kind={kind} children={has_children}"
                        );
                        let better = prefer_candidate(
                            rect,
                            has_children,
                            kind,
                            parent_area,
                            chosen.as_ref().map(|(_, rect, children, kind)| (*rect, *children, *kind)),
                        );
                        if better {
                            chosen = Some((current.clone(), rect, has_children, kind));
                        }
                    }
                }
                child = unsafe { walker.GetNextSiblingElementBuildCache(&current, cache) }.ok();
            }
            let Some((next, rect, has_children, kind)) = chosen else {
                trace!("walk end depth={depth} best={best:?} kind={best_kind} time={:?}", started.elapsed());
                break;
            };
            best = rect;
            best_kind = kind;
            node = next;
            if should_stop(kind, rect, parent_area, has_children) {
                trace!("walk stop at depth={depth} rect={rect:?} kind={kind}");
                break;
            }
        }
        let confident = is_confident(best_kind, area(best), root_area);
        trace!(
            "walk done rect={best:?} kind={best_kind} confident={confident} time={:?}",
            started.elapsed()
        );
        Some(Located {
            rect: best,
            confident,
        })
    }

    fn area(rect: RECT) -> i64 {
        let width = (rect.right - rect.left).max(0) as i64;
        let height = (rect.bottom - rect.top).max(0) as i64;
        width * height
    }

    /// A childless rectangle that spans almost the whole parent is a painting
    /// surface, not a control: CEF/Chromium exposes one next to the real tree.
    fn is_surface(area: i64, parent_area: i64) -> bool {
        parent_area > 0 && area * 100 >= parent_area * 92
    }

    /// A control answer is worth showing only when it is clearly more specific
    /// than the window. An atomic control (even a large editor) is a real
    /// target; a text/container slab that merely trims the title bar is not.
    fn is_confident(kind: i32, area: i64, root_area: i64) -> bool {
        if is_atomic_control(kind) {
            return true;
        }
        root_area > 0 && area * 100 <= root_area * 70
    }

    /// `0` atomic control, `1` small text/image end point, `2` container,
    /// `3` painting surface. Lower ranks win.
    fn rank(kind: i32, has_children: bool, area: i64, parent_area: i64) -> u8 {
        if !has_children && is_surface(area, parent_area) {
            return 3;
        }
        if is_atomic_control(kind) {
            return 0;
        }
        if terminal_by_size(kind, area, parent_area) {
            return 1;
        }
        2
    }

    /// Stop the walk once the chosen control is specific enough. A document
    /// sized Text element is still a container; descending into it is how a
    /// paragraph-sized outline is found.
    fn should_stop(kind: i32, rect: RECT, parent_area: i64, has_children: bool) -> bool {
        if is_atomic_control(kind) {
            return true;
        }
        if !has_children {
            return true;
        }
        terminal_by_size(kind, area(rect), parent_area)
    }

    /// Whether a containing child should replace the incumbent candidate.
    /// Surfaces lose to real controls; atomic controls (buttons, rows, tabs)
    /// beat containers and text runs inside them; otherwise the smaller,
    /// more specific rectangle wins.
    fn prefer_candidate(
        rect: RECT,
        has_children: bool,
        kind: i32,
        parent_area: i64,
        current: Option<(RECT, bool, i32)>,
    ) -> bool {
        let candidate_area = area(rect);
        let candidate_rank = rank(kind, has_children, candidate_area, parent_area);
        match current {
            None => true,
            Some((best, best_children, best_kind)) => {
                let best_area = area(best);
                let best_rank = rank(best_kind, best_children, best_area, parent_area);
                if candidate_rank != best_rank {
                    return candidate_rank < best_rank;
                }
                // A clearly smaller rectangle is the more specific control.
                if candidate_area * 100 < best_area * 70 {
                    return true;
                }
                if best_area * 100 < candidate_area * 70 {
                    return false;
                }
                (has_children && !best_children)
                    || (has_children == best_children && candidate_area < best_area)
            }
        }
    }

    /// Test-only: print the control-view tree of a window (bounded) so a
    /// machine-specific layout can be inspected without guessing.
    #[cfg(test)]
    pub(crate) fn debug_tree(hwnd: isize) {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
                .expect("uia automation");
        let walker = unsafe { automation.ControlViewWalker() }.expect("control view walker");
        let window = unsafe { automation.ElementFromHandle(HWND(hwnd)) }.expect("window element");
        print_element("root", &window);
        dump_children(&walker, &window, 0);
    }

    #[cfg(test)]
    fn dump_children(walker: &IUIAutomationTreeWalker, parent: &IUIAutomationElement, depth: usize) {
        if depth >= 10 {
            return;
        }
        let Ok(mut child) = (unsafe { walker.GetFirstChildElement(parent) }) else {
            return;
        };
        for index in 0..2 {
            print_element(&format!("{}-{index}", "  ".repeat(depth)), &child);
            dump_children(walker, &child, depth + 1);
            match unsafe { walker.GetNextSiblingElement(&child) } {
                Ok(next) => child = next,
                Err(_) => break,
            }
        }
    }

    #[cfg(test)]
    fn print_element(label: &str, element: &IUIAutomationElement) {
        let name = unsafe { element.CurrentName() }
            .map(|name| name.to_string())
            .unwrap_or_default();
        let class = unsafe { element.CurrentClassName() }
            .map(|class| class.to_string())
            .unwrap_or_default();
        let kind = unsafe { element.CurrentControlType() }
            .map(|kind| kind.0)
            .unwrap_or_default();
        let rect = rect_of(element);
        println!(
            "uia: {label} type={kind} class={class:?} name={name:?} rect={rect:?}"
        );
    }

    #[cfg(test)]
    mod candidate_tests {
        use super::*;

        fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
            RECT {
                left,
                top,
                right,
                bottom,
            }
        }

        #[test]
        fn a_container_beats_a_leaf_surface_even_when_larger() {
            // CEF paints its UI into a slightly shorter "Intermediate D3D
            // Window" that has no children; the real BrowserRootView is one
            // pixel taller but owns the whole element tree.
            let parent_area = 1463 * 870;
            let surface = rect(0, 0, 1463, 866);
            assert!(prefer_candidate(surface, false, 0, parent_area, None));
            assert!(prefer_candidate(
                rect(0, 0, 1463, 867),
                true,
                0,
                parent_area,
                Some((surface, false, 0))
            ));
            assert!(!prefer_candidate(
                surface,
                false,
                0,
                parent_area,
                Some((rect(0, 0, 1463, 867), true, 0))
            ));
        }

        #[test]
        fn same_kind_prefers_the_smaller_rectangle() {
            let parent_area = 100 * 100;
            assert!(prefer_candidate(
                rect(10, 10, 20, 20),
                true,
                0,
                parent_area,
                Some((rect(0, 0, 100, 100), true, 0))
            ));
            assert!(!prefer_candidate(
                rect(0, 0, 100, 100),
                true,
                0,
                parent_area,
                Some((rect(10, 10, 20, 20), true, 0))
            ));
        }

        #[test]
        fn a_small_leaf_beats_a_much_larger_container() {
            // A toolbar icon can be exposed as a childless element; it is the
            // control the user pointed at, so it must beat the toolbar body.
            let parent_area = 800 * 600;
            let icon = rect(10, 10, 34, 34);
            let toolbar = rect(0, 0, 400, 60);
            assert!(prefer_candidate(
                icon,
                false,
                CT_IMAGE,
                parent_area,
                Some((toolbar, true, 0))
            ));
            assert!(!prefer_candidate(
                toolbar,
                true,
                0,
                parent_area,
                Some((icon, false, CT_IMAGE))
            ));
        }

        #[test]
        fn a_near_window_text_slab_is_not_a_confident_target() {
            let root_area = 1000 * 800;
            let slab = rect(0, 40, 1000, 760);
            assert!(!is_confident(CT_TEXT, area(slab), root_area));
            let paragraph = rect(40, 60, 700, 240);
            assert!(is_confident(CT_TEXT, area(paragraph), root_area));
            assert!(is_confident(50000, area(slab), root_area));
        }

        #[test]
        fn an_atomic_button_beats_the_text_inside_it() {
            // The text run is smaller, but the button is what a screenshot
            // should snap to when the pointer is on its label.
            let parent_area = 800 * 600;
            let button = rect(100, 100, 220, 140);
            let label = rect(120, 112, 190, 128);
            assert!(prefer_candidate(
                button,
                true,
                50000,
                parent_area,
                Some((label, false, CT_TEXT))
            ));
            assert!(!prefer_candidate(
                label,
                false,
                CT_TEXT,
                parent_area,
                Some((button, true, 50000))
            ));
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use std::sync::Mutex;
    use std::time::Duration;

    /// Manual smoke test: there is no desktop in CI, and the answer depends on
    /// whatever window is under the cursor. Run with
    /// `cargo test --bin wunder-desktop uia_probe -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads the live desktop and needs a window under the cursor"]
    fn probe_answers_for_the_window_under_the_cursor() {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetAncestor, GetCursorPos, GA_ROOT, WindowFromPoint,
        };

        let mut point = POINT { x: 0, y: 0 };
        assert_ne!(unsafe { GetCursorPos(&mut point) }, 0, "cursor position");
        let foreground = unsafe { WindowFromPoint(point) };
        let root = unsafe { GetAncestor(foreground, GA_ROOT) };
        assert_ne!(root, 0, "window under cursor");
        imp::debug_tree(root as isize);

        let (sender, receiver) = channel();
        let sender = Mutex::new(sender);
        let probe = spawn(move |answer| {
            let _ = sender.lock().expect("probe channel").send(answer);
        })
        .expect("uia probe thread");
        probe.request(root as isize, point.x, point.y, 1, 1);
        let answer = receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("uia answer");
        println!(
            "uia: root={root} cursor=({}, {}) rect=({}, {}, {}, {})",
            point.x,
            point.y,
            answer.rect.left,
            answer.rect.top,
            answer.rect.right,
            answer.rect.bottom
        );
        assert!(answer.rect.right > answer.rect.left);
        assert!(answer.rect.bottom > answer.rect.top);
    }
}
