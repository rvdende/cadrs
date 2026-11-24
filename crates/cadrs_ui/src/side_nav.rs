//! A side navigation: a column of destinations under small group headings, one of them
//! selected, like the section list of a settings window or the modules of an admin page
//! (gpui-component's `Sidebar` with `SidebarGroup` and `SidebarMenuItem`).
//!
//! The column gets the builder's name and each item `<name>-<key>`. Clicking an item selects it
//! and triggers [`SideNavSelect`] on the column; the owner shows the destination. To select from
//! code (or to show none selected, while a search's results are up), set [`SideNavState`].
//!
//! An item can carry a count at its right edge ([`SideNav::count`]): the open requests, the
//! unread messages.

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::Activate;

use crate::list::ListItem;
use crate::style::Selected;
use crate::theme::Theme;

pub struct SideNavPlugin;

impl Plugin for SideNavPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_item)
            .add_systems(PostUpdate, sync_selection);
    }
}

/// An item was chosen. Targets the column.
#[derive(EntityEvent, Clone, Debug)]
pub struct SideNavSelect {
    pub entity: Entity,
    /// The item's place among the items (headings not counted).
    pub index: usize,
    pub key: String,
}

/// The column's selected item (by its place among the items), or none.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SideNavState {
    pub selected: Option<usize>,
}

/// One item of a column.
#[derive(Component, Debug, Clone)]
struct SideNavItem {
    nav: Entity,
    index: usize,
    key: String,
}

enum Entry {
    Group(String),
    Item {
        key: Cow<'static, str>,
        icon: Option<Cow<'static, str>>,
        label: String,
        count: Option<String>,
    },
}

/// Builder for a side navigation.
pub struct SideNav {
    name: Cow<'static, str>,
    entries: Vec<Entry>,
    selected: Option<usize>,
}

impl SideNav {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            entries: Vec::new(),
            selected: None,
        }
    }

    /// A heading over the items that follow, shown in small capitals.
    pub fn group(mut self, label: impl Into<String>) -> Self {
        self.entries.push(Entry::Group(label.into()));
        self
    }

    /// An item: its key (its name is `<name>-<key>`), an icon and its label.
    pub fn item(
        mut self,
        key: impl Into<Cow<'static, str>>,
        icon: impl Into<Cow<'static, str>>,
        label: impl Into<String>,
    ) -> Self {
        self.entries.push(Entry::Item {
            key: key.into(),
            icon: Some(icon.into()),
            label: label.into(),
            count: None,
        });
        self
    }

    /// A count at the right edge of the item just added (nothing for an empty string).
    pub fn count(mut self, count: impl Into<String>) -> Self {
        let count = count.into();
        if let Some(Entry::Item { count: c, .. }) = self.entries.last_mut() {
            *c = (!count.is_empty()).then_some(count);
        }
        self
    }

    /// The item selected to start with, by its place among the items.
    pub fn selected(mut self, index: Option<usize>) -> Self {
        self.selected = index;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let SideNav {
            name,
            entries,
            selected,
        } = self;
        (
            Name::new(name.clone().into_owned()),
            SideNavState { selected },
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                ..default()
            },
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let nav = p.target_entity();
                let mut index = 0;
                for entry in entries {
                    match entry {
                        Entry::Group(label) => {
                            p.spawn((
                                t.text(
                                    label.to_uppercase(),
                                    t.font_xs,
                                    FontWeight::SEMIBOLD,
                                    t.muted_foreground,
                                ),
                                Node {
                                    margin: UiRect::new(
                                        Val::Px(10.0),
                                        Val::ZERO,
                                        Val::Px(14.0),
                                        Val::Px(4.0),
                                    ),
                                    ..default()
                                },
                                Pickable::IGNORE,
                            ));
                        }
                        Entry::Item {
                            key,
                            icon,
                            label,
                            count,
                        } => {
                            let mut item = ListItem::new(format!("{name}-{key}"))
                                .label(label)
                                .height(30.0)
                                .padding_left(10.0)
                                .selected(selected == Some(index));
                            if let Some(icon) = icon {
                                item = item.icon(icon);
                            }
                            if let Some(count) = count {
                                item = item.detail(count);
                            }
                            p.spawn((
                                item.build(&t),
                                SideNavItem {
                                    nav,
                                    index,
                                    key: key.into_owned(),
                                },
                            ))
                            .entry::<Node>()
                            .and_modify(|mut n| {
                                n.border_radius = BorderRadius::all(Val::Px(t.radius));
                            });
                            index += 1;
                        }
                    }
                }
            })),
        )
    }
}

fn on_item(
    ev: On<Activate>,
    items: Query<&SideNavItem>,
    mut navs: Query<&mut SideNavState>,
    mut commands: Commands,
) {
    let Ok(item) = items.get(ev.entity) else {
        return;
    };
    if let Ok(mut state) = navs.get_mut(item.nav)
        && state.selected != Some(item.index)
    {
        state.selected = Some(item.index);
    }
    commands.trigger(SideNavSelect {
        entity: item.nav,
        index: item.index,
        key: item.key.clone(),
    });
}

/// The items show their column's selection.
fn sync_selection(
    navs: Query<(Entity, &SideNavState), Changed<SideNavState>>,
    items: Query<(Entity, &SideNavItem, Has<Selected>)>,
    mut commands: Commands,
) {
    for (nav, state) in &navs {
        for (entity, item, selected) in &items {
            if item.nav != nav {
                continue;
            }
            let want = state.selected == Some(item.index);
            if want && !selected {
                commands.entity(entity).try_insert(Selected);
            } else if !want && selected {
                commands.entity(entity).try_remove::<Selected>();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nav(world: &mut World) -> Entity {
        let theme = Theme::default();
        world
            .spawn(
                SideNav::new("org-nav")
                    .item("overview", "home", "Overview")
                    .group("Sales")
                    .item("requests", "receipt", "Requests")
                    .count("3")
                    .item("clients", "users", "Clients")
                    .selected(Some(0))
                    .build(&theme),
            )
            .id()
    }

    fn named(world: &mut World, name: &str) -> Entity {
        let mut q = world.query::<(Entity, &Name)>();
        q.iter(world)
            .find(|(_, n)| n.as_str() == name)
            .map(|(e, _)| e)
            .unwrap_or_else(|| panic!("no {name}"))
    }

    #[test]
    fn items_are_named_by_key_and_one_is_selected() {
        let mut world = World::new();
        let nav = nav(&mut world);
        world.flush();
        let mut items = world.query::<&SideNavItem>();
        let keys: Vec<(usize, String)> = {
            let mut all: Vec<_> = items
                .iter(&world)
                .map(|i| (i.index, i.key.clone()))
                .collect();
            all.sort();
            all
        };
        assert_eq!(
            keys,
            [
                (0, "overview".to_string()),
                (1, "requests".to_string()),
                (2, "clients".to_string())
            ]
        );
        for name in ["org-nav-overview", "org-nav-requests", "org-nav-clients"] {
            named(&mut world, name);
        }
        assert_eq!(world.get::<SideNavState>(nav).unwrap().selected, Some(0));
    }

    #[test]
    fn a_click_selects_and_says_so() {
        let mut world = World::new();
        world.add_observer(on_item);
        #[derive(Resource, Default)]
        struct Chosen(Vec<(usize, String)>);
        world.init_resource::<Chosen>();
        world.add_observer(|ev: On<SideNavSelect>, mut chosen: ResMut<Chosen>| {
            chosen.0.push((ev.index, ev.key.clone()));
        });
        let nav = nav(&mut world);
        world.flush();
        let requests = named(&mut world, "org-nav-requests");
        world.trigger(Activate { entity: requests });
        world.flush();
        assert_eq!(world.get::<SideNavState>(nav).unwrap().selected, Some(1));
        assert_eq!(world.resource::<Chosen>().0, [(1, "requests".to_string())]);
        // The items follow the state, also when it is set from code.
        let sync = world.register_system(sync_selection);
        world.run_system(sync).unwrap();
        world.flush();
        assert!(world.get::<Selected>(requests).is_some());
        world.get_mut::<SideNavState>(nav).unwrap().selected = None;
        world.run_system(sync).unwrap();
        world.flush();
        assert!(world.get::<Selected>(requests).is_none());
    }
}
