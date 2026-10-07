//! cadrs_ui: the cadrs widget library on top of `bevy_ui` and `bevy_ui_widgets`.
//!
//! The component set, builder API and interaction details follow gpui-component
//! (`~/.cargo/registry/src/*/gpui-component-0.6.6/src/`): every widget is a builder that ends in
//! `.build(&theme)` and returns a bundle. Every widget takes a name, which becomes its `Name` so
//! scripted scenarios can find it.
//!
//! Components: [`Button`], [`IconButton`], [`TextInput`], [`Menu`]/[`MenuItem`], [`Dialog`],
//! [`ListItem`], [`GridItem`], [`Tooltip`], [`icon`], [`TableHeader`]/[`TableRow`],
//! [`Collapsible`], [`Spinner`], the sketch diagnostics' [`FloatingPanel`], [`Switch`] and
//! [`ActionRow`] (P3D.2), [`Avatar`], context menus ([`open_context_menu`]), document
//! [`Tab`]s, [`ToolButton`]/[`Kbd`], [`TreeItem`], [`DockPanel`] and in-place editing
//! ([`begin_inline_edit`]), feature dialogs ([`FeatureDialog`]) with [`SelectionField`]s and
//! [`Checkbox`]es, colour swatches and the [`ColorMixer`] (P3.5), toast [`Notification`]s and the sketch [`QuickDim`] value box and the [`DimEdit`] dimension editor. Tokens
//! live in [`Theme`].

pub mod action_row;
pub mod anim;
pub mod avatar;
pub mod button;
pub mod checkbox;
pub mod collapsible;
pub mod command_palette;
pub mod color_legend;
pub mod color_picker;
pub mod cursor;
pub mod dialog;
pub mod dialog_fields;
pub mod dim_edit;
pub mod dock;
pub mod doc_details;
pub mod document_browser;
pub mod ellipsis;
pub mod entry_list;
pub mod feature_dialog;
pub mod file_picker;
pub mod floating_panel;
pub mod fonts;
pub mod glyphs;
pub mod gallery;
pub mod icon;
pub mod inline_edit;
pub mod input;
pub mod list;
pub mod quick_dim;
pub mod radio;
pub mod scrollbar;
pub mod selection_field;
pub mod selection_list;
pub mod menu;
pub mod name_popup;
pub mod path_field;
pub mod spinner;
pub mod splitter;
pub mod style;
pub mod switch;
pub mod symbols;
pub mod surface;
pub mod tab_strip;
pub mod text_dialog;
pub mod table;
pub mod tabs;
pub mod tag;
pub mod theme;
pub mod timeline;
pub mod toast;
pub mod toolbar;
pub mod tooltip;
pub mod tree;
pub mod version_graph;

use bevy::prelude::*;

pub use action_row::{ActionRow, ActionRowAction};
pub use anim::{FadeIn, FinishAnimations};
pub use floating_panel::{FloatingPanel, FloatingPanelBody, FloatingPanelClose, panel_caption};
pub use switch::{Switch, SwitchChange, SwitchState};
pub use avatar::Avatar;
pub use color_legend::{ColorLegend, color_map};
pub use color_picker::{ColorMixer, ColorMixerChange, ColorMixerState, ColorSwatch, SwatchColor, SwatchSelected};
pub use command_palette::{CommandPalette, CommandPaletteClose, CommandPaletteLayer, CommandPaletteResults, palette_row};
pub use collapsible::{Collapsible, CollapsibleState, CollapsibleToggled};
pub use spinner::Spinner;
pub use table::{Column, ColumnSort, TableBody, TableColumnResize, TableHeader, TableRoot, TableRow, TableSortChange};
pub use button::{Button, ButtonSize, ButtonVariant, IconButton};
pub use dialog::{Dialog, DialogClose, DialogRoot};
pub use dialog_fields::{
    NumberField, NumberFieldCancel, NumberFieldCommit, NumberFieldEdit, NumberFieldState, OptionRow,
    Select, SelectChange, SelectState, Slider, SliderChange, SliderState, form_row,
};
pub use entry_list::{
    Entry, EntryGroup, EntryGroupAction, EntryGroupActivate, EntryGroupState, EntryRemove, EntryState, EntryToggled,
};
pub use dock::{DockGrip, DockPanel, DockPanelResized, DockPanelState, DockPanelToggled, ToggleDockPanel};
pub use inline_edit::{
    DoubleClick, DoubleClickable, InlineEdit, InlineEditCancel, InlineEditCommit,
    InlineEditLabel, InlineEditOptions, begin_inline_edit,
};
pub use tabs::Tab;
pub use tag::Tag;
pub use tab_strip::{TabStrip, TabStripSelect, TabStripState, select_tab};
pub use toolbar::{Kbd, ToolButton, toolbar_separator};
pub use tree::{TreeItem, TreeRowToggle, TreeRowToggled, TreeToggle, tree_guide};
pub use icon::{Icon, IconAtlas, icon};
pub use input::{CaretBlinkOverride, TextCancel, TextInput, TextInputField, TextSubmit};
pub use list::{GridItem, ListItem};
pub use menu::{
    ContextMenuRequested, ContextMenuTarget, Menu, MenuAction, MenuEntry, MenuItem,
    close_all_menus, open_context_menu, open_menu,
};
pub use style::{ForceState, InheritFg, Selected, StateColors, VisualState, Visuals};
pub use surface::RenderSurface;
pub use theme::Theme;
pub use timeline::{TimelineMarker, TimelineRow, TimelineRowState};
pub use version_graph::{VersionGraph, VersionGraphNode, VersionGraphSelect, VersionNode, VersionNodeKind};
pub use document_browser::{BrowserSearch, DocumentRow, OpenedHeader, location_row};
pub use doc_details::{LabelChip, details_caption, details_divider, details_header, details_value, side_tab};
pub use toast::{
    Notification, ToastHost, close_toasts, close_transient_toasts, show_notification, show_toast, show_toast_for,
    toast_action,
};
pub use checkbox::{Checkbox, CheckboxChange, CheckboxState};
pub use radio::{RadioChange, RadioGroup, RadioGroupState, RadioOption};
pub use path_field::{PathField, PathFieldBrowse, path_field_value, set_path_field, set_path_field_display};
pub use file_picker::{FilePicked, FilePickerState, FilesPicked, open_file_picker, open_files_picker};
pub use feature_dialog::{
    FeatureDialog, FeatureDialogAccept, FeatureDialogCancel, FeatureDialogState,
};
pub use selection_field::{
    SelectionField, SelectionFieldActivate, SelectionFieldClear, SelectionFieldState,
};
pub use selection_list::{
    SelectionList, SelectionListActivate, SelectionListItem, SelectionListMove, SelectionListRemove, SelectionListReorder,
    SelectionListReplace, SelectionListReplaceable, SelectionListState,
};
pub use quick_dim::{
    QuickDim, QuickDimBox, QuickDimCancel, QuickDimCommit, QuickDimField, start_typing as quick_dim_start_typing,
};
pub use dim_edit::{DimEdit, DimEditBox, DimEditCancel, DimEditCommit, DimEditField};
pub use tooltip::{Tooltip, TooltipStyle};
pub use name_popup::{NamePopup, NamePopupCancel, NamePopupCommit};
pub use text_dialog::{TextDialog, TextDialogState};
pub use scrollbar::{horizontal_scrollbar, vertical_scrollbar};
pub use splitter::{HoverCursor, Splitter, SplitterMoved, horizontal_splitter};
pub use cursor::{CursorKind, CursorRequest, CursorState};

/// A named action sent by a scripted scenario (`Custom("...")` steps), for set-ups that would
/// take hundreds of clicks, such as filling a sketch with 500 entities. The app handles the
/// commands it knows; the text is `name arg arg…`.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct ScriptCommand(pub String);

/// Background work the screen is still waiting for (drawing views being projected, P3C.7, a
/// rebuild, a view animation, a section's caps): scripted steps wait until it is done, so they
/// never act on, or catch, a half-finished screen. Cleared in `PreUpdate` every frame; the app's
/// systems set it later in the frame while they have work in flight.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PendingWork(pub bool);

/// What set [`PendingWork`] this frame ("rebuild", "view animation", …), for a scripted step
/// that times out waiting. Cleared with it.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub struct PendingWhy(pub Vec<&'static str>);

impl PendingWhy {
    pub fn add(&mut self, why: &'static str) {
        if !self.0.contains(&why) {
            self.0.push(why);
        }
    }
}

/// Commonly used items, including Bevy's `Activate` event and the `observe` bundle helper.
pub mod prelude {
    pub use crate::{
        Avatar, Button, ButtonSize, ButtonVariant, Collapsible, Column, ColumnSort,
        ContextMenuRequested, ContextMenuTarget, Dialog, DialogClose, GridItem, IconButton,
        ListItem, Menu, MenuAction, MenuEntry, MenuItem, RenderSurface, Selected, Spinner,
        TableHeader, TableRow, TableSortChange, TextCancel, TextInput, TextSubmit, Theme, Tooltip,
        VisualState, icon, open_context_menu, open_menu,
    };
    pub use crate::{
        Checkbox, DockPanel, DoubleClick, FeatureDialog, InlineEditCommit, Kbd, SelectionField, Tab,
        ToolButton, TreeItem,
        toolbar_separator,
    };
    pub use bevy::ui_widgets::{Activate, observe};
}

/// Global z-order layers for overlays.
pub mod z {
    pub const DIALOG: i32 = 100;
    pub const MENU: i32 = 200;
    pub const TOOLTIP: i32 = 300;
}

/// Adds the theme, fonts, icons and all widget systems.
pub struct CadrsUiPlugin;

impl Plugin for CadrsUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Theme>()
            .init_resource::<PendingWork>()
            .init_resource::<PendingWhy>()
            .add_systems(PreUpdate, |mut p: ResMut<PendingWork>, mut why: ResMut<PendingWhy>| {
                p.set_if_neq(PendingWork(false));
                if !why.0.is_empty() {
                    why.0.clear();
                }
            })
            .init_resource::<RenderSurface>()
            .insert_resource(ClearColor(Theme::default().viewport_background))
            .add_plugins(bevy::input_focus::tab_navigation::TabNavigationPlugin)
            .add_plugins((
                fonts::FontsPlugin,
                icon::IconPlugin,
                style::StylePlugin,
                anim::AnimPlugin,
                input::InputPlugin,
                menu::MenuPlugin,
                dialog::DialogPlugin,
                tooltip::TooltipPlugin,
                table::TablePlugin,
                collapsible::CollapsiblePlugin,
                spinner::SpinnerPlugin,
                toast::ToastPlugin,
                inline_edit::InlineEditPlugin,
                tree::TreePlugin,
                dock::DockPlugin,
            ))
            .add_plugins((
                checkbox::CheckboxPlugin,
                selection_field::SelectionFieldPlugin,
                selection_list::SelectionListPlugin,
                feature_dialog::FeatureDialogPlugin,
                quick_dim::QuickDimPlugin,
                dim_edit::DimEditPlugin,
                cursor::CursorPlugin,
                tab_strip::TabStripPlugin,
                scrollbar::ScrollbarPlugin,
                dialog_fields::DialogFieldsPlugin,
                text_dialog::TextDialogPlugin,
                color_picker::ColorPickerPlugin,
                symbols::SymbolsPlugin,
                ellipsis::EllipsisPlugin,
                command_palette::CommandPalettePlugin,
            ))
            .add_plugins(file_picker::FilePickerPlugin)
            .add_plugins(radio::RadioPlugin)
            .add_plugins(entry_list::EntryListPlugin)
            .add_plugins(name_popup::NamePopupPlugin)
            .add_plugins((floating_panel::FloatingPanelPlugin, switch::SwitchPlugin, version_graph::VersionGraphPlugin, splitter::SplitterPlugin))
            .add_message::<ScriptCommand>();
    }
}
