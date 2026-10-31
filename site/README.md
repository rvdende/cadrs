# cadrs site

The project page at <https://cadrs.net/>: a [Proa](https://proa.so) site,
exported to static files for GitHub Pages by `.github/workflows/pages.yml`.

- `src/pages/home.rs`: the page, as HTML and as Markdown (`/index.md`).
- `src/components/`: [Proa UI](https://ui.proa.so) components, vendored with `proa ui add`.
  The page's layout follows its `hero_saas` and `features_bento` blocks.
- `src/icons.rs`: draws [icon-rs](https://github.com/rvdende/icon-rs) icons (the platform
  icons on the download buttons) with Proa UI's `Icon`.
- `styles/input.css`: Tailwind, with the Proa UI theme tokens and the cadrs blue.
- The screenshots (`/images`) and the app icon (`/icons`) are served from the repository's
  `.github/images` and `assets/icons`, so the README and the site share one copy.

## Tools

The static export (`proa export`) is on our Proa fork until it is merged upstream; the Proa
crates are patched to the same branch in `Cargo.toml`.

```sh
cargo install --locked --git https://github.com/rvdende/proa --branch feat/static-export proa-cli --bin proa
```

You also need the [Tailwind CSS CLI v4](https://github.com/tailwindlabs/tailwindcss/releases)
on your `PATH` as `tailwindcss`.

The site builds on the stable toolchain in `rust-toolchain.toml`. The repository's
`.cargo/config.toml` adds nightly-only rustflags for the app, so clear them for anything you
run here with `RUSTFLAGS=`.

## Format it

```sh
./fmt.sh
```

Not `cargo fmt`: it would also reformat the vendored Proa UI sources in `src/components`, and
`proa ui add` refuses to update files that differ from what it installed. `fmt.sh` formats
only the site's own files (`main.rs` and one test with the repository's nightly rustfmt, whose
`skip_children` keeps it out of `src/components`).

## Run it

```sh
RUSTFLAGS= proa dev --open
```

## Export it

```sh
./export.sh                # into site/dist, for https://cadrs.net/
./export.sh out /cadrs     # into site/out, served under /cadrs/ (as rvdende.github.io/cadrs/ was)
```

`export.sh` builds the release binary and renders it with `proa export`. It puts the browser
build of cadrs in `app/`: the latest release's `cadrs-web.tar.gz`, or a local
`tools/web/build.sh` output with `CADRS_WEB_DIR=target/web`. When there is one, the hero runs it
across the page (`/app/?quick-start`: straight into a document), loaded at once on a wide screen
and on request on a phone, behind a veil until it is clicked so the wheel scrolls the page.
The download links point at the latest GitHub release at export time (set
`CADRS_RELEASE=v0.1.5` to choose one);
when the page loads, `RELEASE_SCRIPT` (`src/pages/home.rs`) points them at whatever release is
latest now, so publishing, replacing or deleting a release needs no redeploy. `index.md` keeps
the links from the export.
