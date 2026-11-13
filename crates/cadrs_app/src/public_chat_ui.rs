//! The public chat (the documents page's app menu, *Public chat…*): what anyone online says, for
//! everyone (`cadrs_social::chat`: signed, with work done, at a few lines a minute; blocked
//! accounts are dropped). Lines travel in the public room (`cadrs_net::public_chat`).

use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight};
use bevy::ui_widgets::{Activate, observe};
use cadrs_social::chat::{self, Room};
use cadrs_ui::Button;
use cadrs_ui::dialog::{Dialog, DialogClose};
use cadrs_ui::i18n::tr;
use cadrs_ui::input::{TextInput, TextInputField};
use cadrs_ui::theme::Theme;

use crate::social::{Social, ensure_loaded};

pub struct PublicChatPlugin;

impl Plugin for PublicChatPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, hear);
    }
}

#[derive(Component)]
struct ChatDialog;

const FILE: &str = "public-chat.bin";

fn save(world: &World) {
    let room = world.resource::<Social>().public_chat.clone();
    crate::social::save_file(world, FILE, &room);
}

/// Takes in what the room says; refreshes the dialog if it is open.
fn hear(world: &mut World) {
    let Some(heard) = world
        .get_resource_mut::<cadrs_net::public_chat::PublicChat>()
        .map(|mut c| c.take_heard())
    else {
        return;
    };
    if heard.is_empty() {
        return;
    }
    ensure_loaded(world);
    let now = cadrs_social::wall_millis();
    let mut any = false;
    for bytes in heard {
        any |= world
            .resource_mut::<Social>()
            .public_chat
            .hear(&bytes, now)
            .is_ok();
    }
    if any {
        save(world);
        let open = world.query::<&ChatDialog>().iter(world).next().is_some();
        if open {
            show(world);
        }
    }
}

fn close(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<ChatDialog>>();
    for e in q.iter(world).collect::<Vec<_>>() {
        world.trigger(DialogClose { entity: e });
    }
}

/// Opens (or redraws) the public chat.
pub fn show(world: &mut World) {
    ensure_loaded(world);
    if world
        .get_resource::<cadrs_net::public_chat::PublicChat>()
        .is_none()
    {
        crate::linked::toast(world, tr("public-chat-offline"));
    }
    let social = world.resource::<Social>();
    let me = social.keys().map(|k| k.account_id());
    let lines: Vec<(String, String, bool, cadrs_social::keys::AccountId)> = social
        .public_chat
        .lines()
        .map(|l| {
            let who = Room::account(l);
            (l.name.clone(), l.text.clone(), Some(who) == me, who)
        })
        .collect();
    close(world);
    let theme = world.resource::<Theme>().clone();
    let t = theme.clone();
    let tb = theme.clone();
    let state = world.resource::<State<crate::AppState>>().get().to_owned();
    world.spawn((
        Dialog::new("public-chat-dialog")
            .title(tr("public-chat-title"))
            .width(600.0)
            .body(move |b| {
                let t = &tb;
                if lines.is_empty() {
                    b.spawn(t.paragraph(tr("public-chat-empty"), t.font_sm, t.muted_foreground));
                }
                for (i, (name, text, mine, who)) in lines.into_iter().enumerate() {
                    b.spawn(Node {
                        column_gap: Val::Px(8.0),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|r| {
                        let name = if name.is_empty() {
                            tr("public-chat-someone")
                        } else {
                            name
                        };
                        r.spawn(t.text(name, t.font_sm, FontWeight::BOLD, t.foreground));
                        if !mine {
                            r.spawn((
                                Button::new(format!("public-chat-block-{i}"))
                                    .label(tr("public-chat-block"))
                                    .small()
                                    .ghost()
                                    .build(t),
                                observe(move |_: On<Activate>, mut commands: Commands| {
                                    commands.queue(move |w: &mut World| {
                                        w.resource_mut::<Social>().public_chat.block(who);
                                        save(w);
                                        show(w);
                                    });
                                }),
                            ));
                        }
                    });
                    b.spawn((
                        Name::new(format!("public-chat-{i}")),
                        t.paragraph(text, t.font_base, t.foreground),
                    ));
                }
                b.spawn(
                    TextInput::new("public-chat-input")
                        .placeholder(tr("chat-placeholder"))
                        .autofocus()
                        .build(t),
                );
            })
            .footer(|f| {
                let t = f.world().resource::<Theme>().clone();
                f.spawn((
                    Button::new("public-chat-send")
                        .label(tr("chat-send"))
                        .primary()
                        .build(&t),
                    observe(|_: On<Activate>, mut commands: Commands| commands.queue(say)),
                ));
                f.spawn((
                    Button::new("public-chat-close")
                        .label(tr("common-close"))
                        .build(&t),
                    observe(|_: On<Activate>, mut commands: Commands| commands.queue(close)),
                ));
            })
            .build(&t),
        ChatDialog,
        DespawnOnExit(state),
    ));
}

/// Says the line in the box: the work is done here (a fraction of a second), then it goes.
pub fn say(world: &mut World) {
    let text = {
        let mut q = world.query_filtered::<(&Name, &EditableText), With<TextInputField>>();
        q.iter(world)
            .find(|(n, _)| n.as_str() == "public-chat-input-field")
            .map(|(_, t)| t.value().to_string())
            .unwrap_or_default()
    };
    say_text(world, &text);
    show(world);
}

/// Says `text` in the public chat.
pub fn say_text(world: &mut World, text: &str) -> bool {
    ensure_loaded(world);
    let name = crate::social::my_name(world);
    let Some(keys) = world.resource::<Social>().keys().cloned() else {
        return false;
    };
    let now = cadrs_social::wall_millis();
    let Some(line) = chat::say(&keys.device, &keys.cert, &name, text, now) else {
        return false;
    };
    if world
        .resource_mut::<Social>()
        .public_chat
        .add(line.clone(), now)
        .is_err()
    {
        crate::linked::toast(world, tr("public-chat-slow-down"));
        return false;
    }
    save(world);
    if !world.contains_resource::<cadrs_net::bevy_iroh::Iroh>() {
        crate::network::connect(world);
    }
    if let Some(mut out) = world.get_resource_mut::<cadrs_net::public_chat::PublicChat>() {
        out.to_say.push(chat::encode(&line));
    }
    true
}
