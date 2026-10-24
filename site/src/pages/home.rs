use std::sync::OnceLock;

use proa_core::{perf_ok, DataLoader, WebContext, WebRenderSync, WriteError};
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
/// release left the links go to the releases page; a platform the release has no archive for
/// goes to the release's own page.
pub const RELEASE_SCRIPT: &str = "<script>(function(){\
var key=\"cadrs-release\",releases=\"https://github.com/rvdende/cadrs/releases\";\
var suffix={windows:\"-x86_64-windows.zip\",linux:\"-x86_64-linux.tar.gz\"};\
function show(r){var l=document.querySelectorAll(\"[data-release-label]\");for(var i=0;i<l.length;i++)\
l[i].textContent=r?\"Latest release: \"+r.tag:\"Latest release\";\
var a=document.querySelectorAll(\"[data-release-asset]\");for(var i=0;i<a.length;i++)\
a[i].href=r?r.assets[a[i].getAttribute(\"data-release-asset\")]||r.page:releases}\
function ready(f){if(document.readyState===\"loading\")document.addEventListener(\"DOMContentLoaded\",f);else f()}\
try{var c=JSON.parse(sessionStorage.getItem(key));if(c&&Date.now()-c.t<6e5){ready(function(){show(c.r)});return}}catch(e){}\
fetch(\"https://api.github.com/repos/rvdende/cadrs/releases/latest\").then(function(res){\
return res.status===404?{}:res.ok?res.json():null}).then(function(j){if(!j)return;var r=null;\
if(j.tag_name){r={tag:j.tag_name,page:j.html_url||releases,assets:{}};(j.assets||[]).forEach(function(x){\
for(var p in suffix)if(x.name.slice(-suffix[p].length)===suffix[p])r.assets[p]=x.browser_download_url})}\
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
    /// A screenshot shown beside the text, with its size.
    shot: Option<(&'static str, u32, u32)>,
    /// Grid classes for the feature's card in the bento layout.
    class: &'static str,
}

/// The bento grid of features, in reading order: three columns from `lg`.
const FEATURES: &[Feature] = &[
    Feature {
        title: "Part Studios",
        body: "Extrude, revolve, sweep, loft, fillet, chamfer, shell, draft, holes, patterns and booleans. Every feature stays editable, and the part rebuilds.",
        shot: Some(("/images/part-studio.png", 1600, 1000)),
        class: "overflow-hidden md:col-span-2",
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
        title: "Rendering and simulation",
        body: "A path-traced renderer, linear static FEA, and measurement and mass properties.",
        shot: Some(("/images/render.png", 1920, 1080)),
        class: "overflow-hidden md:col-span-2",
    },
    Feature {
        title: "Drawings",
        body: "Standard and projected views, sections, dimensions, annotations, callouts and BOM tables.",
        shot: None,
        class: "",
    },
    Feature {
        title: "PCB Studio",
        body: "Schematic, board layout and routing in the same document as the enclosure, with IDF import and export.",
        shot: None,
        class: "",
    },
    Feature {
        title: "Onshape import",
        body: "Bring your Onshape documents across with their feature history, instead of dead STEP geometry.",
        shot: None,
        class: "",
    },
    Feature {
        title: "A whole cylinder, one document",
        body: "Instances, subassemblies and fasteners, put together with mates and listed in a bill of materials.",
        shot: Some(("/images/assembly.png", 1600, 1000)),
        class: "overflow-hidden md:col-span-2",
    },
    Feature {
        title: "Files in, files out",
        body: "STEP, IGES and STL import and export. Drop an SVG, DXF, DWG, PDF or EPS drawing onto a sketch to get editable curves. Versions, history and a variable table come with every document.",
        shot: None,
        class: "",
    },
];

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
                Some((image, width, height)) => {
                    html_sync! {
                        {Card {
                            class: Some(feature.class),
                            children: Some(html_sync! {
                                <div class="grid h-full gap-2 xl:grid-cols-[0.75fr_1.25fr]">
                                    <div class="flex flex-col justify-center">{header}</div>
                                    <a
                                        href={cx.request().url(image)}
                                        class="block self-end px-6 pb-6 xl:px-0 xl:pt-6 xl:pb-0"
                                    >
                                        <img
                                            src={cx.request().url(image)}
                                            width={width}
                                            height={height}
                                            alt={feature.title}
                                            loading="lazy"
                                            class="block h-auto w-full rounded-lg border border-border xl:rounded-tr-none xl:rounded-b-none xl:border-r-0 xl:border-b-0"
                                        />
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

/// One button per desktop platform: the latest release for Windows and Linux, and the build
/// notes for macOS, which has no release build.
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
                <a href="#build" class={link_button(ButtonSize::Lg, ButtonVariant::Outline)}>
                    {Icon {
                        icon: IconRs(icon_rs::PLATFORM_MACOS),
                        size: Some(IconSize::Sm),
                        class: None,
                    }}
                    "macOS"
                </a>
            </div>
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
                                ". On macOS, build from source: it runs natively on Apple Silicon."
                            </p>
                        }),
                    }}
                }),
            }}
        }
        .render(cx)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HomePage;

// The page follows the layout of Proa UI's `hero_saas` and `features_bento` blocks.
impl<L: DataLoader> WebRenderSync<L> for HomePage {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let home = cx.request().url("/");
        let icon = cx.request().url("/icons/cadrs.png");
        let demo = cx.request().url("/images/pcb-demo.gif");
        let markdown = cx.request().url("/index.md");
        html_sync! {
            <header class="sticky top-0 z-10 border-b border-border/70 bg-background/85 backdrop-blur">
                <nav
                    aria-label="Site"
                    class="mx-auto flex h-14 max-w-6xl items-center justify-between px-4 sm:px-6"
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
                    <div class="relative mx-auto grid max-w-6xl items-center gap-12 px-4 py-14 sm:px-6 md:py-20 lg:grid-cols-[0.9fr_1.1fr]">
                        <div class="flex flex-col items-start">
                            {Badge {
                                variant: BadgeVariant::Outline,
                                class: Some("rounded-full bg-background/80"),
                                children: Some("Open source · MIT or Apache-2.0"),
                            }}
                            <h1 class="mt-6 max-w-xl text-balance text-4xl font-semibold tracking-tight text-foreground md:text-5xl lg:text-6xl">
                                "Parametric CAD, written in Rust"
                            </h1>
                            <p class="mt-6 max-w-lg text-pretty text-lg leading-8 text-muted-foreground">
                                "Sketch on a plane, constrain and dimension, build parts from features, put them together in assemblies, and make drawings from them. cadrs follows Onshape's workflow, with a Bevy UI and its own B-rep kernel written in Rust, and runs on your own machine."
                            </p>
                        </div>
                        <div class="relative mx-auto w-full max-w-2xl">
                            <div
                                aria-hidden="true"
                                class="absolute -inset-6 rounded-[2rem] bg-primary/10 blur-3xl"
                            ></div>
                            {Card {
                                class: Some("relative overflow-hidden rounded-2xl border-border/80 bg-card/95 shadow-2xl"),
                                children: Some(html_sync! {
                                    <div class="flex items-center justify-between border-b border-border px-5 py-4">
                                        <div>
                                            <p class="text-xs font-medium uppercase tracking-widest text-muted-foreground">
                                                "One document"
                                            </p>
                                            <p class="mt-1 font-medium text-foreground">
                                                "From sketch to part to PCB"
                                            </p>
                                        </div>
                                        {Badge {
                                            variant: BadgeVariant::Secondary,
                                            class: Some("rounded-full"),
                                            children: Some("90 s demo"),
                                        }}
                                    </div>
                                    <a href="https://youtu.be/qT1FKZqUAxA" class="block">
                                        <img
                                            src={demo}
                                            width="960"
                                            height="526"
                                            alt="From sketch to solid, then a PCB in the same Part Studio: schematic, board layout and the routed board in 3D"
                                            class="block h-auto w-full"
                                        />
                                    </a>
                                    <div class="flex items-center justify-between border-t border-border px-5 py-3 text-sm">
                                        <span class="text-muted-foreground">
                                            "Schematic, layout and the routed board in 3D"
                                        </span>
                                        <a
                                            href="https://youtu.be/qT1FKZqUAxA"
                                            class="font-medium text-primary underline-offset-4 hover:underline"
                                        >
                                            "Watch on YouTube"
                                        </a>
                                    </div>
                                }),
                            }}
                        </div>
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
                                            class="text-sm font-medium text-primary underline-offset-4 hover:underline"
                                        >
                                            "Full build notes"
                                        </a>
                                    }),
                                }}
                            }),
                        }}
                    </div>
                </section>
                <section id="features" class="scroll-mt-14 border-y border-border bg-muted/30">
                    <div class="mx-auto max-w-6xl px-4 py-14 sm:px-6 md:py-20">
                        <div class="max-w-2xl">
                            {Badge {
                                variant: BadgeVariant::Outline,
                                class: Some("rounded-full bg-background"),
                                children: Some("Features"),
                            }}
                            <h2 class="mt-5 text-balance text-3xl font-semibold tracking-tight text-foreground md:text-4xl">
                                "From sketch to assembly to drawing"
                            </h2>
                            <p class="mt-5 text-pretty text-lg leading-8 text-muted-foreground">
                                "Every change goes through undo, every feature can be edited later, and the kernel runs in a process of its own, so a crash costs a rebuild, not your work."
                            </p>
                        </div>
                        <div class="mt-10 grid gap-4 md:grid-cols-2 lg:grid-cols-3">{Features}</div>
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
                        <a href="https://github.com/rvdende/cadrs" class="hover:text-foreground">
                            "GitHub"
                        </a>
                        <a
                            href="https://github.com/rvdende/cadrs/releases"
                            class="hover:text-foreground"
                        >
                            "Releases"
                        </a>
                        <a href={markdown} class="hover:text-foreground">"This page as Markdown"</a>
                        <span>
                            "Built with "
                            <a href="https://proa.so" class="hover:text-foreground">"Proa"</a>
                        </span>
                    </div>
                    {Separator {
                        orientation: SeparatorOrientation::Horizontal,
                        decorative: true,
                        class: None,
                    }}
                    <p class="max-w-3xl leading-6">
                        "Licensed under MIT or Apache-2.0, at your option. cadrs is an independent project. It is not affiliated with, endorsed by or sponsored by PTC or Onshape. Onshape is a trademark of PTC Inc."
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

            Open source under MIT or Apache\-2\.0\.

            Sketch on a plane, constrain and dimension, build parts from features, put them together in assemblies, and make drawings from them\. cadrs follows Onshape's workflow, with a Bevy UI and its own B\-rep kernel written in Rust, and runs on your own machine\.

            From sketch to part to PCB, in one document: [watch the 90\-second demo](https://youtu.be/qT1FKZqUAxA)\.

            {Downloads}

            ## Build from source

            A Rust toolchain and a C compiler\. The modeling kernel is pure Rust, so there is no C\+\+ or CMake build\. [Full build notes](https://github.com/rvdende/cadrs#building)\.

                git clone https://github.com/rvdende/cadrs
                cd cadrs
                cargo install-local

            `cargo install-local` builds in release and installs cadrs for your user, with a launcher entry\. Run it again to update\. To run without installing, use `cargo run --release`\.

            ## From sketch to assembly to drawing

            Every change goes through undo, every feature can be edited later, and the kernel runs in a process of its own, so a crash costs a rebuild, not your work\.

            {Features}

            Source: https://github.com/rvdende/cadrs

            Licensed under MIT or Apache\-2\.0, at your option\. cadrs is an independent project\. It is not affiliated with, endorsed by or sponsored by PTC or Onshape\. Onshape is a trademark of PTC Inc\.
            "###
        }
        .render_md(cx)
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

            Every release, with SHA\-256 checksums, is on [GitHub Releases](https://github.com/rvdende/cadrs/releases)\. On macOS, build from source: it runs natively on Apple Silicon\.
            "###
        }
        .render_md(cx)
    }
}

use axum::{http::HeaderMap, response::IntoResponse};
use proa_framework_axum::{explicit_markdown, negotiate, MarkdownAlternate, RouteResponse};

use crate::{error::AppError, layouts::RootDocument};

pub async fn handler(headers: HeaderMap) -> Result<impl IntoResponse, AppError> {
    let html = RouteResponse::ssr_async(RootDocument {
        title: "cadrs: parametric CAD, written in Rust",
        description: "cadrs is an open-source, parametric, feature-based CAD app written in Rust: sketches, Part Studios, assemblies, drawings and PCBs, following Onshape's workflow.",
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
