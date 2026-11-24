//! The conversation about one request for a quote: the shop's side (in its organization
//! document, the Requests tab) and the client's (Quote requests…). Messages go as letters
//! (`cadrs_org::letters::Message::Chat`); the shop keeps them in its document, the client in its
//! book of requests.

use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{Activate, observe};
use cadrs_org::Op;
use cadrs_org::letters::Message;
use cadrs_ui::Button;
use cadrs_ui::dialog::{Dialog, DialogClose};
use cadrs_ui::i18n::tr;
use cadrs_ui::input::{TextInput, TextInputField};
use cadrs_ui::theme::Theme;

/// Whose side of a request's conversation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Thread {
    /// The open organization's request.
    Shop(cadrs_org::Id),
    /// One of this user's requests.
    Client(cadrs_org::Id),
}

#[derive(Component)]
struct ChatDialog(Thread);

/// The messages so far: (mine, text).
fn messages(world: &World, thread: Thread) -> Vec<(bool, String)> {
    match thread {
        Thread::Shop(id) => world
            .get_resource::<crate::organization::OpenOrg>()
            .and_then(|o| o.session.org().requests.get(&id).map(|r| r.chat.clone()))
            .unwrap_or_default()
            .into_iter()
            .map(|(from_client, text, _)| (!from_client, text))
            .collect(),
        Thread::Client(id) => world
            .resource::<crate::social::Social>()
            .book
            .sent
            .get(&id)
            .map(|s| s.chat.clone())
            .unwrap_or_default()
            .into_iter()
            .map(|(from_shop, text, _)| (!from_shop, text))
            .collect(),
    }
}

fn close(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<ChatDialog>>();
    for e in q.iter(world).collect::<Vec<_>>() {
        world.trigger(DialogClose { entity: e });
    }
}

/// Opens (or refreshes) the conversation about a request.
pub fn open(world: &mut World, thread: Thread) {
    crate::social::ensure_loaded(world);
    close(world);
    let lines = messages(world, thread);
    let theme = world.resource::<Theme>().clone();
    let t = theme.clone();
    let tb = theme.clone();
    let state = world.resource::<State<crate::AppState>>().get().to_owned();
    world.spawn((
        Dialog::new("chat-dialog")
            .title(tr("chat-title"))
            .width(520.0)
            .body(move |b| {
                let t = &tb;
                if lines.is_empty() {
                    b.spawn(t.paragraph(tr("chat-empty"), t.font_sm, t.muted_foreground));
                }
                for (i, (mine, text)) in lines.into_iter().enumerate() {
                    b.spawn((
                        Name::new(format!("chat-message-{i}")),
                        Node {
                            align_self: if mine {
                                AlignSelf::FlexEnd
                            } else {
                                AlignSelf::FlexStart
                            },
                            max_width: Val::Percent(80.0),
                            padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                            border_radius: BorderRadius::all(Val::Px(8.0)),
                            margin: UiRect::bottom(Val::Px(4.0)),
                            ..default()
                        },
                        BackgroundColor(if mine { t.primary } else { t.secondary }),
                    ))
                    .with_child(t.paragraph(
                        text,
                        t.font_base,
                        if mine {
                            t.primary_foreground
                        } else {
                            t.foreground
                        },
                    ));
                }
                b.spawn(
                    TextInput::new("chat-input")
                        .placeholder(tr("chat-placeholder"))
                        .autofocus()
                        .build(t),
                );
            })
            .footer(move |f| {
                let t = f.world().resource::<Theme>().clone();
                f.spawn((
                    Button::new("chat-send")
                        .label(tr("chat-send"))
                        .primary()
                        .build(&t),
                    observe(move |_: On<Activate>, mut commands: Commands| {
                        commands.queue(move |w: &mut World| send(w, thread));
                    }),
                ));
                f.spawn((
                    Button::new("chat-close")
                        .label(tr("common-close"))
                        .build(&t),
                    observe(|_: On<Activate>, mut commands: Commands| commands.queue(close)),
                ));
            })
            .build(&t),
        ChatDialog(thread),
        DespawnOnExit(state),
    ));
}

fn send(world: &mut World, thread: Thread) {
    let text = {
        let mut q = world.query_filtered::<(&Name, &EditableText), With<TextInputField>>();
        q.iter(world)
            .find(|(n, _)| n.as_str() == "chat-input-field")
            .map(|(_, t)| t.value().to_string())
            .unwrap_or_default()
    };
    if text.trim().is_empty() {
        return;
    }
    let sent = match thread {
        Thread::Shop(request) => say_as_shop(world, request, text.trim()),
        Thread::Client(request) => say_as_client(world, request, text.trim()),
    };
    if sent {
        open(world, thread);
    }
}

/// The shop writes to the client of `request`.
pub fn say_as_shop(world: &mut World, request: cadrs_org::Id, text: &str) -> bool {
    let note = Op::Note {
        request,
        from_client: false,
        text: text.to_owned(),
    };
    if !crate::organization::write(world, note) {
        return false;
    }
    crate::organization::tell_client(
        world,
        request,
        Message::Chat {
            request,
            text: text.to_owned(),
        },
    );
    true
}

/// The client writes to the shop of `request`.
pub fn say_as_client(world: &mut World, request: cadrs_org::Id, text: &str) -> bool {
    let social = world.resource::<crate::social::Social>();
    let Some(shop) = social.book.sent.get(&request).map(|s| s.shop) else {
        return false;
    };
    let Some(card) = social.contacts.get(&shop).cloned() else {
        crate::linked::toast(world, tr("social-no-devices"));
        return false;
    };
    let mut book = social.book.clone();
    let Ok(message) = book.say(request, text, cadrs_social::wall_millis()) else {
        return false;
    };
    if !crate::social::send(world, &card, message, None, tr("chat-what")) {
        return false;
    }
    world.resource_mut::<crate::social::Social>().book = book;
    crate::social::save_book(world);
    true
}

/// Refreshes an open conversation when a message arrives in it.
pub(crate) fn refresh(world: &mut World) {
    let mut q = world.query::<&ChatDialog>();
    let showing: Vec<Thread> = q.iter(world).map(|d| d.0).collect();
    for thread in showing {
        let shown = {
            let mut q = world.query::<&Name>();
            q.iter(world)
                .filter(|n| n.as_str().starts_with("chat-message-"))
                .count()
        };
        if messages(world, thread).len() != shown {
            open(world, thread);
        }
    }
}
