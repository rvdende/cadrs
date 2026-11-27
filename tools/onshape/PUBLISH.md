# Publishing the importer branch and the app (fork → PR, AppImage → Actions/Release)

GitHub has no "Builds" tab: a build's files are an **Actions artifact** (Actions → the run →
Artifacts, expires after 90 days) or a **release asset** (Releases, permanent). For an AppImage
the release is the useful one.

This repository's checkout is on branch `onshape-importer-v2` (15 commits at the time of
writing) and the fork `xitix/cadrs` already exists. SSH works for that account; upstream
`rvdende/cadrs` is read-only for it.

## 1. Push the branch to the fork

```sh
cd /home/xitix5r/Documents/business/admin_local_tools/CADrs_linux_buids/cadrs
git remote add fork git@github.com:xitix/cadrs.git   # once; skip if it exists
git push -u fork onshape-importer-v2
```

If `git remote -v` already has `fork`, just `git push fork onshape-importer-v2`.

Using HTTPS instead of SSH needs `gh auth login` once (or a personal access token as the
password), then `git remote add fork https://github.com/xitix/cadrs.git`.

## 2. Open the pull request to `rvdende/cadrs`

Web: <https://github.com/rvdende/cadrs/compare/main...xitix:cadrs:onshape-importer-v2?expand=1>

or, after `gh auth login`:

```sh
gh pr create --repo rvdende/cadrs \
  --head xitix:onshape-importer-v2 \
  --title "Onshape importer: transfer app, reports, verification, SolidCraft" \
  --body-file /home/xitix5r/Documents/business/.local-run/PR-ONSHAPE-IMPORTER.md
```

`xitix` has no write access to `rvdende/cadrs`, so the PR must come from the fork (the compare
URL above does that).

## 3. The AppImage

The commit adds `.github/workflows/onshape-importer-app.yml`, which builds and packages the app
on `ubuntu-24.04` with `tools/onshape/build.sh` and uploads
`tools/onshape/desktop/dist/*.AppImage`.

**a) Artifact (Actions):** in the fork, Actions → *Onshape importer app* → *Run workflow*
(branch `onshape-importer-v2`) → after ~15–25 min the run has an artifact
`cadrs-onshape-importer-appimage` (a zip containing the AppImage). Anyone with access to the run
can download it; it expires after 90 days.

**b) Release asset (permanent, what to link in the PR):**

```sh
git tag onshape-importer-v0.1.8
git push fork onshape-importer-v0.1.8
```

The tag triggers the same build; the `release` job attaches the AppImage to the release
`onshape-importer-v0.1.8` on the fork. The PR body can link
`https://github.com/xitix/cadrs/releases/tag/onshape-importer-v0.1.8`.

**c) Without Actions:** GitHub → the fork → Releases → *Draft a new release* → tag
`onshape-importer-v0.1.8` → attach
`tools/onshape/desktop/dist/cadrs Onshape Importer-0.1.8.AppImage` → publish. (A comment
attachment cannot take a 128 MB file; release assets take up to 2 GB.)

The version in the filename comes from `tools/onshape/desktop/package.json` (currently 0.1.8), so
bump it before tagging if the app changes again.

## 4. Remarks

- Re-running the workflow with `workflow_dispatch` only produces the artifact; the release job
  runs for tags.
- `gh workflow run onshape-importer-app.yml --repo xitix/cadrs --ref onshape-importer-v2` does
  the same as the Actions button (after `gh auth login`).
- The AppImage is not committed to git (too large); it lives only in Actions/Releases.
- The importer binary is in the local checkout at `target/release/cadrs-onshape`; the app package
  embeds it.

## GitHub Packages (`https://github.com/xitix/cadrs/packages`)

GitHub Packages is a **package registry** (npm, Maven/Gradle, NuGet, RubyGems and container images
at `ghcr.io`), not a place for arbitrary binaries. The AppImage therefore belongs in **Releases**
(above); `/packages` stays empty unless we publish a registry package.

What would make sense from this project:

### a) A container image of the headless importer (recommended if Packages is wanted)

The GUI app needs a display, but the CLI (`cadrs-onshape`) is useful in automation and fits a
container. A minimal image after a build:

```dockerfile
# tools/onshape/Dockerfile.cli
FROM debian:bookworm-slim
COPY target/release/cadrs-onshape /usr/local/bin/cadrs-onshape
ENTRYPOINT ["cadrs-onshape"]
```

Local publish (needs a **classic PAT with `write:packages`**, or `gh auth refresh -s write:packages`):

```sh
export CR_PAT=<token>
echo "$CR_PAT" | docker login ghcr.io -u xitix --password-stdin
cargo build -r -p cadrs_onshape
docker build -f tools/onshape/Dockerfile.cli -t ghcr.io/xitix/cadrs-onshape-importer:0.1.8 .
docker push ghcr.io/xitix/cadrs-onshape-importer:0.1.8
```

In Actions no PAT is needed: give the job `permissions: packages: write` and use
`docker/login-action` with `username: ${{ github.actor }}`, `password: ${{ secrets.GITHUB_TOKEN }}`,
then `docker/build-push-action` with `push: true` and
`tags: ghcr.io/${{ github.repository_owner }}/cadrs-onshape-importer:${{ github.ref_name }}`.
After the first push the package appears under the owner's *Packages*; from the package settings
(`https://github.com/users/xitix/packages/container/.../settings`) change visibility to **Public**
and link it to the `cadrs` repository if GitHub has not already.

### b) An npm package (not recommended for the Electron app)

The app's JavaScript could be scoped as `@xitix/cadrs-onshape-desktop`, but it is not a library
and would still download Electron at install time. If it is really wanted:

```sh
# .npmrc
@xitix:registry=https://npm.pkg.github.com
# then, with a PAT that has write:packages:
npm login --registry=https://npm.pkg.github.com --scope=@xitix
npm publish
```

### c) What to link from the PR

Release asset (permanent):
`https://github.com/xitix/cadrs/releases/tag/onshape-importer-v0.1.8`
Container (if published): `ghcr.io/xitix/cadrs-onshape-importer:0.1.8`
