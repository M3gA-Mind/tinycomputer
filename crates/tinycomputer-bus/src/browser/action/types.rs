//! Payload types for interacting with the active page.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How an element is named.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    /// A ref from a snapshot, such as `e12`.
    ///
    /// Refs belong to the snapshot that produced them. Acting on a ref after the
    /// page has navigated or re-rendered is refused rather than guessed at —
    /// a stale ref that silently resolves to whatever now occupies that position
    /// is how an agent ends up clicking the wrong thing and reporting success.
    Ref {
        /// The ref identity, with or without its leading `@`.
        value: String,
    },
    /// A CSS selector, matched against the first element it finds.
    Selector {
        /// The selector text.
        value: String,
    },
    /// A semantic locator: role, visible text, label, and so on.
    Locator {
        /// The locator itself.
        value: Locator,
    },
}

impl Target {
    /// Reads a target from the string form a host tool receives from a model.
    ///
    /// A leading `@` means a ref, and anything else is a CSS selector. That rule
    /// exists because the tool surface an agent sees takes one `selector` string
    /// for both, and `@` is not valid at the start of a CSS selector — so the
    /// two vocabularies cannot collide.
    ///
    /// # Examples
    ///
    /// ```
    /// # use tinycomputer_bus::browser::Target;
    /// assert_eq!(Target::parse("@e12"), Target::reference("e12"));
    /// assert_eq!(Target::parse("#submit"), Target::selector("#submit"));
    /// ```
    #[must_use]
    pub fn parse(target: &str) -> Self {
        let trimmed = target.trim();
        match trimmed.strip_prefix('@') {
            Some(reference) => Self::reference(reference),
            None => Self::selector(trimmed),
        }
    }

    /// A target naming the snapshot ref `value`, with any leading `@` removed.
    #[must_use]
    pub fn reference(value: impl AsRef<str>) -> Self {
        Self::Ref {
            value: value.as_ref().trim_start_matches('@').to_string(),
        }
    }

    /// A target naming the first element matching the CSS selector `value`.
    #[must_use]
    pub fn selector(value: impl Into<String>) -> Self {
        Self::Selector {
            value: value.into(),
        }
    }

    /// A target naming an element semantically.
    #[must_use]
    pub fn locator(value: Locator) -> Self {
        Self::Locator { value }
    }
}

/// The dimension a [`Locator`] searches on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocateBy {
    /// The element's accessibility role, narrowed by its accessible name.
    Role,
    /// Visible text content.
    Text,
    /// The text of the element's `<label>`, or its `aria-label`.
    Label,
    /// A form field's placeholder.
    Placeholder,
    /// A `data-testid` attribute.
    TestId,
    /// An image's alternative text.
    AltText,
    /// A `title` attribute.
    Title,
}

/// An element named by what it is rather than where it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Locator {
    /// The dimension to search on.
    pub by: LocateBy,
    /// What to search for. Compared case-insensitively against trimmed text.
    pub value: String,
    /// For [`LocateBy::Role`], the accessible name to narrow the role to.
    pub name: Option<String>,
    /// Require the whole value to match rather than a substring. Defaults to
    /// `false`: an agent naming a button "Submit" should find "Submit order".
    pub exact: bool,
    /// Which match to take when several qualify. Defaults to the first.
    pub index: usize,
}

impl Default for Locator {
    fn default() -> Self {
        Self {
            by: LocateBy::Text,
            value: String::new(),
            name: None,
            exact: false,
            index: 0,
        }
    }
}

impl Locator {
    /// A locator matching `value` on `by`, taking the first match.
    ///
    /// # Examples
    ///
    /// ```
    /// # use tinycomputer_bus::browser::{LocateBy, Locator};
    /// let locator = Locator::new(LocateBy::Role, "button").with_name("Submit");
    /// assert_eq!(locator.name.as_deref(), Some("Submit"));
    /// ```
    #[must_use]
    pub fn new(by: LocateBy, value: impl Into<String>) -> Self {
        Self {
            by,
            value: value.into(),
            ..Self::default()
        }
    }

    /// Narrows a [`LocateBy::Role`] locator to elements with this accessible
    /// name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

/// Which way [`Action::Scroll`] moves the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollDirection {
    /// Towards the end of the document.
    Down,
    /// Towards the start of the document.
    Up,
    /// Towards the inline start.
    Left,
    /// Towards the inline end.
    Right,
    /// All the way to the top.
    Top,
    /// All the way to the bottom.
    Bottom,
}

/// The condition [`Action::WaitFor`] blocks on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitState {
    /// The element exists in the DOM.
    Attached,
    /// The element is gone from the DOM.
    Detached,
    /// The element exists and is rendered.
    ///
    /// The default: an element that exists but is not rendered is one an agent
    /// cannot act on, so waiting for mere existence usually means waiting for
    /// the wrong thing.
    #[default]
    Visible,
    /// The element is absent or not rendered.
    Hidden,
}

/// One interaction with the active page.
///
/// The variants deliberately mirror the verbs an agent-facing browser tool
/// already exposes, so a host tool's dispatch is a rename rather than a
/// translation layer with its own bugs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    /// Click an element, scrolling it into view first.
    ///
    /// Fails when another element covers the click point — a consent banner or a
    /// modal — and names the covering element, because a click that lands on an
    /// overlay reports success and does nothing, which is the single most
    /// expensive failure mode an agent can be handed.
    Click {
        /// The element to click.
        target: Target,
        /// Open the result in a new tab, as a middle click would.
        #[serde(default)]
        new_tab: bool,
    },
    /// Double-click an element.
    DoubleClick {
        /// The element to double-click.
        target: Target,
    },
    /// Move the pointer over an element, firing the hover handlers a menu needs.
    Hover {
        /// The element to hover.
        target: Target,
    },
    /// Give an element keyboard focus without clicking it.
    ///
    /// Refused when the focus does not land on the element (a heading, say,
    /// which takes none), rather than reported done with the focus elsewhere.
    Focus {
        /// The element to focus.
        target: Target,
    },
    /// Clear a field and set its value in one step.
    ///
    /// The field is given the focus first, and the fill is refused unless the
    /// focus lands on it: a value meant for one field never goes into
    /// whichever field last had the focus.
    Fill {
        /// The field to fill.
        target: Target,
        /// The value to set.
        value: String,
    },
    /// Type text as a sequence of key events, leaving anything already there.
    ///
    /// Distinct from [`Action::Fill`] because a field that reacts per keystroke
    /// — an autocomplete, a search-as-you-type box — never sees the input event
    /// a bulk value assignment skips.
    Type {
        /// The field to type into: it is given the focus first, and typing is
        /// refused unless the focus lands on it. Without one, the text goes to
        /// whatever has the focus.
        #[serde(default)]
        target: Option<Target>,
        /// The text to type.
        text: String,
        /// Delay between keystrokes in milliseconds.
        #[serde(default)]
        delay_ms: Option<u64>,
    },
    /// Press a key or chord, such as `Enter`, `Tab`, or `Control+a`.
    Press {
        /// The key or chord.
        key: String,
        /// The element to press it on: it is given the focus first, and the
        /// press is refused unless the focus lands on it, so a key meant for
        /// one element never presses whichever control last had the focus.
        /// Without one, the key goes to whatever has the focus.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<Target>,
    },
    /// Choose one or more options in a `<select>`.
    Select {
        /// The select element.
        target: Target,
        /// The option values to choose.
        values: Vec<String>,
    },
    /// Set a checkbox or radio to a state, rather than toggling it blindly.
    Check {
        /// The checkbox or radio.
        target: Target,
        /// The state to leave it in.
        checked: bool,
    },
    /// Scroll the page, or an element that scrolls within it.
    Scroll {
        /// Which way to scroll.
        direction: ScrollDirection,
        /// How far, in CSS pixels. Defaults to one viewport.
        #[serde(default)]
        pixels: Option<u32>,
        /// The scrollable element. Defaults to the page.
        #[serde(default)]
        target: Option<Target>,
    },
    /// Read an element's text.
    GetText {
        /// The element to read.
        target: Target,
    },
    /// Read one attribute of an element.
    GetAttribute {
        /// The element to read.
        target: Target,
        /// The attribute name.
        attribute: String,
    },
    /// Report whether an element is present and rendered.
    IsVisible {
        /// The element to test.
        target: Target,
    },
    /// Block until a condition holds.
    WaitFor {
        /// An element to wait on.
        #[serde(default)]
        target: Option<Target>,
        /// Text to wait for anywhere in the page.
        #[serde(default)]
        text: Option<String>,
        /// The state `target` must reach.
        #[serde(default)]
        state: WaitState,
        /// A flat delay in milliseconds, applied when neither `target` nor
        /// `text` is given.
        #[serde(default)]
        ms: Option<u64>,
        /// Deadline in milliseconds. Falls back to the session's default.
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
    /// Go back in history.
    Back,
    /// Go forward in history.
    Forward,
    /// Reload the page.
    Reload,
}

/// What an [`Action`] did.
///
/// Every action returns the same shape, including the ones that only read, so a
/// host tool renders one result type instead of fifteen. The reading actions put
/// their answer in [`ActionOutcome::value`]; the acting ones leave it null and
/// are described entirely by the page state they left behind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionOutcome {
    /// The value a reading action produced: a string for
    /// [`Action::GetText`], a boolean for [`Action::IsVisible`], null for an
    /// action that only acts.
    pub value: Value,
    /// Where the page was when the action finished.
    pub page: crate::browser::PageState,
    /// The element the action resolved to, when it resolved one. Reported so an
    /// agent that named a target loosely can see what it actually hit.
    pub matched: Option<String>,
}

impl ActionOutcome {
    /// An outcome with no value, for an action that only acts.
    #[must_use]
    pub fn acted(page: crate::browser::PageState) -> Self {
        Self {
            value: Value::Null,
            page,
            matched: None,
        }
    }

    /// An outcome carrying `value`, for an action that reads.
    #[must_use]
    pub fn read(page: crate::browser::PageState, value: Value) -> Self {
        Self {
            value,
            page,
            matched: None,
        }
    }

    /// Records which element the action resolved to.
    #[must_use]
    pub fn matching(mut self, matched: impl Into<String>) -> Self {
        self.matched = Some(matched.into());
        self
    }
}
