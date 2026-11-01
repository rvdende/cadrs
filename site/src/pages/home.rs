use std::sync::OnceLock;

use proa_core::{perf_ok, DataLoader, StaticRaw, WebContext, WebRenderSync, WriteError};
use proa_core::{MdRenderSync, WriteBuf};
use proa_macros::html_sync;
use proa_macros::md_sync;

use crate::components::{
    button_recipe, Badge, BadgeVariant, ButtonRecipeProps, ButtonSize, ButtonStyle, ButtonVariant,
    Card, CardContent, CardDescription, CardHeader, CardTitle, Icon, IconSize, Separator,
    SeparatorOrientation,
};
use crate::github::GithubButton;
use crate::icons::IconRs;
use crate::theme::ThemeSwitcher;

const RELEASES: &str = "https://github.com/rvdende/cadrs/releases";
const LATEST_RELEASE: &str = "https://github.com/rvdende/cadrs/releases/latest";

/// What the download links point at, worked out once.
struct Release {
    label: String,
    linux: String,
    windows: String,
}

/// Points the download links and the release label at the latest GitHub release when the page
/// loads, so a release published, replaced or deleted since the export shows without a redeploy
/// (kept for ten minutes per browser session, as unauthenticated calls are rate-limited). With no
/// release left the links go to the releases page. A platform the latest release has no file for
/// takes the newest pre-release's that has one, and shows its `data-release-pre` mark ("beta":
/// macOS, while only betas have its disk image); with none it goes where its link's
/// `data-release-fallback` says (macOS: the build notes), else to the release's own page.
pub const RELEASE_SCRIPT: &str = "<script>(function(){\
var key=\"cadrs-releases\",releases=\"https://github.com/rvdende/cadrs/releases\";\
var suffix={windows:\"-x86_64-windows.zip\",linux:\"-x86_64-linux.tar.gz\",macos:\"-universal-macos.dmg\"};\
function show(r){var l=document.querySelectorAll(\"[data-release-label]\");for(var i=0;i<l.length;i++)\
l[i].textContent=r?\"Latest release: \"+r.tag:\"Latest release\";\
var a=document.querySelectorAll(\"[data-release-asset]\");for(var i=0;i<a.length;i++){\
var p=a[i].getAttribute(\"data-release-asset\"),f=a[i].getAttribute(\"data-release-fallback\");\
a[i].href=r?r.assets[p]||f||r.page:f||releases;\
var m=a[i].querySelector(\"[data-release-pre]\");if(m)m.classList.toggle(\"hidden\",!(r&&r.pre[p]))}}\
function ready(f){if(document.readyState===\"loading\")document.addEventListener(\"DOMContentLoaded\",f);else f()}\
try{var c=JSON.parse(sessionStorage.getItem(key));if(c&&Date.now()-c.t<6e5){ready(function(){show(c.r)});return}}catch(e){}\
fetch(\"https://api.github.com/repos/rvdende/cadrs/releases?per_page=20\").then(function(res){\
return res.ok?res.json():null}).then(function(js){if(!js)return;var r=null,pre={};\
js.forEach(function(j){if(j.draft)return;var as={};(j.assets||[]).forEach(function(x){\
for(var p in suffix)if(x.name.slice(-suffix[p].length)===suffix[p])as[p]=x.browser_download_url});\
if(!j.prerelease&&!r)r={tag:j.tag_name,page:j.html_url||releases,assets:as,pre:{}};\
if(j.prerelease)for(var p in as)if(!pre[p])pre[p]=as[p]});\
if(r)for(var p in pre)if(!r.assets[p]){r.assets[p]=pre[p];r.pre[p]=true}\
try{sessionStorage.setItem(key,JSON.stringify({r:r,t:Date.now()}))}catch(e){}ready(function(){show(r)})})\
.catch(function(){})\
})()</script>";

/// The release the download links point at. `export.sh` sets `CADRS_RELEASE` to the latest
/// GitHub release's tag, such as `v0.1.5`; without it the links go to the latest release's page.
/// [`RELEASE_SCRIPT`] updates them when the page loads.
fn release() -> &'static Release {
    static RELEASE: OnceLock<Release> = OnceLock::new();
    RELEASE.get_or_init(|| {
        let asset = |tag: &str, platform: &str, extension: &str| {
            // Built once per process, not per render.
            perf_ok(format!(
                "{RELEASES}/download/{tag}/cadrs-{tag}-x86_64-{platform}.{extension}"
            ))
        };
        match std::env::var("CADRS_RELEASE") {
            Ok(tag) if !tag.is_empty() => Release {
                label: perf_ok(format!("Latest release: {tag}")),
                linux: asset(&tag, "linux", "tar.gz"),
                windows: asset(&tag, "windows", "zip"),
            },
            _ => Release {
                label: "Latest release".into(),
                linux: LATEST_RELEASE.into(),
                windows: LATEST_RELEASE.into(),
            },
        }
    })
}

struct Feature {
    title: &'static str,
    body: &'static str,
    /// A screenshot beside the text, for the light and the dark theme, with its size.
    shot: Option<Shot>,
    /// Grid classes for the feature's card in the bento layout.
    class: &'static str,
}

/// A screenshot of the app in each theme; the page shows the one matching its own.
#[derive(Clone, Copy)]
struct Shot {
    light: &'static str,
    dark: &'static str,
    width: u32,
    height: u32,
    alt: &'static str,
}

const DOCUMENTS: Shot = Shot {
    light: "/images/documents-light.webp",
    dark: "/images/documents-dark.webp",
    width: 1920,
    height: 1049,
    alt: "The documents page: a folder of documents with thumbnails, sorted by when they were modified",
};

const SCHEMATIC: Shot = Shot {
    light: "/images/schematic-light.webp",
    dark: "/images/schematic-dark.webp",
    width: 1600,
    height: 1000,
    alt: "A schematic in a PCB Studio: a battery, a resistor and an LED between VCC and GND",
};

/// The render isn't of the app's chrome, so one image serves both themes.
const RENDER: Shot = Shot {
    light: "/images/render.png",
    dark: "/images/render.png",
    width: 1920,
    height: 1080,
    alt: "A path-traced render of a two-part lever",
};

/// The bento grid of features, in reading order: three columns from `lg`.
const FEATURES: &[Feature] = &[
    Feature {
        title: "Part Studios",
        body: "Extrude, revolve, sweep, loft, fillet, chamfer, shell, draft, holes, patterns and booleans. Every feature stays editable, and the part rebuilds.",
        shot: None,
        class: "",
    },
    Feature {
        title: "Sketching",
        body: "Lines, arcs, splines, ellipses, slots, polygons and text, with snapping and inference. Geometric constraints and driving dimensions are solved live while you drag.",
        shot: None,
        class: "",
    },
    Feature {
        title: "Assemblies",
        body: "Fastened, revolute, slider, cylindrical and planar mates, mate connectors, exploded views and a bill of materials.",
        shot: None,
        class: "",
    },
    Feature {
        title: "PCB Studio",
        body: "Schematic, board layout and routing in the same document as the enclosure. Gerber and drill files for the board house, IDF to and from other EDA tools.",
        shot: Some(SCHEMATIC),
        class: "overflow-hidden md:col-span-2",
    },
    Feature {
        title: "Drawings",
        body: "Standard and projected views, sections, details, dimensions, annotations, callouts and BOM tables.",
        shot: None,
        class: "",
    },
    Feature {
        title: "Rendering and simulation",
        body: "A path-traced renderer, linear static FEA, and measurement and mass properties.",
        shot: Some(RENDER),
        class: "overflow-hidden md:col-span-2",
    },
    Feature {
        title: "Versions and branches",
        body: "Every document keeps its history: name a version, branch from it to try an idea, merge the branch back. Link a part into another document at a version.",
        shot: None,
        class: "",
    },
    Feature {
        title: "Your documents, on your computer",
        body: "Folders, labels, search, a trash you can restore from, and thumbnails. Documents live in a folder you own; nothing is uploaded.",
        shot: Some(DOCUMENTS),
        class: "overflow-hidden md:col-span-2",
    },
    Feature {
        title: "Sharing, peer to peer",
        body: "Share a document with someone by their cadrs ID and edit it together, with comments and Follow mode. The two apps connect directly; no server keeps a copy.",
        shot: None,
        class: "",
    },
];

/// A screenshot in both themes, linking to the full-size image.
fn shot_classes(shot: &Shot) -> (&'static str, &'static str) {
    if shot.light == shot.dark {
        ("block h-auto w-full", "hidden")
    } else {
        (
            "block h-auto w-full dark:hidden",
            "hidden h-auto w-full dark:block",
        )
    }
}

struct ShotImages {
    shot: Shot,
    /// The first image on the page loads at once; the others when scrolled to.
    eager: bool,
}

impl<L: DataLoader> WebRenderSync<L> for ShotImages {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let shot = self.shot;
        let (light_class, dark_class) = shot_classes(&shot);
        let loading = if self.eager { "eager" } else { "lazy" };
        html_sync! {
            <img
                src={cx.request().url(shot.light)}
                width={shot.width}
                height={shot.height}
                alt={shot.alt}
                loading={loading}
                class={light_class}
                data-shot="light"
            />
        }
        .render(cx)?;
        if shot.light != shot.dark {
            html_sync! {
                <img
                    src={cx.request().url(shot.dark)}
                    width={shot.width}
                    height={shot.height}
                    alt={shot.alt}
                    loading={loading}
                    class={dark_class}
                    data-shot="dark"
                />
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct Features;

impl<L: DataLoader> WebRenderSync<L> for Features {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        for feature in FEATURES {
            let header = html_sync! {
                {CardHeader {
                    class: None,
                    children: Some(html_sync! {
                        {CardTitle { class: Some("text-lg"), children: Some(feature.title) }}
                        {CardDescription {
                            class: Some("text-base leading-7"),
                            children: Some(feature.body),
                        }}
                    }),
                }}
            };
            match feature.shot {
                Some(shot) => {
                    html_sync! {
                        {Card {
                            class: Some(feature.class),
                            children: Some(html_sync! {
                                <div class="grid h-full gap-2 xl:grid-cols-[0.75fr_1.25fr]">
                                    <div class="flex flex-col justify-center">{header}</div>
                                    <a
                                        href={cx.request().url(shot.light)}
                                        class="block self-end overflow-hidden px-6 pb-6 xl:px-0 xl:pt-6 xl:pb-0 [&>img]:rounded-lg [&>img]:border [&>img]:border-border xl:[&>img]:rounded-tr-none xl:[&>img]:rounded-b-none xl:[&>img]:border-r-0 xl:[&>img]:border-b-0"
                                    >
                                        {ShotImages { shot, eager: false }}
                                    </a>
                                </div>
                            }),
                        }}
                    }
                    .render(cx)?;
                }
                None => {
                    html_sync! {
                        {Card { class: Some(feature.class), children: Some(header) }}
                    }
                    .render(cx)?;
                }
            }
        }
        Ok(())
    }
}

/// Link styling from the Proa UI button recipe, for links that look like buttons.
fn link_button(size: ButtonSize, variant: ButtonVariant) -> ButtonStyle {
    button_recipe().resolve(ButtonRecipeProps {
        size: Some(size),
        variant: Some(variant),
        ..ButtonRecipeProps::default()
    })
}

/// One button per desktop platform: the latest release's archive for Windows and Linux, and the
/// newest disk image for macOS (the page's script finds it, marked "beta" while it comes from a
/// pre-release; the build notes until there is one).
struct PlatformButtons;

impl<L: DataLoader> WebRenderSync<L> for PlatformButtons {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let release = release();
        let (windows, linux) = (release.windows.as_str(), release.linux.as_str());
        html_sync! {
            <div class="flex flex-wrap gap-3">
                <a
                    href={windows}
                    data-release-asset="windows"
                    class={link_button(ButtonSize::Lg, ButtonVariant::Solid)}
                >
                    {Icon {
                        icon: IconRs(icon_rs::PLATFORM_WINDOWS),
                        size: Some(IconSize::Sm),
                        class: None,
                    }}
                    "Windows"
                </a>
                <a
                    href={linux}
                    data-release-asset="linux"
                    class={link_button(ButtonSize::Lg, ButtonVariant::Solid)}
                >
                    {Icon {
                        icon: IconRs(icon_rs::PLATFORM_LINUX),
                        size: Some(IconSize::Sm),
                        class: None,
                    }}
                    "Linux"
                </a>
                <a
                    href="#build"
                    data-release-asset="macos"
                    data-release-fallback="#build"
                    class={link_button(ButtonSize::Lg, ButtonVariant::Solid)}
                >
                    {Icon {
                        icon: IconRs(icon_rs::PLATFORM_MACOS),
                        size: Some(IconSize::Sm),
                        class: None,
                    }}
                    "macOS"
                    <span data-release-pre class="hidden text-xs font-normal opacity-80">
                        "beta"
                    </span>
                </a>
            </div>
        }
        .render(cx)
    }
}

/// Whether the export has the browser build at `app/` (`export.sh` sets `CADRS_WEB_APP`).
fn web_app() -> bool {
    std::env::var_os("CADRS_WEB_APP").is_some_and(|v| !v.is_empty())
}

/// The hero's link to the browser build on a page of its own, when the export has one.
struct TryInBrowser;

impl<L: DataLoader> WebRenderSync<L> for TryInBrowser {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        if !web_app() {
            return Ok(());
        }
        let app = cx.request().url("/app/");
        html_sync! {
            <a href={app} class={link_button(ButtonSize::Lg, ButtonVariant::Outline)}>
                "Open it full screen"
            </a>
        }
        .render(cx)
    }
}

/// Runs [`HeroApp`]: loads the app at once on a wide screen, on a narrow one (a phone: a large
/// download, and WebGPU seldom there) only when asked; lifts the veil on a click and lowers it
/// again when the pointer leaves, so the wheel scrolls the page until the app is wanted. Until
/// that first click the page keeps the keyboard and its place: the app takes focus as it starts,
/// which would scroll the page to it, so focus is taken back and a scroll nobody made undone.
const APP_SCRIPT: &str = "<script>(function(){\
var box=document.querySelector(\"[data-cadrs-embed]\");if(!box)return;\
var frame=box.querySelector(\"iframe\"),veil=box.querySelector(\"[data-cadrs-veil]\"),\
ask=box.querySelector(\"[data-cadrs-ask]\");\
function show(e,on){e.classList.toggle(\"hidden\",!on);e.classList.toggle(\"flex\",on)}\
function load(){if(!frame.getAttribute(\"src\"))frame.src=frame.getAttribute(\"data-src\");show(ask,false);show(veil,true)}\
if(matchMedia(\"(min-width: 768px)\").matches)load();else show(ask,true);\
box.querySelector(\"[data-cadrs-load]\").addEventListener(\"click\",load);\
var wanted=false,y=scrollY,input=0;\
[\"wheel\",\"touchmove\",\"keydown\",\"pointerdown\"].forEach(function(t){\
addEventListener(t,function(){input=Date.now()},{capture:true,passive:true})});\
addEventListener(\"scroll\",function(){if(!wanted&&Date.now()-input>1000&&document.activeElement===frame)\
scrollTo(0,y);else y=scrollY});\
addEventListener(\"blur\",function(){setTimeout(function(){\
if(!wanted&&document.activeElement===frame){frame.blur();scrollTo(0,y)}})});\
veil.addEventListener(\"click\",function(){wanted=true;show(veil,false);frame.focus()});\
box.addEventListener(\"mouseleave\",function(){if(frame.getAttribute(\"src\"))show(veil,true)})\
})()</script>";

/// The browser build itself across the page, where a screenshot used to be (it aged with every
/// release): the real app, as it is now. Until it is clicked, a veil keeps the wheel scrolling
/// the page ([`APP_SCRIPT`]).
struct HeroApp;

impl<L: DataLoader> WebRenderSync<L> for HeroApp {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        if !web_app() {
            return Ok(());
        }
        // Straight into a document, ready to draw in (the app's `?quick-start`).
        let app = cx.request().url("/app/?quick-start");
        html_sync! {
            <div
                id="app"
                data-cadrs-embed=""
                class="relative h-[min(80vh,860px)] min-h-[480px] w-full scroll-mt-14 border-y border-border bg-card"
            >
                <iframe
                    title="cadrs, running in this page"
                    data-src={app}
                    allow="fullscreen; clipboard-read; clipboard-write"
                    class="absolute inset-0 h-full w-full border-0"
                ></iframe>
                <button
                    type="button"
                    data-cadrs-veil=""
                    aria-label="Use cadrs"
                    class="absolute inset-0 hidden cursor-pointer items-start justify-center pt-28"
                >
                    <span class="rounded-full border border-border bg-background/90 px-4 py-2 text-sm text-foreground shadow-lg">
                        "Click to use cadrs"
                    </span>
                </button>
                <div
                    data-cadrs-ask=""
                    class="absolute inset-0 hidden flex-col items-center justify-center gap-4 px-6 text-center"
                >
                    <p class="max-w-sm text-pretty text-muted-foreground">
                        "cadrs runs in the browser too: about 40 MB, in a browser with WebGPU."
                    </p>
                    <button
                        type="button"
                        data-cadrs-load=""
                        class={link_button(ButtonSize::Lg, ButtonVariant::Solid)}
                    >
                        "Start cadrs here"
                    </button>
                </div>
            </div>
            {StaticRaw(APP_SCRIPT)}
        }
        .render(cx)
    }
}

struct Downloads;

impl<L: DataLoader> WebRenderSync<L> for Downloads {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let latest = release().label.as_str();
        html_sync! {
            {Card {
                class: Some("min-w-0"),
                children: Some(html_sync! {
                    {CardHeader {
                        class: None,
                        children: Some(html_sync! {
                            {CardTitle { class: Some("text-lg"), children: Some("Download") }}
                            {CardDescription {
                                class: None,
                                children: Some(html_sync! {
                                    <span data-release-label>{latest}</span>
                                }),
                            }}
                        }),
                    }}
                    {CardContent {
                        class: Some("flex flex-col gap-4"),
                        children: Some(html_sync! {
                            {PlatformButtons}
                            <p class="text-sm leading-6 text-muted-foreground">
                                "Windows and Linux builds are for 64-bit x86. Every release, with SHA-256 checksums, is on "
                                <a
                                    href="https://github.com/rvdende/cadrs/releases"
                                    class="font-medium text-primary underline-offset-4 hover:underline"
                                >
                                    "GitHub Releases"
                                </a>
                                ". The macOS app is universal, one download for Apple silicon and Intel Macs. It isn't signed yet, so open cadrs the first time with right-click › Open. Or build from source."
                            </p>
                        }),
                    }}
                }),
            }}
        }
        .render(cx)
    }
}

// ---------------------------------------------------------------------------------------------
// The page's sections, as data the HTML and the Markdown both render.

/// A titled paragraph: a step, a point or an answer.
struct Item {
    title: &'static str,
    body: &'static str,
}

/// "Get started": what to do first, in order, each with where on the page it goes on.
const STEPS: &[(Item, &str, &str)] = &[
    (
        Item {
            title: "Download cadrs",
            body: "Archives for Windows and Linux, a universal app for macOS. No account and no sign-in: your documents stay on your computer.",
        },
        "Downloads",
        "#download",
    ),
    (
        Item {
            title: "Make your first part",
            body: "Sketch a rectangle, extrude it, then change one dimension and watch the part rebuild.",
        },
        "The walkthrough",
        "#first-part",
    ),
    (
        Item {
            title: "Connect an AI assistant",
            body: "Turn on the MCP server, and Claude, Cursor or another assistant can sketch, extrude and take screenshots in the open document.",
        },
        "MCP setup",
        "#mcp",
    ),
];

/// "How it works": parametric modeling in four points.
const HOW: &[Item] = &[
    Item {
        title: "A sketch on a plane",
        body: "Lines, arcs and circles on a plane or a face. Constraints (horizontal, tangent, coincident and more) and dimensions pin down their shape.",
    },
    Item {
        title: "Features make solids",
        body: "Extrude, revolve, sweep and loft turn sketches into parts; fillet, chamfer, shell, holes and patterns finish them.",
    },
    Item {
        title: "A history you can edit",
        body: "The feature list keeps every step in order. Double-click one to change it and everything after it rebuilds; drag the rollback bar to see the part as it was.",
    },
    Item {
        title: "One document, many tabs",
        body: "Part Studios, assemblies, drawings, PCB Studios, renders and CAM jobs sit side by side as tabs, with versions and branches for the whole document.",
    },
];

/// "Your first part": the steps of the walkthrough.
const FIRST_PART: &[Item] = &[
    Item {
        title: "Start a document",
        body: "New document on the documents page opens a Part Studio with the Top, Front and Right planes.",
    },
    Item {
        title: "Sketch a rectangle",
        body: "Sketch (Shift+S), then pick the Top plane. Draw a corner rectangle (G) and dimension it (D): 40 mm wide and 30 mm deep. Accept the sketch with its check mark.",
    },
    Item {
        title: "Extrude it",
        body: "Extrude (Shift+E), pick the rectangle's region, set the depth to 10 mm and accept.",
    },
    Item {
        title: "Change one dimension",
        body: "Double-click Sketch 1 in the feature list, double-click the 40 mm dimension, type 60 and press Enter. Accept the sketch: the extrude follows it. The same features make a longer block.",
    },
];

/// "Ways to use it": (how, what it takes, what to know).
const WAYS: &[(&str, &str, &str)] = &[
    (
        "Desktop app",
        "Windows and Linux on 64-bit x86; macOS universal (beta).",
        "Documents are files in a folder on your computer, and it works offline.",
    ),
    (
        "From source",
        "Anywhere Rust builds: cargo install-local.",
        "The modeling kernel is pure Rust, so there is no C++ or CMake step.",
    ),
    (
        "With an AI assistant",
        "Any MCP client, over HTTP on your own machine.",
        "Off until you turn it on in Preferences. The assistant's edits go through undo like yours.",
    ),
    (
        "With other people",
        "Share a document by cadrs ID and edit it together live.",
        "Peer to peer: the apps connect directly, or through a relay when they must.",
    ),
];

/// "File formats": (formats, what for, what to know).
const FORMATS: &[(&str, &str, &str)] = &[
    (
        "cadrs (.cadrs)",
        "The whole document: every tab, the feature history, versions and branches.",
        "Keep it to keep editing. cadrs is in beta, so keep a STEP of work that matters too.",
    ),
    (
        "STEP, IGES, BREP",
        "Exact solids in and out (STEP AP214, IGES 5.3).",
        "An imported file becomes an Import feature to build on, without the feature history it was made with.",
    ),
    (
        "STL, OBJ",
        "Meshes for printing and rendering: STL in and out, OBJ out.",
        "A mesh has no exact faces or edges to dimension against.",
    ),
    (
        "SVG, DXF, DWG, PDF, EPS",
        "A drawing dropped onto a sketch becomes editable curves; flat patterns go out as DXF or DWG.",
        "The curves land in the sketch you drop them on.",
    ),
    (
        "Gerber, Excellon, IDF",
        "A board's fabrication files, and its outline and parts to and from other EDA tools.",
        "Gerber X2 with a job file, and drill files for plated and non-plated holes.",
    ),
];

/// "Is it a fit?": what cadrs is good for, and what to check before a bigger project.
const FIT_GOOD: &[&str] = &[
    "3D-printed parts, brackets, enclosures and fixtures.",
    "A circuit board and the enclosure around it, in one document.",
    "Learning parametric CAD without an account or a subscription.",
    "Working offline, with files you keep.",
    "Building models with an AI assistant at your side.",
];

const FIT_CHECK: &[&str] = &[
    "cadrs is in beta (0.1). Keep exports of work that matters.",
    "Imported STEP and IGES files come in as solids, without the feature history they were made with.",
    "Very large Part Studios, thousands of features, get slow: split them into several.",
    "The macOS app is a beta and isn't signed yet.",
    "Missing a tool you need? Check the issues, or open one.",
];

/// The questions people ask.
const FAQ: &[Item] = &[
    Item {
        title: "Is cadrs free?",
        body: "Yes. It is open source under MIT or Apache-2.0, at your option. There is no account and no subscription.",
    },
    Item {
        title: "Which systems does it run on?",
        body: "Windows and Linux on 64-bit x86, and macOS as a universal app (a beta: open it the first time with right-click, Open). Or build it from source.",
    },
    Item {
        title: "Where are my documents kept?",
        body: "In a folder on your computer; the app menu on the documents page opens it. Nothing is uploaded.",
    },
    Item {
        title: "Can I work with other people?",
        body: "Yes: share a document with someone by their cadrs ID (the account menu shows yours) and edit it together. The apps connect to each other directly.",
    },
    Item {
        title: "Can it open my existing CAD files?",
        body: "STEP, IGES and BREP come in as solids and STL as a mesh, ready to build on. The feature history from another CAD program doesn't come across.",
    },
    Item {
        title: "How does an AI assistant use it?",
        body: "Through MCP: with the server turned on, the assistant calls cadrs's tools (sketch, extrude, edit a feature, undo, take a screenshot, export STEP) in the running app.",
    },
    Item {
        title: "Is cadrs connected to a CAD company?",
        body: "No. cadrs is an independent open-source project, not affiliated with or endorsed by any CAD vendor.",
    },
];

/// The MCP client configuration (a project's `.mcp.json`).
const MCP_JSON: &str = "{\n  \"mcpServers\": {\n    \"cadrs\": {\n      \"type\": \"http\",\n      \"url\": \"http://127.0.0.1:7680/mcp\"\n    }\n  }\n}";

/// The same for Claude Code, from a terminal.
const MCP_CLAUDE_CODE: &str = "claude mcp add --transport http cadrs http://127.0.0.1:7680/mcp";

/// The page's sections, for the "On this page" links and the header.
const SECTIONS: &[(&str, &str)] = &[
    ("Get started", "#get-started"),
    ("How it works", "#how-it-works"),
    ("Ways to use it", "#ways"),
    ("File formats", "#formats"),
    ("AI assistants", "#mcp"),
    ("Features", "#features"),
    ("Is it a fit?", "#fit"),
    ("FAQ", "#faq"),
];

const SECTION_HEADING: &str =
    "mt-5 text-balance text-3xl font-semibold tracking-tight text-foreground md:text-4xl";
const SECTION_LEAD: &str = "mt-5 max-w-2xl text-pretty text-lg leading-8 text-muted-foreground";
const LINK: &str = "font-medium text-primary underline-offset-4 hover:underline";
const TABLE_HEAD: &str = "px-4 py-3 text-left text-sm font-semibold text-foreground";
const TABLE_CELL: &str = "px-4 py-3 align-top text-sm leading-6 text-muted-foreground";
const TABLE_NAME: &str =
    "px-4 py-3 text-left align-top text-sm font-medium leading-6 text-foreground";

/// A section's small label over its heading.
struct Eyebrow(&'static str);

impl<L: DataLoader> WebRenderSync<L> for Eyebrow {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            {Badge {
                variant: BadgeVariant::Outline,
                class: Some("rounded-full bg-background"),
                children: Some(self.0),
            }}
        }
        .render(cx)
    }
}

struct Steps;

impl<L: DataLoader> WebRenderSync<L> for Steps {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        for (i, (step, link, href)) in STEPS.iter().enumerate() {
            let number = ["1", "2", "3"][i.min(2)];
            html_sync! {
                {Card {
                    class: Some("min-w-0"),
                    children: Some(html_sync! {
                        {CardHeader {
                            class: None,
                            children: Some(html_sync! {
                                <span class="flex size-8 items-center justify-center rounded-full bg-primary/10 text-sm font-semibold text-primary">
                                    {number}
                                </span>
                                {CardTitle {
                                    class: Some("mt-3 text-lg"),
                                    children: Some(step.title),
                                }}
                                {CardDescription {
                                    class: Some("text-base leading-7"),
                                    children: Some(step.body),
                                }}
                            }),
                        }}
                        {CardContent {
                            class: None,
                            children: Some(html_sync! {
                                <a href={*href} class={LINK}>{*link}</a>
                            }),
                        }}
                    }),
                }}
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct Points(&'static [Item]);

impl<L: DataLoader> WebRenderSync<L> for Points {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        for item in self.0 {
            html_sync! {
                <div class="min-w-0">
                    <h3 class="font-semibold text-foreground">{item.title}</h3>
                    <p class="mt-2 leading-7 text-muted-foreground">{item.body}</p>
                </div>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

/// The walkthrough's steps, numbered.
struct Walkthrough;

impl<L: DataLoader> WebRenderSync<L> for Walkthrough {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        for (i, item) in FIRST_PART.iter().enumerate() {
            let number = ["1", "2", "3", "4"][i.min(3)];
            html_sync! {
                <li class="flex gap-4">
                    <span class="flex size-8 shrink-0 items-center justify-center rounded-full border border-border bg-background text-sm font-semibold text-foreground">
                        {number}
                    </span>
                    <div class="min-w-0">
                        <h3 class="font-semibold text-foreground">{item.title}</h3>
                        <p class="mt-1 leading-7 text-muted-foreground">{item.body}</p>
                    </div>
                </li>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

/// A three-column table: (name, what, what to know).
struct Table {
    rows: &'static [(&'static str, &'static str, &'static str)],
    heads: [&'static str; 3],
}

impl<L: DataLoader> WebRenderSync<L> for Table {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let [a, b, c] = self.heads;
        html_sync! {
            <div class="mt-10 overflow-x-auto rounded-xl border border-border bg-card">
                <table class="w-full min-w-[640px] border-collapse">
                    <thead class="border-b border-border bg-muted/40">
                        <tr>
                            <th scope="col" class={TABLE_HEAD}>{a}</th>
                            <th scope="col" class={TABLE_HEAD}>{b}</th>
                            <th scope="col" class={TABLE_HEAD}>{c}</th>
                        </tr>
                    </thead>
                    <tbody class="divide-y divide-border">{TableRows(self.rows)}</tbody>
                </table>
            </div>
        }
        .render(cx)
    }
}

struct TableRows(&'static [(&'static str, &'static str, &'static str)]);

impl<L: DataLoader> WebRenderSync<L> for TableRows {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        for (name, what, note) in self.0 {
            html_sync! {
                <tr>
                    <th scope="row" class={TABLE_NAME}>{*name}</th>
                    <td class={TABLE_CELL}>{*what}</td>
                    <td class={TABLE_CELL}>{*note}</td>
                </tr>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

/// A bulleted list.
struct Bullets(&'static [&'static str]);

impl<L: DataLoader> WebRenderSync<L> for Bullets {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        for line in self.0 {
            html_sync! {
                <li class="flex gap-3 leading-7 text-muted-foreground">
                    <span aria-hidden="true" class="mt-3 size-1.5 shrink-0 rounded-full bg-primary"></span>
                    <span>{*line}</span>
                </li>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct Faq;

impl<L: DataLoader> WebRenderSync<L> for Faq {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        for item in FAQ {
            html_sync! {
                <details class="group border-b border-border py-5">
                    <summary class="flex cursor-pointer list-none items-center justify-between gap-4 font-medium text-foreground">
                        {item.title}
                        <span
                            aria-hidden="true"
                            class="text-muted-foreground transition group-open:rotate-45"
                        >
                            "+"
                        </span>
                    </summary>
                    <p class="mt-3 max-w-3xl leading-7 text-muted-foreground">{item.body}</p>
                </details>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

/// The "On this page" links.
struct SectionLinks;

impl<L: DataLoader> WebRenderSync<L> for SectionLinks {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        for (label, href) in SECTIONS {
            html_sync! {
                <a
                    href={*href}
                    class="rounded-full border border-border bg-background px-3 py-1.5 text-sm text-muted-foreground transition hover:border-primary/40 hover:text-foreground"
                >
                    {*label}
                </a>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HomePage;

// The page follows the layout of Proa UI's `hero_saas` and `features_bento` blocks, its
// sections those of a project guide: what it is, how to start, how it works, ways to use it,
// formats, AI assistants, features, whether it fits, questions and sources.
impl<L: DataLoader> WebRenderSync<L> for HomePage {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let home = cx.request().url("/");
        let icon = cx.request().url("/icons/cadrs.png");
        let markdown = cx.request().url("/index.md");
        let latest = release().label.as_str();
        html_sync! {
            <header class="sticky top-0 z-10 border-b border-border/70 bg-background/85 backdrop-blur">
                <nav
                    aria-label="Site"
                    class="mx-auto flex h-14 max-w-6xl items-center justify-between gap-4 px-4 sm:px-6"
                >
                    <a
                        href={home}
                        class="flex items-center gap-2 font-semibold tracking-tight text-foreground"
                    >
                        <img
                            src={cx.request().url("/icons/cadrs.png")}
                            width="28"
                            height="28"
                            alt=""
                        />
                        "cadrs"
                    </a>
                    <div class="hidden items-center gap-5 text-sm text-muted-foreground lg:flex">
                        <a href="#get-started" class="hover:text-foreground">"Get started"</a>
                        <a href="#features" class="hover:text-foreground">"Features"</a>
                        <a href="#formats" class="hover:text-foreground">"File formats"</a>
                        <a href="#mcp" class="hover:text-foreground">"AI assistants"</a>
                        <a href="#faq" class="hover:text-foreground">"FAQ"</a>
                    </div>
                    <div class="flex items-center gap-1">
                        <a
                            href="https://github.com/sponsors/rvdende"
                            aria-label="Sponsor cadrs"
                            title="Sponsor cadrs"
                            class={link_button(ButtonSize::Icon, ButtonVariant::Ghost)}
                        >
                            {Icon {
                                icon: IconRs(icon_rs::HEART),
                                size: Some(IconSize::Sm),
                                class: Some("text-pink-500 dark:text-pink-400"),
                            }}
                        </a>
                        {ThemeSwitcher}
                        {GithubButton}
                    </div>
                </nav>
            </header>
            <main>
                <section class="relative overflow-hidden">
                    <div
                        aria-hidden="true"
                        class="absolute inset-x-0 top-0 h-80 bg-gradient-to-b from-primary/10 to-transparent"
                    ></div>
                    <div class="relative mx-auto max-w-6xl px-4 pb-10 pt-12 sm:px-6 md:pt-14">
                        <div class="mx-auto flex max-w-3xl flex-col items-center text-center">
                            <a
                                href={LATEST_RELEASE}
                                class="inline-flex items-center gap-2 rounded-full border border-border bg-background/80 px-3 py-1 text-sm text-muted-foreground hover:text-foreground"
                            >
                                <span class="size-2 rounded-full bg-primary"></span>
                                <span data-release-label>{latest}</span>
                                <span class="font-medium text-primary">"What's new"</span>
                            </a>
                            <h1 class="mt-6 text-balance text-4xl font-semibold tracking-tight text-foreground md:text-5xl lg:text-6xl">
                                "Parametric CAD, written in Rust"
                            </h1>
                            <p class="mt-6 max-w-2xl text-pretty text-lg leading-8 text-muted-foreground">
                                "Sketch, constrain and dimension, then build parts, assemblies, drawings and circuit boards from features you can always go back and change. It runs on your own computer, with your documents in a folder you own."
                            </p>
                            <div class="mt-8 flex flex-wrap justify-center gap-3">
                                <a
                                    href="#download"
                                    class={link_button(ButtonSize::Lg, ButtonVariant::Solid)}
                                >
                                    "Download"
                                </a>
                                {TryInBrowser}
                                <a
                                    href="https://youtu.be/qT1FKZqUAxA"
                                    class={link_button(ButtonSize::Lg, ButtonVariant::Outline)}
                                >
                                    "Watch the 90-second demo"
                                </a>
                            </div>
                            <p class="mt-6 text-sm text-muted-foreground">
                                "Free and open source (MIT or Apache-2.0) · Windows, Linux and macOS · An independent project"
                            </p>
                        </div>
                    </div>
                    {HeroApp}
                    <div class="relative mx-auto max-w-6xl px-4 py-14 sm:px-6">
                        <p class="text-xs font-medium uppercase tracking-widest text-muted-foreground">
                            "On this page"
                        </p>
                        <nav aria-label="On this page" class="mt-3 flex flex-wrap gap-2">
                            {SectionLinks}
                        </nav>
                    </div>
                </section>
                <section id="get-started" class="scroll-mt-20 border-t border-border">
                    <div class="mx-auto max-w-6xl px-4 py-14 sm:px-6 md:py-20">
                        {Eyebrow("Get started")}
                        <h2 class={SECTION_HEADING}>"Three steps to your first part"</h2>
                        <div class="mt-10 grid gap-4 md:grid-cols-3">{Steps}</div>
                    </div>
                </section>
                <section
                    id="download"
                    class="mx-auto grid max-w-6xl scroll-mt-20 gap-6 px-4 pb-14 sm:px-6 md:pb-20 lg:grid-cols-2"
                >
                    {Downloads}
                    <div id="build" class="min-w-0 scroll-mt-20">
                        {Card {
                            class: Some("h-full"),
                            children: Some(html_sync! {
                                {CardHeader {
                                    class: None,
                                    children: Some(html_sync! {
                                        {CardTitle {
                                            class: Some("text-lg"),
                                            children: Some("Build from source"),
                                        }}
                                        {CardDescription {
                                            class: None,
                                            children: Some("A Rust toolchain and a C compiler. The modeling kernel is pure Rust, so there is no C++ or CMake build."),
                                        }}
                                    }),
                                }}
                                {CardContent {
                                    class: Some("flex flex-col gap-4"),
                                    children: Some(html_sync! {
                                        <pre class="overflow-x-auto rounded-lg border border-border bg-muted/50 p-4 text-sm leading-6">
                                            <code>
                                                "git clone https://github.com/rvdende/cadrs\ncd cadrs\ncargo install-local"
                                            </code>
                                        </pre>
                                        <p class="text-sm text-muted-foreground">
                                            "This builds in release and installs cadrs for your user, with a launcher entry. Run it again to update. To run without installing, use "
                                            <code>"cargo run --release"</code>
                                            "."
                                        </p>
                                        <a
                                            href="https://github.com/rvdende/cadrs#building"
                                            class={LINK}
                                        >
                                            "Full build notes"
                                        </a>
                                    }),
                                }}
                            }),
                        }}
                    </div>
                </section>
                <section id="how-it-works" class="scroll-mt-20 border-y border-border bg-muted/30">
                    <div class="mx-auto max-w-6xl px-4 py-14 sm:px-6 md:py-20">
                        {Eyebrow("How it works")}
                        <h2 class={SECTION_HEADING}>
                            "Features you can edit, in the order you made them"
                        </h2>
                        <p class={SECTION_LEAD}>
                            "A part in cadrs is a recipe, not a lump of geometry: sketches and the features built on them, each one editable. Change a number at the start and the rest follows."
                        </p>
                        <div class="mt-10 grid gap-8 md:grid-cols-2">{Points(HOW)}</div>
                    </div>
                </section>
                <section id="first-part" class="scroll-mt-20">
                    <div class="mx-auto grid max-w-6xl gap-10 px-4 py-14 sm:px-6 md:py-20 lg:grid-cols-[0.8fr_1.2fr]">
                        <div>
                            {Eyebrow("Try it")}
                            <h2 class={SECTION_HEADING}>"Your first part: change one dimension"</h2>
                            <p class={SECTION_LEAD}>
                                "A block 40 by 30 by 10 mm becomes 60 long with one edit. The values are only suggestions; use your own."
                            </p>
                        </div>
                        <ol class="flex flex-col gap-6">{Walkthrough}</ol>
                    </div>
                </section>
                <section id="ways" class="scroll-mt-20 border-y border-border bg-muted/30">
                    <div class="mx-auto max-w-6xl px-4 py-14 sm:px-6 md:py-20">
                        {Eyebrow("Ways to use it")}
                        <h2 class={SECTION_HEADING}>"Choose how you work"</h2>
                        <p class={SECTION_LEAD}>
                            "One app, used on its own, from source, with an AI assistant or with other people."
                        </p>
                        {Table { rows: WAYS, heads: ["Way", "What it takes", "Good to know"] }}
                    </div>
                </section>
                <section id="formats" class="scroll-mt-20">
                    <div class="mx-auto max-w-6xl px-4 py-14 sm:px-6 md:py-20">
                        {Eyebrow("File formats")}
                        <h2 class={SECTION_HEADING}>"What goes in, what comes out"</h2>
                        <p class={SECTION_LEAD}>
                            "Keep the .cadrs document to keep editing; exchange solids as STEP and meshes as STL. The list covers the main formats, not every one."
                        </p>
                        {Table { rows: FORMATS, heads: ["Format", "Use it for", "Good to know"] }}
                    </div>
                </section>
                <section id="mcp" class="scroll-mt-20 border-y border-border bg-muted/30">
                    <div class="mx-auto grid max-w-6xl gap-10 px-4 py-14 sm:px-6 md:py-20 lg:grid-cols-2">
                        <div>
                            {Eyebrow("AI assistants")}
                            <h2 class={SECTION_HEADING}>"Model with an AI assistant"</h2>
                            <p class={SECTION_LEAD}>
                                "cadrs serves MCP (the Model Context Protocol), so an assistant can work in the open document beside you."
                            </p>
                            <ul class="mt-8 flex flex-col gap-3">
                                {Bullets(&[
                                    "Turn it on in Preferences: Enable MCP server. It listens on your own machine only, at 127.0.0.1:7680.",
                                    "Forty tools: open and create documents, sketch, extrude, edit and delete features, undo, take screenshots, export STEP, and place and route a board.",
                                    "Every change goes through undo and shows in the feature list, as yours do.",
                                ])}
                            </ul>
                        </div>
                        <div class="flex min-w-0 flex-col gap-6">
                            <div>
                                <p class="text-sm font-medium text-foreground">
                                    "A project's .mcp.json (Claude Code, Cursor and other clients)"
                                </p>
                                <pre class="mt-3 overflow-x-auto rounded-lg border border-border bg-background p-4 text-sm leading-6">
                                    <code>{MCP_JSON}</code>
                                </pre>
                            </div>
                            <div>
                                <p class="text-sm font-medium text-foreground">
                                    "Or, for Claude Code, in a terminal"
                                </p>
                                <pre class="mt-3 overflow-x-auto rounded-lg border border-border bg-background p-4 text-sm leading-6">
                                    <code>{MCP_CLAUDE_CODE}</code>
                                </pre>
                            </div>
                        </div>
                    </div>
                </section>
                <section id="features" class="scroll-mt-14">
                    <div class="mx-auto max-w-6xl px-4 py-14 sm:px-6 md:py-20">
                        <div class="max-w-2xl">
                            {Eyebrow("Features")}
                            <h2 class={SECTION_HEADING}>
                                "From sketch to assembly, drawing and board"
                            </h2>
                            <p class={SECTION_LEAD}>
                                "Every change goes through undo, every feature can be edited later, and the kernel runs in a process of its own, so a crash costs a rebuild, not your work."
                            </p>
                        </div>
                        <div class="mt-10 grid gap-4 md:grid-cols-2 lg:grid-cols-3">{Features}</div>
                    </div>
                </section>
                <section id="fit" class="scroll-mt-20 border-y border-border bg-muted/30">
                    <div class="mx-auto max-w-6xl px-4 py-14 sm:px-6 md:py-20">
                        {Eyebrow("Is it a fit?")}
                        <h2 class={SECTION_HEADING}>
                            "What cadrs is good for, and what to check first"
                        </h2>
                        <div class="mt-10 grid gap-4 md:grid-cols-2">
                            {Card {
                                class: None,
                                children: Some(html_sync! {
                                    {CardHeader {
                                        class: None,
                                        children: Some(html_sync! {
                                            {CardTitle {
                                                class: Some("text-lg"),
                                                children: Some("A good fit"),
                                            }}
                                        }),
                                    }}
                                    {CardContent {
                                        class: None,
                                        children: Some(html_sync! {
                                            <ul class="flex flex-col gap-2">{Bullets(FIT_GOOD)}</ul>
                                        }),
                                    }}
                                }),
                            }}
                            {Card {
                                class: None,
                                children: Some(html_sync! {
                                    {CardHeader {
                                        class: None,
                                        children: Some(html_sync! {
                                            {CardTitle {
                                                class: Some("text-lg"),
                                                children: Some("Before a bigger project"),
                                            }}
                                        }),
                                    }}
                                    {CardContent {
                                        class: Some("flex flex-col gap-4"),
                                        children: Some(html_sync! {
                                            <ul class="flex flex-col gap-2">
                                                {Bullets(FIT_CHECK)}
                                            </ul>
                                            <a
                                                href="https://github.com/rvdende/cadrs/issues"
                                                class={LINK}
                                            >
                                                "Open issues on GitHub"
                                            </a>
                                        }),
                                    }}
                                }),
                            }}
                        </div>
                    </div>
                </section>
                <section id="faq" class="scroll-mt-20">
                    <div class="mx-auto max-w-6xl px-4 py-14 sm:px-6 md:py-20">
                        {Eyebrow("FAQ")}
                        <h2 class={SECTION_HEADING}>"Questions"</h2>
                        <div class="mt-8 border-t border-border">{Faq}</div>
                    </div>
                </section>
                <section id="sources" class="scroll-mt-20 border-t border-border bg-muted/30">
                    <div class="mx-auto max-w-6xl px-4 py-14 sm:px-6">
                        <h2 class="text-xl font-semibold text-foreground">"Sources and links"</h2>
                        <div class="mt-5 grid gap-3 text-sm sm:grid-cols-2 lg:grid-cols-3">
                            <a href="https://github.com/rvdende/cadrs" class={LINK}>
                                "The source code, on GitHub"
                            </a>
                            <a href={RELEASES} class={LINK}>"Releases and their notes"</a>
                            <a href="https://github.com/rvdende/cadrs/issues" class={LINK}>
                                "Issues: report a bug or ask for a tool"
                            </a>
                            <a href="https://github.com/rvdende/cadrs#building" class={LINK}>
                                "Build notes"
                            </a>
                            <a href="https://youtu.be/qT1FKZqUAxA" class={LINK}>
                                "The 90-second demo"
                            </a>
                            <a href="https://github.com/sponsors/rvdende" class={LINK}>
                                "Sponsor cadrs"
                            </a>
                        </div>
                    </div>
                </section>
            </main>
            <footer class="border-t border-border">
                <div class="mx-auto flex max-w-6xl flex-col gap-4 px-4 py-10 text-sm text-muted-foreground sm:px-6">
                    <div class="flex flex-wrap items-center gap-x-6 gap-y-2">
                        <span class="flex items-center gap-2 font-semibold text-foreground">
                            <img src={icon} width="20" height="20" alt="" />
                            "cadrs"
                        </span>
                        <a href="#download" class="hover:text-foreground">"Download"</a>
                        <a href="#first-part" class="hover:text-foreground">"First part"</a>
                        <a href="#mcp" class="hover:text-foreground">"AI assistants"</a>
                        <a href="https://github.com/rvdende/cadrs" class="hover:text-foreground">
                            "GitHub"
                        </a>
                        <a href={RELEASES} class="hover:text-foreground">"Releases"</a>
                        <a href={markdown} class="hover:text-foreground">"This page as Markdown"</a>
                        <span class="ml-auto flex items-center gap-1.5">
                            "Made with"
                            <a
                                href="https://www.rust-lang.org"
                                aria-label="Rust"
                                title="Rust"
                                class="text-lg leading-none transition hover:scale-110"
                            >
                                "🦀"
                            </a>
                        </span>
                    </div>
                    {Separator {
                        orientation: SeparatorOrientation::Horizontal,
                        decorative: true,
                        class: None,
                    }}
                    <p class="max-w-3xl leading-6">
                        "Licensed under MIT or Apache-2.0, at your option. cadrs is an independent project."
                    </p>
                </div>
            </footer>
        }
        .render(cx)
    }
}

#[proa_macros::content(
    skip,
    reason = "A Markdown summary of the page; the Proa UI components are outside the coverage analysis"
)]
impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for HomePage {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        md_sync! { r###"
            # cadrs: parametric CAD, written in Rust

            Free and open source under MIT or Apache\-2\.0\. An independent project\.

            Sketch, constrain and dimension, then build parts, assemblies, drawings and circuit boards from features you can always go back and change\. It runs on your own computer, with your documents in a folder you own\.

            [Watch the 90\-second demo](https://youtu.be/qT1FKZqUAxA)\.

            ## Get started

            {Steps}

            {Downloads}

            ## Build from source

            A Rust toolchain and a C compiler\. The modeling kernel is pure Rust, so there is no C\+\+ or CMake build\. [Full build notes](https://github.com/rvdende/cadrs#building)\.

                git clone https://github.com/rvdende/cadrs
                cd cadrs
                cargo install-local

            `cargo install-local` builds in release and installs cadrs for your user, with a launcher entry\. Run it again to update\. To run without installing, use `cargo run --release`\.

            ## How it works

            A part in cadrs is a recipe, not a lump of geometry: sketches and the features built on them, each one editable\. Change a number at the start and the rest follows\.

            {Points(HOW)}

            ## Your first part: change one dimension

            {Walkthrough}

            ## Ways to use it

            {Table { rows: WAYS, heads: ["Way", "What it takes", "Good to know"] }}

            ## File formats

            {Table { rows: FORMATS, heads: ["Format", "Use it for", "Good to know"] }}

            ## AI assistants \(MCP\)

            Turn on Preferences › Enable MCP server\. It listens on your own machine only, at `http://127.0.0.1:7680/mcp`\. Every change the assistant makes goes through undo\.

                {{
                  "mcpServers": {{
                    "cadrs": {{
                      "type": "http",
                      "url": "http://127.0.0.1:7680/mcp"
                    }}
                  }}
                }}

            For Claude Code: `claude mcp add --transport http cadrs http://127.0.0.1:7680/mcp`

            ## Features

            Every change goes through undo, every feature can be edited later, and the kernel runs in a process of its own, so a crash costs a rebuild, not your work\.

            {Features}

            ## Is it a fit?

            A good fit:

            {Bullets(FIT_GOOD)}

            Before a bigger project:

            {Bullets(FIT_CHECK)}

            ## FAQ

            {Faq}

            ## Sources and links

            [Source code](https://github.com/rvdende/cadrs) · [Releases](https://github.com/rvdende/cadrs/releases) · [Issues](https://github.com/rvdende/cadrs/issues) · [Sponsor](https://github.com/sponsors/rvdende)

            Licensed under MIT or Apache\-2\.0, at your option\. cadrs is an independent project\.
            "###
        }
        .render_md(cx)
    }
}

#[proa_macros::content(skip, reason = "The steps as a numbered list")]
impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Steps {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        for (step, _, _) in STEPS {
            md_sync! { r###"
                - **{step.title}**: {step.body}
                "###
            }
            .render_md(cx)?;
        }
        Ok(())
    }
}

#[proa_macros::content(skip, reason = "The points as a bullet list")]
impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Points {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        for item in self.0 {
            md_sync! { r###"
                - **{item.title}**: {item.body}
                "###
            }
            .render_md(cx)?;
        }
        Ok(())
    }
}

#[proa_macros::content(skip, reason = "The walkthrough as a list")]
impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Walkthrough {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        for item in FIRST_PART {
            md_sync! { r###"
                - **{item.title}**: {item.body}
                "###
            }
            .render_md(cx)?;
        }
        Ok(())
    }
}

#[proa_macros::content(skip, reason = "The table's rows as a list")]
impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Table {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        for (name, what, note) in self.rows {
            md_sync! { r###"
                - **{*name}**: {*what} {*note}
                "###
            }
            .render_md(cx)?;
        }
        Ok(())
    }
}

#[proa_macros::content(skip, reason = "The list as Markdown bullets")]
impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Bullets {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        for line in self.0 {
            md_sync! { r###"
                - {*line}
                "###
            }
            .render_md(cx)?;
        }
        Ok(())
    }
}

#[proa_macros::content(skip, reason = "The questions and answers as paragraphs")]
impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Faq {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        for item in FAQ {
            md_sync! { r###"
                **{item.title}** {item.body}
                "###
            }
            .render_md(cx)?;
        }
        Ok(())
    }
}

#[proa_macros::content(
    skip,
    reason = "The same features as a bullet list, without the cards and screenshots"
)]
impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Features {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        for feature in FEATURES {
            md_sync! { r###"
                - **{feature.title}**: {feature.body}
                "###
            }
            .render_md(cx)?;
        }
        Ok(())
    }
}

#[proa_macros::content(skip, reason = "The same links as Markdown links instead of buttons")]
impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for Downloads {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let release = release();
        let (latest, linux, windows) = (
            release.label.as_str(),
            release.linux.as_str(),
            release.windows.as_str(),
        );
        md_sync! { r###"
            ## Download

            {latest}

            [Linux (x86_64)]({linux}) · [Windows (x86_64)]({windows})

            Every release, with SHA\-256 checksums, is on [GitHub Releases](https://github.com/rvdende/cadrs/releases)\. The macOS app is universal, one download for Apple silicon and Intel Macs\. It isn't signed yet, so open cadrs the first time with right\-click › Open\. Or build from source\.
            "###
        }
        .render_md(cx)?;
        if web_app() {
            let app = cx.request().url("/app/");
            md_sync! { r###"
                Or [try it in your browser]({app}) \(it needs WebGPU; your documents stay in the browser\)\.
                "###
            }
            .render_md(cx)?;
        }
        Ok(())
    }
}

use axum::{http::HeaderMap, response::IntoResponse};
use proa_framework_axum::{explicit_markdown, negotiate, MarkdownAlternate, RouteResponse};

use crate::{error::AppError, layouts::RootDocument};

pub async fn handler(headers: HeaderMap) -> Result<impl IntoResponse, AppError> {
    let html = RouteResponse::ssr_async(RootDocument {
        title: "cadrs: parametric CAD, written in Rust",
        description: "cadrs is a free, open-source parametric CAD app written in Rust: sketches, Part Studios, assemblies, drawings and PCBs in one document, on your own computer.",
        content: proa_core::to_async(HomePage),
    });
    Ok(negotiate(
        &headers,
        html,
        HomePage,
        MarkdownAlternate::new("/index.md"),
    )?)
}

pub async fn markdown() -> Result<impl IntoResponse, AppError> {
    Ok(explicit_markdown(HomePage)?)
}
