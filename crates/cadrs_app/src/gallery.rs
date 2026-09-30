//! The hidden gallery state showing every `cadrs_ui` component.

use bevy::prelude::*;
use cadrs_ui::{IconAtlas, Theme, gallery};

use crate::AppState;

pub struct GalleryPlugin;

impl Plugin for GalleryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Gallery), spawn)
            .add_observer(gallery::on_gallery_menu_action);
    }
}

fn spawn(mut commands: Commands, theme: Res<Theme>, atlas: Res<IconAtlas>) {
    let root = gallery::spawn_gallery(&mut commands, &theme, &atlas);
    commands
        .entity(root)
        .insert(DespawnOnExit(AppState::Gallery));
}
