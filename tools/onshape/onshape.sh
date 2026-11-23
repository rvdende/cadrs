#!/usr/bin/env bash
# Start, stop, restart and check the cadrs Onshape importer desktop app.
#
#   tools/onshape/onshape.sh start              start it in the background
#   tools/onshape/onshape.sh stop               stop it (only what this script started)
#   tools/onshape/onshape.sh restart            stop, then start
#   tools/onshape/onshape.sh status             is it running? (exit 0 running, 3 stopped)
#   tools/onshape/onshape.sh logs [-f] [N]      the app's log (default 50 lines; -f follows)
#   tools/onshape/onshape.sh run                run in the foreground (Ctrl-C to stop)
#   tools/onshape/onshape.sh help
#
# It runs the packaged app when there is one (the newest tools/onshape/desktop/dist/*.AppImage),
# else Electron from the app's node_modules, i.e. a checkout. It never kills a process it did not
# start: the pid file records the process's start time and both must still match.
#
# State (pid, log, command): ${CADRS_ONSHAPE_STATE:-${XDG_STATE_HOME:-$HOME/.local/state}/cadrs-onshape-importer}/
#
# Environment:
#   CADRS_ONSHAPE_APP_CMD          run this command instead of the app (same start/stop/status)
#   CADRS_ONSHAPE_APPIMAGE         use this AppImage instead of the newest one in dist/
#   CADRS_ONSHAPE_ELECTRON_ARGS    extra arguments for Electron (default: --no-sandbox)
#   CADRS_ONSHAPE_APPIMAGE_EXTRACT=0   run the AppImage directly (needs FUSE) instead of
#                                  the safe --appimage-extract-and-run
#   CADRS_ONSHAPE_STATE            the state directory
#   CADRS_ONSHAPE_LOG_LINES        lines shown by `logs` (default 50)
#
# The app is a GUI: start it from a desktop session (DISPLAY or WAYLAND_DISPLAY set), or through
# `tools/onshape/build.sh --run` when debugging from source. In an environment without a display
# start fails with the app's own error in the log; `status` then says stopped.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
APP="$HERE/desktop"
STATE="${CADRS_ONSHAPE_STATE:-${XDG_STATE_HOME:-$HOME/.local/state}/cadrs-onshape-importer}"
PIDFILE="$STATE/app.pid"   # "<pid> <process start time>"
CMDFILE="$STATE/app.cmd"   # what was started, for `status`
LOG="$STATE/app.log"

die() { printf 'onshape.sh: %s\n' "$*" >&2; exit 1; }
say() { printf '%s\n' "$*"; }

ensure_state() {
  mkdir -p "$STATE" 2>/dev/null || die "cannot create the state directory $STATE (set CADRS_ONSHAPE_STATE)"
  [[ -w "$STATE" ]] || die "the state directory $STATE is not writable (set CADRS_ONSHAPE_STATE)"
}

# The process's start time in clock ticks since boot (field 22 of /proc/<pid>/stat), or empty.
proc_starttime() {
  local pid=$1
  [[ -r "/proc/$pid/stat" ]] || return 1
  awk '{print $22}' "/proc/$pid/stat" 2>/dev/null
}

# "<pid> <starttime>" of the app this script started, if that process is still that process.
running() {
  local pid start now
  [[ -r "$PIDFILE" ]] || return 1
  read -r pid start <"$PIDFILE" || return 1
  [[ "$pid" =~ ^[0-9]+$ ]] || return 1
  now=$(proc_starttime "$pid") || return 1
  [[ -n "$now" && "$now" == "$start" ]]
}

forget_pid() {
  rm -f "$PIDFILE" "$CMDFILE"
}

# Newest packaged AppImage, if any.
newest_appimage() {
  local file candidate
  file=''
  for candidate in "$APP"/dist/*.AppImage; do
    [[ -f "$candidate" ]] || continue
    if [[ -z "$file" || "$candidate" -nt "$file" ]]; then file=$candidate; fi
  done
  printf '%s' "$file"
}

# What `start`/`run` executes: the command and a human-readable description.
resolve_launch() {
  if [[ -n "${CADRS_ONSHAPE_APP_CMD:-}" ]]; then
    KIND="command: $CADRS_ONSHAPE_APP_CMD"
    LAUNCH=(bash -c "$CADRS_ONSHAPE_APP_CMD")
    return 0
  fi
  local appimage electron_args
  local -a extra=()
  appimage="${CADRS_ONSHAPE_APPIMAGE:-$(newest_appimage)}"
  electron_args="${CADRS_ONSHAPE_ELECTRON_ARGS:---no-sandbox}"
  if [[ -n "$appimage" ]]; then
    [[ -f "$appimage" && -x "$appimage" ]] || die "the AppImage is not executable: $appimage"
    KIND="AppImage: $appimage"
    local args=()
    if [[ "${CADRS_ONSHAPE_APPIMAGE_EXTRACT:-1}" != "0" ]]; then args+=(--appimage-extract-and-run); fi
    read -r -a extra <<<"$electron_args"
    LAUNCH=("$appimage" "${args[@]}" "${extra[@]}")
    return 0
  fi
  if [[ -f "$APP/node_modules/electron/cli.js" ]]; then
    KIND="Electron from source: $APP"
    read -r -a extra <<<"$electron_args"
    LAUNCH=(node "$APP/node_modules/electron/cli.js" "$APP" "${extra[@]}")
    return 0
  fi
  die "no app to run: build it with tools/onshape/build.sh (or set CADRS_ONSHAPE_APP_CMD)"
}

show_tail() {
  say ""
  say "last lines of $LOG:"
  tail -n 15 "$LOG" 2>/dev/null | sed 's/^/  /'
}

cmd_start() {
  ensure_state
  if running; then
    say "already running (pid $(cut -d' ' -f1 "$PIDFILE"))"
    return 1
  fi
  forget_pid
  local appimage_kind
  resolve_launch
  local display_note=""
  if [[ -z "${DISPLAY:-}" && -z "${WAYLAND_DISPLAY:-}" ]]; then
    display_note=" (no DISPLAY/WAYLAND_DISPLAY: the app needs a desktop session)"
    if [[ "$KIND" != command:* ]]; then
      say "cannot start the app: no DISPLAY or WAYLAND_DISPLAY (it is a GUI)."
      say "Start it from your desktop session, or with: tools/onshape/build.sh --run"
      return 1
    fi
  fi
  say "starting $KIND$display_note (waiting up to 4 s for the window)"
  {
    printf '\n=== %s start (%s)%s ===\n' "$(date -Is)" "$KIND" "$display_note"
    printf '$ %s\n' "${LAUNCH[*]}"
  } >>"$LOG" 2>&1
  # A new session (setsid) so stopping can signal the whole tree, and detached from this script
  # so it outlives us. `exec` keeps the pid: it is the session leader recorded below.
  ( cd "$APP" && exec setsid "${LAUNCH[@]}" >>"$LOG" 2>&1 ) &
  local pid start
  pid=$!
  [[ "$pid" =~ ^[0-9]+$ ]] || { die "could not start $KIND"; }
  # Give it a few seconds: an AppImage extracts itself first, and Electron dies on a missing
  # display only when it opens its window.
  local waited=0
  while (( waited < 40 )) && kill -0 "$pid" 2>/dev/null; do
    sleep 0.1
    (( waited++ ))
  done
  start=$(proc_starttime "$pid") || {
    forget_pid
    say "failed to start $KIND$display_note"
    if [[ "$KIND" != command:* && -z "$display_note" ]]; then
      say "the app is a GUI: start it from a desktop session (DISPLAY/WAYLAND_DISPLAY)."
      say "AppImage: try CADRS_ONSHAPE_APPIMAGE_EXTRACT=0 (needs FUSE), or rebuild with tools/onshape/build.sh."
    fi
    show_tail
    return 1
  }
  printf '%s %s\n' "$pid" "$start" >"$PIDFILE"
  printf '%s\n' "$KIND" >"$CMDFILE"
  say "started (pid $pid) $KIND"
  say "log: $LOG"
  return 0
}

cmd_stop() {
  if ! running; then
    if [[ -e "$PIDFILE" ]]; then
      say "not running (removed a stale pid file)"
      forget_pid
    else
      say "not running"
    fi
    return 0
  fi
  local pid
  pid=$(cut -d' ' -f1 "$PIDFILE")
  # setsid made the app its own session/process-group leader: signal the group, then the pid.
  kill -TERM -- "-$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null
  local waited=0
  while running && (( waited < 100 )); do
    sleep 0.1
    (( waited++ ))
  done
  if running; then
    say "did not stop after 10 s; killing it"
    kill -KILL -- "-$pid" 2>/dev/null || kill -KILL "$pid" 2>/dev/null
    sleep 0.2
  fi
  forget_pid
  say "stopped (pid $pid)"
}

cmd_status() {
  if running; then
    local pid start uptime kind
    read -r pid start <"$PIDFILE"
    uptime=$(ps -o etime= -p "$pid" 2>/dev/null | tr -d ' ')
    kind=$(cat "$CMDFILE" 2>/dev/null || echo unknown)
    say "running (pid $pid, up ${uptime:-?}, $kind)"
    say "log: $LOG"
    return 0
  fi
  if [[ -d "$STATE" && -e "$PIDFILE" ]]; then
    say "stopped (stale pid file removed)"
    forget_pid
  else
    say "stopped"
  fi
  return 3
}

cmd_restart() {
  cmd_stop
  cmd_start
}

cmd_logs() {
  ensure_state
  local follow=0 lines="${CADRS_ONSHAPE_LOG_LINES:-50}"
  while (($#)); do
    case "$1" in
      -f|--follow) follow=1 ;;
      -n) shift; lines="${1:-$lines}" ;;
      [0-9]*) lines="$1" ;;
      *) die "logs: unknown argument $1" ;;
    esac
    shift
  done
  [[ -r "$LOG" ]] || { say "no log yet ($LOG)"; return 0; }
  if (( follow )); then tail -n "$lines" -f "$LOG"; else tail -n "$lines" "$LOG"; fi
}

cmd_run() {
  resolve_launch
  say "running in the foreground ($KIND); Ctrl-C to stop"
  cd "$APP" && exec "${LAUNCH[@]}"
}

usage() {
  sed -n '2,18p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

case "${1:-help}" in
  start) shift; cmd_start "$@" ;;
  stop) shift; cmd_stop "$@" ;;
  restart) shift; cmd_restart "$@" ;;
  status) shift; cmd_status "$@" ;;
  logs) shift; cmd_logs "$@" ;;
  run) shift; cmd_run "$@" ;;
  help|-h|--help) usage ;;
  *) usage >&2; exit 2 ;;
esac
