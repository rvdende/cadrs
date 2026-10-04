# Sheet metal reference

Gathered 2026-10-01 for building Onshape-style sheet metal in cadrs.

- [simultaneous-sheet-metal.md](simultaneous-sheet-metal.md): the requirements (`SM*`, exercises
  `E1`–`E4`, cross-cutting `X*`), paraphrased from the course and the help pages.
- [simultaneous-sheet-metal-gaps.md](simultaneous-sheet-metal-gaps.md): what cadrs has, the
  proposed design, the icons to add to icon-rs, and milestones **P3I.1–P3I.9** with their judging
  (fresh judge, 0–10, pass at 8.5).

## Sources

| Source | What | Read |
|---|---|---|
| Learning Center catalog, topic "Sheet Metal" (`learn.onshape.com/catalog?labels=["Topic"]&values=["Sheet Metal"]`) | 1 course + 15 videos (all of them course lessons) | 2026-10-01 |
| Course **Simultaneous Sheet Metal** (`learn.onshape.com/courses/simultaneous-sheet-metal`) | 18 lessons (16 videos, 2 animated slides), 4 slide exercises; the 2 Forms lessons are paid and were not read | 2026-10-01 |
| Help Center, 16 pages: `PartStudio/sheet_metal_{model,flange,hem,tab,bend,jog,form,loft,make_joint,corner,bend_relief,joint,corner_break,table}.htm`, `PartStudio/finish_sheet_metal_model.htm`, `Drawing/drawing_flat_pattern_view.htm` | Exact dialog fields, options, ranges, defaults; dialog screenshots | 2026-10-01 (pages "Last Updated: September 24, 2026") |

The Help Center now needs a signed-in browser (plain HTTP gets redirected to sign-in); its images
are still public. The course needs an Onshape account.

## Local-only files (git-ignored, see `../.gitignore`)

These are for internal reference only. Never commit or ship them, and never copy Onshape's icons,
logo or artwork from them (CLAUDE.md).

| Path | Contents |
|---|---|
| `raw/<lesson>.txt`, `raw/ex<n>-*.txt` | Each lesson's key takeaways and transcript; each exercise's goal and step text (with the Wistia id) |
| `raw/help-*.txt` | Each help page's full text with every dropdown expanded; `[IMG path]` marks where each image sits |
| `raw/help-images.txt` | The 193 image paths the help pages use |
| `raw/<lesson>.mp4` | The lesson videos (1080p) |
| `<lesson>/poster.jpg`, `<lesson>/tSSSS.S.png` | Poster and frames of each lesson video, named by time in seconds (match them to the transcript); 18 lessons, ~590 frames |
| `07-corner/slide-1.gif`, `08-bend-relief/slide-1.gif` | The two animated slide lessons |
| `ex1-importing-dxf-bend/`, `ex2-creating-sheet-metal-parts/`, `ex3-drawings/`, `ex4-sheet-metal-rework/` | Each exercise's cover/goal image and `step-NN` slides (the "as shown" values are in these) |
| `help/<path>` | The help pages' images under their help paths, e.g. `help/feature-tools/sheetmetal-dialog-convert-02.png` (dialogs), `help/icons/*` (Onshape's tool icons: reference only) |

Most useful images: the dialogs `help/feature-tools/sheetmetal-dialog-convert-02.png`,
`sheetmetal-extrude-02.png`, `sheetmetal-thicken-02.png`, `sheetmetalflange-dialog-01.png`,
`sheetmetalflange-dialog-03.png`, `shmetal-hem-dialog.png`, `sheetmetaltab-dialog.png`,
`bend-dialog.png`, `sm-jog-01.png`, `sheetmetalmakejoint-dialog.png`, `sheetmetalcorner-dialog.png`,
`sheetmetalbendrelief-dialog.png`, `modify-joint-02.png`, `sheetmetal-export-01-03.png`,
`extrude2_abbrev_dialogbox.png`; the panel `sheetmetalflatpatterntable-02.png`; the relief shapes
`sheetmetal-corner-*`, `sheetmetalcorner-*`, `bendrelief-*`; and the lesson frames for the live UI
(toolbar group, panel layout, labels, highlighting).

## Re-creating the local files

- Frames and videos: `./fetch_frames.sh` (reads `media.txt`: lesson slug → Wistia media id; needs
  `curl`, `ffmpeg`, `python3`).
- Text, exercise slides and help images were captured from a signed-in Chrome session (Claude in
  Chrome): course pages with `get_page_text`, exercise carousels by stepping "Next Slide", help
  pages with every `.MCDropDownHotSpot` clicked open. Slide and help images download without a
  session: `https://d36ai2hkxl16us.cloudfront.net/thoughtindustries/image/upload/v1/course-uploads/6e557ed6-d03d-4c48-9492-4d18d145d7a1/<name>`
  and `https://cad.onshape.com/help/Content/Resources/Images/<path>`.
