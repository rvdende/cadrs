<!-- Made from the code: CADRS_BLESS=1 cargo test -p cadrs_app --lib docs_reference writes this page again. Don't edit it by hand. -->

# Preferences

The app menu's **Preferences…** opens them. The search box at the top finds a setting in any section, and **Restore Defaults** puts back the defaults of the section shown. Preferences belong to your computer, not to a document. The browser version has no AI assistants section, and no vertical sync or profiling settings.

## Interface

How cadrs looks, and what the 3D view shows.

### Appearance

- **Color scheme:** System · Light · Dark. Default: System.
- **UI scale:** 75% · 90% · 100% (display default) · 110% · 125% · 150% · 175% · 200%. Default: 100% (display default).
- **Language:** Afrikaans · Deutsch · English · Español · Français · Italiano · Nederlands · Português (Brasil) · Русский. Default: English.

  Takes effect when you press Apply or OK.

### Grid and origin

- **Grid:** Off · Crossed · Lines. Default: Lines.
- **Origin:** Hidden · Dot · Axes (X, Y, Z). Default: Dot.

### Display

- **Frame stats:** Off · Frame rate · Frame rate and details. Default: Off.
- **Display:** Vertical sync (VSync), on by default.

  On, frames wait for the screen's refresh. Off, they are drawn as fast as they are made: the frame rate shows what a frame costs, but cadrs uses more power.
- **Profiling:** Time systems, off by default.

  Shows where each frame's CPU time goes, system by system. It slows cadrs down, so it's off unless you need it. Takes effect at the next start.

## Navigation

How the mouse moves the 3D view.

### 3D view mouse controls

- **Mouse controls:** Onshape · SolidWorks · Inventor · Creo. Default: Onshape.
- **Wheel zoom:** Invert zoom direction, off by default.

  Off, rolling the wheel away from you zooms in. On, rolling it toward you zooms in.

What each choice of Mouse controls does with the mouse:

| | Onshape | SolidWorks | Inventor | Creo |
| --- | --- | --- | --- | --- |
| Rotate | Right drag | Middle drag | Shift+Middle drag | Middle drag |
| Rotate without roll | Alt+Right drag | — | — | — |
| Pan | Middle drag, Ctrl+Right drag | Ctrl+Middle drag | Middle drag | Shift+Middle drag |
| Zoom | Scroll wheel, Shift+Middle drag | Scroll wheel, Shift+Middle drag | Scroll wheel, Ctrl+Middle drag | Scroll wheel, Ctrl+Middle drag |

### View

- **Camera:** Keep camera up, off by default.

  Rotating turns about the vertical and tilts, without rolling the view.
- **Sketch:** View normal to the sketch plane, off by default.

  When a sketch gets its plane, and when one is opened for editing, the view turns square to it, as N does.

### SpaceMouse

- **Zoom by:** Pulling up and pushing down · Moving forward and backward. Default: Pulling up and pushing down.

  The other of the two moves the view up and down.
- **Speed:** 25% · 50% · 75% · 100% · 150% · 200% · 300%. Default: 100%.

## Assembly

How parts move in an assembly.

### Assembly manipulation

- **Rotation snap increment:** default 45 deg.
- **Snap tolerance:** default 10 deg.

  Snap when within the tolerance of an increment. Hold Shift to disable snapping. Increment: 0–180° (0 disables). Tolerance: 0–90°.

## Collaboration

Sharing documents, peer to peer.

### Network

- **cadrs ID**
- **P2P networking:** Connect when cadrs starts, off by default.

  Documents are shared peer to peer, with no server, through iroh's relays. Off, cadrs stays offline until you connect with the network button in the top bar, or share. Give your cadrs ID to someone who wants to share a document with you.

### Public documents

- **Public documents:** Find public documents while online, on by default.
- **Seed peers**

## AI assistants (MCP)

Letting AI assistants on this computer work in cadrs.

### MCP server

- **MCP server:** Enabled, off by default.
- **Endpoint**
- **Port**

  While it is on, AI assistants on this computer (Claude, Cursor, …) can create and edit documents.
