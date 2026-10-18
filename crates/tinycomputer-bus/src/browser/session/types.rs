//! Payload types for opening, listing, and closing sessions.

use serde::{Deserialize, Serialize};

/// The identity of one open session.
///
/// A newtype rather than a bare `String` so a session id cannot be passed where
/// an output id is expected — the two are both opaque strings on the wire, and
/// the compiler is the only thing that will notice them being swapped.
///
/// It serializes as the bare string it wraps, so the wire form is a JSON string
/// and a host that logs one sees the id rather than an object.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(String);

impl SessionId {
    /// Wraps `id` as a session identity.
    ///
    /// # Examples
    ///
    /// ```
    /// # use tinycomputer_bus::browser::SessionId;
    /// assert_eq!(SessionId::new("s-1").as_str(), "s-1");
    /// ```
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The identity as it appears on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for SessionId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl From<&str> for SessionId {
    fn from(id: &str) -> Self {
        Self(id.to_string())
    }
}

/// The page size a session renders at.
///
/// Present because the accessibility snapshot and every coordinate-based
/// interaction depend on layout: a headless default of 800x600 makes a
/// responsive site serve its mobile tree, and an agent then cannot find the
/// navigation an operator sees.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    /// Width in CSS pixels.
    pub width: u32,
    /// Height in CSS pixels.
    pub height: u32,
    /// Device pixel ratio. `1.0` for an ordinary desktop page.
    pub device_scale_factor: f64,
    /// Whether to emulate a touch-capable mobile device.
    pub mobile: bool,
}

impl Viewport {
    /// A desktop viewport of `width` by `height` CSS pixels at 1x.
    ///
    /// # Examples
    ///
    /// ```
    /// # use tinycomputer_bus::browser::Viewport;
    /// let viewport = Viewport::desktop(1280, 800);
    /// assert_eq!((viewport.width, viewport.mobile), (1280, false));
    /// ```
    #[must_use]
    pub fn desktop(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            device_scale_factor: 1.0,
            mobile: false,
        }
    }
}

impl Default for Viewport {
    /// A 1280x800 desktop viewport: wide enough that mainstream sites serve
    /// their desktop layout, small enough that a full-page screenshot of an
    /// ordinary article stays under the module's image cap.
    fn default() -> Self {
        Self::desktop(1280, 800)
    }
}

/// One entry of a client-hint brand list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserAgentBrand {
    /// The brand, such as `Chromium`.
    pub brand: String,
    /// Its version, major only in `brands` and full in `full_version_list`.
    pub version: String,
}

/// The client hints a browser reports beside its `User-Agent`: the
/// `sec-ch-ua*` request headers and `navigator.userAgentData`.
///
/// A browser whose user agent is overridden without these drops them, which a
/// site can tell from a real browser of the same user agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserAgentMetadata {
    /// Brands for `sec-ch-ua` and `navigator.userAgentData.brands`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brands: Option<Vec<UserAgentBrand>>,
    /// Brands with their full versions, for `sec-ch-ua-full-version-list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_version_list: Option<Vec<UserAgentBrand>>,
    /// The browser's full version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_version: Option<String>,
    /// The operating system, such as `macOS`, `Windows` or `Linux`.
    pub platform: String,
    /// The operating system's version.
    #[serde(default)]
    pub platform_version: String,
    /// The CPU architecture, such as `x86` or `arm`.
    #[serde(default)]
    pub architecture: String,
    /// The device model; empty on a desktop.
    #[serde(default)]
    pub model: String,
    /// Whether the browser reports a mobile device.
    #[serde(default)]
    pub mobile: bool,
    /// The CPU's word size, such as `64`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bitness: Option<String>,
    /// Whether a 32-bit browser runs on 64-bit Windows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wow64: Option<bool>,
}

/// What `screen.width`, `screen.height` and `devicePixelRatio` report.
///
/// The page's [`Viewport`] keeps its own size: this is the monitor behind it.
/// A session that sets a screen takes its pixel ratio for the page too, in
/// place of the viewport's.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Screen {
    /// Width in CSS pixels.
    pub width: u32,
    /// Height in CSS pixels.
    pub height: u32,
    /// Device pixel ratio. Defaults to `1.0`.
    #[serde(default = "Screen::default_scale")]
    pub device_scale_factor: f64,
}

impl Screen {
    fn default_scale() -> f64 {
        1.0
    }
}

/// How to obtain the browser a session drives.
///
/// Every field is optional with a documented default, so the common case —
/// "give me a headless Chrome" — is `SessionOptions::default()` and a host only
/// spells the parts it actually cares about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionOptions {
    /// Attach to an already-running browser at this `DevTools` endpoint —
    /// `http://127.0.0.1:9222`, or a `ws://`/`wss://` browser socket — instead
    /// of launching one.
    ///
    /// This is how a host reuses a browser it manages itself, and how a sandbox
    /// points the module at a Chrome running in another container.
    pub endpoint: Option<String>,
    /// Path to the browser binary to launch. Defaults to the first Chrome,
    /// Chromium, or Chrome for Testing build the module finds on this host.
    pub executable: Option<String>,
    /// Run without a visible window. Defaults to `true`: a module loaded into a
    /// daemon usually has no display to draw on.
    pub headless: bool,
    /// The size to render at. Defaults to [`Viewport::default`].
    pub viewport: Viewport,
    /// Overrides the browser's own `User-Agent`.
    pub user_agent: Option<String>,
    /// Profile directory for a launched browser. Defaults to a fresh temporary
    /// directory that is removed when the session closes, so one session's
    /// cookies and logins never leak into the next.
    pub user_data_dir: Option<String>,
    /// Absolute directory where Chrome may save downloads.
    ///
    /// The module creates it when absent and never removes it when the session
    /// closes. Relative paths are refused because they would resolve inside an
    /// unspecified module host working directory.
    pub download_dir: Option<String>,
    /// Extra command-line arguments for a launched browser.
    pub args: Vec<String>,
    /// If non-empty, the only origins this session may show pages from.
    ///
    /// An entry is an origin (`https://example.com`), a host with a leading
    /// dot for its subdomains (`.example.com`), or `*` for any public host,
    /// private and local addresses and names refused. A navigation outside
    /// the list is refused before the browser is asked to make a request; no
    /// call acts on or reads a page outside it; a page a session is taken to
    /// by a click, a key, or a redirect is left and reported; and a first
    /// page outside it is left as the session opens (an attached browser's
    /// tab is kept as it is, and the session opens a blank tab of its own).
    /// Only pages are checked: the files a page loads from other hosts (its
    /// CDN, its APIs, its frames) load as they would in any browser. A name
    /// is never resolved, so under `*` a public name that leads to a local
    /// address is admitted.
    pub allowed_origins: Vec<String>,
    /// Default deadline in milliseconds for operations that do not carry their
    /// own. Defaults to 30 seconds.
    pub default_timeout_ms: u64,
    /// IANA timezone the page reports, such as `Asia/Tokyo`. Defaults to the
    /// host's own.
    ///
    /// Applies to a browser the module launches, not one it attaches to.
    pub timezone: Option<String>,
    /// BCP 47 locale the page reports, such as `ja-JP`. Defaults to the
    /// browser's own. Applies to a launched browser only.
    pub locale: Option<String>,
    /// Languages for the `Accept-Language` request header, comma-separated
    /// without weights (`ja-JP,ja`). The header does not follow `locale`, so
    /// a host that sets one usually sets both. Applies to a launched browser
    /// only.
    pub accept_language: Option<String>,
    /// Client hints to go with `user_agent`. Applies to a launched browser
    /// only.
    pub user_agent_metadata: Option<UserAgentMetadata>,
    /// What the page sees as its screen. Applies to a launched browser only.
    pub screen: Option<Screen>,
    /// Source code run in every page, and every frame, before the page's own
    /// scripts. Applies to a launched browser only.
    pub init_scripts: Vec<String>,
    /// Keep the browser from announcing that an automation tool drives it:
    /// the protocol call that does, `Runtime.enable`, is never made. A
    /// session that asks for it gets no console or page-error capture.
    /// Defaults to `false`. Applies to a launched browser only.
    pub stealth: bool,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            endpoint: None,
            executable: None,
            headless: true,
            viewport: Viewport::default(),
            user_agent: None,
            user_data_dir: None,
            download_dir: None,
            args: Vec::new(),
            allowed_origins: Vec::new(),
            default_timeout_ms: 30_000,
            timezone: None,
            locale: None,
            accept_language: None,
            user_agent_metadata: None,
            screen: None,
            init_scripts: Vec::new(),
            stealth: false,
        }
    }
}

/// What the module is holding for one session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionInfo {
    /// The identity every other member takes.
    pub id: SessionId,
    /// The `ws://` browser endpoint the module is driving.
    ///
    /// Reported so an operator debugging a stuck session can attach `DevTools` to
    /// the same browser rather than guessing which one it is.
    pub endpoint: String,
    /// Whether this browser was launched by the module. A session that attached
    /// to someone else's browser leaves it running when it closes.
    pub launched: bool,
    /// Whether the browser is headless.
    pub headless: bool,
    /// The viewport the session renders at.
    pub viewport: Viewport,
    /// The URL of the page the session is currently driving.
    pub url: String,
    /// The title of that page, empty if it has none yet.
    pub title: String,
}

/// A session named in a request that carries nothing else.
///
/// Every browser member takes one JSON object, like the desktop members, so a
/// model calling it as a tool spells the session by name rather than by
/// position: `{"session": "s-1"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRef {
    /// The session, as [`SessionInfo::id`] reported it.
    pub session: SessionId,
}

/// A request addressed to one session: the session, and the member's own
/// fields beside it in the same object.
///
/// The request's fields are flattened, so a navigation is
/// `{"session": "s-1", "url": "https://example.com"}` and a click is
/// `{"session": "s-1", "action": "click", "target": {"kind": "ref", "value": "e3"}}`
/// — one flat object a model can write, not a positional pair.
///
/// # Examples
///
/// ```
/// # use tinycomputer_bus::browser::{NavigateRequest, SessionId, SessionRequest};
/// let request = SessionRequest::new(SessionId::new("s-1"), NavigateRequest::new("https://example.com"));
/// let wire = serde_json::to_value(&request)?;
/// assert_eq!(wire["session"], "s-1");
/// assert_eq!(wire["url"], "https://example.com");
/// # Ok::<(), serde_json::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionRequest<T> {
    /// The session, as [`SessionInfo::id`] reported it.
    pub session: SessionId,
    /// The member's own request.
    #[serde(flatten)]
    pub request: T,
}

impl<T> SessionRequest<T> {
    /// Addresses `request` to `session`.
    #[must_use]
    pub fn new(session: SessionId, request: T) -> Self {
        Self { session, request }
    }
}
