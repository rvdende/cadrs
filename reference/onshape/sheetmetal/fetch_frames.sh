#!/bin/bash
# Re-creates the local (git-ignored) video frames of the Simultaneous Sheet Metal course lessons.
#
# For each "slug wistia-id" line in media.txt: downloads the lesson's 1080p MP4 into raw/, its
# poster into <slug>/poster.jpg, and saves frames into <slug>/tSSSS.S.png (named by their time in
# seconds, to match the transcript in raw/<slug>.txt): one at every scene change (threshold 0.01,
# about one every few seconds of a screen recording), plus one every 4 s for lessons that would
# otherwise average fewer than one frame per 7 s.
set -e
cd "$(dirname "$0")"
mkdir -p raw
while read -r slug id; do
  [ -z "$slug" ] && continue
  mkdir -p "$slug"
  json=$(curl -sS </dev/null "https://fast.wistia.com/embed/medias/$id.json")
  url=$(python3 -c "import json,sys; a=json.loads(sys.argv[1])['media']['assets']; v=[x for x in a if x['type']=='hd_mp4_video' and x.get('height')==1080] or [x for x in a if x['type']=='original']; print(v[0]['url'])" "$json")
  poster=$(python3 -c "import json,sys; a=json.loads(sys.argv[1])['media']['assets']; print([x for x in a if x['type']=='still_image'][0]['url'])" "$json")
  [ -f "raw/$slug.mp4" ] || curl -sS </dev/null -o "raw/$slug.mp4" "${url%.bin}.mp4"
  curl -sS </dev/null -o "$slug/poster.jpg" "${poster%.bin}.jpg"
  rm -f "$slug"/frame-*.png "$slug"/t*.png
  ffmpeg -nostdin -i "raw/$slug.mp4" -vf "select='gt(scene,0.01)',showinfo" -vsync vfr "$slug/frame-%03d.png" 2> "raw/$slug.frames.log"
  i=1
  for t in $(grep -o 'pts_time:[0-9.]*' "raw/$slug.frames.log" | cut -d: -f2); do
    f=$(printf "%s/frame-%03d.png" "$slug" $i)
    [ -f "$f" ] && mv "$f" "$(printf "%s/t%06.1f.png" "$slug" "$t")"
    i=$((i+1))
  done
  d=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "raw/$slug.mp4")
  n=$(ls "$slug"/t*.png 2>/dev/null | wc -l)
  if python3 -c "import sys; sys.exit(0 if $d / max($n, 1) > 7 else 1)"; then
    ffmpeg -nostdin -loglevel error -i "raw/$slug.mp4" -vf fps=1/4 "$slug/p%03d.png"
    for p in "$slug"/p*.png; do
      k=$(basename "$p" .png | sed 's/^p0*//')
      mv "$p" "$(printf "%s/t%06.1f.png" "$slug" $(( (k - 1) * 4 )))"
    done
  fi
  echo "$slug: $(ls "$slug" | wc -l) images"
done < media.txt
