# English: the source catalog. Every key the app uses is here; the other languages translate
# these (a key missing there shows this text). Fluent syntax: `key = text`, `# ` lines are
# notes for translators, `{ $name }` is filled in by the app (keep it as it is).

## Common words, used across the app (reused by every area; a key's text is exactly its English).
common-ok = OK
common-cancel = Cancel
common-close = Close
common-apply = Apply
common-done = Done
common-save = Save
common-open = Open
common-add = Add
common-create = Create
common-delete = Delete
common-remove = Remove
common-rename = Rename
common-rename-ellipsis = Rename…
common-edit-ellipsis = Edit…
common-copy = Copy
common-paste = Paste
common-undo = Undo
common-redo = Redo
common-select = Select
common-show = Show
common-hide = Hide
common-help = Help
common-restore = Restore
common-import = Import
common-export = Export
common-browse = Browse…
common-properties = Properties…
common-name = Name
common-type = Type
common-description = Description
common-value = Value
common-size = Size
common-units = Units
common-format = Format
common-text = Text
common-none = None
common-version = Version
common-create-version = Create version
common-folder = Folder
common-new-folder = New folder
common-choose-folder = Choose a folder
common-file-name = File name
common-untitled-document = Untitled document
common-move-up = Move up
common-move-down = Move down
common-clear-selection = Clear selection
common-remove-selection = Remove selection
common-zoom-to-fit = Zoom to fit
common-zoom-to-selection = Zoom to selection
common-add-comment = Add comment
common-suppress = Suppress
common-unsuppress = Unsuppress
# A solid body of a Part Studio.
common-part = Part
common-face = Face
common-edge = Edge
common-plane = Plane
common-sketch = Sketch
common-solid = Solid
common-dimension = Dimension
common-material = Material
common-mate-connector = Mate connector
common-part-studio = Part Studio
common-assembly = Assembly
common-drawing = Drawing
common-render-studio = Render Studio
common-bill-of-materials = Bill of materials
common-extrude = Extrude
common-direction = Direction
common-opposite-direction = Opposite direction
common-flip-direction = Flip direction
common-symmetric = Symmetric
common-distance = Distance
common-angle = Angle
common-depth = Depth
common-offset = Offset
common-thickness = Thickness
common-rotation = Rotation
# How a feature's new solids join the parts already there.
common-merge-scope = Merge scope
common-merge-with-all = Merge with all
# { $name } is a part's name.
common-face-of = Face of { $name }
# { $name } is a part's name.
common-edge-of = Edge of { $name }
# { $name } is a part's name.
common-vertex-of = Vertex of { $name }
# { $name } is a sketch's name.
common-line-of = Line of { $name }
# { $name } is a sketch's name.
common-curve-of = Curve of { $name }
# { $name } is what is edited (a feature's or a document's name).
common-edit-named = Edit { $name }
# { $name } is what is cancelled (a feature's name).
common-cancel-named = Cancel { $name }
# { $name } is what is inserted (a part's, a sketch's or a document's name).
common-insert-named = Insert { $name }

## The Preferences dialog.
preferences = Preferences
preferences-mouse-heading = 3D view mouse controls
preferences-mouse-controls = Mouse controls
preferences-interface = Interface
# How large the interface is drawn (a percentage).
preferences-ui-scale = UI scale
# The row that chooses the interface's language.
preferences-language = Language
# The row of display options (vertical sync): the monitor, not showing something.
preferences-display = Display
# After the language changes: what was built once (toolbars, the top bar) follows later.
preferences-language-later = Some texts change when they are next opened, or after a restart.

## The document window.
# The button that shares the document with other people.
share = Share
share-tooltip = Share this document
rename-document = Rename document

## The feature list (a Part Studio's history of modelling operations).
# { $count } is how many features the list has.
features-count = Features ({ $count })
filter-placeholder = Filter by name or type
# The solid bodies a Part Studio makes.
parts = Parts
# Bodies with no volume (sheets).
surfaces = Surfaces

## crates/cadrs_ui/src/color_editor.rs
# A heading over the preset color swatches.
ui-color-editor-colors = Colors
# A heading over the colors the user saved.
ui-color-editor-custom-colors = Custom colors
# A heading over the hue/saturation mixer.
ui-color-editor-mixer = Mixer
# The label of the hexadecimal color code field (#RRGGBB).
ui-color-editor-hex = Hex
ui-color-editor-opacity = Opacity
# A button that puts the color back to its default.
ui-color-editor-default = Default
ui-color-editor-save-color = Save this color

## crates/cadrs_ui/src/color_scheme.rs
# Tooltips of the light / dark / system appearance toggle.
ui-color-scheme-light = Light
ui-color-scheme-dark = Dark
ui-color-scheme-system = Match the system

## crates/cadrs_ui/src/dock.rs
ui-dock-hide-panel = Hide panel

## crates/cadrs_ui/src/document_browser.rs
# The placeholder of the document browser's search field.
ui-document-browser-search = Search or paste document id
# Tooltip of the button that filters the documents (a verb).
ui-document-browser-filter = Filter
ui-document-browser-back = Back to the documents
# Tooltip of the button that opens the graph of a document's versions and branches.
ui-document-browser-version-graph = Version graph

## crates/cadrs_ui/src/entry_list.rs
# Tooltips of the button that folds or unfolds an entry (verbs).
ui-entry-list-collapse = Collapse
ui-entry-list-expand = Expand

## crates/cadrs_ui/src/feature_dialog.rs
# Tooltip of a feature dialog's check-mark button. Keep "(Enter)" as it is: the tooltip shows it as a key.
ui-feature-dialog-accept = Accept (Enter)
# Tooltip of a feature dialog's × button. Keep "(Esc)" as it is: the tooltip shows it as a key.
ui-feature-dialog-cancel = Cancel (Esc)

## crates/cadrs_ui/src/file_picker.rs
# The file types the file picker shows when any file will do.
ui-file-picker-all-files = All files
# The label of the field with the folder the picker shows.
ui-file-picker-look-in = Look in
ui-file-picker-up = Up one folder
# A hint under a folder picker; "Select folder" is the name of its button (ui-file-picker-select-folder).
ui-file-picker-folders-hint = Open a folder, then Select folder
# { $types } is the list of file patterns the picker shows ("*.step, *.stp") or ui-file-picker-all-files.
ui-file-picker-types-multi = Files of type: { $types }. Ctrl+click to choose several files
# { $types } is the list of file patterns the picker shows ("*.step, *.stp") or ui-file-picker-all-files.
ui-file-picker-types = Files of type: { $types }
# The button that picks the folder shown.
ui-file-picker-select-folder = Select folder
ui-file-picker-empty = No matching files in this folder

## crates/cadrs_ui/src/name_popup.rs
# Tooltip of the check-mark button that accepts a typed name.
ui-name-popup-accept = Accept

## crates/cadrs_ui/src/selection_field.rs
# Tooltip of the × that empties a selection field (a verb).
ui-selection-field-clear = Clear

## crates/cadrs_ui/src/selection_list.rs
# Tooltip of the button that empties a selection list.
ui-selection-list-clear-all = Clear all
ui-selection-list-reorder = Reorder items
# Tooltip of an item's drag handle.
ui-selection-list-drag = Drag to reorder
# Tooltip of the button that swaps an item for another selection.
ui-selection-list-replace = Replace reference

## crates/cadrs_ui/src/tab_strip.rs
# Tooltip of a tab whose page isn't made yet.
ui-tab-strip-not-available = Not available yet

## crates/cadrs_ui/src/text_dialog.rs
# Tooltips of the text dialog's style buttons.
ui-text-dialog-bold = Bold
ui-text-dialog-italic = Italic
ui-text-dialog-flip-h = Flip left to right
ui-text-dialog-flip-v = Flip upside down
# { $count } is how many characters the text has, { $max } the most it may have.
ui-text-dialog-characters = { $count }/{ $max } characters

## crates/cadrs_ui/src/variable_list.rs
# The last row of the variable suggestions, which makes a new variable.
ui-variable-list-new-variable = New variable…

## crates/cadrs_ui/src/version_graph.rs
# The version graph's legend: the symbol for the workspace (the editable state of a branch).
ui-version-graph-workspace = Workspace
# The version graph's legend: a version the app made by itself.
ui-version-graph-auto-version = Auto version

## crates/cadrs_app/src/app_menu.rs
# Tooltip of the cadrs mark at the top left, which opens the app menu.
shell-app-menu = Menu
shell-preferences-ellipsis = Preferences…
shell-app-menu-shortcuts = Keyboard shortcuts
shell-app-menu-github = GitHub
# A disabled hint under the workspaces: how to make another one ("History" is the version history panel).
shell-app-menu-workspace-hint = Branch from a version in History
# The submenu listing the document's workspaces (its main line and branches) to switch to.
shell-app-menu-workspace = Workspace
# Goes back to the documents page.
shell-app-menu-all-documents = All documents
shell-app-menu-rename-document = Rename document…
shell-app-menu-export-document = Export document…
shell-app-menu-create-version = Create version…
# Opens the dialog with the document's default units.
shell-app-menu-workspace-units = Workspace units…
# Rebuilds every tab from scratch, throwing away the cached results.
shell-app-menu-full-rebuild = Full rebuild (clear cache)

## crates/cadrs_app/src/top_bar.rs
# The account menu on a narrow window: opens the comments panel.
shell-top-bar-comments = Comments
# The account menu on a narrow window: opens the version history.
shell-top-bar-history = History
shell-top-bar-share = Share…
# The bell's tooltip and its entry in the account menu.
shell-notifications = Notifications
shell-top-bar-network-online = Network: online
shell-top-bar-network-failed = Network: could not go online
shell-top-bar-network-offline = Network: offline
# { $version } is the new version's number ("0.9.1").
shell-top-bar-update = Update to v{ $version }…
shell-top-bar-update-tooltip = Read the release notes and update

## crates/cadrs_app/src/notifications.rs
shell-notice-whats-new-title = A new look for cadrs
# "☰" is the old menu button's symbol; Labels, Trash, Properties, History and Used in are names of menu items.
shell-notice-whats-new-body = The cadrs mark at the top left is the menu now: Labels, Trash, the documents folder, Preferences and the shortcuts, and in a document what ☰ had. Right-click a document for Properties, History and Used in. Your name at the top right switches identities.
# { $version } is the new version's number ("0.9.1").
shell-notice-update-title = cadrs { $version } is available
shell-notice-update-body = Click to read the release notes and update.
shell-notice-none = You're all caught up

## crates/cadrs_app/src/network.rs
# The network button's tooltips.
shell-network-offline-tooltip = Offline: nothing is sent to the network
shell-network-failed = Could not go online
shell-network-online-tooltip = Online for sharing (peer to peer)
# { $why } is the error, as the network library gives it (in English).
shell-network-cannot-go-online = Cannot go online: { $why }
# Goes online (a verb).
shell-network-connect = Connect
# { $sending } and { $receiving } are data rates ("1.2 MB/s").
shell-network-rates = Sending { $sending } · receiving { $receiving }
# The network menu's subtitle while online and alone in the document.
shell-network-alone = No one else in this document
shell-network-one-other = 1 other person here
# { $count } is how many other people are in the document (2 or more).
shell-network-others = { $count } others here
# The network menu's title: the state of the connection.
shell-network-online = Online
shell-network-offline = Offline
shell-network-nothing-sent = Nothing is sent to the network
# Goes offline (a verb).
shell-network-disconnect = Disconnect
# Stops following another person's view.
shell-person-stop-following = Stop following
# { $name } is another person's name: follow their view as they move it.
shell-person-follow = Follow { $name }
# { $name } is another person's name: move the view to where they look.
shell-person-go-to = Go to { $name }'s view
shell-network-invite = Invite people…
shell-network-invite-tooltip = Share this document: by link, or with people by their cadrs ID
shell-network-autoconnect = Connect when cadrs starts

## crates/cadrs_app/src/collab/mod.rs
# Toasts when sharing or joining needs the network: while it is off for this run (CADRS_NET=0 is a setting, keep it), or while cadrs is offline (the toast has a Connect button).
shell-collab-share-net-off = Sharing needs the network, and the network is off (CADRS_NET=0)
shell-collab-share-offline = Sharing needs the network, and cadrs is offline
shell-collab-join-net-off = Joining needs the network, and the network is off (CADRS_NET=0)
shell-collab-join-offline = Joining needs the network, and cadrs is offline
# Why a shared document is read-only; { $owner } is the name of the person who shared it.
shell-collab-read-only-reason = Connecting to { $owner }… (while cadrs is offline, this is the last copy)
# { $owner } is the name of the person who shared the document.
shell-collab-may-not-export = { $owner } hasn't let you export this document
# { $owner } is the name of the person who shared the document; { $what } is what they didn't allow (an English verb, as "copy").
shell-collab-may-not = { $owner } hasn't let you { $what } this document
shell-collab-only-saved = Only a saved document can be shared
# { $owner } is the name of the person who shared the document.
shell-collab-only-owner-shares = Only { $owner } can share this document
shell-collab-share-needs-network = Sharing needs the network
shell-collab-link-copied-edit = Link copied: anyone with it can open and edit this document
shell-collab-own-link = That is a link to your own document
# { $name } is the shared document's name.
shell-collab-joining = Joining “{ $name }”…
# { $name } is the shared document's name; "Shared with me" is the name of a folder on the documents page.
shell-collab-in-shared-with-me = “{ $name }” is in Shared with me
# { $why } is what went wrong.
shell-collab-cannot-join = Cannot join: { $why }
shell-collab-another-document = The owner sent another document

## crates/cadrs_app/src/collab/presence.rs
# Toasts when someone comes into the shared document: { $name } is their name, { $tab } the tab they are in. "is here": they were already there when this user came.
shell-presence-is-here-in = { $name } is here, in { $tab }
shell-presence-joined-in = { $name } joined, in { $tab }
shell-presence-is-here = { $name } is here
shell-presence-joined = { $name } joined
# { $name } is another person's name.
shell-presence-follows-you = { $name } is following you
# { $name } is another person's name.
shell-presence-stopped-following-you = { $name } stopped following you
# { $name } is another person's name: they left the shared document.
shell-presence-left = { $name } left
# Buttons on the toast saying someone joined: go to their view once, or follow it.
shell-presence-toast-go-to = Go to
shell-presence-toast-follow = Follow
# The person this user followed left the document.
shell-presence-leader-left = Stopped following: they left
# { $name } is the followed person's name, { $tab } the tab they moved to.
shell-presence-following-to = Following { $name } to { $tab }
# Someone hasn't done anything for a while (under a minute).
shell-presence-idle = idle
# { $minutes } is how many minutes someone has been idle; "min" is minutes.
shell-presence-idle-minutes = idle { $minutes } min
# Where someone is: { $tab } is a tab's name, { $feature } the feature they edit.
shell-presence-in-tab = In { $tab }
shell-presence-in-tab-editing = In { $tab }, editing { $feature }
# The same, after the person's name in a list ("Alex · in Part Studio 1").
shell-presence-after-name-in-tab = in { $tab }
shell-presence-after-name-in-tab-editing = in { $tab }, editing { $feature }
# { $feature } is the feature someone edits (when their tab isn't known).
shell-presence-editing = editing { $feature }
# Part of a person's avatar tooltip, after their name: they follow this user.
shell-presence-avatar-follows-you = following you
# { $name } is a person whose connection dropped for a while.
shell-presence-reconnecting-name = { $name } · reconnecting…
shell-presence-voice-join = Join voice: talk with everyone in this document
shell-presence-voice-unmute = Unmute
shell-presence-voice-mute = Mute
shell-presence-voice-leave = Leave voice
# A person's avatar tooltip: { $who } is their name and where they are ("Alex · In Part Studio 1").
shell-presence-avatar-following = { $who }. Following: click for more
# A person's avatar tooltip: { $who } is their name and where they are ("Alex · In Part Studio 1").
shell-presence-avatar-click = { $who }. Click to follow or go to them
# A person's card while their connection dropped for a while.
shell-presence-reconnecting = Reconnecting…
shell-presence-follow-tooltip = See their tab and their view as they move
shell-presence-go-to-tooltip = Open their tab and view, once
# The banner over the graphics area while following someone; { $name } is their name.
shell-presence-following-banner = Following { $name }
shell-presence-stop-tooltip = Stop following (Esc, or click anywhere)
# The banner's button that stops following (a verb); the Esc key cap is beside it.
shell-presence-stop = Stop

## crates/cadrs_app/src/collab/pointers.rs
# Stands for a person whose name isn't known yet, in shell-pointers-ping.
shell-pointers-someone = Someone
# A toast: someone points at something; { $name } is their name, { $tab } the tab it is in.
shell-pointers-ping-in = { $name } wants you to look here in { $tab }
# A toast: someone points at something; { $name } is their name.
shell-pointers-ping = { $name } wants you to look here
# Tooltip of a person's chip at the view's edge; { $who } is their name and what they do ("Alex · Sketch 1").
shell-pointers-edge-tooltip = { $who }: click to look there
# A label at where someone's camera is; { $name } is their name.
shell-pointers-view-of = { $name }'s view
# A label at a point someone pinged; { $name } is their name.
shell-pointers-look-here-name = { $name }: look here
shell-pointers-look-here = Look here

## crates/cadrs_app/src/collab/comments.rs
# The comments panel's filters.
shell-comments-filter-all = All comments
shell-comments-filter-open-tasks = All open tasks
shell-comments-filter-mine = Open tasks assigned to me
shell-comments-filter-completed = Completed tasks
shell-comments-no-document = No document is open
# { $owner } is the name of the person who shared the document.
shell-comments-may-not-comment = { $owner } hasn't let you comment on this document
# { $owner } is the name of the person who shared the document, who is offline now.
shell-comments-owner-away = Comments reach { $owner } once they're back
# { $name } is who wrote the comment, { $text } the comment.
shell-comments-task-for-you = { $name } gave you a task: { $text }
# { $name } is who wrote the comment, { $text } the comment.
shell-comments-mentioned-you = { $name } mentioned you: { $text }
# What a comment is tagged to, when that tab or feature no longer exists.
shell-comments-a-tab = a tab
shell-comments-a-feature = a feature
# The comments panel's title.
shell-comments-title = Comments
shell-comments-none-yet = No comments yet
# The comments list when the filter leaves nothing.
shell-comments-nothing-here = Nothing here
# { $name } is the tab or feature the next comment is tagged to.
shell-comments-tagged = Tagged: { $name }
# { $name } is who wrote the comment the next one replies to.
shell-comments-replying-to = Replying to { $name }
shell-comments-placeholder = Add a comment (@name to mention)
shell-comments-assign = Assign to the first @mention
shell-comments-reply = Reply
shell-comments-delete-reply = Delete reply
shell-comments-delete-thread = Delete thread
# A comment that is a task; { $name } is whom it is assigned to.
shell-comments-task-open = Task for { $name } · open
shell-comments-task-done = Task for { $name } · done
shell-comments-mention-needed = Mention someone with @ to give them the task

## crates/cadrs_app/src/collab/live.rs
# The top bar's word on a shared document while its owner ({ $owner }) is away and this user's cadrs keeps the edits in order.
shell-live-status-you-keep-order = { $owner } away · you keep order
# The top bar's word on a shared document while its owner ({ $owner }) is away and { $host }'s cadrs keeps the edits in order.
shell-live-status-host-keeps-order = { $owner } away · { $host } keeps order
# The top bar's short words on how a shared document's copy stands.
shell-live-status-connecting = Connecting…
shell-live-status-owner-away = Owner away · last copy
shell-live-status-removed = No longer shared
shell-live-status-returning = Handing back…
shell-live-status-view-only = View only
# Why a shared document is read-only; { $owner } is the name of the person who shared it.
shell-live-connecting-to = Connecting to { $owner }…
shell-live-owner-away = { $owner } isn't sharing this document right now: you're viewing the last copy
shell-live-removed = { $owner } no longer shares this document with you: this is the last copy
shell-live-handing-back = Handing the document back to { $owner }…
shell-live-view-only = You can view this document; { $owner } hasn't given you edit access
# { $owner } is the name of the person who shared the document.
shell-live-you-keep-order = { $owner } is away: your cadrs keeps the edits in order until they're back
# { $owner } is the name of the person who shared the document.
shell-live-edits-lost = Some of your edits didn't reach { $owner }

## crates/cadrs_app/src/collab/locks.rs
# The document isn't shared live (an error).
shell-locks-not-shared = Not shared
# { $name } is the person who has the tab open for themselves.
shell-locks-tab-open = { $name } has this tab open
shell-locks-document-not-shared = This document isn't shared
shell-collab-view-access = You have view access to this document
shell-locks-owner-away = The owner's cadrs is away
# { $name } is the person who has the tab open for themselves.
shell-locks-take-over = { $name } has this tab open: right-click the tab to take over
# { $name } is the person who has the tab open for themselves.
shell-locks-open-for-editing = { $name } has this tab open for editing

## crates/cadrs_app/src/collab/versions.rs
shell-versions-not-owner = Not the owner
shell-versions-history-closed = The document's history isn't open
# { $owner } is the name of the person who shared the document.
shell-versions-no-edit-access = { $owner } hasn't given you edit access
# { $owner } is the name of the person who shared the document.
shell-versions-asked-owner = Asked { $owner }'s cadrs for the version
# { $owner } is the name of the person who shared the document.
shell-versions-owner-away = Versions of this document are made by { $owner }'s cadrs, which is away
shell-versions-owner-closed = The owner has closed the document

## crates/cadrs_app/src/collab/sharing.rs
# An error: the document's file can't be found among the user's documents.
shell-sharing-not-in-store = The document is not in the store
# { $owner } is the name of the person who shared the document.
shell-sharing-only-owner-changes = Only { $owner } can change who this is shared with
shell-sharing-own-id = That is your own cadrs ID
# "Preferences ▸ Collaboration" is where the app shows the user's own ID.
shell-sharing-not-an-id = That is not a cadrs ID (Preferences ▸ Collaboration shows yours)
# { $owner } is who shared the document, { $name } the document's name.
shell-sharing-shared-with-you = { $owner } shared “{ $name }” with you
# { $owner } is who shared the document, { $name } the document's name, { $message } what they wrote.
shell-sharing-shared-with-you-message = { $owner } shared “{ $name }” with you: “{ $message }”
shell-sharing-own-document = This is your own document
# { $owner } is the document's owner, { $name } the person they shared it with.
shell-sharing-owner-shared-it = { $owner } shared it with { $name }
# { $owner } is the name of the person who shared the document.
shell-sharing-only-owner-transfers = Only { $owner } can transfer this document
shell-sharing-share-first = Share the document with them first
shell-sharing-only-members-own = Only someone it is shared with can become its owner
# { $document } is the document's name, { $name } its new owner's.
shell-sharing-now-belongs-to = “{ $document }” now belongs to { $name }
# { $name } is the person the document was to go to, { $why } what went wrong.
shell-sharing-cannot-transfer = Cannot transfer it to { $name }: { $why }
# { $name } is the old owner, { $document } the document's name.
shell-sharing-made-you-owner = { $name } made you the owner of “{ $document }”

## crates/cadrs_app/src/collab/share_dialog.rs
# A right the share dialog gives someone: to comment on the document.
shell-share-right-comment = Comment
# A right the share dialog gives someone: to link the document into their own (as a linked part).
shell-share-right-link = Link document
# Permissions someone has on a shared document.
shell-share-can-edit = Can edit
shell-share-can-view = Can view
# The share dialog's title; { $name } is the document's name.
shell-share-title = Share “{ $name }”
# The share dialog's tabs: share with chosen people, or by a link.
shell-share-individuals = Individuals
shell-share-individuals-tooltip = Share with people by their cadrs ID
shell-share-link-sharing = Link sharing
shell-share-link-sharing-tooltip = Share with anyone who has the link
# The label of the field for someone's cadrs ID (their identity's key).
shell-share-cadrs-id = cadrs ID
shell-share-id-placeholder = Paste someone's cadrs ID
# The first, disabled entry of the list of people this user knows.
shell-share-contacts = Contacts
shell-share-permission = Permission
# The label of the field with a message sent with the share link.
shell-share-message = Message
shell-share-message-placeholder = Optional, sent with the link
# The heading of the list of people the document is shared with.
shell-share-shared-with = Shared with
# The document's owner, in the list of people it is shared with.
shell-share-owner = Owner
shell-share-anyone-with-link = Anyone with link
shell-share-link-off = Turn off link sharing: every link to it stops working
shell-share-copy-link = Copy to clipboard
shell-share-link-export = Allow exporting from the link
shell-share-link-hint = Anyone with the link can open this document: it works without adding them here, until link sharing is turned off.
shell-share-link-on = Turn on link sharing
shell-share-stop-sharing = Stop sharing with them
shell-share-link-copied = Link copied
shell-share-id-needed = Paste the cadrs ID of whoever to share with
# { $name } is the person the document was shared with.
shell-share-shared = Shared with { $name }: they get it when their cadrs is running
# { $name } is the person the owner is asked to share the document with.
shell-share-asked-owner = Asked the owner to share it with { $name }
shell-share-transfer-share-first = Share it with someone first: they can become its owner
# The transfer dialog's title; { $name } is the document's name.
shell-share-transfer-title = Transfer ownership of “{ $name }”
shell-share-new-owner = New owner
# "Can edit" is the permission's name (shell-share-can-edit).
shell-share-transfer-hint = Their cadrs must be running. You keep the document, with Can edit.
shell-share-transfer = Transfer ownership
shell-share-transferring = Transferring…
# The share dialog's third tab: making the document public.
shell-share-public = Public
shell-share-public-tooltip = Let anyone using cadrs find, view and fork it
shell-share-public-off = This document is private
# "Public documents" is the documents page's panel (shell-public-title).
shell-share-public-hint = Publishing lists it under Public documents for anyone using cadrs: they can view it and fork a copy of their own, but nobody else can edit it. It is published as it is now; changes you make later stay private until you publish again.
shell-share-publish = Publish
# { $revision } counts the times it was published (1, 2, …); { $date } is when ("2 hours ago").
shell-share-public-on = Public: revision { $revision }, published { $date }
shell-share-public-hint-on = Anyone using cadrs can view it and fork a copy; nobody else can edit it. Changes you made since are private until you publish again.
shell-share-publish-again = Publish again
shell-share-make-private = Make private
# Toasts. { $revision } is as in shell-share-public-on.
shell-share-published = Published (revision { $revision })
shell-share-made-private = It is private again

## crates/cadrs_app/src/collab/public.rs, crates/cadrs_app/src/landing_public.rs
# The documents page's panel of public documents.
shell-public-title = Public documents
# Its order: most starred first, or newest first.
shell-public-by-stars = Stars
shell-public-by-stars-tooltip = Most starred first
shell-public-recent = Recent
shell-public-recent-tooltip = Newest first
shell-public-search = Search public documents…
# Where cadrs stands with other users, under the search box.
shell-public-status-offline = Offline: connect to find public documents
shell-public-status-no-network = The network is off for this run
# Online, but the user doesn't take part in public documents; the button turns it on.
shell-public-status-off = Public documents are off
shell-public-turn-on = Turn on
shell-public-turn-on-tooltip = Find public documents while online (Preferences › Collaboration)
shell-public-status-joining = Looking for other cadrs users…
shell-public-status-alone = Online; nobody else found yet
# { $count } is how many other cadrs users (or seed peers) this one is connected to.
shell-public-status-peers = Online · { $count } connected
shell-public-empty-search = No matching public documents
shell-public-empty-search-text = Try another name, owner or word.
shell-public-empty-text = Documents people publish appear here when cadrs is online.
# Under a public document's name: { $owner } is who published it, { $date } when ("2 days ago").
shell-public-row-detail = { $owner } · { $date }
shell-public-open-tooltip = View it (read-only)
shell-public-star = Star it
shell-public-unstar = Take your star back
# "Fork" copies a public document into a document of the user's own (as GitHub's fork).
shell-public-fork = Fork
shell-public-fork-tooltip = Fork: make a copy of your own
# The banner over a public document opened read-only; { $revision } as in shell-share-public-on.
shell-public-view-banner = “{ $name }” by { $owner }, revision { $revision }: public, view only
# The top bar's label for a public document opened read-only.
shell-public-view-label = “{ $name }” by { $owner } (public)
shell-public-opening = Opening “{ $name }”…
shell-public-forking = Forking “{ $name }”…
# { $why } is the reason, as cadrs gives it.
shell-public-cannot-open = Cannot open “{ $name }”: { $why }
shell-public-gone = That document is not public any more
shell-public-only-owner = Only { $owner } can make this document public
shell-public-net-off = Public documents need the network, and the network is off (CADRS_NET=0)
shell-public-offline = Public documents need the network, and cadrs is offline

## crates/cadrs_app/src/identity.rs
# Tooltip of the user's name and avatar at the top right.
shell-identity-account = Account
shell-identity-offline = Offline: on this computer only
# { $id } is the start and end of the user's cadrs ID ("k51qzi…x8ab2c").
shell-identity-cadrs-id = cadrs ID { $id }
shell-identity-new-ellipsis = New identity…
shell-identity-edit-ellipsis = Edit profile…
shell-identity-copy-id = Copy cadrs ID
shell-identity-copy-id-offline = cadrs is offline: no cadrs ID to share
shell-identity-id-copied = Copied your cadrs ID
# The dialog's title for making a new identity (a name, and a cadrs ID of its own).
shell-identity-new = New identity
shell-identity-new-note = A new identity has a cadrs ID of its own: collaborators see it as someone else.
# The dialog's title for changing the user's name.
shell-identity-edit = Edit profile
shell-identity-edit-note = Collaborators see this name beside your cursor, comments and versions.
shell-identity-name-placeholder = Your name
shell-identity-saved = Profile saved
# { $name } is the identity's name.
shell-identity-restart = Restart cadrs to go online as { $name }
# { $name } is the identity's name.
shell-identity-working-as = Working as { $name }

## crates/cadrs_app/src/update.rs
shell-update-restart = Updated. Restart cadrs to use the new version.
# { $error } is what went wrong (in English).
shell-update-failed = Update failed: { $error }
# The top bar's link when a newer cadrs is out.
shell-update-available = Update available
shell-update-available-tooltip = Click to view release notes
# { $version } is the new version's number ("0.9.1").
shell-update-title = cadrs v{ $version } is available
# { $version } is the number of the version running ("0.9.0").
shell-update-you-have = You have v{ $version }.
shell-update-upgrade-note = Downloads and installs the new version.
shell-update-upgrade = Upgrade
shell-update-no-download = There is no download for this platform; build it from source.
# "git pull" and "cargo install-local" are commands: keep them as they are.
shell-update-from-source = Built from source: update with git pull and cargo install-local.
shell-update-release-page = Open release page
shell-update-installing = Installing…
shell-update-launching = Launching…
# The download's progress; { $done } and { $total } are megabytes downloaded and in all ("12.3").
shell-update-progress = { $done } of { $total } MB
# { $time } is how long the download still takes ("1 min 5 s").
shell-update-time-left = { $time } left

## crates/cadrs_app/src/workspaces.rs
# The title of the dialog that makes a branch (a new workspace) from a version.
shell-workspaces-branch-title = Branch to create workspace
shell-workspaces-from-version = From version:
shell-workspaces-name = Name:
shell-workspaces-description = Description:
# "Main" is the name of the document's main workspace.
shell-workspaces-branch-hint = The new workspace starts as a copy of the version. Main is not changed.
# { $source } is the name of the workspace to merge from.
shell-workspaces-nothing-changed = Nothing to merge: { $source } has no changed tabs.
# { $source } is the workspace merged from, { $dest } the one merged into.
shell-workspaces-merge-title = Merge { $source } into { $dest }
# The button that merges (a verb).
shell-workspaces-merge = Merge
# { $source } is the workspace merged from, { $dest } the one merged into.
shell-workspaces-merge-intro = { $source } changed these tabs. For each, replace it with the branch's version or keep { $dest }'s.
# The merge table's column headings.
shell-workspaces-column-tab = Tab
shell-workspaces-column-change = Change
shell-workspaces-column-action = Action
# How a tab changed in the branch.
shell-workspaces-modified = Modified
shell-workspaces-added = Added
shell-workspaces-deleted = Deleted
# { $source } is the workspace merged from: take its version of the tab.
shell-workspaces-replace-with = Replace with { $source }
# { $dest } is the workspace merged into: keep its version of the tab.
shell-workspaces-keep = Keep { $dest }
shell-workspaces-nothing-to-merge = Nothing to merge

## crates/cadrs_app/src/properties_dialog.rs
# Labels of a part's or assembly's properties.
shell-props-part-number = Part number
shell-props-revision = Revision
# The company that sells the part.
shell-props-vendor = Vendor
shell-props-assembly-title = Assembly properties
shell-props-part-title = Part properties
shell-props-generate-part-number = Generate next part number
shell-props-unit-of-measure = Unit of measure
shell-props-category = Category
shell-props-no-material = No material
# A part's color and look.
shell-props-appearance = Appearance
# { $unit } is the mass unit's symbol ("kg", "lb").
shell-props-mass = Mass ({ $unit })
shell-props-mass-override = Override the computed mass
# How a subassembly shows in the bill of materials (BOM).
shell-props-bom-behavior = Subassembly BOM behavior
# "Part number" and "Description" are the properties' names.
shell-props-standard-content = Standard content: Part number and Description are the configuration's.
shell-props-custom = Custom properties
shell-props-add-property = Add property
# Kinds of a custom property's value.
shell-props-kind-number = Number
shell-props-kind-boolean = Boolean

## crates/cadrs_app/src/search_tools.rs
# A sheet metal tool: a tag for a form feature.
shell-search-tag-form = Tag (Form)
shell-search-placeholder = Search tools…
shell-search-none = No tools found

## crates/cadrs_app/src/tab_preview.rs
# Kinds of tab (a document's elements), beside Part Studio, Assembly, Drawing and Render Studio.
shell-tab-pcb-studio = PCB Studio
shell-tab-cam-studio = CAM Studio
shell-tab-variable-studio = Variable Studio
# What a tab holds, on its preview card: { $count } is the number (1 in the singular keys).
shell-tab-preview-feature = { $count } feature
shell-tab-preview-features = { $count } features
shell-tab-preview-instance = { $count } instance
shell-tab-preview-instances = { $count } instances
shell-tab-preview-sheet = { $count } sheet
shell-tab-preview-sheets = { $count } sheets

## crates/cadrs_app/src/tab_folders.rs
# A folder of tabs with nothing in it, in its list.
shell-tab-folders-empty = Empty folder
# The submenu that makes a new tab in the folder (Part Studio, Assembly, …).
shell-tab-folders-new = New
shell-tab-folders-delete-ellipsis = Delete folder…
# Opens a folder's list of tabs.
shell-tab-folders-show-tabs = Show tabs
# The title of the popup that renames a folder of tabs.
shell-tab-folders-folder-name = Folder name
shell-tab-folders-delete-title = Delete folder
# { $name } is the folder's name.
shell-tab-folders-delete-question = Delete the folder “{ $name }”?
# How many tabs or folders: { $count } is the number (1 in the singular keys).
shell-tab-folders-tab-count = { $count } tab
shell-tab-folders-tabs-count = { $count } tabs
shell-tab-folders-folder-count = { $count } folder
shell-tab-folders-folders-count = { $count } folders
# What a folder holds: { $tabs } is how many tabs ("2 tabs").
shell-tab-folders-holds-tabs = It holds { $tabs }.
# What a folder holds: { $folders } is how many folders ("1 folder"), { $all } how many tabs in all, its folders' too ("5 tabs").
shell-tab-folders-holds-folders = It holds { $folders } ({ $all } in all).
# What a folder holds: { $tabs } is how many tabs ("2 tabs"), { $folders } how many folders ("1 folder"), { $all } how many tabs in all ("5 tabs").
shell-tab-folders-holds-both = It holds { $tabs } and { $folders } ({ $all } in all).
# After shell-tab-folders-holds-…, for one thing in the folder; "Delete folder only" and "Delete folder and tabs" are the dialog's buttons.
shell-tab-folders-delete-hint-one = Delete folder only moves it up a level; Delete folder and tabs deletes every tab in it too.
# After shell-tab-folders-holds-…, for several things in the folder; "Delete folder only" and "Delete folder and tabs" are the dialog's buttons.
shell-tab-folders-delete-hint-many = Delete folder only moves them up a level; Delete folder and tabs deletes every tab in it too.
shell-tab-folders-delete-only = Delete folder only
shell-tab-folders-delete-all = Delete folder and tabs
# Moves a tab out of its folders, to the tab bar itself.
shell-tab-folders-top-level = Top level
# The list of tabs out of sight, when there are none.
shell-tab-folders-all-in-view = Every tab is in view

## crates/cadrs_app/src/linked.rs
# Stands for a linked tab whose copy is missing, in shell-linked-badge.
shell-linked-a-linked-element = a linked element
# The tooltip of a linked item's badge: { $what } says which tab of which document, at which version.
shell-linked-badge-this = Linked document: { $what } (this document)
# The tooltip of a linked item's badge: { $what } says which tab of which document, at which version.
shell-linked-badge = Linked document: { $what }
# The last line of a linked item's tooltip when its document can't be reached.
shell-linked-badge-keeps-geometry = The instance keeps the geometry of its version.
# A linked item's tooltip: { $change } says what is newer ("V2 is available").
shell-linked-badge-update = { $change }. Click to update.
# A linked item pinned to its version; "Update all" is a button's name.
shell-linked-badge-pinned = Pinned: Update all skips it.
# A linked item pinned to its version; "Update all" is a button's name; { $change } says what is newer ("V2 is available").
shell-linked-badge-pinned-stale = Pinned: Update all skips it. { $change }.
shell-linked-badge-this-tab = It does not change when the tab is edited.
shell-linked-no-versions = No versions
shell-linked-no-history = The document has no history
shell-linked-document-no-versions = The document has no versions
shell-linked-version-missing = The version is missing
# Why the workspace node of another document's version graph can't be picked.
shell-linked-pick-a-version = Another document is referenced at a version: pick one below
# The first node of a version graph: where the document began.
shell-linked-graph-start = Start
# Places to pick a document from.
shell-loc-my-documents = My documents
shell-loc-recently-opened = Recently opened
shell-loc-created-by-me = Created by me
# A document label (a tag), when its name isn't known.
shell-loc-label = Label
shell-linked-loading = Loading…
shell-linked-no-documents = No documents
# { $version } is the name of the document's newest version ("V3").
shell-linked-newest-version = Newest version: { $version }
shell-linked-no-versions-yet = No versions yet: a linked document needs a version (create one after opening it)
shell-linked-one-version = 1 version
# { $count } is how many versions a document has (2 or more).
shell-linked-versions = { $count } versions
shell-linked-no-version-hint = This document has no versions. A linked document is referenced at a version, so its later edits never change what you insert. Create a version to use it.
# { $workspace } is the name of the workspace the version is made from ("Main").
shell-linked-create-version-title = Create version from { $workspace }
shell-linked-description-max = (Maximum of 10000 characters)
shell-linked-create-and-edit-properties = Create version and edit properties

## crates/cadrs_app/src/linked_session.rs
# A version of a document: { $version } is the version's name ("V1"), { $document } the document's.
shell-linked-session-version-of = { $version } of { $document }
shell-linked-session-references-workspace = It references the workspace: switch to its tab instead
shell-linked-session-version-unreadable = The version can't be read
# { $document } is a document's name, { $version } a version's ("V1").
shell-linked-session-no-such-tab = { $document } has no such tab at { $version }
# A banner over a linked document open read-only; { $what } is the version and document ("V1 of Block source").
shell-linked-session-viewing = Viewing { $what } (read-only). Edits are turned off.
# Opens the linked document's main workspace ("Main" is its name: keep it) to edit it.
shell-linked-session-edit-main = Edit Main
# { $document } is the linked document's name; "Main" is the workspace's name: keep it.
shell-linked-session-edit-main-tooltip = Open { $document }'s workspace (Main) to edit it
# { $document } is the document to go back to.
shell-linked-session-back = Back to { $document }
# { $document } is the linked document edited, { $back } the one to go back to; "Main" is the workspace's name: keep it.
shell-linked-session-editing-main = Editing Main of { $document }. Create a version, then update the reference in { $back }.
shell-linked-session-tools-off = This version is open read-only: its tools are turned off
# { $what } is the version and document open ("V1 of Block source"), { $back } the document to go back to.
shell-linked-session-cannot-edit = { $what } is open read-only: it can't be edited. Go back to { $back } to edit.
shell-linked-session-read-only = This version is open read-only: it can't be edited

## crates/cadrs_app/src/preferences_ui.rs
# A toast when the interface's size changes; { $percent } is the new size ("125").
shell-prefs-ui-scale-toast = UI scale { $percent }%
# What a mouse drag does in the 3D view.
shell-prefs-rotate = Rotate
shell-prefs-rotate-turntable = Rotate without roll
shell-prefs-pan = Pan
shell-prefs-zoom = Zoom
# The UI scale choice that is the display's own size.
shell-prefs-ui-scale-default = 100% (display default)
shell-prefs-vsync = Vertical sync (VSync)
shell-prefs-vsync-note = On, frames wait for the screen's refresh. Off, they are drawn as fast as they are made: the frame rate shows what a frame costs, but cadrs uses more power.
# The panel F12 opens, with each frame's timings.
shell-prefs-frame-panel = Frame panel (F12)
# Measures how long each of the app's systems takes, each frame.
shell-prefs-time-systems = Time systems
shell-prefs-time-systems-note = Shows where each frame's CPU time goes, system by system. It slows cadrs down, so it's off unless you need it. Takes effect at the next start.
# Moving and rotating parts in an assembly.
shell-prefs-assembly-heading = Assembly manipulation
shell-prefs-rotation-snap = Rotation snap increment
shell-prefs-snap-tolerance = Snap tolerance
shell-prefs-snap-hint = Snap when within the tolerance of an increment. Hold Shift to disable snapping. Increment: 0–180° (0 disables). Tolerance: 0–90°.
# MCP (Model Context Protocol) lets AI assistants use cadrs.
shell-prefs-mcp-heading = AI assistants (MCP)
shell-prefs-mcp-server = MCP server
shell-prefs-enabled = Enabled
# The address the MCP server answers at.
shell-prefs-endpoint = Endpoint
shell-prefs-mcp-note = While it is on, AI assistants on this computer (Claude, Cursor, …) can create and edit documents.
shell-prefs-collab-heading = Collaboration
shell-prefs-id-while-online = Shown while online
shell-prefs-p2p = P2P networking
shell-prefs-public = Public documents
shell-prefs-public-switch = Find public documents while online
# Seed peers: other cadrs users (their cadrs IDs) that stay online, to find public documents through.
shell-prefs-public-seeds = Seed peers
shell-prefs-public-seeds-placeholder = cadrs IDs, separated by commas
# "iroh" is the name of the networking library: keep it.
shell-prefs-collab-note = Documents are shared peer to peer, with no server, through iroh's relays. Off, cadrs stays offline until you connect with the network button in the top bar, or share. Give your cadrs ID to someone who wants to share a document with you.

## crates/cadrs_app/src/document.rs
# Tooltip of the open workspace's name in the top bar (a click lists the workspaces).
shell-doc-workspaces = Workspaces
shell-doc-feature-list-toggle = Show or hide the feature list
# Panels of an assembly.
shell-doc-exploded-views = Exploded views
shell-doc-named-positions = Named positions
# View tools at the right of the toolbar.
shell-doc-section-view = Section view
shell-doc-measure = Measure
shell-doc-analysis-tools = Analysis tools
shell-doc-mass-properties = Mass properties
shell-doc-simulation = Simulation
shell-doc-sheet-metal-table = Sheet metal table and flat view
# Tooltip of the + at the tab bar's left.
shell-doc-insert-tab = Insert new tab
# The tab bar's arrows: scroll to the tabs before or after, or list them all.
shell-doc-earlier-tabs = Earlier tabs
shell-doc-later-tabs = Later tabs
shell-doc-all-tabs = All tabs
# The + menu of the tab bar.
shell-doc-applications = Applications
shell-doc-no-applications = No applications installed
shell-doc-create-material-library = Create Material Library
shell-doc-create-feature-studio = Create Feature Studio
shell-doc-create-cam-studio = Create CAM Studio
shell-doc-create-pcb-studio = Create PCB Studio
shell-doc-create-render-studio = Create Render Studio
shell-doc-create-part-studio = Create Part Studio
shell-doc-create-assembly = Create Assembly
shell-doc-create-variable-studio = Create Variable Studio
shell-doc-create-drawing = Create Drawing…
shell-doc-create-folder = Create folder
shell-doc-import-ellipsis = Import…
# { $name } is the name of the tab copied to paste.
shell-doc-paste-tab = Paste { $name }
# Toolbar tools (their tooltips; keep a shortcut in parentheses, as "(Shift+E)", as it is: the tooltip shows it as keys).
shell-tool-sketch = Sketch (Shift+S)
shell-tool-extrude = Extrude (Shift+E)
shell-tool-revolve = Revolve (Shift+W)
shell-tool-sweep = Sweep
shell-tool-loft = Loft
shell-tool-thicken = Thicken
shell-tool-fillet = Fillet (Shift+F)
shell-tool-chamfer = Chamfer
shell-tool-draft = Draft
shell-tool-rib = Rib
# The shell feature: hollows a part out (not the app's shell).
shell-tool-shell = Shell
shell-tool-hole = Hole
shell-tool-thread = Thread
# The direct edit tools' button.
shell-tool-direct-edit = Move face, Delete face, Simplify
shell-tool-linear-pattern = Linear pattern
shell-tool-circular-pattern = Circular pattern
shell-tool-curve-pattern = Curve pattern
shell-tool-mirror = Mirror
shell-tool-boolean = Boolean (Union, Subtract, Intersect)
shell-tool-split = Split
shell-tool-transform = Transform
shell-tool-composite-part = Composite part
shell-tool-variable = Variable
shell-tool-sheet-metal-model = Sheet metal model
shell-tool-custom-features = Add custom features
# STEP, IGES, BREP and STL are file formats.
shell-tool-import = Import (a STEP, IGES, BREP or STL file)
shell-tool-derived = Derived (parts of another Part Studio)
# The pattern button's tooltip (it opens a menu of the three).
shell-tool-patterns = Linear, Circular and Curve pattern
# The Thicken button's tooltip (it opens a menu of the three).
shell-tool-surfacing = Thicken, Fill, Helix
shell-tool-fill = Fill
shell-tool-helix = Helix
shell-tool-move-face = Move face
shell-tool-delete-face = Delete face
shell-tool-simplify = Simplify
# The assembly toolbar's Insert button (a verb), and its tooltip ("(I)" is its key).
shell-tool-insert = Insert
shell-tool-insert-tooltip = Insert parts and assemblies (I)
# Mates: how two parts of an assembly are held together.
shell-tool-mate-fastened = Fastened mate (M)
shell-tool-mate-revolute = Revolute mate
shell-tool-mate-slider = Slider mate
shell-tool-mate-planar = Planar mate
shell-tool-mate-cylindrical = Cylindrical mate
shell-tool-mate-pin-slot = Pin slot mate
shell-tool-mate-ball = Ball mate
shell-tool-mate-parallel = Parallel mate
shell-tool-mate-tangent = Tangent mate
shell-tool-mate-width = Width mate
shell-tool-relations = Relations
# Groups parts of an assembly so they move together.
shell-tool-group = Group
shell-tool-replicate = Replicate
shell-tool-gear-relation = Gear relation
shell-tool-rack-pinion = Rack and pinion relation
shell-tool-screw-relation = Screw relation
shell-tool-linear-relation = Linear relation
# Moves an assembly's parts apart to show how it goes together (a verb).
shell-tool-explode = Explode
shell-tool-display-states = Display states
shell-tool-display-states-menu = Display states, Create Part Studio in context
shell-doc-feature-errors = A feature has errors: click to select the first one
# The top of a Part Studio's feature list: the planes and origin every Part Studio has.
shell-doc-default-geometry = Default geometry
# The point at the coordinate system's centre (0, 0, 0).
shell-doc-origin = Origin
shell-doc-filter-by-name = Filter by name
shell-doc-list-view = List view
# Tooltip of the + of an assembly's Items (bill of materials items with no part).
shell-doc-add-item = Add item
# An empty assembly's hint; "(I)" is the key of Insert.
shell-doc-insert-to-start = Insert parts to start (I)
# A tab's menu, while the document is shared live: give back a tab this user opened for themselves.
shell-doc-tab-unlock = Let others edit this tab
# A tab's menu, while the document is shared live: { $name } is who has the tab open for themselves.
shell-doc-tab-take-lock = Take over from { $name }
shell-doc-tab-lock = Open tab, close it for other users
shell-doc-open-new-window = Open in new window
# Shows a Part Studio's features as code.
shell-doc-show-code = Show code
# Makes a copy of the tab (a verb).
shell-doc-duplicate = Duplicate
# { $name } is the tab's name.
shell-doc-create-drawing-of = Create Drawing of { $name }…
shell-doc-thumbnail = Select as document thumbnail
shell-doc-move-to-document = Move to document…
shell-doc-move-to-folder = Move to folder
shell-doc-update-linked = Update linked document…
# { $source } is the linked document and version ("Block source (V1)").
shell-doc-open-linked-named = Open linked document ({ $source })
shell-doc-open-linked = Open linked document
shell-doc-change-to-version = Change to version…
shell-doc-export-ellipsis = Export…
shell-doc-export-image = Export image…
shell-doc-create-task = Create task…
# Releases the tab (a formal, approved revision of it).
shell-doc-release = Release…
# The title of the popup that renames a tab.
shell-doc-tab-name = Tab name
# { $name } is the tab's name.
shell-doc-tab-copied = Copied { $name } to the clipboard
shell-doc-tab-deleted = Tab deleted.
# The tab properties dialog's title; { $kind } is the tab's kind ("Part Studio", "Assembly").
shell-doc-tab-properties-title = { $kind } properties
# { $kind } is the tab's kind ("Part Studio", "Assembly").
shell-doc-tab-type = Type: { $kind }
# Why a sketch in the feature list is in error.
shell-doc-sketch-face-gone = The face this sketch is on no longer exists
shell-doc-sketch-errors = The sketch has conflicting constraints or broken references
# Tooltips of a row's eye; { $name } is the feature's, sketch's or part's name.
shell-doc-hide-named = Hide { $name }
shell-doc-show-named = Show { $name }
# A sketch's status beside its row; { $name } is the sketch's name.
shell-doc-sketch-under-defined = { $name } is not fully defined
shell-doc-sketch-fully-defined = { $name } is fully defined
shell-doc-sketch-hidden-under-defined = { $name } (Hidden) is not fully defined
shell-doc-sketch-hidden-fully-defined = { $name } (Hidden) is fully defined
# A feature suppressed by a variable; { $variable } is the variable's name ("#withHole").
shell-doc-suppressed-while-false = Suppressed while { $variable } is 0 (false)
shell-doc-suppressed-while-true = Suppressed while { $variable } is not 0 (true)
# After shell-doc-suppressed-while-…: the feature's state now.
shell-doc-currently-suppressed = currently suppressed
shell-doc-currently-active = currently active
# A PCB board's components row.
shell-doc-hide-components = Hide the components
shell-doc-show-components = Show the components
# A PCB board's view: its 3D model ("3D") or its layout.
shell-doc-board-3d-tooltip = Show the board's 3D model: its solid and its components
shell-doc-board-layout = Layout
shell-doc-board-layout-tooltip = Show the board's layout on its faces (its components hidden)
# A feature's right-click menu.
shell-doc-sketch-appearance = Edit sketch appearance…
shell-doc-feature-appearance = Add appearance to feature…
# { $name } is the feature's name: edit it as it was when it last rebuilt without errors.
shell-doc-edit-healthy = Edit healthy moment of { $name }…
# Keeps a linked reference at its version, so Update all skips it.
shell-doc-pin-reference = Pin reference
shell-doc-unpin-reference = Unpin reference
# { $name } is the source tab's name.
shell-doc-switch-to = Switch to { $name }
# A PCB board in a Part Studio.
shell-doc-board-edit-layout = Edit layout
shell-doc-board-unlink = Break outline link
shell-doc-board-link = Link outline to a sketch
shell-doc-board-show-3d = Show 3D model
shell-doc-board-show-layout = Show layout
shell-doc-create-board = Create PCB board…
shell-doc-copy-sketch = Copy sketch
shell-doc-hide-dimensions = Hide dimensions
shell-doc-show-dimensions = Show dimensions
# DXF and DWG are file formats.
shell-doc-export-dxf = Export as DXF/DWG…
shell-doc-flat-sketch = New sketch on flat pattern
shell-doc-flat-export = Export DXF/DWG of flat pattern…
shell-doc-add-to-folder = Add selection to folder…
shell-doc-show-all-sketches = Show all sketches
shell-doc-section-view-ellipsis = Section view…
# Suppression that follows a variable or a configuration.
shell-doc-dynamic-suppression = Dynamic suppression
shell-doc-suppress-by-variable = Suppress by variable…
shell-doc-remove-suppression-variable = Remove suppression variable
shell-doc-suppress-by-configuration = Suppress by configuration…
shell-doc-show-dependencies = Show dependencies…
# Moves the rollback bar (the end of the feature history in effect) to this feature, or to the end.
shell-doc-roll-here = Roll to here
shell-doc-roll-end = Roll to end
shell-doc-no-flat-pattern = The sheet metal model has no flat pattern part
# Tooltip of the toolbar's tool search; keep "(Alt+C)" as it is: the tooltip shows it as keys.
shell-doc-search-tools-tooltip = Search tools (Alt+C)
# A folder tab's tooltip: { $name } is the folder's name, { $count } how many tabs it holds.
shell-doc-folder-tab-tooltip = { $name } ({ $count } tabs)

## crates/cadrs_app/src/landing.rs
# The documents page's lists (filters).
shell-landing-explore = Explore cadrs
shell-landing-owned-by-me = Owned by me
shell-landing-shared-with-me = Shared with me
shell-landing-public = Public
# The documents' trash (a noun).
shell-landing-trash = Trash
shell-landing-back-to-all = Back to all documents
# The documents page's title for the user's own documents, and for the samples.
shell-landing-documents = Documents
shell-landing-samples = Samples
shell-landing-delete-ellipsis = Delete…
shell-landing-search = Search…
shell-landing-new-document = New document
shell-landing-create-document-tooltip = Create a new document
shell-landing-more-ways = More ways to start: join a shared document, open a sample
# The documents table's columns (and the menu of labels).
shell-landing-labels = Labels
shell-landing-modified = Modified
shell-landing-modified-by = Modified by
shell-landing-owned-by = Owned by
# An empty documents list: a title, then a line of text.
shell-landing-empty-search = No matching documents
# { $place } is the list searched ("Documents", a label's name), { $search } what was typed.
shell-landing-empty-search-text = Nothing in { $place } matches "{ $search }".
shell-landing-empty-folder = This folder is empty
# "Move to…" is the name of a menu item.
shell-landing-empty-folder-text = Drag documents or folders here, or right-click one and choose Move to….
shell-landing-empty-label = No documents with this label
# "Details" is the name of a panel; "Labels" a menu's.
shell-landing-empty-label-text = Give documents this label in the Details panel or with right-click ▸ Labels.
shell-landing-empty-trash = Trash is empty
shell-landing-empty-trash-text = Documents you move to the trash appear here.
shell-landing-empty-shared = Nothing shared with you
shell-landing-empty-shared-text = Documents other people share with you appear here.
shell-landing-empty-public = No public documents
# The Public list (the app menu's Public): the documents the user made public.
shell-landing-empty-public-mine-text = Documents you make public (Share › Public) appear here, for anyone using cadrs to find.
shell-landing-empty-recent = No recently opened documents
shell-landing-empty-recent-text = Documents you open appear here.
shell-landing-empty-none = No documents yet
shell-landing-empty-none-text = Create a document to start modeling.
# Who modified or owns a folder: the user.
shell-landing-me = me
shell-landing-more-actions = More actions
shell-landing-open-sample = Open a sample
shell-landing-new-document-ellipsis = New document…
shell-landing-new-folder-ellipsis = New folder…
shell-landing-import-files = Import files…
# A submenu of cloud storage services to import from.
shell-landing-import-from = Import from
shell-landing-new-label-ellipsis = New label…
shell-landing-from-sample = From a sample…
shell-landing-join-ellipsis = Join shared document…
shell-landing-open-documents-folder = Open documents folder
shell-landing-empty-trash-ellipsis = Empty trash…
# Stands for a document whose name isn't known, in shell-landing-cant-open.
shell-landing-the-document = the document
# { $name } is the document's name in quotes (or "the document"), { $reason } why (in English).
shell-landing-cant-open-newer = Can't open { $name }: { $reason }.
# { $name } is the document's name in quotes (or "the document"), { $error } what went wrong (in English).
shell-landing-cant-open = Can't open { $name }: { $error }
shell-landing-delete-forever-ellipsis = Delete permanently…
shell-landing-open-document-folder = Open document folder
shell-landing-copy-ellipsis = Copy…
shell-landing-move-to = Move to…
shell-landing-create-label = Create new label…
shell-landing-history-ellipsis = History…
# Shows the documents that use this one.
shell-landing-used-in = Used in…
shell-landing-transfer-ellipsis = Transfer ownership…
shell-landing-unshare = Unshare with me
shell-landing-move-to-trash = Move to trash
# Toasts with Undo; { $name } is a document's name.
shell-landing-unshared = Unshared “{ $name }” with you
shell-landing-restored = Restored “{ $name }”
shell-landing-deleted-permanently = Deleted “{ $name }” permanently
shell-landing-renamed-to = Renamed to “{ $name }”
shell-landing-copied-to = Copied to “{ $name }”
shell-landing-moved-to-trash = Moved “{ $name }” to trash
shell-landing-document-name = Document name
shell-landing-document-labels = Document labels
shell-landing-search-labels = Search labels
shell-landing-document-location = Document location
shell-landing-create-document = Create document
shell-landing-join = Join shared document
# The label of the field for a share link.
shell-landing-link = Link
shell-landing-link-placeholder = Paste the link someone shared with you
# Joins the shared document (a verb).
shell-landing-join-button = Join
shell-landing-no-folders = No additional folders
shell-landing-copy-document = Copy document
shell-landing-new-document-name = New document name
# { $name } is the document's name.
shell-landing-trash-question = Move “{ $name }” to the trash?
shell-landing-can-restore = You can restore it from Trash.
shell-landing-delete-forever = Delete permanently
# { $name } is the document's name.
shell-landing-purge-question = Permanently delete “{ $name }”?
shell-landing-cannot-restore = You cannot restore it from Trash.

## crates/cadrs_app/src/landing_details.rs
# Goes up from a folder to the one that holds it.
shell-details-back = Back
shell-details-empty-trash = Empty trash
shell-details-empty-trash-tooltip = Delete everything in the trash permanently
# STEP, IGES and STL are file formats.
shell-details-import-tooltip = Import a STEP, IGES or STL file as a new document
shell-details-new-label = New label
shell-details-create-label-tooltip = Create a label
shell-details-grid-view = Grid view
shell-details-samples-intro = Documents from the courses, bundled with cadrs. Select one to see its details; Open a copy makes your own editable document.
# The course a sample document comes from.
shell-details-course = Course
shell-details-open-copy-ellipsis = Open a copy…
# The details dialog's titles: a document's properties, and the documents that use it.
shell-details-properties = Properties
shell-details-used-in = Used in
shell-details-select = Select a document to see its details.
shell-details-folder-no-versions = Folders have no versions.
shell-details-folder-not-used = Folders are not referenced by documents.
shell-details-sample-no-versions = Samples have no versions. Open a copy to start its history.
shell-details-sample-not-used = Samples are not used by your documents.
shell-details-owner = Owner
shell-details-description = Document description
shell-details-no-description = No description
shell-details-add-description = Add a description
shell-details-trashed-note = In the Trash: restore it to edit its description and labels.
shell-details-created-by = Created by
# When the document was made (a date follows).
shell-details-created = Created
# { $time } is when the document was modified, { $name } who modified it.
shell-details-modified-by = { $time } by { $name }
# Where the document is (its folder).
shell-details-location = Location
shell-details-no-versions = No versions yet. Create one from the document's History panel.
shell-details-not-used = No other document uses this one.
# { $tab } is the tab that uses the document, { $element } the tab it uses, { $version } at which version.
shell-details-tab-uses = { $tab } uses { $element } ({ $version })
shell-details-one-use = 1 use
# { $count } is how many times (2 or more).
shell-details-uses = { $count } uses
# What a folder holds.
shell-details-contents = Contents
shell-details-open-folder = Open folder
shell-details-open-copy = Open a copy
shell-details-no-labels = No labels yet
shell-details-create-label = Create label
shell-details-label-name = Label name
shell-details-label-given = The label is given to the selected document.
shell-details-rename-label = Rename label
shell-details-delete-label = Delete label
# { $name } is the label's name.
shell-details-delete-label-question = Delete the label “{ $name }”?
# { $documents } is how many documents have the label ("3 documents").
shell-details-label-taken-off = It is taken off { $documents }. The documents stay.
# { $name } is the sample's name; "Owned by me" is the name of the documents list.
shell-details-sample-copy-text = “{ $name }” is a sample. Its copy is yours to edit, and is listed in Owned by me.
# { $name } is the label's name.
shell-details-label-exists = A label named “{ $name }” already exists.
shell-details-label-created = Created label “{ $name }”
shell-details-label-renamed = Renamed label to “{ $name }”
shell-details-label-deleted = Deleted label “{ $name }”
# { $name } is the file's name, { $error } what went wrong (in English).
shell-details-cannot-import = Cannot import “{ $name }”: { $error }
# { $name } is the file's name.
shell-details-imported = Imported “{ $name }” as a new document
shell-details-one-document = 1 document
# { $count } is how many documents (2 or more).
shell-details-documents = { $count } documents

## crates/cadrs_app/src/landing_organize.rs
# Move to ▸: the top level of the user's documents.
shell-organize-move-top = Documents (no folder)
# Stands for a document or folder whose name isn't known.
shell-organize-document = document
shell-organize-folder = folder
# { $count } is how many items (2 or more).
shell-organize-items = { $count } items
shell-organize-folder-into-itself = A folder can't go into a folder inside it
# Toasts with Undo: { $items } is a quoted name ("“Bracket”") or a count ("3 items"); { $place } a quoted folder or "Documents".
shell-organize-moved-to = Moved { $items } to { $place }
shell-organize-moved-to-trash = Moved { $items } to trash
shell-organize-deleted-permanently = Deleted { $items } permanently
shell-organize-restored = Restored { $items }
# { $count } is how many documents were copied (2 or more).
shell-organize-copied-documents = Copied { $count } documents
shell-organize-rename-folder = Rename folder
shell-organize-copy-folder = Copy folder
shell-organize-new-folder-name = New folder name
shell-organize-copy-folder-note = Its folders and documents are copied with it.
# { $name } is the folder's name.
shell-organize-purge-folder-question = Permanently delete “{ $name }” and everything in it?
# { $count } is how many items (2 or more).
shell-organize-purge-items-question = Permanently delete { $count } items?
shell-organize-cannot-restore-them = You cannot restore them from Trash.
shell-organize-empty-trash-one = Permanently delete the item in the trash?
# { $count } is how many items are in the trash (2 or more).
shell-organize-empty-trash-many = Permanently delete all { $count } items in the trash?
shell-organize-cannot-restore-them-short = You cannot restore them.
shell-organize-emptied-trash = Emptied the trash

## crates/cadrs_app/src/rebuild_indicator.rs
# The pill shown at the top of the 3D view while a Part Studio rebuilds.
view-rebuilding = Rebuilding…
# { $done } is how many parts are rebuilt so far, { $total } how many there are.
view-rebuilding-progress = Rebuilding… { $done } / { $total }

## crates/cadrs_app/src/camera.rs
# Render modes of the 3D view (the View menu): how faces and edges are drawn.
view-render-shaded = Shaded with edges
view-render-shaded-no-edges = Shaded without edges
view-render-shaded-hidden-edges = Shaded with hidden edges
view-render-hidden-removed = Hidden edges removed
view-render-hidden-visible = Hidden edges visible
view-render-translucent = Translucent

## crates/cadrs_app/src/derived_ui.rs
# The Derived feature's dialog: brings parts of another Part Studio (or a PCB Studio board) in.
# The Part Studio's origin, used as a location (where derived parts are placed).
view-derived-origin = Origin
# The source card's subtitle for a tab of this document. "Main" is the name of the document's main branch.
view-derived-this-document-main = This document · Main (follows the tab)
# { $version } is the name of a version of this document.
view-derived-this-document-version = This document · { $version }
# { $studio } is a PCB Studio's name; its board came from an IDF file and can't be edited.
view-derived-idf-read-only = { $studio } · IDF, read-only
# A source's subtitle: it follows the main branch's tab. "Main" is the name of the document's main branch.
view-derived-main-follows-tab = Main · follows the tab
# { $footprint } is a footprint's reference and value ("R1 10k"); it lies on the board's bottom side.
view-derived-footprint-bottom = { $footprint } (bottom)
# { $footprint } is a footprint's reference and value (maybe with "(bottom)"); it lies outside the board's outline.
view-derived-footprint-off-board = { $footprint } (off the board)
view-derived-help = Derived brings parts, sketches, planes and mate connectors of another Part Studio into this one, or a PCB Studio board with its components. A tab of this document follows its edits; a version (of this or another document) stays as it was until you update it.
# Tabs of the source picker.
view-derived-current-document = Current document
view-derived-other-documents = Other documents
view-derived-keep-source = Keep current source
view-derived-source-linked-tooltip = Referenced at a version: it changes only when you update it
view-derived-source-tab-tooltip = A tab of this document: its edits show here at once
# A button: choose another source.
view-derived-change = Change
view-derived-change-tooltip = Pick another Part Studio, board or version
# A section heading: what to bring in.
view-derived-derive = Derive
view-derived-all = Part Studio (every part, sketch and plane)
# A section heading: the mate connectors the derived parts are placed at.
view-derived-locations = Locations
view-derived-locations-placeholder = Mate connectors (the origin when empty)
# A section heading: how the source is placed.
view-derived-placement = Placement
view-derived-base-origin = Base origin
view-derived-base-connector = Base mate connector
view-derived-no-connectors = The source has no mate connectors
view-derived-include-connectors = Include mate connectors
view-derived-include-properties = Include properties
# A section heading for a board: show it as its 3D model or as its layout.
view-derived-show = Show
view-derived-3d-model = 3D model
# The board's layout (as drawn in the PCB Studio).
view-derived-layout = Layout
# A section heading: which of the board's components come in.
view-derived-components = Components
# Which components: all of them, those on the top side, those on the bottom side.
view-derived-components-all = All
view-derived-components-top = Top
view-derived-components-bottom = Bottom
view-derived-include-off-board = Include parts placed off the board
# { $sketch } is the name of the sketch the board was made from.
view-derived-on-sketch = On { $sketch }'s plane, as the board was made from it. Pick a mate connector to place it elsewhere.
view-derived-on-its-sketch = On its sketch's plane, as the board was made from it. Pick a mate connector to place it elsewhere.
# A section heading: which point of the board lands on the location.
view-derived-board-point = Board point on the location
# The board stays where its outline's centre was.
view-derived-anchor-fixed = Fixed
# The centre of the board's outline.
view-derived-anchor-centre = Centre
view-derived-anchor-footprint = Footprint
view-derived-anchor-fixed-hint = Where the outline's centre was: outline edits don't move the board
view-derived-anchor-origin-hint = The layout's origin (0, 0)
view-derived-anchor-centre-hint = The outline's centre: the board moves when the outline changes
view-derived-anchor-footprint-hint = A footprint's origin: the part lands on the location
# A section heading: which face of the board lies on the location.
view-derived-board-face = Board face on the location
view-derived-bottom-face = Bottom face
view-derived-top-face = Top face
view-derived-skins = Show the mask, copper and silkscreen
view-derived-no-sources = No other Part Studios or boards
view-derived-heading-boards = Boards
view-derived-heading-studios = Part Studios
# A tooltip; { $name } is a Part Studio's or board's name.
view-derived-derive-named = Derive { $name }
# A row of the feature list under a derived board: switches it between its 3D model and its layout.
view-derived-board-view = View
# A row of the feature list: the board's components; { $count } is how many.
view-derived-components-count = Parts ({ $count })

## crates/cadrs_app/src/feature_folders.rs
# A folder's context menu (feature list and parts list).
view-folder-toggle = Open / close folder
view-folder-unpack = Unpack folder
view-folder-delete-with-features = Delete folder and its features
# The label dragged with the pointer when several features are moved; { $count } is how many.
view-drag-features = { $count } features

## crates/cadrs_app/src/feature_list.rs
view-rollback-bar-tooltip = Rollback bar: drag to roll the Part Studio back
# Show dependencies legend: the colours of the selected feature's parents and children (features it uses, features that use it).
view-dependencies-parents = Parents
view-dependencies-children = Children
view-dependencies-stop-tooltip = Stop showing dependencies (Esc)
# A slider in a feature dialog's footer that previews the Part Studio before or after the feature.
view-preview-slider-tooltip = Drag left to see the Part Studio before this feature, right for after

## crates/cadrs_app/src/history_panel.rs
# The Versions and history panel.
view-history-title = Versions and history
view-history-create-version-tooltip = Create version…
# A button: pick a history entry to repair the Part Studio from.
view-history-repair = Repair
view-history-repair-banner = Select a history entry to repair this Part Studio.
view-history-other-workspaces = Other workspaces
view-history-workspace-tooltip = Click to switch; right-click to merge
view-history-footer-hint = Right-click a row for actions
# A version's subtitle when cadrs made the version itself; { $rest } is who and when ("Ann · 3 min ago").
view-history-auto-version = Auto version · { $rest }
# A group of changes in the history.
view-history-one-change = 1 change
# { $count } is how many changes the group has (2 or more).
view-history-changes = { $count } changes
# { $workspace } is the workspace the branch was made from, { $time } when.
view-history-branched = { $workspace } · branched { $time }
# The first row of the history: the document's start.
view-history-start = Start
# A workspace's subtitle; { $version } is the version it was branched from, { $rest } who changed it last and when.
view-history-from-version = From { $version } · { $rest }
# The placeholder of a new version's description.
view-history-description-optional = Description (optional)
view-history-view-in-repair = View in repair
view-history-open-read-only = Open read-only
view-history-branch = Branch to create workspace…
view-history-switch-workspace = Switch to workspace
view-history-merge = Merge into current workspace…

## crates/cadrs_app/src/part_folders.rs
# The label dragged with the pointer when several parts are moved; { $count } is how many.
view-drag-parts = { $count } parts

## crates/cadrs_app/src/parts_list.rs
# Tooltips of a part's eye in the Parts list; { $name } is the part's name.
view-part-show-named = Show { $name }
view-part-hide-named = Hide { $name }
# Stands in for a part's name in the menu's "Copy { $name }" when the part has none.
view-part-fallback-name = part
# A part's context menu.
view-part-assign-material = Assign material…
view-part-edit-appearance = Edit appearance…
view-part-add-to-folder = Add to folder
view-part-remove-from-folder = Remove from folder
view-part-copy-here = Copy here…
# { $name } is the part's name.
view-part-copy-named = Copy { $name }
# { $name } is the part's name.
view-part-create-drawing-named = Create Drawing of { $name }…
view-part-flat-drawing = Create drawing of flat pattern…
view-part-export = Export…
# A part's menu: opens the part in a 3D-printing slicer (a submenu of the slicers found).
view-part-send-to-slicer = Send to slicer
view-part-no-slicer-found = No slicer found
view-part-where-used = Where used…
view-part-create-task = Create task…
# Shows only this part (the others hidden).
view-part-isolate = Isolate…
view-part-make-opaque = Make opaque
view-part-make-transparent = Make transparent…
view-part-delete = Delete…
# The banner over the 3D view while parts are isolated.
view-isolated = Isolated
view-exit-isolate = Exit isolate

## crates/cadrs_app/src/reference_manager.rs
# The reference manager: updates references to versions of this or other documents.
# A reference's tooltip; { $document } is the document its tab moved to.
view-refman-tab-moved-to = Its tab moved to { $document }
# A reference's tooltip; { $version } is the name of the newer version.
view-refman-version-available = { $version } is available
view-refman-nested-newer = A reference inside it has a newer version
view-refman-tab-badge-update = This tab has references with newer versions: right-click the tab → Update linked document…
view-refman-tab-badge-transitive = A reference in this tab has a newer version further down its chain
view-refman-update-all-tooltip = Update all references to latest versions
# A row's change: from the workspace (the live, unversioned document) to version { $version }.
view-refman-workspace-to-version = Workspace ⇒ { $version }
# A row's change; { $document } is the document the tab moved to, { $version } its newest version.
view-refman-moved-to = moved to { $document } · { $version }
# A reference to the live document (not a version) — what it points at.
view-refman-workspace = Workspace
view-refman-title = Reference manager
# Tabs of the reference manager.
view-refman-update-to-latest = Update to latest
view-refman-selective-update = Selective update
view-refman-all-up-to-date = All references are up to date
view-refman-tab-moved = A tab moved to another document
# { $count } is how many documents (2 or more).
view-refman-tabs-moved = Tabs moved to { $count } documents
# References to the workspace get a version (not a newer one).
view-refman-versions-one = Versions for 1 document
# { $count } is how many documents (2 or more).
view-refman-versions-many = Versions for { $count } documents
view-refman-newer-one = Newer versions for 1 document
# { $count } is how many documents (2 or more).
view-refman-newer-many = Newer versions for { $count } documents
# { $document } is a document's name, { $change } the row's change ("moved to Bracket · V2").
view-refman-moved-tip-one = { $document }: { $change }. The 1 reference keeps the old version until you update it to the new document.
# { $document } is a document's name, { $change } the row's change, { $count } how many references (2 or more).
view-refman-moved-tip-many = { $document }: { $change }. The { $count } references keep the old version until you update them to the new document.
# { $document } is a document's name, { $change } the row's change ("V1 ⇒ V2").
view-refman-nested-tip = { $document }: { $change } (inside the document above, before its new version is made)
# { $document } is a document's name.
view-refman-auto-version-tip = { $document }: its own references are updated first, then a new version of it is made (an auto version) and used here
view-refman-update-to-new-document = Update to the new document
# { $documents } is a list of document names, separated by commas.
view-refman-auto-warning = Updating makes a new version of { $documents } to point at the newer references. Undo does not remove versions.
view-refman-no-references = No references
view-refman-pinned-tooltip = Pinned: Update all skips it
# { $version } is the version (or "Workspace") the reference is at.
view-refman-up-to-date = { $version } · up to date
# { $version } is the version the reference is at.
view-refman-open-read-only-tooltip = Open linked document ({ $version }, read-only)
view-refman-graph-tooltip = Version graph: pick the version to update to
view-refman-help = Update to latest: every listed reference goes to its source's newest version;\na source whose own references are out of date gets a new version first.\nSelective update: tick references and pick a version in the version graph.\nPinned references are skipped by Update all; update them here.
view-refman-update-all = Update all
view-refman-update-selected = Update selected
# A line of the update's details; { $version } is the new version's name, { $document } the document's.
view-refman-created-auto-version = Created { $version } of { $document } (auto version)
# A line of the update's details; { $change } is the change ("V1 ⇒ V2").
view-refman-change-auto-version = { $change } (auto version)
view-refman-nothing-to-update = Nothing to update: the references are at those versions already
view-refman-updated-one = Updated 1 reference
# { $count } is how many references (2 or more).
view-refman-updated-many = Updated { $count } references
# A link in the toast that opens the update's details.
view-refman-show-details = show more details
# { $document } is the document the tab moved to, { $version } its version.
view-refman-updated-one-to = Updated 1 reference to { $document } ({ $version })
# { $count } is how many references (2 or more), { $document } the document the tab moved to, { $version } its version.
view-refman-updated-many-to = Updated { $count } references to { $document } ({ $version })
view-refman-updated-title = Updated references
view-refman-details-undo = Undo (Ctrl+Z) points the references back; versions made in other documents stay.
# A drawing's reference: its context menu.
view-refman-unpin = Unpin reference
view-refman-pin = Pin reference
view-refman-update-linked = Update linked document…
view-refman-open-linked = Open linked document
view-refman-change-to-version = Change to version…

## crates/cadrs_app/src/shortcuts.rs
# The Keyboard shortcuts dialog (its title, and a shortcut's action that opens it).
view-shortcuts-title = Keyboard shortcuts
# A tab of the dialog: shortcuts that work everywhere.
view-shortcuts-tab-general = General
# A tab of the dialog: shortcuts of the 3D view.
view-shortcuts-tab-3d-view = 3D view
view-shortcuts-customize = Customize keyboard shortcuts
view-shortcuts-customize-tooltip = Customizing shortcuts is not available in cadrs yet
view-shortcuts-search = Search shortcuts
# The legend: a sample keycap's text.
view-shortcuts-legend-example = example
# The legend: what a normal keycap means (the shortcut as it comes).
view-shortcuts-legend-default = Default
# The legend: what a struck-through keycap means.
view-shortcuts-legend-off = Not available in cadrs yet
# The legend: what the lock means.
view-shortcuts-legend-locked = Cannot be changed
view-shortcuts-none-found = No shortcuts found
# What each shortcut does.
view-shortcut-accept-repeat = Accept & repeat command
view-shortcut-accept = Accept command
view-shortcut-ui-scale = UI scale up or down
view-shortcut-curve-analysis = Curve/surface analysis
view-shortcut-delete-selection = Delete selection
view-shortcut-dihedral-analysis = Dihedral analysis
view-shortcut-measure = Measure
view-shortcut-select-other = Select other
view-shortcut-search-tools = Search tools
view-shortcut-look-here = Look here: point it out to everyone in a shared document
view-shortcut-shortcut-toolbar = Shortcut toolbar
view-shortcut-tab-manager = Tab manager
view-shortcut-cycle-tabs = Cycle recent tabs
view-shortcut-delete-features = Delete selected features
view-shortcut-fillet = Fillet
view-shortcut-revolve = Revolve
view-shortcut-toggle-sketches = Show or hide all sketches
view-shortcut-toggle-mate-connectors = Show or hide mate connectors
view-shortcut-fastened-mate = Fastened mate
view-shortcut-insert-parts = Insert parts and assemblies
view-shortcut-hide-instance = Hide the instance under the pointer
view-shortcut-show-all-instances = Show all instances
view-shortcut-snap-mode = Snap mode
view-shortcut-rename-instance = Rename the selected instance
view-shortcut-toggle-mates = Show or hide mates
view-shortcut-show-mates-mode = Toggle show-mates mode
view-shortcut-front = Front view
view-shortcut-back = Back view
view-shortcut-left = Left view
view-shortcut-right = Right view
view-shortcut-top = Top view
view-shortcut-bottom = Bottom view
view-shortcut-isometric = Isometric view
# Ctrl is a key name.
view-shortcut-top-or-bottom = Top view (Ctrl: Bottom)
view-shortcut-front-or-back = Front view (Ctrl: Back)
view-shortcut-right-or-left = Right view (Ctrl: Left)
view-shortcut-opposite = Opposite view
# The numpad's 8, 2, 4 and 6 turn the view as the arrow keys do.
view-shortcut-rotate-numpad = Rotate, as the arrows
view-shortcut-perspective = Perspective or orthographic
view-shortcut-normal-to = Normal to
view-shortcut-walk = Walk (walk mode)
view-shortcut-walk-down-up = Walk down or up (walk mode)
view-shortcut-leave-walk = Leave walk mode
view-shortcut-zoom-window = Zoom to window
view-shortcut-zoom-out = Zoom out
view-shortcut-zoom-in = Zoom in
view-shortcut-rotate-15 = Rotate 15° (hold to keep turning)
view-shortcut-rotate-90 = Rotate 90° (any arrow)
view-shortcut-rotate-5 = Rotate 5° (any arrow)
view-shortcut-pan = Pan (any arrow)
view-shortcut-toggle-planes = Show or hide planes
view-shortcut-toggle-frame-rate = Show or hide the frame rate
view-shortcut-hide-construction = Hide construction geometry
view-shortcut-isolate = Isolate
view-shortcut-transparent = Transparent
view-shortcut-section-view = Section view
view-shortcut-named-views = Named views
view-shortcut-render-hq = Render in high quality
sketch-shortcut-line-tangent-arc = Switch between line and tangent arc
# Sketch: makes the selected curves construction geometry (and back).
sketch-construction = Construction
sketch-select-all = Select all
view-shortcut-zoom-sheet = Zoom to fit the sheet
view-shortcut-sheets-flyout = Sheets flyout
view-shortcut-end-tool = End the current tool
view-shortcut-radial-dimension = Radial dimension
view-shortcut-diameter-dimension = Diameter dimension
# A drawing's note (an annotation).
view-shortcut-note = Note
view-shortcut-update-workspace = Update from this workspace
view-shortcut-next-cell = Next / previous table cell
view-shortcut-no-inferencing = Hold to turn off inferencing

## crates/cadrs_app/src/sketch.rs
# Sketch tools: their names (toolbar menus, Search tools, the Keyboard shortcuts dialog).
sketch-tool-use = Use
sketch-tool-line = Line
sketch-tool-midpoint-line = Midpoint line
sketch-tool-corner-rectangle = Corner rectangle
sketch-tool-center-rectangle = Center point rectangle
sketch-tool-aligned-rectangle = Aligned rectangle
sketch-tool-center-circle = Center point circle
sketch-tool-3-point-circle = 3 point circle
sketch-tool-ellipse = Ellipse
sketch-tool-3-point-arc = 3 point arc
sketch-tool-tangent-arc = Tangent arc
sketch-tool-center-arc = Center point arc
sketch-tool-inscribed-polygon = Inscribed polygon
sketch-tool-circumscribed-polygon = Circumscribed polygon
sketch-tool-spline = Spline
sketch-tool-bezier = Bézier curve
sketch-tool-point = Point
sketch-tool-fillet = Sketch fillet
sketch-tool-chamfer = Sketch chamfer
sketch-tool-trim = Trim
sketch-tool-extend = Extend
# Splits a curve in two at a point.
sketch-tool-split = Split
sketch-tool-slot = Slot
sketch-tool-mirror = Mirror
sketch-tool-pattern = Pattern
# Sketch constraints: their names.
sketch-constraint-coincident = Coincident
sketch-constraint-concentric = Concentric
sketch-constraint-parallel = Parallel
sketch-constraint-tangent = Tangent
sketch-constraint-horizontal = Horizontal
sketch-constraint-vertical = Vertical
sketch-constraint-perpendicular = Perpendicular
sketch-constraint-equal = Equal
sketch-constraint-midpoint = Midpoint
# A line normal (perpendicular) to a curve.
sketch-constraint-normal = Normal
sketch-constraint-pierce = Pierce
sketch-constraint-symmetric = Symmetric
# Fixes geometry in place.
sketch-constraint-fix = Fix
sketch-constraint-curvature = Curvature
# A toast; { $name } is the sketch's name. A Restore button follows it.
sketch-cancelled = { $name } has been cancelled.
# The sketch plane field when the face the sketch was on is gone.
sketch-missing-face = Missing face
sketch-select-plane = Select a sketch plane
sketch-plane = Sketch plane
sketch-implicit-mate-connector = Implicit mate connector
# The sketch dialog's options.
sketch-disable-imprinting = Disable imprinting
sketch-show-constraints = Show constraints
sketch-show-expressions = Show expressions
sketch-show-errors = Show errors
sketch-diagnostics-tooltip = Show sketch diagnostic tools
# A button: shows the Part Studio's final result while editing an earlier sketch.
sketch-final = Final
sketch-final-tooltip = Show the final result
# Sketch toolbar tooltips; the letters in brackets are keys (keep them).
sketch-toolbar-use = Use (U)
sketch-toolbar-intersection = Intersection
sketch-toolbar-line = Line (L)
sketch-toolbar-rectangle = Corner rectangle (G)
sketch-toolbar-circle = Center point circle (C)
sketch-toolbar-arc = 3 point arc (A)
sketch-toolbar-point = Point (Shift+S)
sketch-toolbar-construction = Construction (Q)
sketch-toolbar-insert-image = Insert image
sketch-toolbar-fillet = Sketch fillet (Shift+F)
sketch-toolbar-trim = Trim (M)
sketch-toolbar-offset = Offset (O): pick curves, then click empty space or press Enter
sketch-toolbar-linear-pattern = Linear pattern
sketch-toolbar-insert-dxf = Insert DXF, DWG, SVG, EPS or PDF
sketch-toolbar-dimension = Dimension (D)
sketch-toolbar-constraints = Coincident (I)

## crates/cadrs_app/src/view_cube.rs
# The caption beside the view ball: the view a click on its knob turns to.
view-ball-front = Front
view-ball-back = Back
view-ball-left = Left
view-ball-right = Right
view-ball-top = Top
view-ball-bottom = Bottom
# The caption beside the view ball: the corner view a click turns to.
view-ball-top-front-right = Top front right
view-ball-top-front-left = Top front left
view-ball-top-back-right = Top back right
view-ball-top-back-left = Top back left
view-ball-bottom-front-right = Bottom front right
view-ball-bottom-front-left = Bottom front left
view-ball-bottom-back-right = Bottom back right
view-ball-bottom-back-left = Bottom back left
view-ball-drag-to-turn = Drag to turn
view-cube-roll-ccw = Roll counter-clockwise
view-cube-roll-cw = Roll clockwise
view-options = View options
# The View options menu: standard views.
view-menu-isometric = Isometric
view-menu-dimetric = Dimetric
view-menu-trimetric = Trimetric
view-menu-graphics-preferences = Graphics preferences…
view-menu-named-views = Named views…
view-menu-previous-view = Previous view
view-menu-perspective = Perspective view
view-menu-keep-up = Keep camera up
# W A S D, Q E and Esc are keys.
view-menu-walk-mode = Walk mode (W A S D, Q E, Esc)
view-menu-orient-normal = Orient normal to sketch on edit
view-menu-tangent-edges = Tangent edges visible
view-menu-high-quality = View in high quality
view-menu-boundary-edges = Highlight boundary edges
view-menu-exit-section = Exit section view
view-menu-section-view = Section view…

## crates/cadrs_app/src/view_options.rs
# The Named views panel (its title is view-shortcut-named-views).
view-named-views-empty = No named views in this tab yet
# A tooltip or menu item; { $name } is what is deleted (a named view's or a sketch's name).
view-delete-named = Delete { $name }
view-named-view-add = Add view
view-named-view-add-tooltip = Save the current view under this name

## crates/cadrs_app/src/viewport.rs
# The default planes' names, on the planes in the 3D view; seen from behind, a plane is labelled with the other side's name (Top reads Bottom).
view-plane-top = Top
view-plane-front = Front
view-plane-right = Right
view-plane-bottom = Bottom
view-plane-back = Back
view-plane-left = Left
# Stands in for the name of a sheet metal model feature that can't be found (in "Face of …").
view-sheet-metal-model = Sheet metal model

## crates/cadrs_app/src/viewport_menu.rs
# Stands in for a sketch's name in the menus ("Edit sketch…") when it has none.
sketch-fallback-name = sketch
# The 3D view's context menu.
view-ctx-new-sketch = New sketch
view-ctx-new-sketch-ellipsis = New sketch…
view-ctx-hide-top-plane = Hide Top plane
view-ctx-hide-front-plane = Hide Front plane
view-ctx-hide-right-plane = Hide Right plane
view-ctx-normal-to-plane = View normal to plane
view-ctx-normal-to-face = View normal to face
# Turns the view to look straight at the sketch's plane.
view-ctx-normal-to = View normal to
view-ctx-face-appearance = Add appearance to face…
view-ctx-export-dxf = Export as DXF/DWG…
view-ctx-flat-sketch = New sketch on flat pattern
view-ctx-flat-export = Export DXF/DWG of flat pattern…
# { $name } is the sketch's name.
view-ctx-edit-named = Edit { $name }…
view-ctx-hide-dimensions = Hide dimensions
view-ctx-show-dimensions = Show dimensions
# { $name } is the sketch's name.
view-ctx-show-dependencies = Show dependencies of { $name }…
view-ctx-offset-plane = Offset plane…
view-ctx-extrude = Extrude…
view-ctx-revolve = Revolve…
view-ctx-create-selection = Create selection…
view-ctx-sketch-appearance = Edit sketch appearance…
view-ctx-curve-appearance = Edit curve appearance…
view-ctx-hide-planes = Hide planes
view-ctx-show-planes = Show planes
# The sketch's context menu while it is edited; { $name } is the sketch's name.
sketch-menu-confirm-named = Confirm { $name }
sketch-menu-copy = Copy sketch
sketch-menu-show-all = Show all
sketch-menu-select-other = Select other…
sketch-menu-normal-to-plane = View normal to sketch plane

## crates/cadrs_app/src/repair.rs
# The Repair panel's tab for a version opened read-only (its tab otherwise reads view-history-repair).
view-repair-read-only = Read-only
# { $what } is what the panel shows: a version's name, or a workspace and a history entry ("Main :: Extrude 1").
view-repair-viewing = Viewing { $what }
view-repair-close = Close Repair
view-repair-open-new-tab = Open in a new tab (one window only)
view-repair-help = Repair shows the Part Studio at a history entry, to see where missing references were
view-repair-sync = Synchronize view
# { $name } is a feature's name: edit it at the last history entry where it was healthy.
view-repair-edit-healthy = Edit healthy moment of { $name }…

## crates/cadrs_app/src/sketch_constrain.rs
# A toast; { $constraint } is a constraint's name ("Parallel").
sketch-constraint-not-applicable = { $constraint } does not apply to the selection
sketch-normal-pick-line-first = Pick a line first, then the plane
sketch-normal-plane-parallel = Normal: the plane is parallel to the sketch
# A constraint glyph's context menu.
sketch-glyph-select-geometry = Select geometry
sketch-glyph-hide = Hide constraints
# Leaves the constraint tool (as Esc does).
sketch-glyph-escape = Escape constraint
sketch-unsolved = Sketch could not be solved.
sketch-conflict = Some constraints are not applicable to the current geometry and have not been solved.

## crates/cadrs_app/src/sketch_diagnostics.rs
# The sketch diagnostics menu and its two panels.
sketch-diag-profile-inspector-ellipsis = Profile inspector…
sketch-diag-constraint-manager-ellipsis = Constraint manager…
sketch-diag-profile-inspector = Profile inspector
sketch-diag-constraint-manager = Constraint manager
# Ends of sketch curves that join nothing.
sketch-diag-loose-ends = Loose ends
sketch-diag-no-loose-ends = No loose ends
sketch-diag-loose-end = Loose end
# { $count } is how many loose ends the row groups (2 or more).
sketch-diag-loose-ends-count = Loose ends ({ $count })
sketch-diag-previous = Previous
sketch-diag-next = Next
sketch-diag-profile-help = Profile inspector: lists the ends of regular geometry that join nothing
# Stands in for the name of the feature an external constraint comes from when it is gone (shown in brackets).
sketch-diag-external-source = external
sketch-diag-filters = Filters
sketch-diag-auto-select = Automatically select constraints
# A filter: where a constraint is (in this sketch, to other features, in an assembly's context).
sketch-diag-mode = Mode
sketch-diag-mode-internal = Internal
sketch-diag-mode-external = External
sketch-diag-mode-in-context = In context
# A filter: the constraint's state.
sketch-diag-status = Status
# A dimension that is measured, not driving the geometry.
sketch-diag-status-driven = Driven
sketch-diag-status-solved = Solved
sketch-diag-status-errors = Errors
sketch-diag-sort-by = Sort by
# Sort by tabs: by constraint, or by the entity (curve, point) it uses.
sketch-diag-sort-constraint = Constraint
sketch-diag-sort-entity = Entity
sketch-diag-list-caption = Entities and constraints
sketch-diag-shared-end = Shared end: drag the ends apart to separate them
sketch-diag-dismiss = Remove from the list
sketch-diag-delete-all = Delete all
sketch-diag-delete-all-tooltip = Delete every constraint listed
sketch-diag-manager-help = Constraint manager: lists the sketch's constraints and dimensions
# Dimension types (filter tooltips).
sketch-diag-type-radius = Radius
sketch-diag-type-diameter = Diameter
# A count dimension (how many sides, instances).
sketch-diag-type-count = Count

## crates/cadrs_app/src/sketch_dimension.rs
# A sketch dimension's context menu: a driving dimension sets the geometry, a driven one only measures it.
sketch-dim-make-driving = Change to driving dimension
sketch-dim-make-driven = Change to driven dimension
# Leaves the dimension tool (as Esc does).
sketch-dim-escape = Escape dimension
# Errors under a dimension's value field.
sketch-dim-gone = The dimension is gone
# # starts a variable's name in a value ("#width").
sketch-dim-type-variable-name = Type a variable's name after #
# { $variable } is a variable's name (after the #).
sketch-dim-variable-below = #{ $variable } is defined below this sketch; move it above the sketch to use it
# { $variable } is a variable's name (after the #).
sketch-dim-no-variable = There is no variable #{ $variable }

## crates/cadrs_app/src/sketch_dxf.rs
# The Insert DXF panel of a sketch (DXF, DWG, SVG, EPS and PDF are file formats).
sketch-dxf-title = Insert a DXF or DWG file
sketch-dxf-current-folder = Current folder
sketch-dxf-other-folders = Other folders
sketch-dxf-search = Search DXF, DWG or vector files
sketch-dxf-no-files = No DXF, DWG or vector files here. Use Import… to pick one.
sketch-dxf-use-origin = Use file origin position
sketch-dxf-import = Import…
sketch-dxf-picker-title = Insert a DXF, DWG or vector file
# { $file } is the file's name or path, { $error } why it failed.
sketch-dxf-cannot-insert-named = Cannot insert { $file }: { $error }
# { $file } is the file's path.
sketch-dxf-no-curves = { $file } has no lines, arcs, circles or splines
# Stands in for a file's name when it has none.
sketch-dxf-the-file = the file
# { $file } is the file's name, { $count } how many curves came in.
sketch-dxf-inserted = Inserted { $file }: { $count } curves
# { $error } is why it failed.
sketch-dxf-cannot-insert = Cannot insert: { $error }
# { $file } is the file's name.
sketch-dxf-nothing-to-insert = { $file } has no lines or curves to insert

## crates/cadrs_app/src/sketch_edit_tools.rs
# Offset tool warnings.
sketch-offset-branches = Offset selection branches. Select a chain without a junction.
sketch-offset-not-offset-dimension = not an offset dimension
sketch-offset-missing-dimension = missing offset dimension
sketch-offset-nothing = nothing to offset
sketch-offset-failed = Offset could not be created at this distance.
# The Mirror tool's prompts.
sketch-mirror-select-line = Select a mirror line.
sketch-mirror-select-entities = Select entities to be mirrored. Enter or Esc to finish.

## crates/cadrs_app/src/sketch_text.rs
# A sketch text's context menu.
sketch-text-edit = Edit text

## crates/cadrs_app/src/selection_readout.rs
# What is selected (a heading or a field's placeholder; a noun).
model-selection = Selection
# A point of a part (a corner where edges meet).
model-vertex = Vertex
# { $count } is how many more selected items the list doesn't show.
model-readout-more = +{ $count } more

## crates/cadrs_app/src/region_select.rs
# A measured area; { $area } is the value with its unit (e.g. "16682.524 mm²").
model-area-value = Area: { $area }

## crates/cadrs_app/src/suppress_variable.rs
# The dialog that suppresses a feature while a variable is 0 or not.
model-suppress-variable-title = Suppress by variable
# { $feature } is a feature's name.
model-suppress-variable-hint-true = { $feature } is suppressed while the variable is not 0 (true).
# { $feature } is a feature's name.
model-suppress-variable-hint-false = { $feature } is suppressed while the variable is 0 (false).
# { $feature } is a feature's name.
model-suppress-variable-none = No variables are defined above { $feature }.
# A named value used in a Part Studio (a noun: the row's label).
model-variable = Variable
model-suppress-variable-invert = Suppress when true (not 0)

## crates/cadrs_app/src/direct_edit_ui.rs
# Stands in for a part's name when the feature that made it can't be found (as in "Face of part").
model-unknown-part = part
model-direct-edit-faces-to-move = Faces to move
model-direct-edit-faces-to-delete = Faces to delete
model-direct-edit-faces-to-simplify = Faces of the parts to simplify

## crates/cadrs_app/src/feature_menu.rs
# A notice after Copy sketch; { $name } is the sketch's name.
model-sketch-copied = { $name } copied. Paste it into a sketch with Ctrl+V.

## crates/cadrs_app/src/replace_reference.rs
# The Repair panel's dialog that swaps a feature's missing reference for another.
model-replace-reference-title = Replace reference
model-replace-from = Selection to replace
model-replace-with = Selection to replace with
model-replace-propagate = Propagate changes
# A fillet's reference stood for a chain of tangent edges; { $new } and { $old } are edge counts.
model-replace-chain-note = The replacement stands for { $new } of the { $old } edges the missing reference's tangent chain had.
# { $new } and { $old } are edge counts.
model-replace-chain-note-edge = The replacement stands for { $new } of the { $old } edges the missing reference's tangent chain had. Its edges aren't tangent-connected: pick the face to take all of them.

## crates/cadrs_app/src/applied_dialog.rs
# A fillet measured by its radius; also a field's label (the radius).
model-radius = Radius
# A fillet measured by its width (the chord between its two contact lines); also a field's label.
model-width = Width
# A fillet's cross section: a conic curve (with a Rho value).
model-fillet-conic = Conic
# A fillet's cross section: curvature continuous (with a Magnitude).
model-fillet-curvature = Curvature
# A chamfer measured along the faces (tangent), as opposed to Offset.
model-chamfer-tangent = Tangent
model-chamfer-equal-distance = Equal distance
model-chamfer-two-distances = Two distances
model-chamfer-distance-and-angle = Distance and angle
# A hole's style: a plain hole (no counterbore or countersink).
model-hole-simple = Simple
model-hole-counterbore = Counterbore
model-hole-countersink = Countersink
# A hole's type: a plain drilled hole.
model-hole-drilled = Drilled
# A hole's type: a clearance hole (a fastener passes through it).
model-hole-clearance = Clearance
# A hole's type: a threaded hole.
model-hole-tapped = Tapped
model-hole-straight-tap = Straight tap
model-hole-tapered-tap = Tapered tap
# A PEM self-clinching fastener; CLS is the maker's series name.
model-hole-pem-nut = Self-clinching nut (CLS)
# A PEM self-clinching fastener; FH is the maker's series name.
model-hole-pem-stud = Self-clinching stud (FH)
# A clearance hole's fit around its fastener.
model-hole-fit-close = Close
# A clearance hole's fit around its fastener.
model-hole-fit-normal = Normal
# A clearance hole's fit around its fastener.
model-hole-fit-loose = Loose
model-hole-start-part = Start from part
model-hole-start-sketch-plane = Start from sketch plane
model-hole-start-selected-plane = Start from selected plane
# A hole's termination: through the whole part.
model-hole-end-through = Through
# An end condition: a given depth or distance.
model-blind = Blind
# An end condition: up to the next face met.
model-up-to-next = Up to next
# An end condition: up to a picked face, plane or vertex (also that field's placeholder).
model-up-to-entity = Up to entity
# A feature dialog's button: shows the Part Studio with the features after this one too.
model-dialog-final = Final
model-dialog-final-tooltip = Show the final result
model-fillet-full-round = Full round
model-fillet-side-face-1 = Side face 1
model-fillet-center-face = Center face
model-fillet-side-face-2 = Side face 2
model-fillet-entities = Entities to fillet
# A checkbox: the selection extends along edges that meet tangentially.
model-tangent-propagation = Tangent propagation
# How a fillet or chamfer is measured (a field's label).
model-measurement = Measurement
# A fillet's cross-section control (a field's label).
model-fillet-control = Control
# A conic fillet's shape parameter (the Greek letter's name).
model-fillet-rho = Rho
# How strongly a curve or surface keeps its direction (a number field's label).
model-magnitude = Magnitude
model-chamfer-entities = Entities to chamfer
model-chamfer-type = Chamfer type
model-distance-1 = Distance 1
model-distance-2 = Distance 2
model-chamfer-direction-overrides = Direction overrides
# A checkbox: the Shell hollows whole parts instead of removing faces.
model-shell-hollow = Hollow
model-shell-parts-to-hollow = Parts to hollow
model-shell-faces-to-remove = Faces to remove
model-shell-thickness = Shell thickness
# A hole's units tab: inch sizes (ANSI).
model-hole-inch = Inch
# A hole's units tab: metric sizes (ISO).
model-hole-metric = Metric
model-hole-points = Sketch points to place holes
model-select-mate-connectors = Select mate connectors
# The standard a hole's or thread's sizes come from (ANSI, ISO…).
model-standard = Standard
model-hole-type = Hole type
model-hole-tap-type = Tap type
# A PEM hole's fastener (a field's label).
model-hole-fastener = Fastener
model-hole-fastener-fit = Fastener fit
# A thread's pitch (the distance between its turns).
model-pitch = Pitch
# Ø is the diameter sign.
model-hole-tap-drill-diameter = Tap drill Ø
# Ø is the diameter sign.
model-hole-diameter = Hole Ø
model-hole-diameter-tolerance = Diameter tolerance
# Where a hole starts (a field's label).
model-hole-start = Start plane
model-hole-start-plane = Hole start plane
# How a hole ends (a field's label).
model-hole-termination = Termination
model-hole-depth = Hole depth
model-hole-depth-tolerance = Depth tolerance
model-hole-tip-angle = Tip angle
# Ø is the diameter sign.
model-hole-counterbore-diameter = Counterbore Ø
model-hole-counterbore-depth = Counterbore depth
# Ø is the diameter sign.
model-hole-countersink-diameter = Countersink Ø
model-hole-countersink-angle = Countersink angle
model-hole-tapped-depth = Tapped depth
# How far a tapped hole's drill goes past its thread.
model-hole-tap-clearance = Tap clearance

## crates/cadrs_app/src/draft_ui.rs
# The Smooth fillet corners option's tooltip.
model-fillet-smooth-corners-why = Where three fillets meet, set the corner back 1.5 × the radius and blend it with one smooth patch
# { $name } is the name of the feature that made the edge.
model-point-on-edge-of = Point on Edge of { $name }
# A Draft's type, and its field: the plane whose normal is the pull direction.
model-draft-neutral-plane = Neutral plane
# A Draft's type: drafting from a parting line (not built yet).
model-draft-parting-line = Parting line
model-draft-faces = Faces to draft
model-draft-angle = Draft angle
model-draft-reference-propagation = Reference entity propagation
model-draft-reference-propagation-why = Not available: faces are drafted as picked
# A checkbox: a fillet with a different radius on each side.
model-fillet-asymmetric = Asymmetric
model-fillet-second-radius = Second radius
model-fillet-partial = Partial fillet
# A checkbox: a fillet whose radius varies along its edges.
model-fillet-variable = Variable fillet
model-fillet-allow-overflow = Allow edge overflow
model-fillet-smooth-corners = Smooth fillet corners
# The tooltip of a variable fillet's disabled Magnitude rows.
model-fillet-magnitude-why = Magnitude applies to the Curvature cross section with a variable fillet; not available
# How a partial fillet's bounds are given (a field's label).
model-fillet-boundary-type = Boundary type
# A partial fillet's bound given as a parameter (0 to 1 along the edge).
model-fillet-bound-parameter = Parameter
# A partial fillet's bound given as a length along the edge.
model-fillet-bound-length = Length
model-fillet-first-bound = First bound
model-fillet-second-bound = Second bound
# A variable fillet's group of vertices (each with its radius).
model-fillet-vertices = Vertices
model-fillet-points-on-edges = Points on edges
# The Points on edges group's action that removes every point (written in capitals, as a small link).
model-fillet-clear-points = CLEAR
# Where a point lies along its edge (0 to 1).
model-fillet-location = Location
model-fillet-add-point = Add point on edge
model-fillet-smooth-transition = Smooth transition
# A checkbox: a tapped hole's thread class.
model-hole-thread-class = Thread class
# A thread's class (its tolerance grade).
model-hole-class = Class
# A symmetrical tolerance's value (a field's label).
model-tolerance = Tolerance
# A tolerance's upper deviation.
model-tolerance-upper = Upper
# A tolerance's lower deviation.
model-tolerance-lower = Lower
# A tolerance's number of decimals.
model-tolerance-precision = Precision
model-tolerance-symmetrical = Symmetrical
model-tolerance-deviation = Deviation
model-tolerance-limits = Limits
# A tolerance type: a minimum value.
model-tolerance-min = Min
# A tolerance type: a maximum value.
model-tolerance-max = Max
# A tolerance type: a basic (theoretically exact) dimension.
model-tolerance-basic = Basic
# Ø is the diameter sign.
model-hole-counterbore-diameter-tolerance = Counterbore Ø tolerance
model-hole-counterbore-depth-tolerance = Counterbore depth tolerance
# Ø is the diameter sign.
model-hole-countersink-diameter-tolerance = Countersink Ø tolerance
model-hole-countersink-angle-tolerance = Countersink angle tolerance

## crates/cadrs_app/src/extrude_dialog.rs
# Stands in for a sketch's name when the sketch can't be found.
model-unknown-sketch = sketch
# { $name } is a sketch's name (several regions of it).
model-faces-of = Faces of { $name }
# { $name } is a sketch's name (the boundary curves of its regions).
model-curves-of = Curves of { $name }
# An input that no longer resolves; { $item } is how it read ("Face of Sketch 3").
model-missing-item = Missing { $item }
# An end condition: up to a picked face (also its field's placeholder).
model-up-to-face = Up to face
# An end condition: up to a picked part (also its field's placeholder).
model-up-to-part = Up to part
# An end condition: up to a picked vertex (also its field's placeholder).
model-up-to-vertex = Up to vertex
# An end condition: through every part.
model-through-all = Through all
# A boolean operation tab: the feature makes new parts.
model-op-new = New
# A boolean operation tab: keeps only where the new body and the parts overlap.
model-op-intersect = Intersect
model-measure-a-distance = Measure a distance
model-offset-distance = Offset distance
# A body type tab: a body with no volume (a sheet).
model-body-surface = Surface
# A body type tab: a thin-walled body.
model-body-thin = Thin
model-extrude-curves = Sketch curves to extrude
model-extrude-regions = Faces and sketch regions to extrude
model-thickness-1 = Thickness 1
model-thickness-2 = Thickness 2
# A thin body's flip button: puts the wall on the other side.
model-flip-wall = Flip wall
# A thin body's option: the wall centred on the profile.
model-thin-mid-plane = Mid plane
model-starting-offset = Starting offset
# An extrude's option: tapers its sides by an angle.
model-extrude-draft = Draft
model-second-end-position = Second end position

## crates/cadrs_app/src/advanced_dialog.rs
# { $edge } is an edge's label ("Edge of Extrude 1").
model-plane-midpoint-of = Midpoint of { $edge }
# { $percent } is a number (how far along the edge, in percent); { $edge } is an edge's label ("Edge of Extrude 1").
model-plane-point-at-of = Point at { $percent }% of { $edge }
# The origin point of a Part Studio.
model-origin = Origin
# The things a feature is built on (a field's placeholder).
model-entities = Entities
model-plane-flip-alignment = Flip alignment
# A checkbox: turns the plane's normal the other way.
model-plane-flip-normal = Flip normal
model-sweep-curves = Edges and sketch curves to sweep
model-sweep-regions = Faces and sketch regions to sweep
model-sweep-path = Sweep path
# The profiles a Loft goes through (a field's placeholder).
model-loft-profiles = Profiles
# The heading of a Loft's start and end conditions.
model-loft-end-conditions = End conditions
model-loft-start-condition = Start profile condition
model-loft-start-direction = Start direction
model-loft-start-magnitude = Start magnitude
model-loft-end-condition = End profile condition
model-loft-end-direction = End direction
model-loft-end-magnitude = End magnitude
model-split-parts = Parts or surfaces to split
model-split-faces = Faces to split
model-split-tool = Entity to split with
# A checkbox: the tool bodies stay after the feature.
model-keep-tools = Keep tools
model-split-trim = Trim to face boundaries
model-split-keep-both = Keep both sides
model-split-keep-tool-surfaces = Keep tool surfaces and curves
# A plane type: through a point, parallel to a plane.
model-plane-type-plane-point = Plane point
# A plane type: through a line, at an angle.
model-plane-type-line-angle = Line angle
# A plane type: through a point, normal to a line.
model-plane-type-point-normal = Point normal
model-plane-type-three-point = Three point
# A plane type: midway between two planes or points.
model-plane-type-mid-plane = Mid plane
# A plane type: normal to a curve at a point.
model-plane-type-curve-point = Curve point
# A plane type: fitted through points.
model-plane-type-fit = Fit
# A plane type: tangent to a cylindrical or conical face.
model-plane-type-tangent = Tangent
model-sweep-keep-orientation = Keep profile orientation
model-sweep-lock-direction = Lock profile direction
model-sweep-lock-faces = Lock profile faces
model-loft-normal-to-profile = Normal to profile
model-loft-tangent-to-profile = Tangent to profile
model-loft-match-tangent = Match tangent
model-loft-match-curvature = Match curvature
model-loft-normal-direction = Normal direction
model-loft-tangent-direction = Tangent direction

## crates/cadrs_app/src/pattern_dialog.rs
model-pattern-parts = Parts to pattern
model-pattern-features = Features to pattern
model-pattern-faces = Faces to pattern
model-mirror-parts = Parts to mirror
model-mirror-features = Features to mirror
model-mirror-faces = Faces to mirror
# A pattern's type: it copies parts.
model-pattern-type-part = Part pattern
# A pattern's type: it copies features.
model-pattern-type-feature = Feature pattern
# A pattern's type: it copies faces.
model-pattern-type-face = Face pattern
model-mirror-type-part = Part mirror
model-mirror-type-feature = Feature mirror
model-mirror-type-face = Face mirror
# A mate connector's origin type: on one entity.
model-connector-on-entity = On entity
# A mate connector's origin type: midway between two entities.
model-connector-between-entities = Between entities
model-connector-origin-entity = Origin entity
model-connector-between-entity = Between entity
# The entity whose direction a mate connector's primary axis takes.
model-connector-alignment = Alignment
# A checkbox: the mate connector's axes are set by picked entities.
model-connector-realign = Realign
model-connector-primary-axis = Primary axis
model-connector-secondary-axis = Secondary axis
# A checkbox: the mate connector is moved by an offset (a verb).
model-connector-move = Move
model-connector-x-translation = X translation
model-connector-y-translation = Y translation
model-connector-z-translation = Z translation
model-connector-owner-entity = Owner entity
model-connector-select-owner-entity = Select owner entity
model-connector-flip-primary = Flip primary axis
model-connector-reorient-secondary = Reorient secondary axis (90°)
model-pattern-instance-count = Instance count
model-pattern-skip-instances = Skip instances
model-pattern-instances-to-skip = Instances to skip
# The Instances to skip row's action that clears the list (written in capitals, as a small link).
model-pattern-clear-skipped = CLEAR
model-pattern-click-instances = Click instances in the view
# A button's tooltip: makes a selection of faces (a noun: "Create selection" feature).
model-create-selection = Create selection
model-select-mate-connector = Select mate connector
# A checkbox: the instances are centred on the seed.
model-pattern-centered = Centered
model-pattern-second-direction = Second direction
model-pattern-axis = Axis of pattern
model-pattern-equal-spacing = Equal spacing
model-pattern-path = Path to pattern along
model-pattern-tangent-to-curve = Tangent to curve
model-reapply-features = Reapply features
model-mirror-plane = Mirror plane

## crates/cadrs_app/src/transform_ui.rs
model-transform-parts = Parts to transform
model-transform-line = Line or axis
model-transform-from = From mate connector
model-transform-to = To mate connector
# { $axis } is the axis's code name ("plus y", "minus x"…).
model-transform-reorient-secondary = Reorient secondary axis ({ $axis })
model-transform-axis = Axis of rotation
model-transform-scale-point = Point to scale about (origin)
# A scale factor (a number field's label).
model-transform-scale = Scale
# A checkbox: the Transform moves a copy of the part.
model-transform-copy-part = Copy part
model-transform-by-line = Translate by line
model-transform-by-distance = Translate by distance
model-transform-by-xyz = Translate by XYZ
model-transform-by-mate-connectors = Transform by mate connectors
# A transform type (a verb).
model-transform-rotate = Rotate
model-transform-copy-in-place = Copy in place
model-transform-scale-uniformly = Scale uniformly

## crates/cadrs_app/src/surfacing_ui.rs
model-thicken-entities = Faces and surfaces to thicken
# A helix's type: around a cylindrical or conical face.
model-helix-cylinder-cone = Cylinder/Cone
# A helix's type, and its field: an axis.
model-axis = Axis
# A helix's type, and its field: a circle.
model-circle = Circle
model-helix-face = Cylindrical or conical face
# How a helix's length is given (a field's label).
model-helix-input-type = Input type
# A helix's length given in turns.
model-helix-turns = Turns
model-helix-turns-and-pitch = Turns and pitch
# How many turns a helix makes.
model-helix-revolutions = Revolutions
model-height = Height
model-start-angle = Start angle
model-clockwise = Clockwise
model-counterclockwise = Counterclockwise
# The Fill's boundary edges and curves (a field's placeholder).
model-fill-edges = Edges and curves
# How smoothly a surface meets its neighbours (a field's label).
model-fill-continuity = Continuity
model-fill-merge-scope = Merge scope (surfaces it meets)
# A continuity: the surfaces only touch (G0).
model-continuity-position = Position (G0)
# A continuity: the surfaces meet tangentially (G1).
model-continuity-tangency = Tangency (G1)
# A continuity: the surfaces meet with the same curvature (G2).
model-continuity-curvature = Curvature (G2)

## crates/cadrs_app/src/thread_ui.rs
# A thread's handedness.
model-thread-right-hand = Right-hand
# A thread's handedness.
model-thread-left-hand = Left-hand
model-thread-edge = Cylinder or hole edge
model-thread-major-diameter = Major diameter
# A thread's end: it runs to the end of its cylinder.
model-thread-up-to-end = Up to end
model-length = Length
# The Size list's last entry: a diameter and pitch typed in.
model-thread-custom = Custom

## crates/cadrs_app/src/flat_ui.rs
model-flat-no-pattern = The sheet metal model has no flat pattern to sketch on
model-flat-sketch-note = The flat view stays open while a sketch on the flat pattern is edited

## crates/cadrs_app/src/composite_ui.rs
model-composite-parts = Parts and composite parts
# A checkbox: a closed composite part (its members can't be picked on their own).
model-composite-closed = Closed

## crates/cadrs_app/src/revolve_dialog.rs
# A revolve type: a full turn (360°).
model-revolve-full = Full
model-revolve-angle = Revolve angle
model-revolve-offset-angle = Offset angle
model-revolve-curves = Sketch curves to revolve
model-revolve-regions = Faces and sketch regions to revolve
# The axis a Revolve turns about (a field's placeholder).
model-revolve-axis = Revolve axis
model-select-a-mate-connector = Select a mate connector

## crates/cadrs_app/src/boolean.rs
# The parts a Boolean uses (a field's placeholder).
model-boolean-tools = Tools
# The parts a Subtract cuts (a field's placeholder).
model-boolean-targets = Targets
model-boolean-offset-all = Offset all
model-boolean-faces-to-offset = Faces to offset
# A Boolean's operation: joins parts into one.
model-boolean-union = Union
# A Boolean's operation: cuts the tools from the targets.
model-boolean-subtract = Subtract

## crates/cadrs_app/src/create_selection.rs
# Faces of parts (a tab, or a field's placeholder).
model-faces = Faces
# Edges of parts (a tab).
model-edges = Edges
# A selection rule: the faces of a pocket.
model-create-selection-pocket = Pocket
# A selection rule: the faces of a boss (a raised feature).
model-create-selection-boss = Boss
model-create-selection-tangent-connected = Tangent connected
# A selection rule: the faces a feature made.
model-create-selection-feature = Feature
# A selection rule: a loop of edges.
model-create-selection-loop = Loop
# A selection rule: the boundary edges.
model-create-selection-boundary = Boundary
model-create-selection-patterns = Select patterns
model-create-selection-add = Add selection
# { $count } is 1.
model-create-selection-one-face = { $count } face selected
# { $count } is how many faces are selected (not 1).
model-create-selection-n-faces = { $count } faces selected
# { $count } is 1.
model-create-selection-one-edge = { $count } edge selected
# { $count } is how many edges are selected (not 1).
model-create-selection-n-edges = { $count } edges selected

## crates/cadrs_app/src/appearance.rs
# The appearance dialog's title for parts (a verb phrase: change their colour).
model-appearance-edit = Edit appearance
model-appearance-face = Face appearance
# { $count } is how many faces (more than 1); { $name } is a part's name.
model-appearance-n-faces-of = { $count } faces of { $name }
model-appearance-feature = Feature appearance
model-appearance-sketch = Sketch appearance
model-appearance-curve = Curve appearance
# A custom colour swatch's menu item: sets it to the colour now chosen.
model-appearance-update-color = Update color

## crates/cadrs_app/src/analysis.rs
model-analysis-exit-draft = Exit draft analysis
model-analysis-draft-ellipsis = Draft analysis…
# The draft analysis (a view colouring faces by their draft angle; also its dialog's title).
model-analysis-draft = Draft analysis
# The curvature view (faces coloured by how curved they are; also its legend's title).
model-analysis-curvature = Curvature
model-analysis-combs = Curvature combs
model-analysis-zebra = Zebra stripes
# The direction a moulded part is pulled out of its mould (a field's placeholder).
model-analysis-pull-direction = Pull direction
model-analysis-flip-pull = Flip the pull direction
# { $pull } is the pull direction's reference ("Top plane").
model-analysis-pull-flipped = { $pull } (flipped)
model-analysis-mean-curvature = Mean curvature (1 / radius)
# A legend band; { $low } and { $high } are angles in degrees.
model-analysis-degrees-range = { $low }° to { $high }°
# A legend band; { $low } and { $high } are curvatures (1/mm).
model-analysis-curvature-range = { $low } to { $high } /mm
# The legend's band of flat faces (zero curvature).
model-analysis-flat = 0 (flat)

## crates/cadrs_app/src/measure.rs
# The largest distance between two entities.
model-measure-max-distance = Maximum distance
# The distance between two circles' or holes' centres.
model-measure-center-distance = Center distance
# A measured distance; { $value } is the value with its unit.
model-measure-distance-value = Distance: { $value }
# { $value } is the value with its unit.
model-measure-max-distance-value = Maximum distance: { $value }
# { $value } is the value with its unit.
model-measure-center-distance-value = Center distance: { $value }
# { $value } is the angle with its unit.
model-measure-angle-value = Angle: { $value }
# { $value } is the value with its unit.
model-measure-diameter-value = Diameter: { $value }
# { $value } is the value with its unit.
model-measure-length-value = Length: { $value }
model-area = Area
model-diameter = Diameter
# The Measure panel's title (a noun or the tool's name).
model-measure = Measure
# A measuring mode: the shortest distance.
model-measure-minimum = Minimum
# A measuring mode: the largest distance.
model-measure-maximum = Maximum
model-measure-center-to-center = Center to center
model-measure-empty = Select entities to measure

## crates/cadrs_app/src/mass_props.rs
# A checkbox: a value typed in is used instead of the computed one.
model-mass-override = Override
# { $units } are the units of the values ("kg mm²").
model-mass-inertia-heading = Mass moments of inertia ({ $units })
model-mass-title = Mass and section properties
model-mass-faces = Faces to measure
# The centre of a face's area.
model-mass-centroid = Centroid
model-mass-reference-frame = Mate connector for reference frame
model-mass-show-variance = Show calculation variance
model-mass = Mass
model-mass-unit = Mass unit
model-mass-volume = Volume
model-mass-surface-area = Surface area
model-mass-center-of-mass = Center of mass
model-mass-instances = Instances to measure
model-mass-parts = Parts to measure

## crates/cadrs_app/src/mcp.rs
# A notice: the server AI clients connect to has started; { $url } is its address.
model-mcp-server-on = MCP server on { $url }
# { $port } is a port number; { $error } is the system's error message (in English).
model-mcp-cannot-start = Cannot start the MCP server on port { $port }: { $error }

## crates/cadrs_app/src/simulation_ui.rs
# { $count } is 1.
model-sim-one-face = { $count } face
# { $count } is how many faces (not 1).
model-sim-n-faces = { $count } faces
# A force load's summary; { $force } is the force with its unit ("100 N"); it pushes along the faces' normals.
model-sim-force-normal = { $force } normal
# { $force } is the force with its unit ("100 N"); it pulls out along the faces' normals.
model-sim-force-normal-out = { $force } normal, out
# { $force } is the force with its unit ("100 N"); { $axis } is a signed axis ("−Z", "+X").
model-sim-force-along = { $force } along { $axis }
# A force's direction.
model-sim-normal-to-faces = Normal to faces
model-sim-along-x = Along X
model-sim-along-y = Along Y
model-sim-along-z = Along Z
# A mesh density (how fine the analysis mesh is).
model-sim-mesh-coarse = Coarse
# A mesh density (how fine the analysis mesh is).
model-sim-mesh-medium = Medium
# A mesh density (how fine the analysis mesh is).
model-sim-mesh-fine = Fine
model-sim-no-elastic-properties = no Young's modulus or Poisson's ratio
model-sim-no-material = no material: right-click it → Assign material
# The analysis mesh (a label).
model-sim-mesh = Mesh
# { $count } is how many elements the mesh has.
model-sim-elements = { $count } elements
model-sim-max-displacement = Max displacement
# The largest von Mises stress.
model-sim-max-von-mises = Max von Mises
model-sim-min-safety-factor = Min safety factor
model-sim-hint = Add a Fixed load on the faces that are held, and a Force or a Pressure, then Solve.
# The Simulation panel's title (stress analysis).
model-sim-title = Simulation
model-sim-materials = Materials
model-sim-no-parts = No parts.
# { $count } is how many loads there are.
model-sim-loads-count = Loads ({ $count })
# A load type: faces that are held in place.
model-sim-fixed = Fixed
# A load type: a force (also its field's label).
model-sim-force = Force
# A load type: a pressure (also its field's label).
model-sim-pressure = Pressure
model-sim-add-fixed-tooltip = Add Fixed: faces that don't move
model-sim-add-force-tooltip = Add Force: a force spread over faces
model-sim-add-pressure-tooltip = Add Pressure: a pressure on faces
# { $name } is the load's name; { $summary } describes it ("100 N along −Z").
model-sim-load-tooltip = { $name }: { $summary }\nClick to edit
# The button that runs the analysis (a verb).
model-sim-solve = Solve
model-sim-meshing = Meshing…
model-sim-stale = The model or its loads changed since the last solve: Solve again for results.
model-sim-results = Results
# Which result the view colours by (a label before a list).
model-sim-show = Show
model-sim-von-mises-stress = Von Mises stress
model-sim-displacement = Displacement
model-sim-safety-factor = Safety factor
# How the result's shape is shown (a label before a list).
model-sim-deformation = Deformation
# The deformation shown larger than it is.
model-sim-exaggerated = Exaggerated
# { $factor } is the scale factor (a number).
model-sim-exaggerated-by = Exaggerated ×{ $factor }
model-sim-undeformed = Undeformed
# The deformation at its real size.
model-sim-actual = Actual (×1)
model-sim-probe-hint = Hover the model to read the value at a point.
# A stage of the analysis, shown with its progress.
model-sim-stage-meshing = Meshing
# A stage of the analysis (building the equations).
model-sim-stage-assembling = Assembling
# A stage of the analysis (factoring the matrix).
model-sim-stage-factoring = Factoring
model-sim-stage-solving = Solving
model-sim-stage-stresses = Computing stresses
# { $stage } is the analysis's stage ("Meshing"); { $percent } is how far it is (a number).
model-sim-progress = { $stage }… { $percent } %
# Stands in for a part's name when it is gone (as in "Face of a part").
model-sim-a-part = a part
model-sim-fixed-note = The faces don't move.
model-sim-pressure-note = Pushes into the faces.
# { $name } is the load's name.
model-sim-load-row-tooltip = { $name }\nDouble-click to edit (Simulation panel)
model-sim-undeformed-shape = Undeformed shape
model-sim-deformed-actual = Deformed ×1 (actual)
# { $factor } is the scale factor (a number).
model-sim-deformed-exaggerated = Deformed ×{ $factor } (exaggerated)
# { $value } is the stress with its unit.
model-sim-probe-von-mises = Von Mises: { $value }
# { $value } is the displacement with its unit.
model-sim-probe-displacement = Displacement: { $value }
# { $value } is a number (or a dash when there is none).
model-sim-probe-safety-factor = Safety factor: { $value }

## crates/cadrs_app/src/variables_ui.rs
# The variable's name field; # starts a variable's name where it is used ("#width").
model-variable-name-field = Name  #
model-variable-name-rule = A name is letters, digits and _
# A variable's type: a plain number.
model-variable-type-number = Number
# A variable's type: any value (a number, a length, an angle…).
model-variable-type-any = Any

## crates/cadrs_app/src/variable_studio_ui.rs
# A Variable Studio is a tab holding variables for the whole document (a feature name).
model-vs-insert = Insert Variable Studio
model-vs-insert-tooltip = Insert the variables of another Variable Studio, of this document or a version of another
# { $name } is a variable's name; { $tabs } lists other tabs' names.
model-vs-also-in = #{ $name } is also in { $tabs }: this one is used
# { $name } is a variable's name; { $tab } is another tab's name.
model-vs-used-from = Part Studios use #{ $name } of { $tab } (a later tab)
model-vs-available = Make variables available to Part Studios and Assemblies
model-vs-available-tooltip = Unchecked, this studio's variables are a draft: no Part Studio or Assembly sees them
# { $count } is 1.
model-vs-one-variable = { $count } variable
# { $count } is how many variables (not 1).
model-vs-n-variables = { $count } variables
# { $count } is 1.
model-vs-one-error = { $count } error
# { $count } is how many errors (not 1).
model-vs-n-errors = { $count } errors
# { $count } is 1 (one tab uses the variables).
model-vs-used-in-one-tab = used in { $count } tab
# { $count } is how many tabs use the variables (not 1).
model-vs-used-in-n-tabs = used in { $count } tabs
# The table column with each variable's computed value.
model-vs-evaluates-to = Evaluates to
# An inserted table that follows the studio it came from.
model-vs-follows-document = Follows this document
# { $version } is a version's name.
model-vs-newer-version = Newer version: { $version }
model-vs-version-stays = Version: stays as inserted
model-vs-change-version-ellipsis = Change version…
model-vs-change-version-tooltip = Insert another version of this Variable Studio in its place
model-vs-remove-table = Remove this table from the Variable Studio
# The tooltip of a button that opens a menu of more actions.
model-vs-more = More
model-vs-empty = Type a name in the table's last row to add a variable. Every Part Studio and Assembly of this document can then use it in any value field as #name, and follows it when it changes.
model-vs-drag-handle = Drag to reorder; click to pick (Delete deletes it)
model-vs-delete-variable = Delete this variable
model-vs-add-variable = Add variable
model-vs-collapse-all = Collapse all tables
model-vs-expand-all = Expand all tables
model-vs-insert-above = Insert variable above
# A menu item: makes a copy of the variable (a verb).
model-vs-duplicate = Duplicate
# Why a Variable Studio can't be inserted: it inserts this one (a loop).
model-vs-would-cycle = Inserts this Variable Studio
model-vs-already-inserted = Already inserted
# A dialog's title and button: puts another version in an inserted table's place.
model-vs-change-version = Change version
# The insert dialog's button (a verb).
model-vs-insert-button = Insert
model-vs-current-document = Current document
model-vs-other-documents = Other documents
# { $document } is the document's name.
model-vs-studios-of = Variable Studios of { $document }: inserted live, so they follow their edits
model-vs-none-in-document = This document has no other Variable Studio. Create one with + ▸ Create Variable Studio.
model-vs-none-in-version = This version has no Variable Studio.
# A checkbox: insert all the table's variables.
model-vs-whole-table = Whole table

## crates/cadrs_app/src/assembly/animate.rs
# The Animate dialog's title (it plays a mate's motion).
asm-animate = Animate
# A playback type of the Animate dialog: plays once.
asm-animate-mode-single = Single
# A playback type of the Animate dialog: back and forth until stopped.
asm-animate-mode-reciprocate = Reciprocate
# A playback type of the Animate dialog: start to end, repeated.
asm-animate-mode-loop = Loop
# A mate's degree of freedom: movement along X.
asm-dof-x-translation = X translation
# A mate's degree of freedom: movement along Y.
asm-dof-y-translation = Y translation
# A mate's degree of freedom: movement along Z.
asm-dof-z-translation = Z translation
# A mate's degree of freedom: turning about Z.
asm-dof-z-rotation = Z rotation
# A mate (a joint between two parts of an assembly), as a noun.
asm-mate = Mate
# The value the animation starts at.
asm-animate-start = Start
# The value the animation ends at.
asm-animate-end = End
# How many steps the animation has.
asm-animate-steps = Steps
asm-reverse-direction = Reverse direction
asm-animate-current-value = Current value
# Button: plays the animation.
asm-animate-play = Play
# Button: stops the animation.
asm-animate-stop = Stop
# The animation is pausing at one end before it turns back.
asm-animate-turning = turning at the end
# The animation is moving toward its End value.
asm-animate-toward-end = → toward End
# The animation is moving toward its Start value.
asm-animate-toward-start = ← toward Start
# The animation isn't playing.
asm-animate-stopped = stopped

## crates/cadrs_app/src/assembly/bom_panel.rs
# A bill of materials view: subassemblies shown with their parts nested under them.
asm-bom-structured = Structured
# A bill of materials view: every part listed once, no nesting.
asm-bom-flattened = Flattened
asm-bom-apply-template = Apply template
# Button: adds a column to the bill of materials table.
asm-bom-add-column = Add column
# Tooltip of a button that opens a menu of further actions.
asm-more = More
# The bill of materials' totals row.
asm-bom-totals = Totals
# { $count } is the total number of items.
asm-bom-total-quantity = Quantity { $count }
# { $mass } is the total mass with its unit (e.g. "1.2 kg").
asm-bom-total-mass = Mass { $mass }
asm-bom-no-saved-templates = No saved templates
asm-bom-save-as-template = Save as template…
asm-bom-copy-table = Copy table
asm-bom-export-csv = Export to CSV
asm-bom-hide-excluded = Hide excluded/suppressed
asm-bom-show-excluded = Show excluded/suppressed
asm-bom-hide-top-row = Hide top-level assembly row
asm-bom-show-top-row = Show top-level assembly row
asm-bom-reset-sort = Reset sort
asm-bom-generate-missing-numbers = Generate missing part numbers
asm-bom-no-material = No material
asm-assign-material = Assign material…
# { $name } is the name of a tab (a Part Studio or an assembly) to switch to.
asm-switch-to-named = Switch to { $name }
# Shown when there is no tab to switch to.
asm-switch-to = Switch to
# BOM is short for bill of materials.
asm-bom-unsuppress-in-bom = Unsuppress in this BOM
asm-bom-suppress-from-bom = Suppress from this BOM
asm-bom-generate-next-number = Generate next part number
asm-bom-remove-column = Remove column
asm-bom-move-left = Move left
asm-bom-move-right = Move right
# { $name } is the name the template was saved under.
asm-bom-saved-template = Saved template "{ $name }"
asm-bom-template-name = Template name
asm-bom-every-item-numbered = Every item has a part number
asm-bom-generated-one-number = Generated 1 part number
# { $count } is how many part numbers were generated (2 or more).
asm-bom-generated-numbers = Generated { $count } part numbers
# { $count } is how many rows were copied.
asm-bom-copied-table = Copied the table ({ $count } rows)
# An error: no folder has been set to export files to.
asm-bom-no-export-folder = no folder to export to
# { $file } is the exported file's name.
asm-bom-exported-file = Exported { $file }
# { $error } is what went wrong.
asm-bom-export-failed = Export failed: { $error }

## crates/cadrs_app/src/assembly/connector_tool.rs
# The empty field for the instance a mate connector belongs to.
asm-connector-select-owner-instance = Select owner instance
# Stands in for a part's name when it has none: one placed copy of a part in an assembly.
asm-instance = instance
# A mate connector at the origin of the assembly (or of a part).
asm-connector-origin = Origin
# { $connector } is a mate connector's name, { $part } the name of the part it is on.
asm-connector-of-part = { $connector } of { $part }
# { $part } is the name of the part the mate connector is on.
asm-mate-connector-of = Mate connector of { $part }

## crates/cadrs_app/src/assembly/explode.rs
# The panel listing an assembly's exploded views (the parts moved apart to show how they fit).
asm-exploded-views = Exploded views
asm-explode-add-view = Add exploded view
asm-explode-no-views = No exploded views yet
# { $name } is an exploded view's name; it has one step.
asm-explode-view-one-step = { $name }: 1 step
# { $name } is an exploded view's name, { $count } how many steps it has (not 1).
asm-explode-view-steps = { $name }: { $count } steps
# The left end of the exploded view's slider: the parts in place.
asm-explode-assembled = Assembled
# The right end of the exploded view's slider: the parts moved apart.
asm-explode-exploded = Exploded
asm-explode-slider-tooltip = 0 % assembled … 100 % exploded
# A checkbox: draws lines along the paths the parts moved.
asm-explode-trail-lines = Trail lines
asm-explode-edit-steps = Edit steps
asm-explode-export-image = Export image…
# { $name } is an exploded view's name; its steps are listed below.
asm-explode-steps-heading = { $name }: steps
asm-explode-no-steps = No steps yet
# { $step } is the step's number, { $who } the parts it moves, { $what } its motion (e.g. "Z 120 mm").
asm-explode-step-tooltip = Step { $step }: { $who }, { $what }. Click to change it
asm-explode-delete-step = Delete step
# { $step } is the number of the step being changed.
asm-explode-change-step = Change step { $step }
asm-explode-new-step = New step
asm-explode-instances-placeholder = Instances (select in the view)
asm-explode-move-along-axis = Move along an axis
asm-explode-rotate-about-axis = Rotate about an axis
# { $axis } is X, Y or Z: the step turns the parts about it.
asm-explode-about-axis = About { $axis }
# { $axis } is X, Y or Z: the step moves the parts along it.
asm-explode-along-axis = Along { $axis }
asm-explode-drag-hint = Or drag an arrow of the triad in the view
asm-explode-update-step = Update step
asm-explode-add-step = Add step
asm-explode-rename-view = Rename exploded view

## crates/cadrs_app/src/assembly/folders.rs
asm-folder-name = Folder name
# Takes a folder's items out of it and removes the folder.
asm-folder-unpack = Unpack folder
# A badge while dragging one item into a folder.
asm-folder-one-item = 1 item
# A badge while dragging items into a folder; { $count } is how many (2 or more).
asm-folder-items = { $count } items

## crates/cadrs_app/src/assembly/group_dialog.rs
# The placed parts (instances) of an assembly; also the heading of their list.
asm-instances = Instances

## crates/cadrs_app/src/assembly/in_context.rs
# Stands in for an assembly's name when it isn't known (inside "{ $context } of { $assembly }").
asm-context-the-assembly = the assembly
# The in-context bar's title: { $context } is an assembly context's name (e.g. "Context 1"), { $assembly } the assembly's name.
asm-context-of-assembly = { $context } of { $assembly }
asm-context-assembly-changed = Assembly changed
asm-context-assembly-changed-tooltip = The assembly has changed since this context was taken: Update context
# The context has no primary instance (the placed part the Part Studio is edited in).
asm-context-no-primary = No primary instance
# Button: takes the assembly context again, as the assembly is now.
asm-context-update = Update context
asm-context-update-tooltip = Take the assembly as it is now; the features that reference it follow
asm-context-show = Show context
asm-context-hide = Hide context
asm-context-go-to-assembly = Go to assembly
asm-context-version-and-go = Create version and go to assembly
asm-context-insert-and-go = Insert and go to assembly
asm-context-done-tooltip = Edit outside of the context
asm-context-select-transparent = Select transparent geometry
# How see-through the assembly context is drawn (a slider's label).
asm-context-transparency = Transparency
asm-context-transparency-tooltip = How see-through the context is
asm-context-edit-outside = Edit outside of context
asm-context-assembly-contexts = Assembly contexts
asm-context-select-tooltip = The context shown, or editing outside of context
asm-context-update-available = An update is available
asm-context-more-tooltip = Rename, update or delete the context
asm-context-delete = Delete context
asm-context-rename = Rename context
# { $contexts } is a list of assembly context names.
asm-context-in-context = In context: { $contexts }
# Added after a tooltip's text, following a "·".
asm-context-an-update-is-available = an update is available
# { $studio } is a Part Studio's name (with its document's).
asm-context-primary-linked = Primary instance of an in-context Part Studio ({ $studio })
# { $studio } is a Part Studio's name.
asm-context-secondary-instance = Secondary instance: { $studio } has in-context references
# { $contexts } is a list of assembly context names, { $studio } a Part Studio's name.
asm-context-primary-of = Primary instance of { $contexts } ({ $studio })

## crates/cadrs_app/src/assembly/insert.rs
asm-insert-title = Insert parts and assemblies
# A tab of the Insert dialog, on two short lines (\n is a line break).
asm-insert-tab-current-document = Current\ndocument
# A tab of the Insert dialog, on two short lines (\n is a line break).
asm-insert-tab-other-documents = Other\ndocuments
# A tab of the Insert dialog (standard parts: screws, nuts…), on two short lines (\n is a line break).
asm-insert-tab-standard-content = Standard\ncontent
asm-insert-rigid-tooltip = Insert the Part Studio as rigid
asm-insert-version-graph-tooltip = Version graph: insert from the workspace or a version
# A tab: the document's Part Studios to insert from.
asm-insert-tab-part-studios = Part Studios
# A tab: the document's assemblies to insert.
asm-insert-tab-assemblies = Assemblies
# A tab: the document's circuit boards (PCB Studio) to insert.
asm-insert-tab-boards = Boards
asm-insert-search-part-studios = Search Part Studios
asm-insert-search-assemblies = Search assemblies
asm-insert-search-boards = Search boards
asm-insert-sketches-not-yet = Sketches (not available yet)
# { $count } is how many instances this dialog has inserted.
asm-insert-inserted-count = Inserted: { $count }
asm-insert-undo-to-remove = Undo to remove instances
# { $name } is a Part Studio's name, { $count } how many parts it has.
asm-insert-rigid-row-tooltip = { $name }: inserted as one rigid instance ({ $count } parts)
asm-insert-no-other-assemblies = No other assemblies
asm-insert-no-boards = No PCB Studio boards in this document
asm-insert-no-part-studios-match = No Part Studios match

## crates/cadrs_app/src/assembly/interference.rs
# An error: the geometry kernel's worker stopped before it answered.
asm-kernel-thread-stopped = The kernel thread stopped
# Stands in for a part's name when it has none.
asm-part-fallback = part
# The interference check is running.
asm-interference-checking = Checking…
# { $error } is what went wrong.
asm-interference-failed = Check interference failed: { $error }
asm-interference-none = No interference found
asm-interference-one = 1 interference
# { $count } is how many interferences (overlaps between parts) were found (2 or more).
asm-interference-many = { $count } interferences
asm-interference-all-instances = All instances
# The dialog that checks an assembly for overlapping parts; also its menu command.
asm-interference-title = Check interference
# { $instances } is "All instances" or a list of instance names.
asm-interference-instances = Instances: { $instances }
asm-interference-select-instances = Select these instances

## crates/cadrs_app/src/assembly/items.rs
# The heading of an assembly's items (bill of materials entries with no geometry: glue, paint…); { $count } is how many.
asm-items-count = Items ({ $count })
# An item's tooltip, under its name; { $quantity } is how many the bill of materials lists.
asm-item-row-tooltip = Item (no geometry), quantity { $quantity }; listed in the Bill of Materials
asm-item-add-ellipsis = Add item…
# The title of the dialog that adds an item (an entry with no geometry) to the bill of materials.
asm-item-add = Add item
# How many (a field's label).
asm-quantity = Quantity
# An item's unit of measure (each, litre, metre…).
asm-item-unit = Unit
asm-part-number = Part number

## crates/cadrs_app/src/assembly/linked_context.rs
# { $error } is why the linked document can't be opened.
asm-linked-document-cant-open = The linked document can't be opened: { $error }
asm-linked-studio-gone = The linked Part Studio is no longer in its document
# An error: the assembly context wasn't made from another document.
asm-linked-not-linked-context = not a linked context
asm-linked-assembly-document-cant-read = The assembly's document can't be read
# { $error } is why the assembly's document can't be opened.
asm-linked-assembly-document-cant-open = The assembly's document can't be opened: { $error }

## crates/cadrs_app/src/assembly/list.rs
# Stands in for a missing part's name; { $index } is its number.
asm-missing-part = Missing part <{ $index }>
# Stands in for a part's name; { $index } is its number.
asm-part-index = Part <{ $index }>
# The heading of an assembly's mates (joints between parts); { $count } is how many.
asm-mate-features-count = Mate Features ({ $count })
# The heading of an assembly's instances (placed parts); { $count } is how many.
asm-instances-count = Instances ({ $count })
asm-list-open-subassembly-tooltip = Open the subassembly's tab to edit it
# A tooltip line under an instance's name.
asm-list-rigid-studio-instance = Rigid Part Studio instance
# An instance fixed in place.
asm-list-fixed = Fixed
asm-list-one-dof = This instance has 1 degree of freedom
# { $count } is how many degrees of freedom (ways to move) the instance has (2 or more).
asm-list-dofs = This instance has { $count } degrees of freedom
# { $name } is a named position's name.
asm-list-follows-position = Follows the named position "{ $name }"
asm-list-flexible-tooltip = Flexible: its mates act here. Click to make it rigid
asm-list-rigid-tooltip = Rigid. Click to make it flexible
# { $name } is a part's name; it is unticked in the open Edit dialog and taken out when the dialog is accepted (✓).
asm-list-studio-part-unticked = { $name }: unticked in Edit (taken out on ✓)
# { $name } is a part's name.
asm-list-studio-part = { $name } (in the rigid Part Studio instance)
# { $name } is a Replicate feature's name, { $count } how many instances it made.
asm-list-replicate-tooltip = { $name }: { $count } instances
asm-list-root-fastened-tooltip = Fastened to the origin by a mate
asm-list-root-fixed-tooltip = Fastened to the origin: an instance of this assembly is fixed (not carried into a parent assembly)

## crates/cadrs_app/src/assembly/managed_context.rs
# Saved sets of which parts are shown (a feature not made yet).
asm-display-states = Display states
asm-display-states-planned = Named display states aren't in cadrs yet
asm-create-studio-in-context = Create Part Studio in context
asm-origin-of-new-studio = Origin of new Part Studio
asm-select-origin-or-connector = Select origin/mate connector
asm-insert-and-go-title = Insert and go to Assembly
asm-parts-to-insert = Parts to insert

## crates/cadrs_app/src/assembly/mod.rs
# A mate type (a joint that holds two parts together, no motion).
asm-mate-type-fastened = Fastened
# A mate type (turns about one axis).
asm-mate-type-revolute = Revolute
# A mate type (slides along one axis).
asm-mate-type-slider = Slider
# A mate type (slides along and turns about one axis).
asm-mate-type-cylindrical = Cylindrical
# A mate type (a pin moving in a slot).
asm-mate-type-pin-slot = Pin slot
# A mate type (moves in a plane).
asm-mate-type-planar = Planar
# A mate type (a ball joint).
asm-mate-type-ball = Ball
# A mate type (keeps two axes parallel).
asm-mate-type-parallel = Parallel
# A mate type (keeps two surfaces touching).
asm-mate-type-tangent = Tangent
# A mate type (centres a tab between two faces).
asm-mate-type-width = Width
# A mate relation type (two revolute mates turning together like gears).
asm-relation-type-gear = Gear
# A mate relation type (a turn becomes a slide).
asm-relation-type-rack-pinion = Rack and pinion
# A mate relation type (a turn and a slide along the same axis).
asm-relation-type-screw = Screw
# A mate relation type (two slides together).
asm-relation-type-linear = Linear
# A mate's turning about its Z axis.
asm-dof-z-angle = Z angle

## crates/cadrs_app/src/assembly/mate_dialog.rs
asm-mate-tab-connectors = Tab mate connectors
asm-mate-width-connectors = Width mate connectors
# The geometry (faces, edges) a Tangent mate holds together.
asm-mate-entities = Entities
asm-mate-tangent-propagation = Tangent propagation
asm-mate-connectors = Mate connectors
asm-mate-rotate-about-x = Rotate about X
asm-mate-rotate-about-y = Rotate about Y
asm-mate-rotate-about-z = Rotate about Z
asm-mate-rotation-angle = Rotation angle
# How far a mate may move (minimum and maximum values).
asm-mate-limits = Limits
asm-mate-simulation-connection = Simulation connection
asm-mate-flip-primary-axis = Flip primary axis
asm-mate-reorient-secondary-axis = Reorient secondary axis
# DOF: degree of freedom (a way the mate can move).
asm-mate-animate-dof = Animate mate DOF
# Button: solves the assembly's mates (a verb).
asm-mate-solve = Solve
asm-mate-solve-all = Solve all mates

## crates/cadrs_app/src/assembly/mate_display.rs
# A banner while the Show mates mode is on (H is its key).
asm-show-mates-mode = Show mates mode (H): hover an instance, click to pin

## crates/cadrs_app/src/assembly/mates_list.rs
# Stands in for a part's name inside "Missing reference: …".
asm-mates-an-instance = an instance
# { $part } is the name of the part (or "an instance") whose geometry the mate lost.
asm-mates-missing-reference = Missing reference: the entity this mate was on is gone from { $part } (its source changed). Edit the mate and pick it again.
# A tooltip line: the row is a group of mates (a mate type that holds parts together as one).
asm-mates-group = Group
# A tooltip line; { $type } is a mate type (Fastened, Revolute…).
asm-mates-type-mate = { $type } mate
# Part of a mate's offset: { $angle } is an angle with its unit, { $axis } is X, Y or Z.
asm-mates-angle-about-axis = { $angle } about { $axis }
# A tooltip line; { $offset } lists a mate's offsets (e.g. "X 5 mm, 90° about Z").
asm-mates-offset-line = Offset: { $offset }
# A tooltip line; { $dof } is the motion (X, Y, Z, Z angle), { $min } and { $max } its limits with units.
asm-mates-limits-line = Limits { $dof }: { $min } … { $max }
asm-mates-no-tangent-propagation = No tangent propagation
# A gear relation's ratio; { $first } and { $second } are numbers.
asm-relation-ratio-pair = Ratio { $first } : { $second }
# A linear relation's ratio; { $ratio } is a number.
asm-relation-ratio = Ratio { $ratio }
# A rack and pinion relation; { $distance } is a length with its unit.
asm-relation-distance-per-revolution = Distance per revolution { $distance }
# A screw relation; { $pitch } is a length with its unit.
asm-relation-pitch = Pitch { $pitch }
# A tooltip line; { $type } is a relation type (Gear, Screw…).
asm-mates-type-relation = { $type } relation
# The relation's direction is reversed.
asm-relation-reversed = Reversed
# A tooltip line; { $instances } is how many instances (e.g. "3 instances"), { $mate } the mate it copies.
asm-mates-replicate-tooltip = Replicate: { $instances } on matching geometry, with { $mate }
# A tooltip line; { $owner } is the name of the part the mate connector is on.
asm-mates-connector-on = Mate connector on { $owner }
asm-mates-hide-mate = Hide mate
asm-mates-show-mate = Show mate
# A tooltip line: the mate is suppressed (turned off).
asm-mates-suppressed = Suppressed
# A tooltip line; { $name } is the Replicate feature that made this mate.
asm-mates-made-by = Made by { $name }
# A tooltip line; { $name } is the relation this mate is in.
asm-mates-in = In { $name }
# { $dof } is the motion (X, Y, Z, Z angle), { $value } the limit with its unit.
asm-mates-limit-minimum = { $dof } minimum ({ $value })
# { $dof } is the motion (X, Y, Z, Z angle), { $value } the limit with its unit.
asm-mates-limit-maximum = { $dof } maximum ({ $value })
asm-mates-apply-limit-position = Apply limit position
# Mates (joints between parts), plural noun.
asm-mates = Mates
# Menu: opens a row to show what it holds.
asm-mates-expand = Expand
# Menu: closes a row's list.
asm-mates-collapse = Collapse
asm-make-opaque = Make opaque
asm-make-transparent = Make transparent…
# Menu: puts the mate back to its starting position (a verb).
asm-mates-reset = Reset
asm-mates-show-all = Show all mates
# Menu: shows only these parts.
asm-isolate-ellipsis = Isolate…
asm-mates-edit-named-position-driver = Edit named position driver mate…
asm-animate-ellipsis = Animate…
asm-add-selection-to-folder = Add selection to folder…

## crates/cadrs_app/src/assembly/menu.rs
asm-menu-linked-cant-open = The linked document can't be opened
asm-menu-only-version-opens = Only a reference to a version opens its document
# Standard content: the library of standard parts (screws, nuts…), which has no tab of its own.
asm-menu-switch-to-standard = Switch to Standard content
asm-menu-linked-no-tab = A linked instance shows a frozen version of its source: there is no tab to switch to
asm-menu-edit-standard-instance = Edit standard content instance…
asm-menu-select-same-config = Select instances with same configuration
asm-menu-select-same-part = Select instances with same part and same configuration
# { $count } is how many instances are copied.
asm-menu-copy-items = Copy { $count } items
# { $name } is the instance's name.
asm-menu-copy-named = Copy { $name }
# Menu: frees a fixed instance (a verb).
asm-menu-unfix = Unfix
# Menu: fixes the instance in place (a verb).
asm-menu-fix = Fix
asm-menu-collaboration-out-of-scope = Collaboration (comments, tasks) is out of scope
asm-menu-release-out-of-scope = Release management is out of scope
asm-menu-full-tessellation = Parts are always drawn from their full tessellation
# { $name } is a circuit board's name.
asm-menu-board-in-context = { $name } in context
asm-menu-board-in-context-tooltip = Edit this board's layout here, among the parts around it
# Menu: edits a circuit board's layout.
asm-menu-edit-layout = Edit layout
# Menu: edits it here, among the parts around it.
asm-menu-in-context = In context
asm-menu-board-edit-in-context-tooltip = Edit the board's layout here, its components moved among the parts around it (the instance follows)
asm-menu-in-pcb-studio = In its PCB Studio
asm-menu-board-edit-layout-tooltip = Edit the board's layout in its PCB Studio (the instance follows)
asm-menu-edit-in-context = Edit in context
asm-menu-version-edited-at-tab = A version of this document is edited at its tab
asm-menu-new-context = New context
asm-menu-set-primary = Set as primary instance
asm-menu-move-to-origin = Move to origin
asm-menu-align-z = Align with Z
asm-menu-anti-align-z = Anti-align with Z
asm-menu-rotate-90 = Rotate 90°
asm-menu-rotate-180 = Rotate 180°
asm-menu-hide-others = Hide other instances
asm-menu-hide-all = Hide all instances
asm-menu-show-mates = Show mates
asm-menu-hide-mates = Hide mates
asm-menu-best-tessellation = Use best available tessellation
asm-menu-connector-at-origin = Add mate connector to instance origin…
asm-menu-replace-instances = Replace instances…
asm-menu-move-to-new-sub = Move to new subassembly
# The subassembly keeps the position it is in now (no named position).
asm-menu-current-position = Current position
asm-menu-dissolve-sub = Dissolve subassembly
asm-menu-make-rigid = Make rigid
asm-menu-make-flexible = Make flexible
asm-menu-follow-position = Lock / follow position to
asm-menu-unpin-reference = Unpin reference
asm-menu-pin-reference = Pin reference
asm-menu-update-linked = Update linked document…
asm-menu-open-linked = Open linked document
asm-menu-change-to-version = Change to version…
asm-menu-standard-no-versions = Standard content has no versions
asm-export-ellipsis = Export…
asm-menu-where-used = Where used…
asm-menu-revision-history = Revision history…
asm-menu-create-task = Create task…
# { $name } is what the drawing shows (a part's or an assembly's name).
asm-menu-create-drawing-of = Create Drawing of { $name }…
asm-menu-create-empty-sub = Create empty subassembly
asm-menu-section-view = Section view…
# { $name } is the instance's name.
asm-menu-unsuppress-named = Unsuppress { $name }
# { $name } is the instance's name.
asm-menu-suppress-named = Suppress { $name }
asm-menu-find-in-list = Find in instance list
# { $name } is the instance's name.
asm-menu-replace-named = Replace { $name }…
# { $name } is a Part Studio's or an assembly's name.
asm-menu-where-used-for = Where used for { $name }…
asm-menu-select-other = Select other…
asm-menu-select-other-not-in-course = Select other (picking hidden faces) is not part of the assemblies course
asm-menu-view-normal-to = View normal to
asm-menu-view-normal-to-not-in-course = View normal to (a face) is not part of the assemblies course
# { $name } is the instance's name.
asm-menu-delete-named = Delete { $name }
# { $name } is the name of the copied instance.
asm-menu-paste-named = Paste { $name }
asm-menu-show-all = Show all
asm-menu-show-all-instances = Show all instances
# A view direction (looking at the model from a corner).
asm-menu-isometric = Isometric
# What the clipboard holds, in "Paste …"; { $count } is how many instances were copied.
asm-clipboard-items = { $count } items

## crates/cadrs_app/src/assembly/named_positions.rs
# The panel of an assembly's named positions (saved sets of mate values).
asm-named-positions = Named positions
asm-named-positions-add = Add position
asm-named-positions-add-tooltip = Save the mates' current positions as a new row
asm-named-positions-empty = No named positions yet
# { $name } is a named position's name.
asm-named-positions-current-row = { $name } (the assembly is in this position)
# { $name } is a named position's name.
asm-named-positions-row = { $name }: double-click to apply
# { $mate } is a mate's name (a column), { $name } a named position's name (a row).
asm-named-positions-value-tooltip = { $mate } in { $name }: type a value, Enter
asm-named-positions-update = Update to current position

## crates/cadrs_app/src/assembly/relation_dialog.rs
# A refused pick: { $name } is the mate picked, { $type } its type (Fastened, Revolute…), which this relation doesn't take.
asm-relation-refused-mate-type = { $name } is a { $type } mate
# A refused pick: { $name } is what was picked.
asm-relation-not-a-mate = { $name } is not a mate
asm-relation-cylindrical-mate = Cylindrical mate
# A field: the ratio between two mates' motions.
asm-relation-ratio-label = Ratio
# A field: how far the rack moves per turn of the pinion.
asm-relation-distance-per-revolution-label = Distance per revolution
# A field: a screw's travel per turn.
asm-relation-pitch-label = Pitch

## crates/cadrs_app/src/assembly/replace_dialog.rs
asm-replace-one-mate-removed = 1 mate removed (no matching geometry)
# { $count } is how many mates were removed (2 or more).
asm-replace-mates-removed = { $count } mates removed (no matching geometry)
asm-replace-choose-part = Choose the part to replace them with
# { $count } is how many mates the replacement keeps.
asm-replace-mates-kept = Mates kept: { $count }
# Follows "Mates kept: …" after a "·"; { $mates } lists the mates that will be removed.
asm-replace-removed-mates = removed (no matching geometry): { $mates }
asm-replace-title = Replace instances
asm-replace-instances-placeholder = Instances to replace
asm-replace-with = Replace with
# { $name } is a part's name, { $studio } the Part Studio it is in.
asm-replace-part-tooltip = { $name } (Part Studio { $studio })

## crates/cadrs_app/src/assembly/replicate_dialog.rs
# { $name } is the mate the Replicate copies.
asm-replicate-seed-mate = Mate: { $name }
asm-replicate-no-mate = The instance has no mate to copy
# { $number } is a hole's number, { $part } the part it is on.
asm-replicate-hole-on = Hole { $number } on { $part }
asm-replicate-instance-placeholder = Instance to replicate
asm-replicate-holes-placeholder = Select matching holes
asm-replicate-all-matching = All matching on the part
asm-instance-count-one = 1 instance
# { $count } is how many instances (not 1).
asm-instance-count = { $count } instances

## crates/cadrs_app/src/assembly/standard.rs
# Standard content (library parts): the standard a part follows (ANSI, ISO…).
asm-std-standard = Standard
# Standard content: the kind of part (bolts & screws, nuts…).
asm-std-category = Category
# Standard content: the group within a category (hex bolts…).
asm-std-class = Class
# Standard content: the part itself (hex cap screw…).
asm-std-component = Component
# { $count } is how many standard content instances are edited at once.
asm-std-edit-count = { $count } instances: Size and Length change on all of them
asm-std-autosize-tooltip = Auto-size: pick a hole or shaft (bolts round down, nuts round up)
# A fastener's length.
asm-std-length = Length
asm-std-thread-length = Thread length
# The shape of the face a fastener bears on.
asm-std-bearing-face = Bearing face
# A fastener's surface finish (zinc plated, black oxide…).
asm-std-finish = Finish
asm-std-library-part-number = Use the library's part number
# Button: applies the changes to the standard content instances (a verb).
asm-std-update = Update
# Button: inserts standard parts (a verb).
asm-std-insert = Insert
asm-std-insert-tooltip = Insert on the selected holes, or pick holes one by one
asm-std-insert-closest = Insert closest to selection
asm-std-insert-furthest = Insert furthest from selection
asm-std-autosize-hint = Click a hole or shaft edge to size it
# A is the key that flips the part, Esc the one that stops.
asm-std-place-hint = Click hole edges to insert (A flips, Esc stops)
asm-std-select-hole-first = Select a hole or a fastener's hole edge first
# { $count } is how many instances the dialog edits.
asm-std-edit-title-count = Edit standard content ({ $count })
asm-std-edit-title = Edit standard content

## crates/cadrs_app/src/assembly/studio_edit.rs
asm-studio-edit-hint = Parts in this rigid instance

## crates/cadrs_app/src/assembly/triad.rs
# A readout of the selected circle; { $diameter } is its diameter with its unit.
asm-triad-diameter = Diameter: { $diameter }

## crates/cadrs_app/src/assembly/where_used.rs
# { $name } is a Part Studio's name.
asm-where-used-part-studio = Part Studio { $name }
# { $name } is a circuit board's name.
asm-where-used-board = Board { $name }
# { $name } is an assembly's name.
asm-where-used-assembly = Assembly { $name }
# { $what } is what is looked up (e.g. "Part Studio Base"), { $document } the linked document's name, { $version } its version's.
asm-where-used-linked-in = { $what }, in { $document } ({ $version })
# { $what } is what is looked up, { $version } the version of this document it comes from.
asm-where-used-this-document = { $what }, this document ({ $version })
# { $count } is a count of instances (e.g. "2 instances").
asm-where-used-in-subassembly = { $count }, in a subassembly
asm-where-used-one-use = 1 use
# { $count } is how many times it is used (not 1).
asm-where-used-uses = { $count } uses
asm-where-used-title = Where used
asm-where-used-in-one-assembly = Used in 1 assembly of this document
# { $count } is how many assemblies (not 1).
asm-where-used-in-assemblies = Used in { $count } assemblies of this document
# { $name } is an assembly's name (its tab opens).
asm-where-used-open = Open { $name }
asm-where-used-no-version-references = No references to it at a version
asm-where-used-version-references = References at a version (this and other documents):

## crates/cadrs_app/src/drawing/annotations.rs
# A drawing tool: a dimension of whatever is picked.
drawing-tool-dimension = Dimension
drawing-tool-radial-dimension = Radial dimension
drawing-tool-diameter-dimension = Diameter dimension
drawing-tool-point-to-point-dimension = Point to point dimension
drawing-tool-line-to-line-dimension = Line to line dimension
drawing-tool-angular-dimension = Angular dimension
drawing-tool-baseline-dimension = Baseline dimension
drawing-tool-ordinate-dimension = Ordinate dimension
drawing-tool-chamfer-dimension = Chamfer dimension
drawing-tool-arc-length-dimension = Arc length dimension
# A geometric tolerance (GD&T) annotation.
drawing-tool-feature-control-frame = Feature control frame
# A GD&T datum feature symbol.
drawing-tool-datum-feature = Datum feature
drawing-tool-point-to-point-centerline = Point to point centerline
drawing-tool-line-to-line-centerline = Line to line centerline
drawing-tool-3-point-circle-centerline = 3 point circle centerline
drawing-tool-2-point-circle-centerline = 2 point circle centerline
# A surface finish (roughness) symbol.
drawing-tool-surface-finish = Surface finish
drawing-tool-weld-symbol = Weld symbol
# Menu: opens a dimension's format palette.
drawing-format-ellipsis = Format…
drawing-hint-note = Note: click the sheet for a note, or an edge or point for a note with a leader
drawing-hint-note-place = Note: click to place the text
drawing-hint-table = Table: click the sheet to place the table
drawing-hint-add-leader = Add leader: pick an edge or point · Esc to finish
drawing-hint-line = Line: click the start, then the end · Esc to finish
drawing-hint-spline = Spline: click its points · double-click or Enter to finish
drawing-hint-place-import = Click the sheet to place it · Esc to cancel
# BOM is short for bill of materials.
drawing-hint-place-bom = Insert BOM: click the sheet to place the table (it snaps to the border's corner or the title block) · Esc to cancel
# ✓ is the button that finishes.
drawing-hint-callout-pick = Callout: pick an edge of a part in an assembly view · ✓ to finish
drawing-hint-callout-place = Callout: click to place it (it lines up with the others) · Esc to cancel
# { $tool } is the dimension tool's name.
drawing-hint-pick-more-points = { $tool }: pick more points to add · Esc to finish
# { $tool } is the dimension tool's name.
drawing-hint-place-text = { $tool }: click to place the text · Esc to cancel
# { $tool } is the dimension tool's name.
drawing-hint-pick-circle-or-arc = { $tool }: pick a circle or arc
# { $tool } is the dimension tool's name.
drawing-hint-pick-two-lines = { $tool }: pick two lines
# { $tool } is the dimension tool's name.
drawing-hint-pick-two-points = { $tool }: pick two points
# { $tool } is the dimension tool's name.
drawing-hint-pick-edges-or-points = { $tool }: pick edges or points
# { $tool } is the dimension tool's name.
drawing-hint-pick-base = { $tool }: pick the base, then a point
# { $tool } is the dimension tool's name.
drawing-hint-pick-zero-point = { $tool }: pick the zero point
# { $tool } is the dimension tool's name.
drawing-hint-pick-chamfer-edge = { $tool }: pick a chamfer's edge
# { $tool } is the dimension tool's name.
drawing-hint-pick-arc = { $tool }: pick an arc
# { $tool } is the annotation tool's name (Feature control frame, Weld symbol…).
drawing-hint-pick-an-edge = { $tool }: pick an edge
# { $tool } is the annotation tool's name (Feature control frame, Weld symbol…).
drawing-hint-click-to-place = { $tool }: click to place · Esc to cancel
drawing-hint-hole-callout-pick = Hole callout: pick a hole's edge
drawing-hint-hole-callout-place = Hole callout: click to place the callout
drawing-hint-centermark = Centermark: pick circles or arcs · Esc to finish
# A virtual sharp: the point where two edges would meet.
drawing-hint-virtual-sharp = Virtual sharp: pick two straight edges
# { $count } is how many points are picked so far.
drawing-hint-centerline-points = Centerline: pick two points ({ $count } of 2)
# { $count } is how many edges are picked so far.
drawing-hint-centerline-edges = Centerline: pick two edges ({ $count } of 2)
# { $count } is how many points are picked so far.
drawing-hint-circle-centerline-points = Circle centerline: pick three points ({ $count } of 3)
drawing-hint-circle-centerline-centre = Circle centerline: pick the centre
drawing-hint-circle-centerline-point = Circle centerline: pick a point on the circle

## crates/cadrs_app/src/drawing/view_kind_tools.rs
# A drawing view showing a circled area enlarged.
drawing-kind-detail-view = Detail view
# A drawing view cut down to a rectangle.
drawing-kind-crop-view = Crop view
# A drawing view cut down to a freehand (spline) outline.
drawing-kind-crop-view-spline = Crop view (spline)
# A drawing view with material removed inside an outline to show what is behind it.
drawing-kind-broken-out-section = Broken-out section
# A drawing view with a middle part left out (two break lines).
drawing-kind-break-view = Break view
drawing-broken-out-depth-help = From the model origin along the direction of sight: the material in front of this depth inside the boundary is removed.
drawing-hint-section-start = Section view: click the cutting line's start on a view
drawing-hint-section-end = Section view: click the cutting line's end
drawing-hint-section-place = Section view: move to either side and click to place · Esc to cancel
drawing-hint-detail-centre = Detail view: click the centre of the detail circle on a view
drawing-hint-detail-radius = Detail view: click a point on the circle
drawing-hint-detail-place = Detail view: click to place the detail · Esc to cancel
drawing-hint-crop-start = Crop view: click a corner of the crop rectangle on a view
drawing-hint-crop-end = Crop view: click the opposite corner
drawing-hint-break-first = Break view: click the first break line on a view
drawing-hint-break-second = Break view: click the second break line
drawing-hint-boundary-start = Click the boundary's points on a view
drawing-hint-crop-spline-points = Crop view: click the boundary's points · Enter or the first point closes it
drawing-hint-broken-out-points = Broken-out section: click the boundary's points · Enter or the first point closes it

## crates/cadrs_app/src/drawing/views.rs
# Shown in place of a drawing view that couldn't be made; { $error } is why.
drawing-view-failed = View failed: { $error }
drawing-view-generating = Generating view…
# A named view direction (looking at the front).
drawing-named-view-front = Front
drawing-named-view-back = Back
drawing-named-view-top = Top
drawing-named-view-bottom = Bottom
drawing-named-view-left = Left
drawing-named-view-right = Right

## crates/cadrs_app/src/drawing/view_tools.rs
# { $name } is a sheet metal part's name.
drawing-flat-pattern-of = Flat pattern of { $name }
# The panel that inserts a drawing view.
drawing-insert-view = Insert view
drawing-close-esc = Close (Esc)
drawing-select-part-or-assembly = Select a part or assembly
drawing-studio-referenced-tooltip = A Part Studio is referenced, not a part
drawing-named-positions-option = Named positions and exploded views (assemblies)
drawing-named-positions-tooltip = Named positions and exploded views come with assemblies
drawing-studio-referenced-warning = A whole Part Studio is referenced. Its properties differ from a part's, so the title block and notes won't show a part's name. Expand the studio and pick the part.
drawing-click-sheet-to-place-view = Click on the sheet to place the view.
# A tab: this document's parts.
drawing-tab-current-document = Current document
# A tab: other documents' parts.
drawing-tab-other-documents = Other documents
drawing-search-parts-or-sketches = Search parts or sketches
drawing-sketches-come-later = Sketches: views of sketches come later
drawing-no-assemblies = No assemblies in this document
drawing-hint-insert-select = Select a part or assembly to insert
drawing-hint-insert-place = Click on the sheet to place the view
drawing-hint-projected-pick = Projected view: click a view to project from
drawing-hint-projected-place = Projected view: move away from the view and click to place · Esc to finish
drawing-hint-auxiliary-pick = Auxiliary view: click a straight edge of a view
drawing-hint-auxiliary-place = Auxiliary view: click to place the view
drawing-hint-align-vertical = Align view vertical: click an edge of the view
drawing-hint-align-horizontal = Align view horizontal: click an edge of the view

## crates/cadrs_app/src/drawing/flat_views.rs
# A filter: sheet metal parts' flat patterns (the part unfolded).
drawing-flat-patterns = Flat patterns
drawing-no-flat-patterns = No sheet metal flat patterns in this document
# Stands in for a tab's name when it isn't known (inside "Switch to …").
drawing-view-menu-the-referenced-tab = the referenced tab
# A submenu: what a flat pattern view shows.
drawing-flat-menu-show-hide = Show/hide
drawing-flat-show-bend-lines = Show bend lines
drawing-flat-hide-bend-lines = Hide bend lines
drawing-flat-show-bend-notes = Show bend notes
drawing-flat-hide-bend-notes = Hide bend notes
drawing-view-menu-hide-hidden-lines = Hide hidden lines
drawing-view-menu-show-hidden-lines = Show hidden lines
drawing-flat-menu-view-orientation = View orientation
drawing-flat-rotate-ccw = Rotate 90° counterclockwise
drawing-flat-rotate-cw = Rotate 90° clockwise
# How a drawing view draws the edges where faces meet smoothly.
drawing-view-menu-tangent-edges = Tangent edges
# Tangent edges: not drawn.
drawing-view-menu-tangent-hidden = Hidden
# Tangent edges: drawn as solid lines.
drawing-view-menu-tangent-solid = Solid
# Tangent edges: drawn as phantom (dash-dot) lines.
drawing-view-menu-tangent-phantom = Phantom
drawing-flat-adjust-linestyle = Adjust linestyle…
drawing-view-menu-properties = View properties…
# A submenu: which view is drawn over which.
drawing-flat-menu-order = Order
drawing-view-menu-bring-to-front = Bring to front
drawing-view-menu-send-to-back = Send to back
drawing-flat-menu-align-view = Align view
drawing-view-menu-align-vertical = Align view vertical
drawing-view-menu-align-horizontal = Align view horizontal
drawing-view-menu-move-to-sheet = Move to sheet…
drawing-color-black = Black
drawing-color-red = Red
drawing-color-blue = Blue
drawing-color-green = Green
drawing-color-orange = Orange
drawing-color-grey = Grey
# The sheet metal flat pattern section of View properties.
drawing-flat-pattern = Flat pattern
# Bend lines of bends that go up.
drawing-flat-up-bend-lines = Up bend lines
# Bend lines of bends that go down.
drawing-flat-down-bend-lines = Down bend lines

## crates/cadrs_app/src/drawing/view_menu.rs
drawing-view-menu-hide-shaded = Hide shaded view
drawing-view-menu-show-shaded = Show shaded view
drawing-view-menu-hide-threads = Hide threads
drawing-view-menu-show-threads = Show threads
drawing-view-menu-hide-intersections = Hide part intersections
drawing-view-menu-show-intersections = Show part intersections
# Which parts an assembly view shows (not available yet).
drawing-view-menu-display-state = Display state
drawing-view-menu-create-projected = Create projected view
# BOM is short for bill of materials; { $name } is the assembly the view shows.
drawing-view-menu-insert-bom-for = Insert BOM for { $name }…
drawing-view-menu-restore-alignment = Restore alignment with parent
drawing-view-menu-suppress-alignment = Suppress alignment with parent
drawing-view-menu-show-hide-sketches = Show/hide sketches…
drawing-view-menu-remove-crop = Remove crop
drawing-view-menu-remove-breaks = Remove breaks
drawing-view-menu-remove-broken-out = Remove broken-out section
# { $name } is the drawing view's name.
drawing-view-properties-title = View properties: { $name }
# What the view shows (a part or an assembly), a field's label.
drawing-view-props-reference = Reference
# A drawing scale (e.g. 1:2), a field's label.
drawing-scale = Scale
drawing-view-props-inherited = Inherited from the parent view
# A drawing sheet, a field's label.
drawing-sheet = Sheet
# { $name } is the drawing view's name.
drawing-move-view-to-sheet-title = Move { $name } view to sheet
drawing-move-to-sheet-help = The view's annotations move with it; its projected views stay where they are.
drawing-show-hide-sketches = Show/hide sketches
drawing-search-sketches = Search sketches
drawing-no-sketches = The referenced Part Studio has no sketches

## crates/cadrs_app/src/drawing/view_linked.rs
drawing-version-graph-tooltip = Version graph: views of the workspace or of a version

## crates/cadrs_app/src/drawing/update.rs
# Ctrl+Q is the shortcut.
drawing-update-updating = Updating from this workspace… (Ctrl+Q)
drawing-update-up-to-date = Update from this workspace: the drawing is up to date (Ctrl+Q)
# BOM is short for bill of materials.
drawing-update-one-bom-table = 1 BOM table
# { $count } is how many BOM tables (2 or more).
drawing-update-bom-tables = { $count } BOM tables
drawing-update-one-view = 1 view
# { $count } is how many drawing views (2 or more).
drawing-update-views = { $count } views
# { $tables } is how many BOM tables (e.g. "2 BOM tables").
drawing-update-tables-stale = Update from this workspace: { $tables } out of date (Ctrl+Q)
# { $views } is how many views (e.g. "2 views"), { $tables } how many BOM tables (e.g. "1 BOM table").
drawing-update-views-tables-stale = Update from this workspace: { $views }, { $tables } out of date (Ctrl+Q)
drawing-update-one-view-stale = Update from this workspace: 1 view is out of date (Ctrl+Q)
# { $count } is how many views are out of date (2 or more).
drawing-update-views-stale = Update from this workspace: { $count } views are out of date (Ctrl+Q)

## crates/cadrs_app/src/drawing/toolbar.rs
# Ctrl+Q is the shortcut.
drawing-tool-update = Update from this workspace (Ctrl+Q)
# A view projected from another (to its side, above or below).
drawing-tool-projected-view = Projected view
# A view projected square to a slanted edge.
drawing-tool-auxiliary-view = Auxiliary view
# A drawing view cut through the part.
drawing-tool-section-view = Section view
# D is the shortcut.
drawing-tool-dimension-shortcut = Dimension (D)
drawing-tool-hole-callout = Hole callout
drawing-tool-surface-finish-symbol = Surface finish symbol
# GD&T: geometric dimensioning and tolerancing.
drawing-tool-geometric-tolerance = Geometric tolerance
# N is the shortcut.
drawing-tool-note-shortcut = Note (N)
# A balloon with an item's number, pointing at a part.
drawing-tool-callout = Callout
drawing-tool-table = Table
# BOM is short for bill of materials.
drawing-tool-insert-bom = Insert BOM
drawing-tool-centerline = Centerline
drawing-tool-centermark = Centermark
# The point where two edges would meet if extended.
drawing-tool-virtual-sharp = Virtual sharp
# A tool that draws a line on the sheet.
drawing-tool-line = Line
# A tool that draws a spline (a smooth curve) on the sheet.
drawing-tool-spline = Spline
drawing-tool-insert-dxf = Insert DXF or DWG
drawing-tool-insert-image = Insert image
drawing-tool-not-yet-update = updating views from the workspace arrives in a later version
drawing-tool-not-yet-sketching = sheet sketching and import arrive in a later version
drawing-tool-not-yet = not available yet
# A disabled tool's tooltip: { $tool } is its name, { $why } why it is disabled.
drawing-tool-disabled-tooltip = { $tool }: { $why }

## crates/cadrs_app/src/drawing/bom_tools.rs
# A BOM table type: the assembly's own items only, subassemblies as one row each.
drawing-bom-type-top-level = Structured - Top level
# A BOM table type: subassemblies with their items nested under them.
drawing-bom-type-multi-level = Structured - Multi level
# A BOM table's row order.
drawing-bom-order-top-to-bottom = Top to bottom
drawing-bom-order-bottom-to-top = Bottom to top
# A callout's border shape.
drawing-callout-border-circle = Circle
# A callout's border shape: a line under the text.
drawing-callout-border-underline = Underline
# A callout's border shape.
drawing-callout-border-box = Box
# A callout's border shape.
drawing-callout-border-triangle = Triangle
# A callout border's size: tight around its text.
drawing-callout-size-tight-fit = Tight Fit
# A callout border's size: wide enough for one character.
drawing-callout-size-one-character = 1 Character
# A callout border's size: wide enough for { $count } characters.
drawing-callout-size-characters = { $count } Characters
# BOM is short for bill of materials; a field's label.
drawing-bom-type = BOM type
# A field's label: the order of a BOM table's rows.
drawing-bom-order = Order
drawing-bom-place-hint = Click the sheet to place the table; it snaps to the border's corner or the title block.
drawing-bom-table-properties = BOM Table properties
# A callout's text field above its middle (a placeholder).
drawing-callout-upper = Upper
# A callout's text field below its middle (a placeholder).
drawing-callout-lower = Lower
# A callout's text field at its left (a placeholder).
drawing-callout-left = Left
# A callout's text field at its right (a placeholder).
drawing-callout-right = Right
# A callout's middle text field (a placeholder).
drawing-callout-center = Center
drawing-callout-insert-component-property = Insert component property
drawing-callout-insert-table-property = Insert BOM table property
drawing-callout-hint = Pick an edge of a part, then click to place
# A menu item: { $property } is a part property (Name, Part number…) to insert.
drawing-callout-part-property = Part: { $property }
# A menu item: { $property } is a BOM table column (Item No., Qty.) to insert.
drawing-callout-table-property = Table: { $property }

## crates/cadrs_app/src/drawing/note_bar.rs
# A card's ✓ button (accepts the edit).
drawing-accept = Accept
drawing-cell-merge-why = select two or more cells
drawing-cell-unmerge-why = select a merged cell
# The note editor's title: a text note on the sheet.
drawing-note = Note
# The note editor's title: a table cell being edited.
drawing-cell = Cell
# Ctrl+B is the shortcut.
drawing-note-bold-shortcut = Bold (Ctrl+B)
# Ctrl+I is the shortcut.
drawing-note-italic-shortcut = Italic (Ctrl+I)
# Ctrl+U is the shortcut.
drawing-note-underline-shortcut = Underline (Ctrl+U)
drawing-note-strikethrough = Strikethrough
# Text alignment.
drawing-align-left = Align left
# Text alignment: centred.
drawing-align-center = Center
# Text alignment.
drawing-align-right = Align right
drawing-note-bulleted-list = Bulleted list
drawing-note-numbered-list = Numbered list
drawing-note-text-height-in = Text height (in)
drawing-note-text-height-mm = Text height (mm)
drawing-note-insert-symbol = Insert symbol
drawing-note-insert-reference-property = Insert sheet reference property
drawing-note-insert-drawing-property = Insert drawing property
# The diameter symbol Ø.
drawing-symbol-diameter = Diameter
# The degree symbol °.
drawing-symbol-degree = Degree
# The plus/minus symbol ±.
drawing-symbol-plus-minus = Plus/minus
# The counterbore symbol ⌴.
drawing-symbol-counterbore = Counterbore
# The countersink symbol ⌵.
drawing-symbol-countersink = Countersink
# The depth symbol ↧.
drawing-symbol-depth = Depth
# The multiplication symbol ×.
drawing-symbol-times = Times
# The symbol ≈.
drawing-symbol-approximately = Approximately
# The symbol ≤.
drawing-symbol-less-or-equal = Less than or equal
# The symbol ≥.
drawing-symbol-greater-or-equal = Greater than or equal
# The symbol Δ.
drawing-symbol-delta = Delta
# The symbol π.
drawing-symbol-pi = Pi
# A field's label: the property inserted into the note.
drawing-note-property = Property
# A date format: the drawing's own.
drawing-note-date-drawing-default = Drawing default
# A field's label: the date's format.
drawing-note-date = Date
# Before a preview of the inserted property's value.
drawing-note-shows = Shows
drawing-cell-insert-row-above = Insert row above
drawing-cell-insert-row-below = Insert row below
drawing-cell-insert-column-left = Insert column left
drawing-cell-insert-column-right = Insert column right
drawing-cell-delete-row = Delete row
drawing-cell-delete-column = Delete column
drawing-cell-merge = Merge cells
drawing-cell-unmerge = Unmerge cell
drawing-bold = Bold
drawing-italic = Italic
drawing-underline = Underline
drawing-select-fixed-corner = Select fixed corner:
# The corner of a table that stays put when it grows.
drawing-corner-top-left = Top left
drawing-corner-top-right = Top right
drawing-corner-bottom-left = Bottom left
drawing-corner-bottom-right = Bottom right
# How many rows the new table has (a field's label).
drawing-table-rows = Rows
# How many columns the new table has (a field's label).
drawing-table-columns = Columns
drawing-table-title-row = Title row
drawing-table-header-row = Header row
drawing-table-revision-table = Revision table
drawing-table-properties = Table properties

## crates/cadrs_app/src/drawing/notes.rs
# Menu: adds a leader (an arrow line) to a note.
drawing-note-add-leader = Add leader
drawing-table-edit-cell = Edit cell…
drawing-table-properties-ellipsis = Table properties…
drawing-bom-table-properties-ellipsis = BOM Table properties…

## crates/cadrs_app/src/drawing/create_dialog.rs
# Drawing templates that come with cadrs.
drawing-template-source-builtin = Built-in
# Drawing templates used in this document.
drawing-template-source-document = This document
drawing-template-source-mine = My templates
drawing-template-source-recent = Recently used
# A sheet orientation.
drawing-orientation-landscape = Landscape
# A sheet orientation.
drawing-orientation-portrait = Portrait
# A projection method (views placed as in Europe).
drawing-projection-first-angle = First angle
# A projection method (views placed as in North America).
drawing-projection-third-angle = Third angle
drawing-units-inches = Inches
drawing-units-millimeters = Millimeters
# { $name } is the new drawing's name.
drawing-create-title = Create Drawing: { $name }
# { $name } is the new drawing's name, { $part } what it shows.
drawing-create-title-of = Create Drawing: { $name } of { $part }
drawing-create-tab-existing = Existing templates
drawing-create-tab-custom = Custom template
# A filter: templates of every standard.
drawing-template-filter-all = All
# A drawing template (a column's title, a field's label).
drawing-template = Template
# The document a template comes from (a column's title).
drawing-template-document = Document
# Who made a template (a column's title).
drawing-template-owner = Owner
drawing-no-templates = No templates here yet
# The drafting standard (ANSI or ISO), a field's label.
drawing-standard = Standard
# A sheet's orientation (landscape or portrait), a field's label.
drawing-orientation = Orientation
# The projection method (first or third angle), a field's label.
drawing-projection = Projection
# { $name } is the custom template's name.
drawing-template-named = Template: { $name }
drawing-custom-template-help = OK creates the drawing and keeps this template in My templates.
drawing-options = Options
drawing-option-four-views = Four views
drawing-option-no-views = No views
drawing-option-no-views-tooltip = Start with an empty sheet and insert views yourself
drawing-option-four-views-tooltip = Front, top, side and isometric views of the referenced object
drawing-option-four-views-disabled = Four views needs a referenced part: use Create Drawing of… on its tab
drawing-select-template-first = Select a template first
drawing-update-from-template-title = Update properties from a template
drawing-update-from-template-help = Replaces the drawing properties, units and projection with the template's.

## crates/cadrs_app/src/drawing/sheet_dialog.rs
drawing-sheet-properties-ellipsis = Sheet properties…
drawing-drawing-properties-ellipsis = Drawing properties…
drawing-insert-view-ellipsis = Insert view…
# { $name } is the sheet's name.
drawing-sheet-properties-title = Sheet properties: { $name }
drawing-sheet-border-and-zones = Border and zones
drawing-sheet-show-border = Show border
# Zones: the lettered and numbered grid along a sheet's border.
drawing-sheet-show-zones = Show zones
drawing-sheet-show-title-block = Show title block
# The part or assembly whose properties the sheet's title block shows.
drawing-sheet-referenced-object = Referenced object
drawing-sheet-reference-help = The title block and parametric notes read this part's or assembly's properties.

## crates/cadrs_app/src/drawing/sheet_items.rs
# An error: the picked file isn't an image cadrs can read.
drawing-not-an-image = not a PNG or JPEG image
# An error: the picked file's kind isn't one the sheet can insert.
drawing-unknown-import = unknown import
# { $file } is the file's path, { $error } why it can't be inserted.
drawing-cannot-insert = Cannot insert { $file }: { $error }

## crates/cadrs_app/src/drawing/symbol_cards.rs
# A GD&T material condition modifier, its symbol (M) in brackets.
drawing-gdt-modifier-mmc = Maximum material (M)
# A GD&T material condition modifier, its symbol (L) in brackets.
drawing-gdt-modifier-lmc = Least material (L)
# A GD&T material condition modifier, its symbol (S) in brackets.
drawing-gdt-modifier-rfs = Regardless of size (S)
# A field's label: the geometric characteristic's symbol (position, flatness…).
drawing-gdt-symbol = Symbol
# A field's label: the tolerance value.
drawing-gdt-tolerance = Tolerance
# A checkbox: the tolerance zone is a diameter (Ø).
drawing-gdt-diameter = Diameter (Ø)
# A field's label: the material condition modifier.
drawing-gdt-modifier = Modifier
# A field's label; { $number } is the datum reference's number (1 to 3).
drawing-gdt-datum-n = Datum { $number }
# A field's label: a datum feature's letter (A, B…).
drawing-datum-letter = Letter
# A weld symbol's side: the side the arrow points to.
drawing-weld-arrow-side = Arrow side
# A weld symbol's side: the other side of the joint.
drawing-weld-other-side = Other side
# A weld symbol option: the weld goes all around the joint.
drawing-weld-all-around = All around
drawing-symbol-card-hint = Pick an edge, then click to place

## crates/cadrs_app/src/drawing/dim_palette.rs
# The palette of a dimension's format (prefix, tolerance, precision…).
drawing-dim-palette = Dimension palette
# A tolerance type: plus and minus the same value (±).
drawing-dim-tolerance-symmetric = Symmetric
# A tolerance type: different upper and lower deviations.
drawing-dim-tolerance-deviation = Deviation
# A tolerance type: the upper and lower limit values.
drawing-dim-tolerance-limits = Limits
# Text before a dimension's value.
drawing-dim-prefix = Prefix
# Text after a dimension's value.
drawing-dim-suffix = Suffix
drawing-dim-symbols = Symbols
# The upper tolerance value.
drawing-dim-upper = Upper
# The lower tolerance value.
drawing-dim-lower = Lower
# How many decimals a dimension shows.
drawing-dim-precision = Precision
# A precision choice: the drawing's own.
drawing-dim-precision-default = Default
# A checkbox; { $unit } is the second unit's symbol (mm or in).
drawing-dim-dual-units = Dual units ({ $unit })
# Enter is the key.
drawing-accept-enter = Accept (Enter)

## crates/cadrs_app/src/drawing/export_dialog.rs
# LibreDWG and the ODA File Converter are programs; PATH is where programs are found.
drawing-export-dwg-note = DWG and DWT need LibreDWG or the ODA File Converter on PATH.
drawing-export-pdf-tooltip = Vector PDF, a page per sheet at its paper size
drawing-export-dxf-tooltip = AutoCAD R2013 DXF in millimetres, a file per sheet
# { $converter } is the converter program's name (LibreDWG, ODA File Converter).
drawing-export-dwg-tooltip = DWG through the { $converter } (a file per sheet)
drawing-export-dwg-no-converter = DWG: no converter on PATH
# { $converter } is the converter program's name (LibreDWG, ODA File Converter).
drawing-export-dwt-tooltip = AutoCAD template (DWG format) through the { $converter }
drawing-export-dwt-no-converter = DWT: no converter on PATH
drawing-export-png-tooltip = PNG image, a file per sheet
drawing-export-jpeg-tooltip = JPEG image, a file per sheet
# Which sheets to export (a field's label).
drawing-export-sheets = Sheets
# { $count } is how many sheets the drawing has.
drawing-export-all-sheets = All sheets ({ $count })
# { $name } is the current sheet's name.
drawing-export-current-sheet = Current sheet ({ $name })
# A field's label: in colour or black and white.
drawing-export-colors = Colors
# An export option: in colour.
drawing-export-color = Color
drawing-export-black-and-white = Black and white
# An image export's resolution (dpi), a field's label.
drawing-export-resolution = Resolution
# { $count } is how many files were written, { $folder } where.
drawing-export-exported-files = Exported { $count } files to { $folder }
# An error: no folder was given.
drawing-export-no-folder = no folder to save to
# An error: no document is open.
drawing-export-no-document = no document
# An error: the drawing was deleted.
drawing-export-drawing-gone = the drawing is gone
# An error: the tab isn't a drawing.
drawing-export-not-a-drawing = not a drawing

## crates/cadrs_app/src/drawing/panels.rs
# Ctrl+S is the shortcut.
drawing-sheets-toggle-tooltip = Sheets (Ctrl+S)
drawing-drawing-properties = Drawing properties
# The sheets list's heading; { $count } is how many sheets the drawing has.
drawing-sheets-count = Sheets({ $count })
drawing-insert-sheet = Insert sheet
# { $name } is a tab's name in this document.
drawing-this-document-named = { $name } (this document)
# Menu: makes this sheet the shown one (a verb).
drawing-sheet-activate = Activate
drawing-update-from-template-ellipsis = Update properties from a template…
drawing-update-from-template-tooltip = Replace the drawing properties with a template's
drawing-lock-properties = Lock drawing properties

## crates/cadrs_app/src/material_dialog.rs
render-material-select = Select a material
# { $unit } is the density's unit (e.g. g/cm³).
render-material-density = Density ({ $unit })
render-material-poisson = Poisson's ratio
# { $unit } is the stress unit (MPa or Psi).
render-material-youngs = Young's modulus ({ $unit })
# { $unit } is the stress unit (MPa or Psi).
render-material-tensile-yield = Tensile yield strength ({ $unit })
# { $unit } is the stress unit (MPa or Psi).
render-material-ultimate-tensile = Ultimate tensile strength ({ $unit })
# { $unit } is the stress unit (MPa or Psi).
render-material-compressive-yield = Compressive yield strength ({ $unit })
# { $unit } is the stress unit (MPa or Psi).
render-material-ultimate-compressive = Ultimate compressive strength ({ $unit })
render-material-none-yet = No materials yet: add one in the Custom tab
render-material-none-match = No materials match
render-material-choose = Choose a material
# A tab: the material libraries.
render-material-tab-library = Library
# A tab: a material of the user's own.
render-material-tab-custom = Custom
render-material-create-library = Create a material library
render-material-search = Search materials
# { $library } is a material library's name.
render-material-add-to = Add to { $library }
render-material-remove = Remove material

## crates/cadrs_app/src/render_ui.rs
render-error-no-source = Choose a Part Studio or an Assembly to render.
render-error-model-unavailable = The model isn't available.
render-error-no-parts = The model has no parts to render.
# { $file } is the saved image's path, { $width } × { $height } its size in pixels.
render-rendered = Rendered { $file } ({ $width } × { $height })
# { $error } is what went wrong.
render-failed-error = Render failed: { $error }
render-failed = Render failed
# { $width } × { $height } is the image size in pixels; { $done } of { $total } samples are done.
render-caption-rendering = Rendering { $width } × { $height }: { $done } of { $total } samples
# { $width } × { $height } is the image size in pixels, { $samples } how many samples, { $seconds } how long it took.
render-caption-result = Render result: { $width } × { $height }, { $samples } samples, { $seconds } s
# { $total } is how many samples the preview took.
render-caption-preview-done = Preview: { $total } samples, denoised
# { $done } of { $total } samples are done.
render-caption-preview = Preview: { $done } of { $total } samples
render-render-ellipsis = Render…
render-render-tooltip = Render the scene to a PNG
render-capture-tooltip = Use the model's current view as the camera
# { $file } is the image file, { $width } × { $height } its size in pixels, { $samples } how many samples, { $seconds } how long it took.
render-result-line = { $file }: { $width } × { $height }, { $samples } samples, { $seconds } s
# The tab whose model is rendered (a section heading and a field's label).
render-model = Model
render-choose-tab = Choose a tab…
# The lighting around the scene (a section heading and a field's label).
render-environment = Environment
# What is behind the model in the image (a field's label).
render-background = Background
render-ground-shadow = Ground shadow
render-camera = Camera
# The view as it is in the 3D view now.
render-current-view = Current view
# The camera's view direction (a field's label).
render-view = View
# A camera projection with depth (not orthographic).
render-perspective = Perspective
# The rendered image's settings (a section heading).
render-output = Output
# { $count } is how many samples (render passes) per pixel.
render-samples = { $count } samples
# How many samples the render takes (a field's label).
render-quality = Quality
# The image's brightness (a field's label).
render-exposure = Exposure
# The random seed of the render (a field's label).
render-seed = Seed
# A checkbox: removes the render's noise.
render-denoise = Denoise
render-materials = Materials
render-no-parts = No parts.
render-rendering = Rendering…
# { $done } of { $total } samples are done, { $percent } the percentage.
render-progress-paused = Paused at { $done } of { $total } samples ({ $percent } %)
# { $done } of { $total } samples are done, { $percent } the percentage.
render-progress = Rendering… { $done } of { $total } samples ({ $percent } %)
# The Render dialog's title and its button (a verb).
render-render = Render
render-dialog-png-help = A PNG image, the same for the same seed.
render-nowhere-to-save = Nowhere to save the render

## crates/cadrs_app/src/section_view.rs
# A section view's plane row; { $number } is its number, { $plane } the plane it cuts along (e.g. "Right plane").
render-section-plane-row = Section plane { $number } ({ $plane })
# The default plane named Top (a choice in a list).
render-section-plane-top = Top
# The default plane named Front (a choice in a list).
render-section-plane-front = Front
# The default plane named Right (a choice in a list).
render-section-plane-right = Right
render-section-top-plane = Top plane
render-section-front-plane = Front plane
render-section-right-plane = Right plane
# A section view mode: the picked items are left out of the cut.
render-section-exclude = Exclude
# A section view mode: only the picked items are cut.
render-section-include = Include
render-section-planes = Section planes
render-section-default-plane-tooltip = Section by a default plane: Top, Front or Right
render-section-flip-side = Flip the side removed
render-section-add-plane = Add the section plane
render-section-items-exclude = Items to exclude
render-section-items-include = Items to include
# { $axis } is the axis the plane is tilted about (X, −Y…).
render-section-tilt-1 = Tilt 1 ({ $axis })
# { $axis } is the axis the plane is tilted about (X, −Y…).
render-section-tilt-2 = Tilt 2 ({ $axis })
# Explains the tilt gizmo's colours (\n is a line break).
render-section-tilt-help = Blue: tilt 1 · Orange: tilt 2\nLocal axes · order: tilt 1, then tilt 2

## crates/cadrs_app/src/cam/compute.rs
# The status shown at the end of a CAM operation's row in the tree (lowercase).
cam-status-not-calculated = not calculated
cam-status-out-of-date = out of date
cam-status-calculating = calculating…
cam-status-failed = failed
# Why a toolpath couldn't be calculated (a machining strategy crashed).
cam-strategy-failed-unexpectedly = The strategy failed unexpectedly
cam-strategy-stopped-unexpectedly = The strategy stopped unexpectedly

## crates/cadrs_app/src/cam/dialogs/fields.rs
# The world coordinate system (a work plane's origin, a stock's direction).
cam-world = World
# A work plane origin option: a point of the box round the parts to machine.
cam-origin-component-box-point = Component box point
# A work plane origin option: a point of the stock's box.
cam-origin-stock-box-point = Stock box point
# Set by hand (an origin typed in, a machine mating offset), not worked out.
cam-manual = Manual
# Worked out by the app (a datum, a machine mating), not set by hand.
cam-automatic = Automatic
# Where along one axis of a box a box point is.
cam-box-minimum = Minimum
cam-box-middle = Middle
cam-box-maximum = Maximum
# Section heading in a CAM setup's or work plane's dialog.
cam-section-origin = Origin
cam-work-plane-origin = Work plane origin
# { $axis } is an axis letter (X, Y or Z).
cam-position-axis = Position { $axis }
# Section heading: which way the work plane's axes point.
cam-section-orientation = Orientation
# { $axis } is an axis direction such as +Z or −X.
cam-z-along = Z along { $axis }
# { $axis } is an axis direction such as +Z or −X.
cam-x-along = X along { $axis }
cam-normal-of-picked-face = Normal of a picked face
cam-tool-axis-z = Tool axis (Z)
cam-planar-face-for-z = Planar face for Z
cam-x-direction = X direction
# The NC datum (G54…) a work plane is written with, instead of the automatic one.
cam-override-datum = Override datum
cam-show-work-coordinate-system = Show work coordinate system
# Dialog titles for inserting a component into a CAM studio.
cam-insert-part = Insert part
cam-insert-workholding = Insert workholding
cam-insert-stock = Insert stock
# The machine tool (mill, lathe, laser…) a CAM job runs on.
cam-machine = Machine
cam-tick-a-part = Tick a part
cam-job-needs-a-name = The job needs a name
cam-pick-bodies-to-machine = Pick the bodies to machine (or insert a part)
cam-no-tool-above-toolpath = No tool above the toolpath
cam-pick-edges-to-engrave = Pick the edges to engrave
cam-pick-faces-to-fill = Pick the faces to fill
# The role a component is inserted with (part, workholding, stock).
cam-insert-as = Insert as
cam-this-document = This document
cam-document = Document
cam-insert-other-document-note = Parts from another document come in at a version: Update brings a newer one.
# The document's workspace, as opposed to one of its versions.
cam-workspace-follows-edits = Workspace (follows edits)
# { $name } is a version's name.
cam-version-named = Version { $name }
# Which version of the parts a component refers to.
cam-reference = Reference
cam-insert-workspace-note = A version keeps the part as it is; the workspace follows every edit.
cam-part-studio-has-no-parts = The Part Studio has no parts
# The role a component has (part, workholding, stock).
cam-used-as = Used as
# { $reference } is the source the component comes from: a Part Studio's name and a version (or "workspace").
cam-component-reference = Reference: { $reference }
cam-setup-name = Setup name
cam-program-number = Program number
# How the setup's coordinates are placed on the machine.
cam-machine-mating = Machine mating
cam-offset-x = Offset X
cam-offset-y = Offset Y
cam-offset-z = Offset Z
cam-cutting-machine-lay-flat = A cutting machine cuts down its Z: lay the sheet flat.
cam-job-name = Job name
cam-bodies-all-when-empty = Bodies (all parts when empty)
# The raw material a part is machined from.
cam-stock = Stock
# A stock shape.
cam-box = Box
# A shape (a stock's, a tool holder's).
cam-cylinder = Cylinder
# A stock that is a component the user inserted.
cam-user-defined = User defined
cam-stock-type = Stock type
cam-insert-stock-component-first = Insert a stock component first
cam-stock-body = Stock body
cam-stock-direction = Stock direction
cam-radial-offset = Radial offset
# How far the stock grows past the parts in each direction.
cam-offset-minus-x = Offset −X
cam-offset-plus-x = Offset +X
cam-offset-minus-y = Offset −Y
cam-offset-plus-y = Offset +Y
cam-offset-minus-z = Offset −Z
cam-offset-plus-z = Offset +Z
# What holds the part on the machine (a vice, clamps).
cam-workholding = Workholding
cam-workholding-bodies = Workholding bodies
cam-group-by-machine-type = Group by machine type
# { $kind } is the kind of machine (mill, laser…), { $controller } the NC dialect it speaks.
cam-machine-kind-controller = { $kind } · { $controller } controller
# { $axis } is an axis letter (X, Y or Z): how far the machine moves along it.
cam-axis-travel = { $axis } travel
cam-rapid-rate = Rapid rate
cam-fastest-feed = Fastest feed
cam-fastest-spindle = Fastest spindle
cam-automatic-tool-changer = Automatic tool changer
cam-indexing-rotary-axes = Indexing rotary axes (3+2)
cam-largest-tilt = Largest tilt
cam-rotary-axes = Rotary axes
cam-rotary-a-and-c = A and C (trunnion)
cam-rotary-b-and-c = B and C (trunnion)
cam-tilted-planes-fanuc-only = Tilted work planes are posted as Fanuc's G68.2 only
# The spindle speed value (S) that means full laser power.
cam-s-at-full-power = S at full power
# The post processor's settings (how the NC program is written).
cam-post-settings = Post settings
cam-output-unit = Output unit
cam-millimetres = Millimetres
cam-inches = Inches
cam-advanced-work-planes = Advanced work planes (own datums)
# Line numbers (N10, N20…) in the NC program.
cam-sequence-numbers = Sequence numbers
cam-block-number-start = Block number start
cam-block-increment = Block increment
cam-numbers-at-tool-changes-only = Numbers at tool changes only
cam-rapid-positioning = Rapid positioning (G0)
cam-rapid-feed = Rapid feed (G1)
cam-output-arcs = Output arcs (G2/G3)
cam-output-helices = Output helices
cam-decimal-places = Decimal places
cam-always-write-decimal-point = Always write a decimal point
cam-retract-on-tool-changes = Retract on tool changes
cam-retract-on-plane-changes = Retract on work plane changes
cam-retract-at-program-end = Retract at the program end
# Section heading: the machine's canned (fixed) drilling cycles.
cam-fixed-cycles = Fixed cycles
cam-cycles-drilling = Drilling canned cycles
cam-cycles-peck-drilling = Peck drilling canned cycles
cam-cycles-tapping = Thread tapping canned cycles
cam-cycles-chip-break = Chip break canned cycles
cam-laser-power = Laser power
cam-laser-dynamic = Dynamic (M4)
cam-laser-constant = Constant (M3)
cam-comments-in-program = Comments in the program
cam-from-tool-library = From the tool library
# The first option of the machining type filter: every type.
cam-all-types = All
cam-machining-type = Machining type
cam-no-tool-of-this-type = No tool of this type for this machine yet
cam-choose-a-tool = Choose a tool…
# A cutting tool (an end mill, a drill…).
cam-tool = Tool
cam-import-tool-library = Import tool library…
cam-export-tool-library = Export tool library…
# Section heading: the cutting tool's own shape.
cam-cutter = Cutter
cam-cutter-type = Cutter type
# The width of material a laser, plasma or waterjet beam removes.
cam-kerf-width = Kerf width
# The unit a tool's sizes are given in.
cam-unit = Unit
# Which way the tool turns (right- or left-handed).
cam-tool-hand = Tool hand
cam-hand-right = Right
cam-hand-left = Left
cam-diameter = Diameter
cam-corner-radius = Corner radius
cam-included-angle = Included angle
cam-tip-diameter = Tip diameter
# A drill's point angle.
cam-point-angle = Point angle
cam-thread-pitch = Thread pitch
cam-cutting-length = Cutting length
cam-overall-length = Overall length
cam-flutes = Flutes
cam-center-cutting = Center cutting
# A laser, plasma or waterjet beam (section heading and tab).
cam-beam = Beam
# A beam's power (a percentage).
cam-power = Power
cam-pierce-delay = Pierce delay
cam-pierce-height = Pierce height
cam-torch-height-control = Torch height control
cam-material-number = Material number (0: as set)
cam-cut-height = Cut height
cam-air-assist = Air assist
# How many times the beam goes round.
cam-passes = Passes
# Section heading: speeds and feeds.
cam-cutting-data = Cutting data
cam-set-by-hand = Set by hand
# The work material a tool's speeds and feeds are worked out for.
cam-worked-out-for = Worked out for
cam-spindle-speed = Spindle speed
cam-feed-rate = Feed rate
cam-plunge-rate = Plunge rate
cam-step-down = Step down
cam-step-over = Step over
# What holds the cutting tool in the spindle.
cam-holder = Holder
cam-holder-diameter = Holder diameter
cam-holder-length = Holder length
# Section heading: the tool assembly (cutter and holder) and its T number.
cam-tool-assembly = Assembly
cam-assembly-number = Assembly number (T)
# Tabs of the toolpath dialog.
cam-tab-tool-path = Tool path
# The moves that link cuts (entries, retracts, leads).
cam-tab-link = Link
cam-tab-axis = Axis
cam-tab-collision = Collision
# Miscellaneous.
cam-tab-misc = Misc
cam-quality = Quality
cam-cut-tolerance = Cut tolerance
cam-smoothing = Smoothing
cam-smooth-corners = Smooth corners
cam-template = Template
cam-save-as-template = Save as template
cam-depth-per-pass = Depth per pass
cam-microjoints-per-part = Microjoints per part
cam-microjoint-width = Microjoint width
cam-small-holes-under = Small holes under
cam-small-hole-feed = Small hole feed

## crates/cadrs_app/src/cam/dialogs/set.rs
# The title of the file picker that imports a tool library.
cam-import-tool-library-title = Import tool library
# { $name } is the template's name (a toolpath's name).
cam-saved-template = Saved the template "{ $name }" to the library
cam-nowhere-to-export = Nowhere to export to
# { $count } is how many tools; { $path } is the file written.
cam-exported-tools = Exported { $count } tools to { $path }
# { $error } is the system's error message.
cam-couldnt-write-tool-library = Couldn't write the tool library: { $error }
# { $path } is the file; { $error } is the system's error message.
cam-couldnt-read-file = Couldn't read { $path }: { $error }
# { $count } is how many tools were added to the library.
cam-imported-tools = Imported { $count } new tools

## crates/cadrs_app/src/cam/dialogs/toolpath_fields.rs
# The direction the cutting tool points along (section heading and field).
cam-tool-axis = Tool axis
cam-work-plane-z = Work plane Z
cam-tool-axis-work-plane-note = The tool cuts along the Z of the work plane above the toolpath (or the setup's).
cam-planar-face-for-tool-axis = Planar face for the tool axis
# { $angle } is a number of degrees (the ° follows it).
cam-tilted-from-setup-z = Tilted { $angle }° from the setup's Z: cut in a tilted working plane (3+2)
cam-machine-no-indexing-axes = The machine has no indexing axes to tilt the tool
cam-machine-cant-tilt-that-far = The machine can't tilt the tool that far
# Section heading of the collision tab: the cutter, its shank and its holder.
cam-tool-assembly-section = Tool assembly
cam-check-shank-and-holder = Check the shank and holder
cam-shank-clearance = Shank clearance
cam-holder-clearance = Holder clearance
# What to do where the shank or holder would hit the part.
cam-where-theyd-hit = Where they'd hit
# { $tool } is the tool's name, { $flutes } and { $stickout } are lengths in mm, { $holder } is the holder's name.
cam-tool-lengths-in-holder = { $tool }: flutes { $flutes } mm, stick-out { $stickout } mm, in { $holder }
# { $tool } is the tool's name, { $flutes } and { $stickout } are lengths in mm.
cam-tool-lengths-no-holder = { $tool }: flutes { $flutes } mm, stick-out { $stickout } mm, no holder
cam-start-from-template = Start from template
# The toolpath's machining pattern (strategy: pocket, contour, adaptive…).
cam-toolpath-pattern = Pattern
cam-sub-pattern = Sub-pattern
# Placeholders of the list of geometry a toolpath machines.
cam-drive-edges = Drive edges (the part's outline when empty)
cam-faces-to-fill = Faces to fill
cam-faces-stock-top = Faces (the stock's top when empty)
cam-floor-faces = Floor faces (all when empty)
cam-faces-to-finish = Faces to finish (all when empty)
cam-drive-holes = Holes (by the cycle's selection when empty)
# { $x } and { $y } are a hole's coordinates.
cam-hole-at = Hole at ({ $x }, { $y })
cam-reverse-direction = Reverse direction
# Which side of the contour the tool cuts on.
cam-side = Side
# Cutter compensation (in the computer or in the control).
cam-compensation = Compensation
cam-wall-stock-to-leave = Wall stock to leave
cam-floor-stock-to-leave = Floor stock to leave
cam-extra-offset = Extra offset
cam-helix-pitch = Helix pitch
cam-first-step-down = First step down (0: same)
# The spacing of a fill's lines.
cam-line-spacing = Line spacing
cam-steep-angle = Steep angle (walls left to Constant Z)
# Section heading: a thread milling toolpath's thread.
cam-thread = Thread
cam-major-diameter = Major diameter (0: from the hole)
cam-radial-passes = Radial passes
cam-left-hand-thread = Left-hand thread
cam-thread-pitch-note = The tool's pitch sets the thread's; climb milling follows the spindle's direction (a left-hand tool turns M4).
# Section heading: the order and direction cuts are made in.
cam-sorting = Sorting
cam-cutting-direction = Cutting direction
# A beam toolpath's direction: which side of the beam the part stays on.
cam-part-on-the-right = Part on the right
cam-part-on-the-left = Part on the left
cam-cutting-method = Cutting method
# What a roughing toolpath works through first (by depth or by region).
cam-machine-by = Machine by
cam-heights = Heights
cam-top-from = Top from
cam-top-offset = Top offset
cam-bottom-from = Bottom from
cam-bottom-offset = Bottom offset
cam-chamfer = Chamfer
cam-chamfer-width = Chamfer width
cam-tip-offset = Tip offset
# Section heading: an engraving toolpath's settings.
cam-engrave = Engrave
# Small bridges left uncut so a cut-out part stays attached.
cam-tabs = Tabs
cam-tabs-per-contour = Tabs per contour
cam-tab-width = Tab width
cam-tab-height = Tab height
# A drilling cycle (section heading and field).
cam-cycle = Cycle
cam-holes = Holes
cam-peck-depth = Peck depth
# How long the tool waits at the bottom of a hole.
cam-dwell = Dwell
cam-retract-between-holes = Retract to clearance between holes
# How far a drill goes past the bottom of a through hole.
cam-break-through = Break through
cam-speeds = Speeds
cam-ramp-rate = Ramp rate
cam-lead-rate = Lead rate
# Section heading: how the toolpath is written in the NC program.
cam-posting = Posting
cam-coolant = Coolant
cam-force-tool-change = Force tool change
cam-stop-before = Stop before (M0)
# A comment written into the NC program.
cam-comment = Comment
cam-rest-machining = Rest machining (what the operations above left)
cam-clearance-area = Clearance area
cam-clearance-above-stock = Clearance above stock
cam-rapid-distance = Rapid distance
cam-feed-distance = Feed distance
cam-gaps = Gaps
cam-small-gaps-up-to = Small gaps up to
cam-small-gaps = Small gaps
cam-large-gaps = Large gaps
cam-first-entry = First entry
# How the tool first enters the material (plunge, ramp, helix).
cam-entry = Entry
cam-ramp-angle = Ramp angle
# Arcs or lines the tool moves along into and out of a cut.
cam-leads = Leads
cam-lead-in = Lead in
cam-lead-out = Lead out
cam-lead-radius = Lead radius
cam-overlap = Overlap
# Section heading: the toolpath repeated in a line or round a centre.
cam-repeat-pattern = Pattern
cam-repeat = Repeat
cam-repeat-linear = Linear
cam-repeat-circular = Circular
# How many copies a pattern makes.
cam-instances = Instances
cam-spacing-x = Spacing X
cam-spacing-y = Spacing Y
cam-centre-x = Centre X
cam-centre-y = Centre Y
cam-angle-between = Angle between
cam-repeat-note = In the toolpath's work plane; each copy is checked for collisions like the first.

## crates/cadrs_app/src/cam/export.rs
# "Post" is to write a setup's toolpaths as an NC (G-code) program for the machine.
cam-select-setup-to-post = Select a setup or toolpaths to post
cam-setup-no-toolpaths-to-post = The setup has no toolpaths to post
cam-calculating-toolpaths-first = Calculating the toolpaths first: post again when they're done
cam-no-cam-studio = No CAM Studio
cam-job-is-gone = The job is gone
cam-setup-is-gone = The setup is gone
cam-parts-still-being-built = The parts are still being built
cam-job-has-no-machine = The job has no machine
# A note to the operator: { $offset } is a work offset to set on the machine (e.g. "G55 to X0 Y0 Z0").
cam-set-offset = Set { $offset }
cam-no-material-number-on-torch = No material number on the torch: QtPlasmaC cuts with the material it has loaded (set one on the tool)
# { $machine } is the machine's name, { $post } the post processor's.
cam-machine-with-post = { $machine } (post: { $post })
# { $count } is how many lines the NC program has.
cam-export-summary-inches = { $count } lines, inches
# { $count } is how many lines the NC program has.
cam-export-summary-millimetres = { $count } lines, millimetres
# The verb: write the NC program (the dialog's title and its button).
cam-post = Post
cam-output-units = Output units
# The toolpaths in the program.
cam-operations = Operations
# An HTML page for the machine operator listing the setup's tools and toolpaths.
cam-setup-sheet = Setup sheet
cam-write-setup-sheet = Write the setup sheet beside it (HTML)
cam-post-anyway = Post anyway: I've checked where it runs on the machine
cam-post-to-folder = Post to folder
# { $problem } is where the program runs beyond the machine's travel. "Post anyway" is the checkbox cam-post-anyway: keep its wording.
cam-beyond-travel-tick-post-anyway = { $problem } (tick "Post anyway" once you've checked it on the machine)
cam-choose-folder-to-post-to = Choose a folder to post to
# { $path } is the NC program written; { $sheet } the setup sheet's file name.
cam-posted-with-sheet = Posted { $path } and its setup sheet { $sheet }
# { $path } is the NC program written.
cam-posted = Posted { $path }
# { $path } is the file; { $error } is the system's error message.
cam-couldnt-write-file = Couldn't write { $path }: { $error }

## crates/cadrs_app/src/cam/preview/analysis.rs
# Items of the line that sums up a checked run (joined by " · ").
# { $length } is the length cut in mm.
cam-verify-cut-length = Cut { $length } mm
# { $count } is 1.
cam-verify-part-free = { $count } part free
# { $count } is how many parts are cut free of the sheet (not 1).
cam-verify-parts-free = { $count } parts free
# { $area } is an area in mm².
cam-verify-cut-from-part = { $area } mm² cut from the part
# { $volume } is the volume of material removed, in mm³.
cam-verify-removed = Removed { $volume } mm³
cam-verify-no-gouges = no gouges
# { $depth } is a depth in mm.
cam-verify-gouges-up-to = gouges up to { $depth } mm deep
cam-verify-no-collisions = no collisions
# { $count } is 1.
cam-verify-collision = { $count } collision
# { $count } is how many collisions were found (not 1).
cam-verify-collisions = { $count } collisions
# { $depth } is the most material left on the part, in mm.
cam-verify-left-up-to = up to { $depth } mm left
# A collision found in the run: { $operation } is the toolpath's name.
cam-collision-rapid-into-stock = { $operation }: a rapid runs into the stock
# A collision found in the run: { $operation } is the toolpath's name.
cam-collision-tool-into-workholding = { $operation }: the tool runs into the workholding
# A collision found in the run: { $operation } is the toolpath's name.
cam-collision-holder-meets-stock = { $operation }: the shank or holder meets the stock
# A collision found in the run: { $operation } is the toolpath's name.
cam-collision-rapid-through-part = { $operation }: a rapid runs through the part
# A collision found in the run: { $operation } is the toolpath's name.
cam-collision-holder-meets-part = { $operation }: the shank or holder meets the part

## crates/cadrs_app/src/cam/preview/mod.rs
cam-select-toolpath-to-preview = Select a toolpath or setup to preview
cam-calculating-toolpaths-first-preview = Calculating the toolpaths first: preview again when they're done
cam-setup-has-no-toolpaths = The setup has no toolpaths
cam-check-failed-unexpectedly = The check failed unexpectedly
cam-checking-the-run = Checking the run…
cam-check-stopped-unexpectedly = The check stopped unexpectedly
# Follows a ⚠ sign at the end of the run's summary.
cam-one-toolpath-warning = 1 toolpath warning
# Follows a ⚠ sign at the end of the run's summary; { $count } is more than 1.
cam-toolpath-warnings = { $count } toolpath warnings
# The preview's modes: the toolpath drawn as the tool moves; the stock as it is cut; the cut with the holder and collisions.
cam-back-plot = Back plot
cam-verify = Verify
cam-simulate = Simulate
# The preview's playback speed (a slider's label).
cam-preview-speed = Speed
cam-playback-speed = Playback speed
cam-step-back = Step back
# Pause the preview's playback.
cam-pause = Pause
# Play the preview.
cam-run = Run
cam-step-next = Step next
cam-go-to-end = Go to end
# Back to the start of the preview.
cam-reset = Reset
cam-close-the-preview = Close the preview
cam-drag-through-program = Drag to move through the program
# Show the toolpath (a checkbox of the preview).
cam-toolpath = Toolpath
cam-pause-on-collision = Pause on collision
# In the preview's readout: { $feed } is the feed rate in mm/min.
cam-readout-feed = feed { $feed } mm/min
# In the preview's readout: the tool is moving at rapid speed.
cam-readout-rapid = rapid
# { $reason } is the collision the preview paused on.
cam-readout-paused = Paused: { $reason }

## crates/cadrs_app/src/cam/toolbar.rs
# Toolbar tooltips of a CAM Studio.
cam-insert-tooltip = Insert part, workholding and stock
cam-create-job = Create job
cam-create-machine = Create machine
cam-create-setup = Create setup
cam-create-tool = Create tool
cam-create-toolpath = Create toolpath
cam-create-work-plane = Create work plane
cam-regenerate-tooltip = Regenerate the selected toolpaths (all out-of-date ones of the active job when none is selected)
cam-preview-tooltip = Preview operation: back plot, verify and simulate
cam-post-tooltip = Post the setup to G-code
cam-show-hide-stock = Show or hide the active job's stock

## crates/cadrs_app/src/cam/tree.rs
# The machine row of a job that has none yet.
cam-no-machine-create-machine = No machine: Create machine
# Shown after a component that follows its Part Studio's workspace (not a version); lowercase.
cam-component-workspace = workspace
# { $count } is how many jobs the CAM Studio has.
cam-jobs-count = Jobs ({ $count })
# { $count } is how many components (parts, workholding, stock) the CAM Studio has.
cam-components-count = Components ({ $count })
cam-tree-insert-part-then-job = Insert the part to machine, then create a job.
cam-tree-create-job-for-parts = Create a job for the parts.
cam-tree-insert-components-note = Insert part, workholding and stock from Part Studios.
cam-update-to-latest-version = Update to the latest version
cam-edit-job = Edit job…
cam-edit-machine = Edit machine…
cam-create-machine-ellipsis = Create machine…
cam-create-setup-ellipsis = Create setup…
cam-regenerate-all-toolpaths = Regenerate all toolpaths
cam-duplicate = Duplicate
cam-edit-setup = Edit setup…
cam-create-tool-ellipsis = Create tool…
cam-create-toolpath-ellipsis = Create toolpath…
cam-create-work-plane-ellipsis = Create work plane…
cam-preview-operation = Preview operation
cam-post-child-operations = Post child operations…
cam-regenerate-operation = Regenerate operation
cam-post-selected-operations = Post selected operations…
# { $place } is a setup's name (with its job's: "Job 1 · Setup 2").
cam-move-to = Move to { $place }
# { $place } is a setup's name (with its job's: "Job 1 · Setup 2").
cam-copy-to = Copy to { $place }

## crates/cadrs_app/src/sheetmetal_ui.rs
# Section headings of the Sheet metal model dialog.
sheet-section-selections = Selections
sheet-section-general = General
# Section heading: the cuts made where bends meet a corner or an edge.
sheet-section-relief = Relief
# The sheet metal tools of the Sheet metal model button's menu.
sheet-tool-finish = Finish sheet metal model
# A wall bent up from an edge.
sheet-tool-flange = Flange
# An edge folded back on itself.
sheet-tool-hem = Hem
# A sheet metal tab added to a flange's edge.
sheet-tool-tab = Tab
sheet-tool-bend = Bend
# Two bends that offset a flat (a joggle).
sheet-tool-jog = Jog
# A sheet metal form: a shape (a louver, a dimple) stamped into the sheet (the tool, a Tag's type, a form to pick).
sheet-form = Form
sheet-tool-loft = Loft
sheet-tool-make-joint = Make joint
sheet-tool-corner = Corner
sheet-tool-bend-relief = Bend relief
sheet-tool-corner-break = Corner break
sheet-not-available-yet = Not available yet: comes with the next sheet metal tools
# Used when a face's part can't be named: "Face of part" (lowercase).
sheet-fallback-part = part
# Used when a region's sketch can't be named: "Face of sketch" (lowercase).
sheet-fallback-sketch = sketch
# { $name } is a sketch's name.
sheet-arc-of = Arc of { $name }
# { $name } is a sketch's name.
sheet-circle-of = Circle of { $name }
sheet-up-to-face = Up to face
sheet-up-to-part = Up to part
sheet-up-to-vertex = Up to vertex
# Tabs of the Sheet metal model dialog: how the sheet metal is made.
sheet-op-convert = Convert
sheet-op-thicken = Thicken
sheet-parts-and-surfaces-to-convert = Parts and surfaces to convert
sheet-faces-to-exclude = Faces to exclude
sheet-edges-or-cylinders-to-bend = Edges or cylinders to bend
sheet-clearance-from-input = Clearance from input
sheet-include-bends = Include bends
sheet-keep-input-parts = Keep input parts
sheet-sketch-curves-to-extrude = Sketch curves to extrude
sheet-arcs-to-extrude-as-bends = Arcs to extrude as bends
sheet-second-end-position = Second end position
sheet-faces-or-regions-to-thicken = Faces or sketch regions to thicken
sheet-tangent-propagation = Tangent propagation
sheet-bend-radius = Bend radius
sheet-flip-direction-up = Flip direction up
# How a bend's flat length is worked out (K Factor, bend allowance or bend deduction).
sheet-bend-calculation = Bend calculation
sheet-default-bend-k-factor = Default bend K Factor
sheet-default-bend-allowance = Default bend allowance
sheet-default-bend-deduction = Default bend deduction
# The K Factor of rolled (cylindrical) bends.
sheet-rolled-k-factor = Rolled K Factor
sheet-minimal-gap = Minimal gap
sheet-corner-relief-type = Corner relief type
sheet-corner-relief-scale = Corner relief scale
sheet-corner-relief-width = Corner relief width
sheet-corner-relief-diameter = Corner relief diameter
sheet-bend-relief-type = Bend relief type
sheet-bend-relief-depth-scale = Bend relief depth scale
sheet-bend-relief-width-scale = Bend relief width scale
# Bend calculations (the options of Bend calculation).
sheet-bend-calc-k-factor = K Factor
sheet-bend-calc-allowance = Bend allowance
sheet-bend-calc-deduction = Bend deduction
# Relief shapes: the shape, then whether its size is given (Sized) or a multiple of the thickness (Scaled).
sheet-relief-square-sized = Square - Sized
sheet-relief-rectangle-scaled = Rectangle - Scaled
sheet-relief-round-sized = Round - Sized
sheet-relief-round-scaled = Round - Scaled
# A corner with no relief cut: the flanges meet.
sheet-relief-closed = Closed
sheet-relief-simple = Simple
# A rounded slot shape.
sheet-relief-obround-scaled = Obround - Scaled
sheet-relief-obround-sized = Obround - Sized
# A bend relief that tears the sheet (no material removed).
sheet-relief-tear = Tear
sheet-clearance-at-least-0 = Clearance from input must be at least 0
sheet-depth-greater-than-0 = Depth must be greater than 0
sheet-not-a-valid-value = Not a valid value

## crates/cadrs_app/src/sheetmetal_joint_ui.rs
# Undo step names of the Modify joint dialog's changes.
sheet-select-joint = Select joint
sheet-use-model-bend-radius = Use model bend radius
sheet-use-model-k-factor = Use model K Factor
sheet-joint-type = Joint type
sheet-rip-style = Rip style
sheet-remove-joint = Remove joint
# { $joint } is a joint's name ("Bend A"), { $model } the sheet metal model's.
sheet-joint-of-model = { $joint } of { $model }
# A bend or rip between two walls of a sheet metal model.
sheet-joint = Joint
# Joint types.
sheet-joint-bend = Bend
# Two walls meeting with a cut (no bend).
sheet-joint-rip = Rip
sheet-joint-tangent = Tangent
# Rip styles: how the two walls of a rip meet.
sheet-rip-edge-joint = Edge joint
sheet-rip-butt-direction-1 = Butt joint - Direction 1
sheet-rip-butt-direction-2 = Butt joint - Direction 2

## crates/cadrs_app/src/sheetmetal_features_ui.rs
# Flange and hem alignments: where the new wall sits against the edge.
sheet-alignment-inner = Inner
sheet-alignment-outer = Outer
sheet-alignment-middle = Middle
sheet-alignment-hold-line = Hold line
# A hem alignment: the hem stays where the edge is.
sheet-hem-alignment-in-place = In place
# Flange end types.
sheet-end-blind = Blind
sheet-up-to-entity = Up to entity
sheet-end-up-to-entity-offset = Up to entity with offset
# The angle of a flange's bend (an angle control option and its value).
sheet-bend-angle = Bend angle
sheet-angle-align-to-geometry = Align to geometry
sheet-angle-from-direction = Angle from direction
# Hem types.
sheet-hem-straight = Straight
sheet-hem-rolled = Rolled
sheet-hem-tear-drop = Tear drop
sheet-second-distance = Second distance
sheet-offset-value = Offset value
sheet-edges-to-flange = Edges or side faces to flange
sheet-flange-alignment = Flange alignment
sheet-end-type = End type
# The geometry the flange's wall is made parallel to.
sheet-parallel-to = Parallel to
sheet-automatic-miter = Automatic miter
sheet-miter-angle = Miter angle
sheet-partial-flange = Partial flange
sheet-overall-parameters = Overall parameters
# Partial flanges: one per edge, or one per chain of edges.
sheet-chain-per-edge = Per edge
sheet-chain-per-chain = Per chain
sheet-flip-sides = Flip sides
sheet-hold-adjacent-edges = Hold adjacent edges
sheet-end-conditions = End conditions
sheet-second-bound = Second bound
sheet-edges-to-hem = Edges or side faces to hem
# Flip the hem (a tooltip of its arrow).
sheet-flip = Flip
# A hem folded flat on the sheet.
sheet-flattened = Flattened
sheet-inner-radius = Inner radius
sheet-total-length = Total length
# The space between a hem and the sheet.
sheet-gap = Gap
sheet-hem-alignment = Hem alignment
sheet-corner-type = Corner type
sheet-edges-to-join = Edges or side faces to join
# Undo step names of dragging a flange's arrows.
sheet-drag-distance = Drag distance
sheet-drag-bound = Drag bound

## crates/cadrs_app/src/sheetmetal_tools_ui.rs
# The line a sheet metal bend folds along (a field and a bend alignment option).
sheet-bend-line = Bend line
sheet-alignment-hold-other-line = Hold other line
# Jog offset anchors: which face of the sheet the jog offset is measured to.
sheet-anchor-inside = Inside
sheet-anchor-nominal = Nominal
sheet-anchor-outside = Outside
# Fillet measurements: the size given as a radius or a width.
sheet-measure-radius = Radius
sheet-measure-width = Width
# A chamfer measurement: the distance measured along the tangent.
sheet-measure-tangent = Tangent
# Chamfer types.
sheet-chamfer-equal-distance = Equal distance
sheet-chamfer-two-distances = Two distances
sheet-chamfer-distance-angle = Distance and angle
# Hold the other side of the bend line in place (an arrow's tooltip and an undo step).
sheet-hold-opposite-side = Hold opposite side
sheet-face-to-bend = Sheet metal face to bend
sheet-bend-alignment = Bend alignment
# Bend the other way (an arrow's tooltip and an undo step).
sheet-opposite-angle = Opposite angle
# How a jog's offset is bounded (blind, up to entity, a thickness factor).
sheet-bounding-type = Bounding type
sheet-jog-offset = Jog offset
sheet-offset-distance = Offset distance
sheet-thickness-factor = Thickness factor
sheet-jog-offset-anchor = Jog offset anchor
sheet-preserve-material = Preserve material
# The sketch region that makes a sheet metal tab.
sheet-tab-profile = Tab profile
sheet-flange-to-merge = Flange to merge
sheet-subtraction-offset = Subtraction offset
sheet-subtraction-scope = Subtraction scope
# The field taking the corner a corner relief goes in.
sheet-corner-field = Corner
# The field taking the bend end a bend relief goes at.
sheet-bend-relief-field = Bend relief
sheet-bend-relief-depth = Bend relief depth
sheet-extend-bend-relief = Extend bend relief
# The Corner break dialog's tabs.
sheet-corner-break-fillet = Fillet
sheet-corner-break-chamfer = Chamfer
sheet-entities-to-fillet-or-chamfer = Entities to fillet or chamfer
sheet-measurement = Measurement
sheet-chamfer-type = Chamfer type
sheet-distance-1 = Distance 1
sheet-distance-2 = Distance 2
# How a fillet's shape is controlled (only Distance for now).
sheet-control = Control
sheet-asymmetric = Asymmetric
sheet-second-width = Second width
sheet-second-radius = Second radius
sheet-flip-asymmetric = Flip asymmetric
sheet-allow-edge-overflow = Allow edge overflow
sheet-sheet-metal-parts = Sheet metal parts
sheet-finish-warning = Features after this one act on the parts as ordinary solids and don't show in the flat pattern
# Undo step names.
sheet-angle-control = Angle control
sheet-corner-break-type = Corner break type
sheet-change-value = Change value
sheet-drag-jog-offset = Drag jog offset

## crates/cadrs_app/src/sheetmetal_p3i9_ui.rs
# { $name } is a sketch's name.
sheet-point-of = Point of { $name }
# { $name } is a sketch's name (its points).
sheet-vertices-of = Vertices of { $name }
# { $percent } is a number: how far along the first profile the point is (the % follows it).
sheet-point-of-profile-1 = Point of Profile 1 ({ $percent }%)
# { $percent } is a number: how far along the second profile the point is (the % follows it).
sheet-point-of-profile-2 = Point of Profile 2 ({ $percent }%)
# The Part Studio a sheet metal form comes from.
sheet-form-part-studio = Form Part Studio
sheet-select-part-studio-ellipsis = Select Part Studio...
sheet-select-part-studio = Select Part Studio
# Where forms are placed (one or more).
sheet-form-locations = Location(s)
sheet-form-connector-tooltip = Mate connector: pick an implicit one in the view
# The faces a form is stamped into (one or more).
sheet-form-target-faces = Target face(s)
sheet-tag-part-to-add = Part to add
sheet-tag-part-to-remove = Part to remove
sheet-tag-sketch-for-flat-view = Sketch for flat view
sheet-tag-form-origin-connector = Form origin mate connector
# A Sheet metal loft tab: a new sheet metal model (the other tab, Add, adds to one).
sheet-loft-new = New
# The loft's two profiles.
sheet-profile-1 = Profile 1
sheet-profile-2 = Profile 2
# Matched points of the two profiles a loft connects.
sheet-connections = Connections
sheet-match-connections = Match connections
# { $count } is 1.
sheet-one-selection = { $count } selection
# { $count } is how many entities a connection has (not 1).
sheet-selections-count = { $count } selections
sheet-remove-connection = Remove connection
sheet-vertices-or-edges = Vertices or edges
sheet-add-connection = Add connection
sheet-chordal-tolerance = Chordal tolerance
sheet-chordal-tolerance-greater-than-0 = Chordal tolerance must be greater than 0
# Undo step names.
sheet-loft-new-add = New / Add
sheet-flip-thickness-direction = Flip thickness direction
sheet-drag-connection = Drag connection
# The form picker's empty states.
sheet-picker-none-tagged = No Part Studio of this document is tagged as a form
sheet-picker-no-match = No form matches the search
sheet-picker-none-in-document = No Part Studio of this document has a Tag (Form)
sheet-picker-none-in-others = No other document has a form Part Studio
# The form picker's select of the library.
sheet-picker-library = Library
# The form picker's tabs.
sheet-picker-current-document = Current document
sheet-picker-other-documents = Other documents
sheet-picker-libraries = Libraries
sheet-picker-search-placeholder = Search by name, folder, or configuration
# The library of the document's own sheet metal forms.
sheet-my-sheet-metal-forms = My sheet metal forms
sheet-drag-along-profile = Drag along the profile

## crates/cadrs_app/src/sheetmetal_table.rs
# Before the select of the sheet metal model the table shows.
sheet-context-label = Sheet metal context:
sheet-no-sheet-metal-model = No sheet metal model
sheet-bend-radius-greater-than-0 = The bend radius must be greater than 0
sheet-k-factor-between-0-and-1 = The K Factor must be between 0 and 1
# A table column: { $unit } is the length unit's symbol (mm, in…).
sheet-col-bend-allowance = Bend allowance({ $unit })
# A table column: { $unit } is the length unit's symbol (mm, in…).
sheet-col-bend-deduction = Bend deduction({ $unit })
# A table column: { $unit } is the length unit's symbol (mm, in…).
sheet-col-radius = Radius({ $unit })
# A table column: the bend's angle in degrees.
sheet-col-angle = Angle(deg)
sheet-col-bend-direction = Bend direction
# A table column: a rip's style.
sheet-col-style = Style
sheet-corner-break-lock = Locked by a Corner break on this model.\nMake Corner breaks after the joints are final.
# The sheet metal table's titles.
sheet-bends = Bends
sheet-other-joints = Other joints
# Bend directions in the table.
sheet-bend-up = Up
sheet-bend-down = Down
# { $feature } is the feature (a Bend or Jog) that sets the bend's value.
sheet-set-in-feature = Set in { $feature }: a new value edits { $feature }
# Not applicable (a tangent joint has no style).
sheet-not-applicable = n/a
# Undo step names: { $name } is a joint's name.
sheet-move-up-named = Move up { $name }
# Undo step names: { $name } is a joint's name.
sheet-move-down-named = Move down { $name }
# { $name } is a part's name.
sheet-flat-pattern-of = Flat pattern of { $name }
# { $joint } is a joint's name ("Bend A").
sheet-convert-to-rip = Convert { $joint } to rip
# { $joint } is a joint's name ("Rip B").
sheet-convert-to-bend = Convert { $joint } to bend
# { $joint } is a joint's name, { $feature } the feature that made it.
sheet-joint-made-by = { $joint } is made by { $feature }: it can't be made a rip
sheet-hem-bend-cant-be-rip = A hem's bend can't be made a rip
# Used in the flat view's menu when no part is under the pointer ("Copy flat pattern"); lowercase.
sheet-flat-pattern-fallback = flat pattern
sheet-right-click-a-flat-pattern = Right-click a flat pattern
# Used in the flat view's menu when the model can't be named ("Edit sheet metal model…"); lowercase.
sheet-model-fallback = sheet metal model
# { $name } is a feature's or a sketch's name.
sheet-edit-named-ellipsis = Edit { $name }…
sheet-model-derived-elsewhere = The sheet metal model is derived from another Part Studio
sheet-hide-dimensions = Hide dimensions
sheet-show-dimensions = Show dimensions
# { $name } is the sheet metal model's name.
sheet-show-dependencies-of = Show dependencies of { $name }…
# { $name } is a flat pattern's name ("Flat pattern of Part 1").
sheet-new-sketch-on = New sketch on { $name }…
# { $name } is a flat pattern's name ("Flat pattern of Part 1").
sheet-copy-named = Copy { $name }
# { $name } is a flat pattern's name ("Flat pattern of Part 1").
sheet-create-drawing-of = Create drawing of { $name }…
# { $name } is a flat pattern's name ("Flat pattern of Part 1").
sheet-export-dxf-of = Export DXF/DWG of { $name }…
sheet-select-other = Select other…
sheet-view-normal-to = View normal to
# { $name } is the sheet metal model's name.
sheet-delete-named = Delete { $name }
# { $name } is a flat pattern's name ("Flat pattern of Part 1").
sheet-flat-copied = { $name } copied. Paste it into a sketch with Ctrl+V.

## crates/cadrs_app/src/units_dialog.rs
# The document's Workspace units dialog.
io-units-title = Workspace units
io-units-length = Length units
io-units-angle = Angle units
# An angle unit, in the Angle units list.
io-units-degree = Degree
# An angle unit, in the Angle units list.
io-units-radian = Radian
io-units-mass = Mass units
io-units-length-decimals = Length decimal places
io-units-angle-decimals = Angle decimal places

## crates/cadrs_app/src/window_title.rs
# The window's title while the Documents page is open (cadrs is the app's name).
io-window-title-documents = Documents – cadrs

## crates/cadrs_app/src/native_document.rs
# The dialog that saves the whole document as a .cadrs file.
io-native-export-title = Export document
# The file type the document is saved as (cadrs is the app's name; *.cadrs its file extension).
io-native-export-format = cadrs native document (*.cadrs)
io-native-export-contents = All current tabs and embedded assets. Saved versions and undo history are excluded.
io-native-export-missing = Enter a file name and destination folder
# { $path } is the file (or folder) that was written.
io-exported-path = Exported { $path }
# { $error } is why it failed.
io-export-failed = Export failed: { $error }
io-slicer-nothing = Nothing to send to the slicer
# { $error } is why (an English technical message).
io-slicer-cannot = Can't send to the slicer: { $error }
# Inside "Send to slicer failed: …" (lowercase, a reason).
io-slicer-no-mesh = no mesh was made
# Inside "Send to slicer failed: …" (lowercase); { $slicer } is a slicer's name (PrusaSlicer…), { $error } why.
io-slicer-cannot-start = cannot start { $slicer }: { $error }
# { $name } is the part's or document's name, { $slicer } the slicer's (PrusaSlicer…).
io-slicer-sent = Sent { $name } to { $slicer }
# { $error } is why it failed.
io-slicer-failed = Send to slicer failed: { $error }
# { $name } is the name of what was imported (a document or a file).
io-imported-name = Imported { $name }
# { $error } is why it failed.
io-import-failed = Import failed: { $error }

## crates/cadrs_app/src/pcb/kicad.rs
pcb-import-open-studio = Open a PCB Studio tab to import into
# An imported board: { $name } is the board's name, { $count } how many components it has.
pcb-import-board-components = { $name } ({ $count } components)
# { $file } is the 3D model file's path.
pcb-model-not-found = 3D model not found: { $file }

## crates/cadrs_app/src/eda/libraries.rs
pcb-library-no-folder = There's no folder to keep libraries in
# { $name } is a component's name.
pcb-library-part-empty = { $name } has no symbol or footprint yet

## crates/cadrs_app/src/drop_import.rs
# A file of a kind that can't be dropped on the window: { $name } is its file name (cadrs is the app's own file type).
io-drop-unknown-kind = { $name }: drop drawings (SVG, EPS, PDF, DXF, DWG), pictures (PNG, JPEG, GIF), models (STEP, IGES, BREP, STL) or native documents (cadrs)
io-drop-native-on-documents = Import native documents from the documents page
# { $name } is the dropped file's name.
io-drop-close-sketch = Close the sketch to drop { $name } into the Part Studio
# { $name } is the dropped file's name.
io-drop-open-part-studio = Open a Part Studio to drop { $name } into it
# { $name } is the dropped file's name.
io-drop-into-part-studio = Drop { $name } into a Part Studio
# { $name } is a file's name, { $error } why it couldn't be inserted.
io-cannot-insert = Cannot insert { $name }: { $error }
# { $name } is a drawing file's name.
io-drop-no-curves = { $name } has no lines or curves to insert
# { $name } is a drawing file's name, { $count } how many curves it added.
io-drop-inserted-curves = Inserted { $name }: { $count } curves

## crates/cadrs_app/src/picture.rs
# { $name } is a file's name, { $error } why it couldn't be read.
io-cannot-read = Cannot read { $name }: { $error }

## crates/cadrs_app/src/eda/layers_panel.rs
# The PCB layout's layers panel (copper and drawing layers of a board).
pcb-layers = Layers
# Draws every layer but the active one faint.
pcb-layers-dim = Dim other layers

## crates/cadrs_app/src/eda/online.rs
# The library browser's JLCPCB parts tab: its status line. LCSC numbers are JLCPCB's part numbers (C followed by digits).
pcb-online-type-query = Type a part name, part number or LCSC number (C…), then Enter
pcb-online-searching = Searching JLCPCB…
pcb-online-choose-part = Choose a part first
# { $part } is a part's LCSC number, { $library } the library it goes into.
pcb-online-adding = Adding { $part } to { $library }…
# { $query } is what was searched for.
pcb-online-no-match = No parts match "{ $query }"
# { $count } is how many parts are listed, { $total } how many match.
pcb-online-first-of = The first { $count } of { $total } parts
# How many parts match: exactly one.
pcb-online-one-part = 1 part
# { $count } is how many parts match (more than one).
pcb-online-parts = { $count } parts
# { $error } is why the search failed.
pcb-online-search-failed = Search failed: { $error }
# { $part } is the added symbol's name, { $library } the library it went into.
pcb-online-added = Added { $part } to { $library }
# { $warnings } lists what went wrong while adding the part.
pcb-online-added-but = Added, but: { $warnings }
# { $error } is why the part couldn't be added.
pcb-online-add-failed = Couldn't add it: { $error }
# JLCPCB's class of an assembly part: a Basic part (always loaded on their machines).
pcb-online-class-basic = Basic
# JLCPCB's class of an assembly part: an Extended part (loaded on demand, for a fee).
pcb-online-class-extended = Extended
# { $stock } is how many JLCPCB has in stock; { $price } is the unit price, with its separator (" · $0.0123"), or nothing.
pcb-online-stock-basic = Stock { $stock } · Basic part{ $price }
# { $stock } is how many JLCPCB has in stock; { $price } is the unit price, with its separator (" · $0.0123"), or nothing.
pcb-online-stock-extended = Stock { $stock } · Extended part{ $price }
# Where a part comes from: { $library } is the library's name and web addresses.
pcb-online-from-library = From the { $library }

## crates/cadrs_app/src/pcb/create_assembly.rs
# The Create assembly dialog's title: { $board } is the PCB board's name.
pcb-create-title = Create assembly from '{ $board }'
pcb-create-select-features = Select features to include in the assembly:
# A checkbox: include the circuit board itself.
pcb-create-asm-board = Board
# A checkbox: include the board's electronic components.
pcb-create-components = Components
# A checkbox: include the board's keep-in and keep-out areas.
pcb-create-keep-areas = Keep-In and Keep-Out Areas
# (cadrs is the app's name.)
pcb-create-note = Building the assembly can take several minutes. You can keep working in cadrs while it is built.
# Shown while the assembly is built: { $board } is the board's name.
pcb-create-progress = Creating assembly from '{ $board }'…
# The background thread doing the modelling work stopped before finishing.
io-kernel-stopped = The kernel thread stopped
# { $board } is the board's name (a Part Studio and an Assembly of that name were made).
pcb-create-done = Created the Part Studio and Assembly "{ $board }"
# { $board } is the board's name, { $folder } the folder the component document went into.
pcb-create-done-one-doc = Created "{ $board }" with 1 component document in { $folder }
# { $board } is the board's name, { $count } how many component documents were made, { $folder } the folder they went into.
pcb-create-done-docs = Created "{ $board }" with { $count } component documents in { $folder }
# { $board } is the board's name; its one component document already existed.
pcb-create-done-its-one-doc = Created "{ $board }" with its 1 component document
# { $board } is the board's name; { $reused } is how many component documents (which already existed) it uses.
pcb-create-done-its-docs = Created "{ $board }" with its { $reused } component documents
# { $board } is the board's name, { $folder } the folder the new document went into, { $reused } how many existing ones it uses.
pcb-create-done-one-new-doc = Created "{ $board }" with 1 new component document in { $folder }, { $reused } reused
# { $board } is the board's name, { $count } how many new component documents, { $folder } their folder, { $reused } how many existing ones it uses.
pcb-create-done-new-docs = Created "{ $board }" with { $count } new component documents in { $folder }, { $reused } reused
# { $board } is the board's name, { $error } why it failed.
pcb-create-failed = Couldn't create the assembly from { $board }: { $error }
# A link in the toast that opens the new Assembly tab.
pcb-create-open-assembly = Open assembly

## crates/cadrs_app/src/flat_export_dialog.rs
# Export options of a sheet metal flat pattern (checkboxes).
io-flat-export-splines = Export splines as polylines
io-flat-export-z-zero = Set z-height to zero and normals to positive
io-flat-export-centerlines = Include bend centerlines
io-flat-export-tangents = Include bend tangent lines
io-flat-export-cbore = Include counterbore and countersink lines
io-flat-export-form-outlines = Include form feature outlines
io-flat-export-form-centermarks = Include form feature centermarks
io-flat-export-sketches = Include visible sketches
io-flat-export-no-flat = That part has no flat pattern
# The dialog that exports a sheet metal flat pattern (DXF and DWG are file formats).
io-flat-export-title = Export as DXF/DWG
io-flat-export-help = Exports the flat pattern as DXF or DWG: outline, cut-outs, bend lines by direction and the options below, each on its own layer
# The DWG format, unavailable: LibreDWG and the ODA File Converter are programs, PATH the system's list of program folders.
io-dwg-needs-converter = DWG (needs LibreDWG or the ODA File Converter on PATH)
# Which parts are exported.
io-flat-export-scope = Scope
io-flat-export-separate = Export each part as its own file
# An export dialog's row choosing what happens to the exported file.
io-export-options = Options
# Save the exported file into the folder below (an export option).
io-export-download = Download
io-export-nowhere = Nowhere to save the export
io-flat-export-nothing = Export failed: no flat pattern to export
# { $count } is how many files were written, { $folder } where.
io-exported-files-to = Exported { $count } files to { $folder }

## crates/cadrs_app/src/import_file.rs
# { $name } is the file's name (STEP, IGES and BREP are file formats).
io-import-only-exchange = { $name }: only STEP, IGES and BREP files can be imported
# { $name } is the file being read.
io-import-reading = Reading { $name }…
# Why an import failed: the background thread doing the work stopped.
io-import-kernel-stopped = the kernel thread stopped
# { $name } is the file's name, { $error } why its import failed.
io-import-of-failed = Import of { $name } failed: { $error }
# What the file holds: { $format } is its format (STEP, IGES…); one part, used once.
io-import-summary-assembly-1-1 = { $format } file: an assembly of 1 part with 1 instance
# What the file holds: { $format } is its format (STEP, IGES…), { $instances } how many times its one part is used.
io-import-summary-assembly-1-n = { $format } file: an assembly of 1 part with { $instances } instances
# What the file holds: { $format } is its format (STEP, IGES…), { $parts } how many different parts.
io-import-summary-assembly-n-1 = { $format } file: an assembly of { $parts } parts with 1 instance
# What the file holds: { $format } is its format (STEP, IGES…), { $parts } how many different parts, { $instances } how many times they are used in all.
io-import-summary-assembly-n-n = { $format } file: an assembly of { $parts } parts with { $instances } instances
# What the file holds: { $format } is its format (STEP, IGES…).
io-import-summary-one-part = { $format } file: 1 part
# What the file holds: { $format } is its format (STEP, IGES…), { $parts } how many parts.
io-import-summary-parts = { $format } file: { $parts } parts
# Where the import goes: { $name } is the new document's name.
io-import-into-new = Into a new document, "{ $name }"
io-import-into-tabs = Into new tabs of this document
# { $list } is the file's first part names, separated by commas.
io-import-parts-list = Parts: { $list }
# { $list } is the file's first part names, separated by commas; { $more } how many more there are.
io-import-parts-list-more = Parts: { $list }, and { $more } more
# A heading over the two ways to import a file.
io-import-as = Import as
# Import every part into one Part Studio, ignoring the file's assembly structure.
io-import-as-studio = Part Studio (flatten)
io-import-as-studio-tip = Every part where the file puts it, in one Part Studio
io-import-as-assembly = Keep assembly structure
io-import-as-assembly-tip = Each part once in a Part Studio, and an Assembly with its instances
io-import-not-assembly = The file is not an assembly
# A checkbox: the file's up direction is its Y axis (it is turned so that Z is up).
io-import-y-up = File is Y axis up
# Why an import failed: no document is open.
io-import-no-document = no open document
# { $name } is the imported file's name.
io-import-done-one-part = Imported { $name }: 1 part
# { $name } is the imported file's name, { $count } how many parts it gave.
io-import-done-parts = Imported { $name }: { $count } parts

## crates/cadrs_app/src/pcb/transfer.rs
# The dialog that brings a Part Studio's or assembly's board shape into the PCB Studio.
pcb-sync-title = Sync a Part Studio or assembly with PCB Studio
pcb-sync-select-source = Select part studio or assembly to use:
pcb-sync-no-source = No Part Studio or assembly in this document
# Which plane the board part's top face lies parallel to.
pcb-sync-top-face = Top face of board part is parallel to:
pcb-sync-nothing = There is no Part Studio or assembly to sync
# { $error } is why the sync failed.
pcb-sync-failed = Couldn't sync: { $error }
# The dialog that exports the board as IDF files (ECAD: electronic CAD).
pcb-export-title = Export ECAD files
# IDF is a board exchange format; its version is chosen below (2.0 or 3.0).
pcb-export-idf-version = Write to IDF Version:
pcb-export-idf2-note = IDF 2.0 files are written without keep-in and keep-out areas.
# { $path } is the file that couldn't be written, { $error } why.
io-export-failed-at = Export failed: { $path }: { $error }
# { $file } is the written file, { $version } the IDF version (2.0 or 3.0), { $folder } where it was written.
pcb-export-idf-done = Exported { $file } (IDF { $version }) to { $folder }
# A link in the toast that shows the exported .emn file (the board's IDF file).
pcb-export-open-emn = Open .emn
# The placeholder of the field that searches the shown file's lines.
pcb-emn-find = Find

## crates/cadrs_app/src/import_dialog.rs
# A button in the Import feature's dialog: pick the file to import.
io-import-choose-file = Choose file
io-import-another-file = Import another file
# An option of the Import feature: the file's up direction is its Y axis.
io-import-y-axis-up = Y axis is up
# An option of the Import feature (STL files): choose the units the file's numbers are in.
io-import-specify-units = Specify units
# The Import feature has made no parts yet.
io-import-no-parts = No parts yet
# How many parts the Import feature made: exactly one.
io-import-one-part = 1 part
# { $count } is how many parts the Import feature made (more than one).
io-import-parts = { $count } parts

## crates/cadrs_app/src/export_image.rs
# The dialog that saves the 3D view as a picture.
io-image-export-title = Export image
io-image-export-view-note = The view as shown: its orientation, shading, edges and any exploded view.
# An image size: the view's own; { $width } and { $height } are in pixels.
io-image-export-viewport-size = Viewport ({ $width } × { $height })
# A size or resolution the user types in (an export dialog's last choice).
io-export-custom = Custom
# The image's width in pixels (px).
io-image-export-width = Width (px)
# The image's height in pixels (px).
io-image-export-height = Height (px)
# A checkbox (PNG is an image format).
io-image-export-transparent = Transparent background (PNG)
# { $max } is the largest width or height allowed, in pixels.
io-image-export-size-range = Width and height must be 1 to { $max } pixels
io-image-export-size-whole = Enter the width and height in whole pixels
# Why Transparent background is off for JPEG (two lines; JPEG and PNG are image formats).
io-image-jpeg-no-transparency = JPEG has no transparency: the background is always the viewport's colour.\nChoose PNG for a transparent background.
io-image-export-nowhere = Nowhere to save the image
# { $path } is the written image file; { $width } × { $height } its size in pixels.
io-image-exported = Exported { $path } ({ $width } × { $height })
io-image-not-rendered = Export failed: the image wasn't rendered
# { $error } is the technical reason.
io-image-read-back-failed = cannot read the image back: { $error }

## crates/cadrs_app/src/image_dialog.rs
# A picture's width, in the Image feature's dialog.
io-image-width = Width
# A picture's height, in the Image feature's dialog.
io-image-height = Height
# A checkbox: changing the picture's width changes its height in proportion (and the other way).
io-image-keep-aspect = Keep aspect ratio

## crates/cadrs_app/src/eda/part_tools.rs
# Tool strip tooltips of the PCB symbol, footprint, schematic and layout editors. Esc is the key.
pcb-tool-select = Select (Esc)
pcb-sym-add-pin = Add a pin
# Insert is the keyboard key.
pcb-sym-repeat-pin = Repeat the last pin (Insert)
pcb-tool-line = Draw a line
pcb-tool-circle = Draw a circle
pcb-tool-rect = Draw a rectangle
# A table of all the symbol's pins.
pcb-sym-pin-table = Pin table
pcb-sym-arrange = Arrange the pins round a box
pcb-sym-props = Symbol properties
pcb-sym-from-library = Start from a library symbol
# A pad: a footprint's copper contact.
pcb-fp-add-pad = Add a pad
pcb-fp-push = Push the selected pad's properties to the other pads
# A row or grid of pads.
pcb-fp-pad-array = Pad array
pcb-fp-renumber = Renumber pads
# The layer the footprint's lines are drawn on: Fab (fabrication), Silkscreen and Courtyard are board layers.
pcb-fp-layer = Drawing layer: Fab, Silkscreen, Courtyard
pcb-fp-props = Footprint properties
pcb-fp-from-library = Start from a library footprint
# The footprint's 3D model (its body shown on the 3D board).
pcb-fp-model = 3D model
pcb-sym-repeat-no-pin = Add a first pin to repeat
pcb-fp-push-no-pad = Select the pad to copy from
# { $layer } is a board layer's name.
pcb-fp-drawing-on = Drawing on { $layer }

## crates/cadrs_app/src/eda/schematic_tools.rs
# Schematic tool strip tooltips; the letter in brackets is the tool's keyboard key.
pcb-sch-add-symbol = Add a symbol (A)
pcb-sch-add-power = Add a power symbol (P)
pcb-sch-add-wire = Add a wire (W)
# A net label names the electrical connection (net) it is placed on.
pcb-sch-add-label = Add a net label (L)
# A flag marking a pin as deliberately left unconnected.
pcb-sch-add-no-connect = Add a no-connect flag (Q)
# Numbers the symbols' references (R1, R2, U1…).
pcb-sch-annotate = Fill in reference designators
pcb-sch-assign = Assign footprints
# ERC: checks the schematic's connections for errors.
pcb-sch-erc = Electrical rules check
# A table of every symbol's fields (reference, value, footprint…).
pcb-sch-fields = Symbol fields table
# SVG and PDF are file formats; a netlist lists the circuit's connections.
pcb-sch-plot = Plot the schematic (SVG, PDF) or export its netlist

## crates/cadrs_app/src/eda/layout_tools.rs
# PCB layout tool strip tooltips; a letter or key in brackets is the tool's shortcut.
pcb-lay-update = Update PCB from schematic (F8)
pcb-lay-board-setup = Board setup
pcb-lay-outline = Draw the board outline
# Tracks are the board's copper connections.
pcb-lay-route = Route tracks (X)
# Nets are the circuit's electrical connections.
pcb-lay-autoroute = Auto-route the unconnected nets
# A zone: a copper area filled around the board's other copper.
pcb-lay-zone = Add a filled zone (Z)
# An area where copper or components must not go.
pcb-lay-keepout = Add a keep-out
pcb-lay-draw-line = Draw lines on the drawing layer
pcb-lay-draw-rect = Draw a rectangle on the drawing layer
pcb-lay-draw-circle = Draw a circle on the drawing layer
pcb-lay-draw-polygon = Draw a filled polygon on the drawing layer (a copper area on a copper layer)
pcb-lay-text = Add text on the drawing layer
# SVG is a file format.
pcb-lay-graphic = Import a graphic (SVG) onto the drawing layer
pcb-lay-measure = Measure (Ctrl+Shift+M)
# The board's front (top) or back (bottom) copper layer.
pcb-lay-layer = Switch the active layer (front / back)
# DRC: checks the board against its design rules (clearances, widths…).
pcb-lay-drc = Design rules checker
# Gerber, drill and other files a board maker needs.
pcb-lay-plot = Plot fabrication outputs
pcb-autoroute-busy = Already auto-routing
pcb-autoroute-nothing = Nothing to route: every net is connected
pcb-autoroute-running = Auto-routing…
pcb-autoroute-board-changed = The board changed while it was being auto-routed: nothing was put down (auto-route again)
# { $seconds } is how long it took; { $tracks } and { $vias } how many were put down.
pcb-autoroute-done-cpu = Auto-routed in { $seconds } s on the CPU: { $tracks } tracks, { $vias } vias
# { $seconds } is how long it took, { $gpu } the graphics card's name; { $tracks } and { $vias } how many were put down.
pcb-autoroute-done-gpu = Auto-routed in { $seconds } s on { $gpu }: { $tracks } tracks, { $vias } vias
# { $seconds } is how long it took; { $tracks } and { $vias } how many were put down; { $nets } lists the nets left unrouted.
pcb-autoroute-partial-cpu = Auto-routed in { $seconds } s on the CPU: { $tracks } tracks, { $vias } vias; not fully routed: { $nets }
# { $seconds } is how long it took, { $gpu } the graphics card's name; { $tracks } and { $vias } how many were put down; { $nets } lists the nets left unrouted.
pcb-autoroute-partial-gpu = Auto-routed in { $seconds } s on { $gpu }: { $tracks } tracks, { $vias } vias; not fully routed: { $nets }
# The title of the file picker for an SVG graphic.
pcb-lay-import-graphic-title = Import a graphic
# { $note } is a sentence saying which sketch the board outline follows.
pcb-outline-cut-out = { $note }. A rectangle drawn now cuts out of the board
# { $sketch } is a sketch's name, { $studio } its Part Studio's name.
pcb-outline-linked = The outline follows { $sketch } in { $studio }: edit the sketch, or break the link (the board's Derived feature's menu)
pcb-footprint-gone = The footprint is gone
# { $width } is the width new tracks get, in millimetres.
pcb-track-width = Track width { $width } mm

## crates/cadrs_app/src/eda/browser.rs
# The library browser's title, choosing a schematic symbol for power (VCC, GND…).
pcb-browser-choose-power-symbol = Choose a power symbol
pcb-browser-choose-symbol = Choose a symbol
pcb-browser-choose-footprint = Choose a footprint
# A tab of the library browser: the parts libraries on this computer.
pcb-browser-libraries = Libraries
# A tab of the library browser: JLCPCB's online catalogue of parts.
pcb-browser-jlcpcb-parts = JLCPCB parts
# The placeholder of the field that filters the list.
pcb-browser-filter = Filter
# A button: search JLCPCB's catalogue.
pcb-browser-search = Search
pcb-browser-add-to-library = Add to library
# { $keywords } are a symbol's search words.
pcb-browser-keywords = Keywords: { $keywords }
# { $footprint } is the footprint's library id.
pcb-browser-footprint = Footprint: { $footprint }
# { $count } is how many pads the footprint has.
pcb-browser-pads = { $count } pads
# { $symbol } is a symbol's id, { $count } how many pins it has.
pcb-browser-symbol-pins = { $symbol } · { $count } pins
# { $footprint } is a footprint's id, { $count } how many pads it has.
pcb-browser-footprint-pads = { $footprint } · { $count } pads
pcb-browser-loading = Loading its symbol and footprint…

## crates/cadrs_app/src/move_document.rs
# The dialog that moves tabs into another (new or existing) document.
io-move-title = Move to document
# A tab of the Move to document dialog: move into a new document.
io-move-tab-new = New document
# A tab of the Move to document dialog: move into one of the other documents.
io-move-tab-other = Other documents
io-move-document-name = Document name
# The button that moves the tabs.
io-move-confirm = Move
# The chosen document has no versions.
io-move-no-versions = No versions
# The chosen document has one version.
io-move-one-version = 1 version
# { $count } is how many versions the chosen document has.
io-move-versions = { $count } versions
# A button: choose another document to move to.
io-move-change = Change
io-move-version-name = Version name
# { $name } is the document the tabs move to.
io-move-target-note = The tabs are added to { $name }, and a version of it is made with them: this document then references that version.
io-move-one-tab = 1 tab will be moved
# { $count } is how many tabs move.
io-move-tabs = { $count } tabs will be moved
# { $count } is how many tabs move, with those they reference.
io-move-referenced-tabs = { $count } referenced tabs will be moved
io-move-show-details = Show details
io-move-hide-details = Hide details
# A tab's tooltip: the user picked it to move.
io-move-picked = Picked to move
io-move-referenced-tip = Referenced by a tab that moves. Untick it to leave it here: the moved tabs then reference it at a new version of this document.
io-move-left-behind = Tabs left here are referenced from the new location at a version of this document, made when you move.
io-move-status-one-new = Moving 1 tab to a new document
# { $count } is how many tabs move.
io-move-status-new = Moving { $count } tabs to a new document
# { $target } is the document the tab moves to.
io-move-status-one = Moving 1 tab to { $target }
# { $count } is how many tabs move, { $target } the document they move to.
io-move-status = Moving { $count } tabs to { $target }
# { $name } is the document the tabs went to, { $error } why this one couldn't be changed.
io-move-written-not-changed = { $name } was written, but this document couldn't be changed: { $error }
# { $version } is the version of the other document made with the moved tabs.
io-move-now-references = This document now references its { $version }.
# { $version } is a version of this document.
io-move-one-reference-left = 1 reference still points at { $version } here; update them in the Reference manager.
# { $count } is how many references, { $version } a version of this document.
io-move-references-left = { $count } references still point at { $version } here; update them in the Reference manager.
# { $version } is the new version of this document.
io-move-source-version = { $version } of this document was made for the tabs left here.
io-move-undo-note = Undo puts the tabs back here; the other document and the versions stay.
# A link in the toast: { $name } is the document the tabs moved to.
io-move-open-target = Open { $name }

## crates/cadrs_app/src/export_dialog.rs
# Length units a mesh file's numbers can be in.
io-unit-millimeter = Millimeter
io-unit-centimeter = Centimeter
io-unit-meter = Meter
io-unit-inch = Inch
io-unit-foot = Foot
# A mesh resolution: the largest chord distance (mm) and angle (°) between the mesh and the surface.
io-export-res-coarse = Coarse (0.5 mm, 30°)
# A mesh resolution: the largest chord distance (mm) and angle (°) between the mesh and the surface.
io-export-res-medium = Medium (0.1 mm, 15°)
# A mesh resolution: the largest chord distance (mm) and angle (°) between the mesh and the surface.
io-export-res-fine = Fine (0.02 mm, 5°)
# What is exported: { $name } is a tab's name or a list of part names.
io-export-what-one-part = { $name } (1 part)
# What is exported: { $name } is a tab's name or a list of part names, { $count } how many parts.
io-export-what-parts = { $name } ({ $count } parts)
# What is exported: { $part } is a part's name.
io-export-what-face = a face of { $part }
# { $what } says what is exported (names and how many parts, or a face).
io-export-source = Exporting: { $what }
# DWG is a file format; LibreDWG and the ODA File Converter are programs, PATH the system's list of program folders.
io-export-dwg-needs-converter = DWG needs LibreDWG or the ODA File Converter on PATH.
# Parasolid is a CAD file format.
io-export-no-parasolid = Parasolid is not offered (a proprietary format).
# Tooltips of the export formats (STEP AP214, IGES 5.3, BREP, STL, OBJ, DXF and DWG are file formats).
io-export-tip-step = STEP AP214: exact solids with part names
io-export-tip-iges = IGES 5.3: exact solids
# OpenCASCADE is a CAD kernel.
io-export-tip-brep = OpenCASCADE BREP (experimental): exact geometry only
io-export-tip-stl = STL mesh, for 3D printing
io-export-tip-obj = Wavefront OBJ mesh
io-export-tip-dxf = DXF in millimetres at full size, for cutting machines
# { $converter } is the converter program's name.
io-export-tip-dwg = DWG through the { $converter }
io-export-tip-dwg-none = DWG: no converter on PATH
# (OpenCASCADE, FreeCAD and CadQuery are programs; cadrs is this app; .cadrs its file type.)
io-export-brep-warning = BREP is OpenCASCADE's own format (FreeCAD, CadQuery and other OpenCASCADE applications read it), written by cadrs' kernel. It holds the exact geometry only: no part names, colours or assembly structure. Use STEP to keep those, or .cadrs to keep the editable document.
# Which kind of STL file: binary or text.
io-export-stl-format = STL format
# An STL file kind: binary.
io-export-stl-binary = Binary
# An STL file kind: plain text (ASCII).
io-export-stl-text = Text
# How finely surfaces are cut into triangles.
io-export-resolution = Resolution
# The largest distance between the mesh and the surface, in mm.
io-export-chord-tolerance = Chord tolerance (mm)
# The largest angle between neighbouring mesh triangles, in degrees.
io-export-angle-tolerance = Angle tolerance (°)
# DXF is a file format.
io-export-dxf-version = DXF version
io-export-y-up = Export models oriented Y axis up
io-export-keep-assembly = Keep assembly structure (each part once, with its instances)
io-export-individual = Export unique parts as individual files
io-export-enter-number = Enter a number
io-export-chord-too-small = Must be more than 0 mm
io-export-chord-too-large = At most 10 mm
io-export-angle-too-small = Must be more than 0°
io-export-angle-too-large = At most 90°
# Why an export failed (it follows "Export failed:").
io-export-no-document = no document
# Why an export failed (it follows "Export failed:"): no tab is open.
io-export-no-tab = no tab
# Why an export failed (it follows "Export failed:").
io-export-not-assembly = not an assembly
# Why an export failed (it follows "Export failed:").
io-export-not-3d = not a 3D export
# Why an export failed (it follows "Export failed:").
io-export-source-gone = the sketch or face is gone
io-export-nothing = Export failed: nothing to export

## crates/cadrs_app/src/pcb/dialogs.rs
# The dialog (and a help topic) that imports boards from ECAD (electronic CAD) files.
pcb-import-title = Import ECAD files
pcb-import-choose-files = Choose Files
pcb-import-no-file = No file chosen
# IDF is a board exchange format: .emn and .emp are its file extensions.
pcb-import-hint = Choose an IDF board file (.emn) and its library file (.emp).
# IDF is a board exchange format: .emn and .emp are its file extensions; KiCad is an ECAD program.
pcb-import-hint-kicad = Choose an IDF board file (.emn) and its library file (.emp), or a KiCad project (.kicad_pro).
# The file picker's title.
pcb-import-picker-title = Choose ECAD files
pcb-import-choose-first = Choose the board (.emn) and library (.emp) files first.
# { $errors } lists why the files couldn't be imported.
pcb-import-failed = Couldn't import: { $errors }
# { $boards } lists the imported boards; { $problem } is a sentence saying what went wrong.
pcb-imported-but = Imported { $boards }. { $problem }
# { $boards } lists the imported boards; { $warning } is the first warning, { $more } how many more there are.
pcb-imported-warning-more = Imported { $boards }. { $warning } (and { $more } more)
# { $boards } lists the imported boards (with their component counts).
pcb-imported-boards = Imported { $boards }
# The component folder's tooltip: { $folder } is a folder on the Documents page.
pcb-settings-folder-tip = Documents › { $folder }
pcb-select-folder = Select a folder
pcb-select-folder-ellipsis = Select a folder…
pcb-settings-title = PCB Studio settings
pcb-settings-library = Component library document
pcb-settings-library-tip = Choose the library document
# ECAD packages: the component packages (footprints) of electronic designs.
pcb-settings-library-hint = Holds the mappings from ECAD packages to their 3D representations. Every PCB Studio in this workspace uses it.
pcb-settings-folder = Create new component documents in this folder
pcb-settings-folder-hint = PCB Studio creates a document in this folder for each ECAD component it models. Everyone who uses this PCB Studio needs write access to it.
# A button: save the settings.
pcb-settings-update = Update
pcb-select-library-document = Select a library document
# The top of the picker's list: the user's own documents.
pcb-picker-my-documents = My documents
pcb-picker-new-library-document = New library document
pcb-picker-no-documents = No documents yet.
pcb-picker-no-folders = No folders yet: New folder makes one.
# The library built into cadrs.
pcb-default-library = Default library
# The version list's first item: the document as it is now.
pcb-custom-current-version = Current (latest changes)
# { $name } is the open document's name.
pcb-custom-this-document = { $name } (this document)
pcb-custom-title = Select custom part
# { $package } is an ECAD package's name.
pcb-custom-part-for = The 3D representation of { $package } in every PCB Studio that uses this library.
# The document the custom part comes from.
pcb-custom-document = Document
pcb-custom-no-parts = This document has no parts.
pcb-custom-choose-part = Choose a part first.
# The PCB Studio help's sections.
pcb-help-title = PCB Studio help
pcb-help-boards-title = Boards
pcb-help-boards = A PCB Studio holds any number of boards. + creates a new board (a 100 × 80 mm outline) and lets you type its name; double-click a board to rename it. Click a board under Boards to show it; the board shown is bold and blue. Right-click a board to delete it (Undo brings it back).
pcb-help-import = Click the Import ECAD files button, choose a board's .emn file and its .emp library (Ctrl+click picks both), then Import. Each import adds a board.
pcb-help-viewport-title = Viewport
# F is the keyboard key.
pcb-help-viewport = The board is green and components are coloured by package. Orbit with the right mouse button, pan with the middle button, zoom with the wheel; F fits the board. Click a component to select it.
pcb-help-components-title = Components
pcb-help-components = + creates a component and lets you type its name; double-click it to rename it. Click a board under Components to list its packages; click a package for its component view (the part alone on a grid). Click the board again to go back.
pcb-help-properties-title = Component properties
pcb-help-properties = The right edge's first button shows the selected component's part name, part number and representation: None, From ECAD data (a box from the outline and height) or Custom part (a part from a Part Studio, moved with Translate, Center and Rotate, then Accept). The choice is kept in the component library, so every PCB Studio follows.
pcb-help-bom = The right edge's second button lists the board's components grouped by part number. Click a row to select its components; double-click a designator to rename it.
pcb-help-search-title = Search
pcb-help-search = Type in Search and press Enter (or the magnifier): matching designators, packages, part numbers and boards light up. The arrows step through them; the cross clears.
pcb-help-settings-title = Settings
pcb-help-settings = The gear sets the component library document and the folder for component documents, for every PCB Studio in this workspace.

## crates/cadrs_app/src/pcb/panes.rs
# The pane showing the selected electronic component's properties.
pcb-pane-component = Component
pcb-component-hint = Click a component in the view to see its properties, or open one under Components.
pcb-part-name = Part name
pcb-part-number = Part number
# How a component is shown in 3D.
pcb-representation = Representation
# A representation: a box made from the ECAD data's outline and height.
pcb-rep-ecad = From ECAD data
pcb-rep-ecad-link = Open in the component view
# A representation: a part from a Part Studio.
pcb-rep-custom = Custom part
# A heading over the custom part's X, Y and Z offsets (a verb: move).
pcb-custom-translate = Translate
# A button: put the part in the middle of the footprint.
pcb-custom-center = Center
pcb-custom-center-tip = Put the part in the middle of the footprint, standing on the board
# A heading over the custom part's rotations about X, Y and Z.
pcb-custom-rotate = Rotate
pcb-custom-accept = Accept
pcb-custom-accept-tip = Accept the new position
# Bill of materials column: how many (quantity).
pcb-bom-qty = Qty
# Bill of materials column: the components' reference designators (R1, U2…).
pcb-bom-designator = Designator
pcb-bom-rename-tip = Double-click to rename
pcb-bom-list-tip = Double-click to list its components
pcb-bom-hint = Double-click a designator to rename it.
# { $designator } is a component's reference designator (R1, U2…), { $error } why it couldn't be renamed.
pcb-bom-rename-failed = Couldn't rename { $designator }: { $error }

## crates/cadrs_app/src/pcb/mod.rs
# A PCB Studio toolbar tooltip (IDF is a board exchange format).
pcb-tool-export = Export this board to IDF
# A PCB Studio toolbar tooltip (ECAD: electronic CAD).
pcb-tool-create-assembly = Create an assembly from this ECAD data
# The placeholder of the PCB Studio's search field.
pcb-search-placeholder = Search
pcb-search-tip = Search designators, packages, part numbers and boards
pcb-search-clear = Clear search
pcb-search-prev = Previous result
pcb-search-next = Next result
# A board's tooltip: { $board } is its name, { $sketch } the sketch its outline follows, { $studio } that sketch's Part Studio.
pcb-board-outline-follows = { $board }: its outline follows { $sketch } in { $studio }
# A board's tooltip: { $board } is its name, { $sketch } the sketch, { $studio } its Part Studio, { $problem } why the outline can't follow it.
pcb-board-outline-broken = { $board }: its outline no longer follows { $sketch } in { $studio }: { $problem }
# The + button's tooltip under Boards.
pcb-create-board = Create board
# The + button's tooltip under Components.
pcb-create-component = Create component
# { $name } is a component's name.
pcb-component-row-tip = { $name }: click to edit, double-click to rename, right-click for more
# { $board } is a board's name, { $count } how many components it has.
pcb-board-components-tip = { $board }: { $count } components
# { $package } is a component package's name, { $count } how many are placed on the board.
pcb-package-placed = { $package }: { $count } placed
# { $package } is a component package's name, { $number } its part number, { $count } how many are placed.
pcb-package-number-placed = { $package }, part number { $number }: { $count } placed
pcb-unlink-outline = Break outline link
# A disabled menu item: { $problem } is why the board's outline can't follow its sketch.
pcb-outline-problem = Outline link: { $problem }
# A menu item: { $sketch } is a sketch's name, { $studio } its Part Studio's name.
pcb-link-outline-to = Link outline to { $sketch } ({ $studio })
pcb-delete-board = Delete this board
# { $name } is the deleted board's name.
pcb-deleted-board = Deleted board { $name }.
# { $name } is the copied component's name.
pcb-paste-component-tip = Adds a copy of { $name }
# A menu item: edit the component (a verb).
pcb-edit-component = Edit
pcb-add-to-my-library = Add to my library
# { $library } is the user library's name.
pcb-add-to-my-library-tip = Keeps it in { $library } for every studio
# { $name } is the copied component's name.
pcb-copied-component = Copied { $name }: right-click Components in any studio to paste it.
# { $name } is a component's name, { $id } its id in the library.
pcb-added-to-library = Added { $name } to your library as { $id }.
# { $name } is a component's name, { $error } why it couldn't be added.
pcb-add-to-library-failed = Couldn't add { $name } to your library: { $error }
# { $name } is the deleted component's name.
pcb-deleted-component = Deleted component { $name }.
pcb-empty-studio = Import an ECAD file, or sync a Part Studio or assembly, from the toolbar to get started.

## crates/cadrs_app/src/eda/mod.rs
# The PCB editor's mode bar: the component's schematic symbol.
pcb-mode-symbol = Symbol
# The PCB editor's mode bar: the component's footprint (its pads on the board).
pcb-mode-footprint = Footprint
# The PCB editor's mode bar: the 3D view.
pcb-mode-3d = 3D
# The PCB editor's mode bar: the circuit's schematic.
pcb-mode-schematic = Schematic
# The PCB editor's mode bar: the board layout.
pcb-mode-layout = Layout

## crates/cadrs_app/src/eda/part_dialogs.rs
# A schematic pin's electrical type (as KiCad names them).
pcb-pin-type-input = Input
pcb-pin-type-output = Output
pcb-pin-type-bidirectional = Bidirectional
pcb-pin-type-tri-state = Tri-state
pcb-pin-type-passive = Passive
pcb-pin-type-free = Free
pcb-pin-type-unspecified = Unspecified
pcb-pin-type-power-input = Power input
pcb-pin-type-power-output = Power output
pcb-pin-type-open-collector = Open collector
pcb-pin-type-open-emitter = Open emitter
pcb-pin-type-unconnected = Unconnected
# The direction a schematic pin points.
pcb-pin-dir-right = Right
pcb-pin-dir-left = Left
pcb-pin-dir-up = Up
pcb-pin-dir-down = Down
# A pad type: plated, through the board.
pcb-pad-kind-through-hole = Through-hole
# A pad type: surface-mounted.
pcb-pad-kind-smd = SMD
# A pad type: a non-plated (mechanical) hole.
pcb-pad-kind-npth = NPTH, mechanical
pcb-pad-shape-circular = Circular
pcb-pad-shape-rectangular = Rectangular
pcb-pad-shape-oval = Oval
pcb-pad-shape-rounded-rectangle = Rounded rectangle
# How a component is mounted: its leads go through holes.
pcb-mount-through-hole = Through hole
# How a component is mounted: on the surface.
pcb-mount-smd = SMD
# How a component is mounted: not said.
pcb-mount-unspecified = Unspecified
# A side of a symbol's box, where pins go.
pcb-side-left = Left
pcb-side-right = Right
pcb-side-top = Top
pcb-side-bottom = Bottom
pcb-pin-props = Pin properties
pcb-pin-name = Pin name
pcb-pin-number = Pin number
pcb-pin-electrical-type = Electrical type
# The direction the pin points.
pcb-pin-orientation = Orientation
# mils: thousandths of an inch.
pcb-pin-x = X position (mils)
pcb-pin-y = Y position (mils)
pcb-pin-length = Pin length (mils)
# { $pins } lists pin numbers.
pcb-pins-off-grid = Pin { $pins } is off the 50 mil grid: it couldn't connect to wires
pcb-sym-name = Symbol name
# The letters a component's reference starts with (R, C, U…).
pcb-sym-reference-prefix = Reference prefix
# The footprint field of a symbol (a library id).
pcb-sym-footprint = Footprint
# { $library } is this studio's library name; <name> stands for a footprint's name.
pcb-sym-footprint-hint = A footprint of this studio: { $library }:<name>. Empty: the component's own footprint.
# Search words for the symbol.
pcb-sym-keywords = Keywords
pcb-sym-show-pin-numbers = Show pin numbers
pcb-sym-show-pin-names = Show pin names
pcb-pad-props = Pad properties
pcb-pad-number = Pad number
pcb-pad-type = Pad type
pcb-pad-shape = Pad shape
pcb-pad-x = Position X (mm)
pcb-pad-y = Position Y (mm)
pcb-pad-size-x = Size X (mm)
pcb-pad-size-y = Size Y (mm)
# The drilled hole's diameter.
pcb-pad-hole = Hole diameter (mm)
pcb-pad-gone = The pad is gone
pcb-fp-name = Footprint name
# How the component is mounted on the board.
pcb-fp-component-type = Component type
pcb-rect-props = Rectangle properties
pcb-rect-start-x = Start X (mm)
pcb-rect-start-y = Start Y (mm)
pcb-rect-end-x = End X (mm)
pcb-rect-end-y = End Y (mm)
pcb-rect-line-width = Line width (mm)
pcb-shape-gone = The shape is gone
# The 3D model's file.
pcb-model-file = Model file
pcb-model-offset-x = Offset x (mm)
pcb-model-offset-y = Offset y (mm)
pcb-model-offset-z = Offset z (mm)
pcb-model-rotation-x = Rotation x (°)
pcb-model-rotation-y = Rotation y (°)
pcb-model-rotation-z = Rotation z (°)
pcb-model-scale = Scale
pcb-model-opacity = Opacity
# { $field } is the field's (technical) name.
pcb-not-a-number = { $field }: not a number
# The file picker's title.
pcb-model-picker-title = Choose a 3D model
# { $name } is a file, { $error } why it couldn't be read.
pcb-couldnt-read = Couldn't read { $name }: { $error }
# The quoted example is pin numbers and names (keep it as it is).
pcb-pin-table-hint = Number, name, electrical type and side of each pin. Add several at once: “1 ANT, 2 GND, 3 3.3V”.
pcb-pin-delete = Delete this pin
pcb-array-no-pad = Add a pad to make an array from
# How the pads of an array are laid out.
pcb-array-arrangement = Arrangement
pcb-array-line = In a line
# Pads arranged in a circle around the footprint's origin.
pcb-array-round = Round the origin
# How many pads the array has, the first one included.
pcb-array-count = Pads in all
pcb-array-step-x = Step X (mm)
pcb-array-step-y = Step Y (mm)
pcb-array-angle-step = Angle step (°)
pcb-array-hint = A line steps by X and Y; a circle turns about the footprint's origin by the angle. Pads are numbered on.
pcb-array-count-whole = Pads in all: a whole number
pcb-array-angle-number = Angle step: a number
pcb-renumber-first = First number
# The order pads are numbered in.
pcb-renumber-order = Order
# ICs: integrated circuits (chips).
pcb-renumber-ccw = Counter-clockwise (ICs)
pcb-renumber-rows = Row by row from the top left
pcb-renumber-first-whole = First number: a whole number

## crates/cadrs_app/src/eda/sch_dialogs.rs
# A schematic label naming the net (electrical connection) it is placed on.
pcb-net-label = Net label
# A schematic label joining every sheet's net of its name.
pcb-global-label = Global label
# The field holding a label's name.
pcb-sch-label = Label
# The dialog that finds a symbol in the schematic.
pcb-find = Find
pcb-find-reference-or-value = Reference or value
# { $text } is what was searched for.
pcb-find-nothing = Nothing matches “{ $text }”
# { $reference } is the symbol's reference (R1, U2…).
pcb-sym-props-of = Symbol properties: { $reference }
# The footprint field's placeholder while it has none.
pcb-props-footprint-none = None: choose one
# A button opening the footprint browser.
pcb-choose-ellipsis = Choose…
pcb-wire-colour = Wire colour
# { $count } is how many wires are connected to it (they all change unless this is ticked).
pcb-wire-only = Only this wire (else all { $count } connected)
# Filter the footprints by the symbol's own footprint filters.
pcb-assign-by-symbol = Symbol's filters
# Filter the footprints by their pin count.
pcb-assign-by-pins = Pin count
pcb-assign-filter = Filter footprints
# ERC: checks the schematic's connections for errors.
pcb-erc-title = Electrical rules checker
pcb-erc-run = Run ERC
pcb-erc-none = No violations
# { $count } is how many violations, { $errors } of them errors and { $warnings } warnings.
pcb-erc-summary = Violations ({ $count }): { $errors } errors, { $warnings } warnings
# { $message } says what is wrong.
pcb-error-message = Error: { $message }
# { $message } says what is wrong.
pcb-warning-message = Warning: { $message }
# Bill of materials column: the components' references (R1, U2…).
pcb-bom-reference = Reference
# DNP: do not populate (a component left off the assembled board).
pcb-bom-dnp = DNP
# A button: export the bill of materials as a CSV file.
pcb-bom-export = Export…
# The folder picker's title (BOM: bill of materials).
pcb-bom-export-to = Export the BOM to
# { $path } is the written file.
pcb-wrote = Wrote { $path }
# { $name } is a file, { $error } why it couldn't be written.
pcb-couldnt-write = Couldn't write { $name }: { $error }
pcb-plot-schematic = Plot schematic
pcb-plot-output-dir = Output directory
# A button: write the netlist (the circuit's connections) as a CSV file.
pcb-plot-netlist = Netlist (CSV)
pcb-no-board = No board
pcb-sym-fields = Symbol fields
# A column: the part's LCSC (JLCPCB) part number.
pcb-fields-lcsc = LCSC part

## crates/cadrs_app/src/eda/lay_dialogs.rs
pcb-lay-update-title = Update PCB from schematic
pcb-update-relink = Re-link footprints to symbols by reference
pcb-update-replace = Replace footprints with those the symbols name
pcb-update-delete = Delete footprints with no symbol
pcb-update-fields = Update footprint fields from symbols
# A button: run the update.
pcb-update-run = Update PCB
# Board design rules (all lengths).
pcb-rule-min-clearance = Minimum clearance
pcb-rule-min-track = Minimum track width
# The copper ring around a hole.
pcb-rule-min-annular = Minimum annular width
pcb-rule-min-via = Minimum via diameter
pcb-rule-hole-clearance = Copper to hole clearance
pcb-rule-edge-clearance = Copper to edge clearance
pcb-rule-min-drill = Minimum drill size
pcb-rule-hole-to-hole = Hole to hole clearance
# A heading: the board's layer stack.
pcb-setup-stackup = Physical stackup
pcb-setup-copper-layers = Copper layers
pcb-setup-thickness = Board thickness
# A heading over the design rules.
pcb-setup-constraints = Constraints
# A heading: { $name } is a net class's name (Default).
pcb-setup-net-class = Net class { $name }
pcb-class-clearance = Clearance
pcb-class-track-width = Track width
pcb-class-via-size = Via size
pcb-class-via-hole = Via hole
pcb-no-default-net-class = No Default net class
pcb-zone-props = Copper zone properties
# A net: an electrical connection of the circuit.
pcb-net = Net
# A zone's net when it takes the net of what is wired to it.
pcb-zone-net-from-wiring = (from what is wired to it)
# A board layer.
pcb-layer = Layer
pcb-top-copper = Top copper
pcb-bottom-copper = Bottom copper
pcb-both-copper = Both copper layers
pcb-zone-gone = The zone is gone
pcb-text-height = Height
# The width of the strokes text is drawn with.
pcb-text-stroke-width = Stroke width
# How text lines up (left, centre, right).
pcb-text-alignment = Alignment
pcb-align-left = Left
pcb-align-centre = Centre
pcb-align-right = Right
# { $layer } is a board layer's name.
pcb-text-layer-note = On { $layer } (pick another layer in the layers panel).
pcb-text-lengths = Height and stroke width are lengths in mm
# The dialog's title: { $layer } is a board layer's name.
pcb-text-on = Text on { $layer }
pcb-text-drag-note = Drag the text to move it (Select: press on it once selected).
# Del is the keyboard key.
pcb-text-empty = The text is empty (Del removes it)
pcb-text-gone = The text is gone
pcb-keepout-props = Keep-out properties
# What a keep-out area keeps out (checkboxes).
pcb-keepout-tracks = No tracks
pcb-keepout-vias = No vias
pcb-keepout-pads = No pads
pcb-keepout-pour = No copper pours
pcb-keepout-footprints = No footprints
# DRC: the design rules check.
pcb-drc-refill = Refill all zones before performing DRC
pcb-drc-run = Run DRC
# { $count } is how many violations ({ $errors } errors, { $warnings } warnings); { $unconnected } how many connections are still missing.
pcb-drc-summary = Violations ({ $count }): { $errors } errors, { $warnings } warnings. Unconnected items: { $unconnected }
# { $net } is a net's name; { $from } and { $to } are the ends' (x, y) positions in mm.
pcb-drc-unconnected = Unconnected: { $net } { $from } – { $to }
# The dialog that writes the board's fabrication files.
pcb-plot-title = Plot
# Gerber X2, Excellon, CPL and STEP are file formats; BOM: bill of materials.
pcb-plot-hint = Gerber X2: copper, board outline, solder mask, silkscreen and paste layers, with a job file. Drill files: Excellon, plated and not. Assembly: JLCPCB's BOM and placement (CPL) files. STEP: the board with its parts' 3D models.
# A button: write JLCPCB's bill of materials and placement files.
pcb-plot-assembly = JLCPCB BOM and placement
pcb-plot-drill = Generate drill files
# A button: write the Gerber files.
pcb-plot-run = Plot
# { $file } is a written file's name.
pcb-plotted = Plotted { $file }
# { $file } is the written job file's name.
pcb-created-job = Created Gerber job file { $file }
# { $file } is a written file's name.
pcb-created-file = Created { $file }
# The dialog's title: { $reference } is the footprint's reference (R1, U2…).
pcb-fp-props-of = Footprint properties: { $reference }
pcb-fprops-x = Position X
pcb-fprops-y = Position Y
# Which side of the board the footprint is on.
pcb-fprops-side = Side
# A side of the board.
pcb-board-side-top = Top
# A side of the board.
pcb-board-side-bottom = Bottom
pcb-fprops-show-model = Show its 3D model
pcb-fprops-values = Position is a length (mm), rotation an angle (°)
# The dialog's title: { $sketch } is a sketch's name.
pcb-create-board-from = Create PCB board from { $sketch }
# The PCB Studio the new board goes into.
pcb-create-board-studio = PCB Studio
# A choice: make a new PCB Studio tab for the board.
pcb-new-pcb-studio = New PCB Studio
# Which of the board's faces lies on the sketch's plane.
pcb-create-board-face = Face on the sketch
pcb-create-board-bottom-face = Bottom face (it stands on it)
pcb-create-board-top-face = Top face (it hangs under it)
# What circles inside the outline become.
pcb-create-board-circles = Circles inside it
pcb-create-board-cut-outs = Cut-outs of the outline
# NPTH: non-plated through holes.
pcb-create-board-holes = Mounting holes (NPTH)
pcb-create-board-note = The outline (and the holes) follow the sketch.
pcb-create-board-thickness = The thickness is a length above 0 (mm)
pcb-move-by-select = Select the footprints to move
# How many footprints move (more than four).
pcb-move-by-footprints = { $count } footprints
# The dialog's title: { $what } lists the footprints' references, or says how many there are.
pcb-move-by-title = Move by: { $what }
pcb-move-by-x = Along X
pcb-move-by-y = Along Y
pcb-move-by-lengths = Type the distances as lengths (mm)
pcb-a-footprint-gone = A footprint is gone
# The dialog's title: { $layer } is a board layer's name.
pcb-shape-on = Shape on { $layer }
pcb-shape-line-width = Line width
# A checkbox: the shape is filled.
pcb-shape-filled = Filled
# A shape's net: none.
pcb-net-none = (none)
pcb-shape-width-length = The line width is a length in mm

## crates/cadrs_app/src/board_fit.rs
# A label at a component that runs into a part around the board.
pcb-fit-in-the-way = In the way
# Board edit bar buttons (a board fitted into a Part Studio or assembly).
pcb-fit-solid = Solid parts
pcb-fit-solid-tip = Show the board's components solid (while the layout is edited they are see-through, so it shows under them)
# The parts of the model around the board.
pcb-fit-context = Parts around
pcb-fit-context-tip = Show the parts around the board's components, and the pins and holes square to it
pcb-fit-snap = Snap
pcb-fit-snap-tip = Snap a moved footprint's pads onto pins and its part's edges onto the parts around
# A button: align a footprint's part with a target (a verb).
pcb-fit-align = Align
pcb-fit-align-tip = Move a footprint so an entity of its part meets a target: pick the part's face, edge or pin, then the target
pcb-fit-keepouts = Keep-outs
pcb-fit-keepouts-tip = Keep-out areas on the layout where the parts around stand
pcb-fit-align-from = Align: pick a face, edge or pin of a component
pcb-fit-align-to = Align: pick where it goes (a face, edge, pin, hole or mate connector)
pcb-fit-align-pick-first = Pick a face, edge or pin of one of the board's components first
pcb-fit-align-cant-meet = They can't meet by moving the footprint on the board: pick a pin or hole square to the board, or faces square to it
# { $reference } is a footprint's reference (R1, U2…), { $distance } a distance in mm.
pcb-fit-align-under = { $reference } is { $distance } mm under it, square to the board
# { $reference } is a footprint's reference (R1, U2…), { $distance } a distance in mm.
pcb-fit-align-over = { $reference } is { $distance } mm over it, square to the board
# { $reference } is a footprint's reference, { $others } lists the footprints it overlaps.
pcb-fit-courtyard-overlaps = { $reference }'s courtyard overlaps { $others }
# { $reference } is a footprint's reference, { $others } lists what its pads are too close to.
pcb-fit-pads-too-close = { $reference }'s pads are too close to { $others }
# { $notes } lists what is still off, separated by semicolons.
pcb-fit-aligned = Aligned. { $notes }
pcb-fit-nothing-around = Nothing around stands over or under the board
# What Keep-outs did (it follows "Parts around:").
pcb-fit-keepouts-removed = the keep-outs from the parts around removed: nothing stands over or under the board now
pcb-fit-keepouts-top-one = 1 keep-out on the top side
# { $top } is how many keep-outs were made on the top side.
pcb-fit-keepouts-top = { $top } keep-outs on the top side
# Shift+N shows the board from below.
pcb-fit-keepouts-bottom-one = 1 keep-out on the bottom side (seen from below: Shift+N)
# { $bottom } is how many keep-outs were made on the bottom side; Shift+N shows the board from below.
pcb-fit-keepouts-bottom = { $bottom } keep-outs on the bottom side (seen from below: Shift+N)
# { $bottom } is how many keep-outs were made on the bottom side.
pcb-fit-keepouts-both-one = 1 keep-out on the top side, { $bottom } on the bottom
# { $top } and { $bottom } are how many keep-outs were made on each side.
pcb-fit-keepouts-both = { $top } keep-outs on the top side, { $bottom } on the bottom
# { $what } says which keep-outs were made.
pcb-fit-keepouts-summary = Parts around: { $what }
# { $what } says which keep-outs were made; { $count } is how many old ones they replace.
pcb-fit-keepouts-summary-replacing = Parts around: { $what } (replacing { $count })

## crates/cadrs_app/src/board_in_studio.rs
# { $name } is a board's name (IDF is a board exchange format: it carries no layout).
pcb-board-from-idf = { $name } came from IDF: it has no layout to edit here (edit it where it was designed, then import it again)
# { $name } is a board's name.
pcb-board-gone = { $name } is gone
# Why a board's layout can't be edited: the feature isn't a Derived feature.
pcb-not-derived = not a Derived feature
# Why a board's layout can't be edited: the Derived feature has no source.
pcb-no-source = no source
# Why a board's layout can't be edited: the Derived feature brings no board.
pcb-not-a-board = not a board
# { $name } is a board's name.
pcb-board-at-version = { $name } is referenced at a version: open the linked document to edit its layout
# The board edit bar's title: { $board } is the board's name, { $studio } its PCB Studio's.
pcb-board-edit-title = Layout of { $board } ({ $studio })
pcb-fit-opacity-tip = How see-through the parts around are
pcb-fit-move-by = Move by…
# Shift+M is the keyboard shortcut.
pcb-fit-move-by-tip = Move the selected footprints by typed distances (Shift+M)
pcb-fit-open-studio = Open in PCB Studio
pcb-fit-open-studio-tip = Show the board in its PCB Studio
# Esc is the key.
pcb-fit-done-tip = Stop editing the layout (Esc)
# { $parts } lists each part run into with the components (by reference) in its way.
pcb-fit-run-into = ⚠ Components run into { $parts }
pcb-sketch-no-region = The sketch has no closed region to make a board from
pcb-no-sketch-before-board = No sketch with a closed region comes before the board

## crates/cadrs_app/src/cam/dialogs/mod.rs
cam-no-part-studio = No Part Studio to take parts from
cam-create-machine-first = Create the job's machine first
cam-create-setup-first = Create a setup first
cam-create-tool-first = Create a tool above the toolpath first
cam-tool-no-patterns = The tool has no toolpath patterns on this machine
cam-document-no-version = The document has no version
cam-component-newest = The component is at the newest version
cam-still-reading-parts = Still reading the Part Studio's parts
# The heading of a CAM dialog's list of warnings.
cam-warnings = Warnings
