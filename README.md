# cadrs
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

<!-- Download buttons: the latest release's files under their names without the version (see
.github/workflows/release.yml), the version on each badge read from GitHub by shields.io. macOS
goes to the releases page (its badge counts pre-releases) until a full release has the Mac
build; then link it to https://github.com/rvdende/cadrs/releases/latest/download/cadrs-universal-macos.dmg like the others. -->
<p>
  <a href="https://github.com/rvdende/cadrs/releases/latest/download/cadrs-x86_64-windows.zip"><img alt="Download for Windows" src="https://img.shields.io/github/v/release/rvdende/cadrs?style=for-the-badge&label=Windows&color=0078d4&logo=data:image/svg%2bxml;base64,PHN2ZyB4bWxucz0naHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmcnIHZpZXdCb3g9JzAgMCAyNCAyNCc+PHBhdGggZmlsbD0nd2hpdGUnIGQ9J00yIDQuNCAxMC4zIDMuM3Y4SDJ6TTExLjMgMy4xIDIyIDEuN3Y5LjZIMTEuM3pNMiAxMi40aDguM3Y4LjJMMiAxOS41ek0xMS4zIDEyLjRIMjJ2OS45bC0xMC43LTEuNXonLz48L3N2Zz4="></a>
  <a href="https://github.com/rvdende/cadrs/releases/latest/download/cadrs-universal-macos.dmg"><img alt="Download for macOS" src="https://img.shields.io/github/v/release/rvdende/cadrs?style=for-the-badge&include_prereleases&label=macOS&color=444444&logo=apple"></a>
  <a href="https://github.com/rvdende/cadrs/releases/latest/download/cadrs-x86_64-linux.tar.gz"><img alt="Download for Linux" src="https://img.shields.io/github/v/release/rvdende/cadrs?style=for-the-badge&label=Linux&color=e95420&logo=linux&logoColor=white"></a>
</p>

**A parametric, feature-based CAD app written in Rust**, with a Bevy UI and its own pure-Rust
B-rep modeling kernel. Its workflow is sketch on a plane, constrain and dimension,
build parts from features, put them together in assemblies, and make drawings from them.

- **Sketching**: lines, rectangles, circles, arcs, splines, Bézier curves, ellipses, polygons,
  slots, points and text. Snapping and inference to points, midpoints, centres, intersections,
  alignments and part edges. Geometric constraints and driving dimensions, with a
  least-squares solver that keeps the sketch consistent while you drag. Trim, extend, split,
  offset, mirror, patterns, and Use (project) of part edges.
- **Part Studios**: extrude, revolve, sweep, loft, fillet, chamfer, shell, draft, hole
  (counterbore, countersink, tapped; ISO and ANSI tables), linear and circular patterns,
  mirror, booleans, Transform, planes, mate connectors, thicken, fill and helix. Every feature
  can be edited later, and the part rebuilds.
- **Assemblies**: instances, fastened, revolute, slider, cylindrical, planar and other mates,
  a triad to move parts, mate connectors, exploded views and a bill of materials.
- **Drawings**: standard and projected views, sections, dimensions, annotations, callouts
  and BOM tables.
- **More**: versions and history, derived parts and linked documents, a variable table and
  expressions, measurement and mass properties, linear static simulation (FEA), a path-traced
  renderer, PCB Studio with IDF import and export, and STEP, IGES and STL import and export.
- **Drag and drop**: drop SVG, EPS, PDF, DXF or DWG drawings onto a Part Studio or an open
  sketch to get editable sketch curves, pictures to place them, and STEP, IGES or STL models to
  import them (EPS and PDF need Ghostscript and Poppler installed).
- **Onshape import**: bring your own Onshape documents across with their feature history
  instead of dead STEP geometry ([tools/onshape](tools/onshape/README.md)).
- **AI assistants (MCP)**: So an AI assistant on your machine (Claude Code, Claude Desktop, Cursor, …) can drive the running app: create documents, draw sketches, add and edit features, read back faces and edges, export STEP and take screenshots. Turn it on in **Preferences › AI assistants (MCP)**; the app then serves Streamable HTTP on `http://127.0.0.1:7680/mcp` (loopback only; the port is a preference). Every tool call runs through the command layer, so the assistant's steps show in the feature list and undo like your own. The repository's `.mcp.json` registers the server with Claude Code for this project (approve
it with `/mcp` the first time; `claude mcp list` shows whether it is connected). Elsewhere,
`claude mcp add --transport http cadrs http://127.0.0.1:7680/mcp` does the same, and other
clients take the URL in their MCP settings.
- **Scripts**: the MCP tools are the app's command registry (`cadrs --commands` lists them with
  their parameters' JSON schemas). `cadrs --run script.json` runs a list of them headless, a
  line of JSON out per answer ([scenarios/scripts/plate.json](scenarios/scripts/plate.json)),
  and a scenario's `Command("extrude", "{…}")` step runs one.

### Codebase

| Crate | What it is |
|---|---|
| `cadrs` | The binary: command-line flags, windowed or headless |
| `cadrs_app` | The app's Bevy plugins: documents page, tabs, viewport, tools and dialogs |
| `cadrs_ui` | A reusable Bevy widget library (theme, buttons, inputs, menus, dialogs, lists) |
| `cadrs_core` | The document model, features, rebuild, commands and undo, persistence (no Bevy) |
| `cadrs_sketch` | Sketch geometry, constraints, the solver, snapping and hit testing (no Bevy) |
| `cadrs_kernel` | The B-rep modeling kernel (pure Rust) and its API, with persistent face and edge names |
| `cadrs_drawing` | Drawing views, projection and annotations |
| `cadrs_fea` | Linear static finite-element analysis |
| `cadrs_render` | A CPU path tracer for photorealistic renders |
| `cadrs_idf`, `cadrs_pcb` | IDF files and PCB boards |
| `cadrs_harness` | Scripted scenarios, synthetic input and screenshots |
| `cadrs_mcp` | The MCP server through which AI assistants drive the app |

### Tests

`cargo test -r -p cadrs_core --test reference_parts -- --nocapture`

## Sponsors
Thank you to: 

[Sponsor cadrs on GitHub](https://github.com/sponsors/rvdende). 

<!-- sponsors --><a href="https://github.com/thomasfla"><img src="https:&#x2F;&#x2F;github.com&#x2F;thomasfla.png" width="60px" alt="User avatar: Thomas Flayols" /></a><!-- sponsors -->
