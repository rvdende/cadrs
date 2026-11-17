//! Request a quote (the document menu): send the open Part Studio's parts to a shop.
//!
//! The parts are measured as they open the dialog (`cadrs_core::manufacture`: flat or not, how
//! thick, how long the cuts) and the document is frozen as it is (packed), so the shop quotes
//! what was sent even if the parts change later. The shop is one of the contacts that is an
//! organization, or a shop's link pasted in, which adds it to the contacts.

use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight};
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::ElementKind;
use cadrs_core::parts::{PartKind, display_name, part_material};
use cadrs_org::capability::PartSummary;
use cadrs_org::letters::{Message, Sent};
use cadrs_org::{Stage, new_id};
use cadrs_social::contact::{Card, Contacts};
use cadrs_social::keys::Signed;
use cadrs_ui::Button;
use cadrs_ui::dialog::{Dialog, DialogClose};
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::input::{TextInput, TextInputField};
use cadrs_ui::tab_strip::{TabStrip, TabStripState};
use cadrs_ui::theme::Theme;

use crate::{ActiveDocument, AppState};

/// What the open dialog will send.
#[derive(Resource)]
struct Draft {
    parts: Vec<PartSummary>,
    snapshot: Vec<u8>,
    shops: Vec<Signed<Card>>,
}

#[derive(Component)]
struct RfqDialog;

fn summary_line(p: &PartSummary) -> String {
    match p.flat_thickness {
        Some(t) => tr_with(
            "rfq-part-flat",
            &[
                ("name", &p.name),
                ("thickness", &format!("{t:.1}")),
                ("length", &format!("{:.0}", p.cut_length)),
                ("pierces", &p.pierces.to_string()),
            ],
        ),
        None => tr_with(
            "rfq-part-3d",
            &[
                ("name", &p.name),
                (
                    "size",
                    &format!("{:.0} × {:.0} × {:.0}", p.size[0], p.size[1], p.size[2]),
                ),
            ],
        ),
    }
}

/// Opens the dialog for the open Part Studio's parts.
pub fn open(world: &mut World) {
    crate::social::ensure_loaded(world);
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return;
    };
    let Some(element) = doc.active_element() else {
        return;
    };
    let ElementKind::PartStudio { parts: props, .. } = &element.kind else {
        crate::linked::toast(world, tr("rfq-not-part-studio"));
        return;
    };
    let props = props.clone();
    let snapshot = match cadrs_core::collab::pack(&doc.doc) {
        Ok(b) => b,
        Err(e) => {
            crate::linked::toast(world, e);
            return;
        }
    };
    let Some(parts) = crate::cam::compute::studio_parts(world, element.id) else {
        crate::linked::toast(world, tr("rfq-still-building"));
        return;
    };
    let mut material = None;
    let summaries: Vec<PartSummary> = parts
        .iter()
        .filter(|p| p.kind == PartKind::Solid)
        .map(|p| {
            let m = part_material(p, &props).map(|m| m.name.clone());
            material = material.clone().or(m.clone());
            cadrs_core::manufacture::part_summary(&p.solid, display_name(p, &props), m)
        })
        .collect();
    if summaries.is_empty() {
        crate::linked::toast(world, tr("rfq-no-parts"));
        return;
    }
    let shops: Vec<Signed<Card>> =
        organizations(&world.resource::<crate::social::Social>().contacts);
    let lines: Vec<String> = summaries.iter().map(summary_line).collect();
    let names: Vec<String> = shops.iter().map(|c| c.value.name.clone()).collect();
    world.insert_resource(Draft {
        parts: summaries,
        snapshot,
        shops,
    });
    let theme = world.resource::<Theme>().clone();
    let t = theme.clone();
    let tb = theme.clone();
    let material = material.unwrap_or_else(|| "mild steel".into());
    world.spawn((
        Dialog::new("rfq-dialog")
            .title(tr("rfq-title"))
            .width(600.0)
            .body(move |b| {
                let t = &tb;
                let label = |b: &mut ChildSpawner, key: &str| {
                    b.spawn(t.text(tr(key), t.font_sm, FontWeight::BOLD, t.foreground));
                };
                label(b, "rfq-parts");
                for line in &lines {
                    b.spawn(t.text(line.clone(), t.font_sm, FontWeight::NORMAL, t.foreground));
                }
                label(b, "rfq-shop");
                if names.is_empty() {
                    b.spawn(t.paragraph(tr("rfq-no-shops"), t.font_sm, t.muted_foreground));
                } else {
                    let mut strip = TabStrip::new("rfq-shop");
                    for n in &names {
                        strip = strip.tab(n.clone());
                    }
                    b.spawn(strip.build(t));
                }
                b.spawn(
                    TextInput::new("rfq-shop-link")
                        .placeholder(tr("rfq-shop-link-placeholder"))
                        .build(t),
                );
                label(b, "rfq-quantity-material");
                b.spawn(Node {
                    column_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|r| {
                    r.spawn(
                        TextInput::new("rfq-quantity")
                            .value("1")
                            .width(Val::Px(80.0))
                            .build(t),
                    );
                    r.spawn(
                        TextInput::new("rfq-material")
                            .value(material.clone())
                            .width(Val::Px(240.0))
                            .build(t),
                    );
                });
                label(b, "rfq-notes");
                b.spawn(
                    TextInput::new("rfq-notes")
                        .placeholder(tr("rfq-notes-placeholder"))
                        .build(t),
                );
            })
            .footer(|f| {
                let t = f.world().resource::<Theme>().clone();
                f.spawn((
                    Button::new("rfq-send")
                        .label(tr("rfq-send"))
                        .primary()
                        .build(&t),
                    observe(|_: On<Activate>, mut commands: Commands| commands.queue(send)),
                ));
                f.spawn((
                    Button::new("rfq-cancel")
                        .label(tr("common-cancel"))
                        .build(&t),
                    observe(|_: On<Activate>, mut commands: Commands| commands.queue(close)),
                ));
            })
            .build(&t),
        RfqDialog,
        DespawnOnExit(AppState::Document),
    ));
}

fn organizations(contacts: &Contacts) -> Vec<Signed<Card>> {
    contacts
        .cards
        .iter()
        .filter(|c| c.value.kind == cadrs_social::contact::Kind::Organization)
        .cloned()
        .collect()
}

fn field(world: &mut World, name: &str) -> String {
    let field = format!("{name}-field");
    let mut q = world.query_filtered::<(&Name, &EditableText), With<TextInputField>>();
    q.iter(world)
        .find(|(n, _)| n.as_str() == field)
        .map(|(_, t)| t.value().to_string())
        .unwrap_or_default()
}

fn close(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<RfqDialog>>();
    for e in q.iter(world).collect::<Vec<_>>() {
        world.trigger(DialogClose { entity: e });
    }
    world.remove_resource::<Draft>();
}

fn send(world: &mut World) {
    let link = field(world, "rfq-shop-link");
    let shop = if link.trim().is_empty() {
        let selected = {
            let mut q = world.query::<(&Name, &TabStripState)>();
            q.iter(world)
                .find(|(n, _)| n.as_str() == "rfq-shop")
                .map_or(0, |(_, s)| s.selected)
        };
        world.resource::<Draft>().shops.get(selected).cloned()
    } else {
        match cadrs_social::contact::from_link(&link) {
            Ok(card) if card.value.kind == cadrs_social::contact::Kind::Organization => {
                crate::social::add_contact(world, card.clone());
                Some(card)
            }
            _ => {
                crate::linked::toast(world, tr("rfq-bad-link"));
                return;
            }
        }
    };
    let Some(shop) = shop else {
        crate::linked::toast(world, tr("rfq-pick-shop"));
        return;
    };
    let quantity: u32 = field(world, "rfq-quantity")
        .trim()
        .parse()
        .unwrap_or(1)
        .max(1);
    let material = field(world, "rfq-material").trim().to_owned();
    let notes = field(world, "rfq-notes");
    let name = crate::social::my_name(world);
    let Some(card) = world.resource::<crate::social::Social>().my_card(&name) else {
        crate::linked::toast(world, tr("social-no-keys"));
        return;
    };
    let Some(draft) = world.get_resource::<Draft>() else {
        return;
    };
    let snapshot = draft.snapshot.clone();
    let parts: Vec<(PartSummary, u32)> = draft
        .parts
        .iter()
        .cloned()
        .map(|mut p| {
            if !material.is_empty() {
                p.material = Some(material.clone());
            }
            (p, quantity)
        })
        .collect();
    let id = new_id();
    let now = cadrs_social::wall_millis();
    let message = Message::Request {
        id,
        parts: parts.clone(),
        notes: notes.clone(),
        deadline: None,
        snapshot,
        client: card,
    };
    // Recorded once on its way: a failure to start leaves the dialog open to try again.
    if !crate::social::send(world, &shop.value, message, None, tr("social-what-request")) {
        return;
    }
    world.resource_mut::<crate::social::Social>().book.record(
        id,
        Sent {
            shop: shop.value.account,
            shop_name: shop.value.name.clone(),
            parts,
            notes,
            stage: Stage::Requested,
            quote: None,
            decline_reason: None,
            chat: vec![],
            sent: now,
            delivery: None,
            review: None,
        },
    );
    crate::social::save_book(world);
    close(world);
}

#[derive(Component)]
struct RequestsDialog;

/// The requests for quotes this user sent, and what became of them: Accept a quote here.
pub fn open_requests(world: &mut World) {
    crate::social::ensure_loaded(world);
    let mut q = world.query_filtered::<Entity, With<RequestsDialog>>();
    for e in q.iter(world).collect::<Vec<_>>() {
        world.trigger(DialogClose { entity: e });
    }
    let rows: Vec<(cadrs_org::Id, String, String, String, bool, usize, Action)> = world
        .resource::<crate::social::Social>()
        .book
        .sent
        .iter()
        .rev()
        .map(|(id, s)| {
            let parts: Vec<String> = s
                .parts
                .iter()
                .map(|(p, n)| format!("{n} × {}", p.name))
                .collect();
            let price = s
                .quote
                .as_ref()
                .map(|q| format!("{} {}", q.currency, q.total.format()))
                .unwrap_or_default();
            let stage = tr(match s.stage {
                Stage::Requested => "org-stage-requested",
                Stage::Quoted => "org-stage-quoted",
                Stage::Declined => "org-stage-declined",
                Stage::Accepted => "org-stage-accepted",
                Stage::InProduction => "org-stage-in-production",
                Stage::Ready => "org-stage-ready",
                Stage::Completed => "org-stage-completed",
            });
            (
                *id,
                format!("{} · {}", s.shop_name, parts.join(", ")),
                stage,
                price,
                s.stage == Stage::Quoted,
                s.chat.len(),
                match (&s.delivery, &s.review) {
                    (Some(cadrs_org::letters::Delivery::ToSign(_)), _) => Action::Sign,
                    (Some(cadrs_org::letters::Delivery::Signed(_)), None) => Action::Review,
                    _ => Action::None,
                },
            )
        })
        .collect();
    let theme = world.resource::<Theme>().clone();
    let t = theme.clone();
    let tb = theme.clone();
    let state = world.resource::<State<AppState>>().get().to_owned();
    world.spawn((
        Dialog::new("rfq-requests-dialog")
            .title(tr("rfq-requests-title"))
            .width(640.0)
            .body(move |b| {
                let t = &tb;
                if rows.is_empty() {
                    b.spawn(t.paragraph(tr("rfq-requests-none"), t.font_sm, t.muted_foreground));
                }
                for (i, (id, what, stage, price, can_accept, messages, action)) in
                    rows.iter().cloned().enumerate()
                {
                    b.spawn(Node {
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(12.0),
                        ..default()
                    })
                    .with_children(|r| {
                        r.spawn((
                            Name::new(format!("rfq-request-{i}")),
                            t.text(what, t.font_base, FontWeight::MEDIUM, t.foreground),
                        ));
                        r.spawn((
                            Name::new(format!("rfq-request-stage-{i}")),
                            t.text(stage, t.font_sm, FontWeight::BOLD, t.foreground),
                        ));
                        r.spawn(t.text(price, t.font_base, FontWeight::NORMAL, t.foreground));
                        r.spawn((
                            Button::new(format!("rfq-chat-{i}"))
                                .label(format!("{} ({messages})", tr("chat-messages")))
                                .small()
                                .ghost()
                                .build(t),
                            observe(move |_: On<Activate>, mut commands: Commands| {
                                commands.queue(move |w: &mut World| {
                                    crate::chat_ui::open(w, crate::chat_ui::Thread::Client(id))
                                });
                            }),
                        ));
                        let next = match action {
                            Action::Sign => Some(("rfq-sign-delivery", true)),
                            Action::Review => Some(("rfq-review", false)),
                            Action::None => None,
                        };
                        if let Some((label, sign)) = next {
                            r.spawn((
                                Button::new(format!("rfq-next-{i}"))
                                    .label(tr(label))
                                    .small()
                                    .primary()
                                    .build(t),
                                observe(move |_: On<Activate>, mut commands: Commands| {
                                    commands.queue(move |w: &mut World| {
                                        if sign {
                                            sign_delivery(w, id)
                                        } else {
                                            open_review(w, id)
                                        }
                                    });
                                }),
                            ));
                        }
                        if can_accept {
                            r.spawn((
                                Button::new(format!("rfq-accept-{i}"))
                                    .label(tr("rfq-accept"))
                                    .small()
                                    .primary()
                                    .build(t),
                                observe(move |_: On<Activate>, mut commands: Commands| {
                                    commands.queue(move |w: &mut World| accept(w, id));
                                }),
                            ));
                        }
                    });
                }
            })
            .footer(|f| {
                let t = f.world().resource::<Theme>().clone();
                f.spawn((
                    Button::new("rfq-requests-close")
                        .label(tr("common-close"))
                        .build(&t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(|w: &mut World| {
                            let mut q = w.query_filtered::<Entity, With<RequestsDialog>>();
                            for e in q.iter(w).collect::<Vec<_>>() {
                                w.trigger(DialogClose { entity: e });
                            }
                        });
                    }),
                ));
            })
            .build(&t),
        RequestsDialog,
        DespawnOnExit(state),
    ));
}

/// Accepts request `id`'s quote: tells the shop.
pub fn accept(world: &mut World, id: cadrs_org::Id) {
    let social = world.resource::<crate::social::Social>();
    let Some(shop) = social.book.sent.get(&id).map(|s| s.shop) else {
        return;
    };
    let Some(card) = social.contacts.get(&shop).cloned() else {
        crate::linked::toast(world, tr("social-no-devices"));
        return;
    };
    let mut book = social.book.clone();
    let message = match book.accept(id) {
        Ok(m) => m,
        Err(e) => {
            crate::linked::toast(world, format!("{e:?}"));
            return;
        }
    };
    if crate::social::send(world, &card, message, None, tr("rfq-what-acceptance")) {
        world.resource_mut::<crate::social::Social>().book = book;
        crate::social::save_book(world);
        open_requests(world);
    }
}

#[derive(Component)]
struct ContactDialog;

/// Add contact: paste someone's (or a shop's) link.
pub fn open_add_contact(world: &mut World) {
    let theme = world.resource::<Theme>().clone();
    let t = theme.clone();
    let tb = theme.clone();
    world.spawn((
        Dialog::new("contact-dialog")
            .title(tr("social-add-contact"))
            .width(560.0)
            .body(move |b| {
                b.spawn(
                    TextInput::new("contact-link")
                        .placeholder(tr("rfq-shop-link-placeholder"))
                        .autofocus()
                        .build(&tb),
                );
            })
            .footer(|f| {
                let t = f.world().resource::<Theme>().clone();
                f.spawn((
                    Button::new("contact-add")
                        .label(tr("social-add"))
                        .primary()
                        .build(&t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(|w: &mut World| {
                            let link = field(w, "contact-link");
                            match cadrs_social::contact::from_link(&link) {
                                Ok(card) => {
                                    let name = card.value.name.clone();
                                    crate::social::add_contact(w, card);
                                    crate::linked::toast(
                                        w,
                                        tr_with("social-added", &[("name", &name)]),
                                    );
                                    let mut q = w.query_filtered::<Entity, With<ContactDialog>>();
                                    for e in q.iter(w).collect::<Vec<_>>() {
                                        w.trigger(DialogClose { entity: e });
                                    }
                                }
                                Err(_) => crate::linked::toast(w, tr("rfq-bad-link")),
                            }
                        });
                    }),
                ));
                f.spawn((
                    Button::new("contact-cancel")
                        .label(tr("common-cancel"))
                        .build(&t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(|w: &mut World| {
                            let mut q = w.query_filtered::<Entity, With<ContactDialog>>();
                            for e in q.iter(w).collect::<Vec<_>>() {
                                w.trigger(DialogClose { entity: e });
                            }
                        });
                    }),
                ));
            })
            .build(&t),
        ContactDialog,
        DespawnOnExit(AppState::Landing),
    ));
}

/// What a request in the list waits for from the client.
#[derive(Clone, Copy)]
enum Action {
    None,
    Sign,
    Review,
}

/// Signs for delivery (with the account's key) and sends the receipt back to the shop.
pub fn sign_delivery(world: &mut World, id: cadrs_org::Id) {
    let social = world.resource::<crate::social::Social>();
    let Some(account) = social.keys().and_then(|k| k.account.clone()) else {
        crate::linked::toast(world, tr("rfq-sign-needs-account"));
        return;
    };
    let Some(shop) = social.book.sent.get(&id).map(|s| s.shop) else {
        return;
    };
    let Some(card) = social.contacts.get(&shop).cloned() else {
        crate::linked::toast(world, tr("social-no-devices"));
        return;
    };
    let mut book = social.book.clone();
    let message = match book.sign_delivery(id, account.key()) {
        Ok(m) => m,
        Err(e) => {
            crate::linked::toast(world, format!("{e:?}"));
            return;
        }
    };
    if crate::social::send(world, &card, message, None, tr("rfq-what-receipt")) {
        world.resource_mut::<crate::social::Social>().book = book;
        crate::social::save_book(world);
        if world
            .query_filtered::<(), With<RequestsDialog>>()
            .iter(world)
            .next()
            .is_some()
        {
            open_requests(world);
        }
    }
}

/// Reviews a completed order: `stars` 1 to 5 and a few words, sent to the shop.
pub fn review(world: &mut World, id: cadrs_org::Id, stars: u8, text: &str) -> bool {
    let social = world.resource::<crate::social::Social>();
    let Some(shop) = social.book.sent.get(&id).map(|s| s.shop) else {
        return false;
    };
    let Some(card) = social.contacts.get(&shop).cloned() else {
        crate::linked::toast(world, tr("social-no-devices"));
        return false;
    };
    let mut book = social.book.clone();
    let message = match book.review(id, stars, text) {
        Ok(m) => m,
        Err(e) => {
            crate::linked::toast(world, format!("{e:?}"));
            return false;
        }
    };
    if !crate::social::send(world, &card, message, None, tr("rfq-what-review")) {
        return false;
    }
    world.resource_mut::<crate::social::Social>().book = book;
    crate::social::save_book(world);
    true
}

#[derive(Component)]
struct ReviewDialog;

fn open_review(world: &mut World, id: cadrs_org::Id) {
    let theme = world.resource::<Theme>().clone();
    let t = theme.clone();
    let tb = theme.clone();
    let state = world.resource::<State<AppState>>().get().to_owned();
    world.spawn((
        Dialog::new("review-dialog")
            .title(tr("rfq-review-title"))
            .width(480.0)
            .body(move |b| {
                let t = &tb;
                let mut stars = TabStrip::new("review-stars").selected(4);
                for n in 1..=5 {
                    stars = stars.tab("★".repeat(n));
                }
                b.spawn(stars.build(t));
                b.spawn(
                    TextInput::new("review-text")
                        .placeholder(tr("rfq-review-placeholder"))
                        .autofocus()
                        .build(t),
                );
            })
            .footer(move |f| {
                let t = f.world().resource::<Theme>().clone();
                f.spawn((
                    Button::new("review-send")
                        .label(tr("rfq-review-send"))
                        .primary()
                        .build(&t),
                    observe(move |_: On<Activate>, mut commands: Commands| {
                        commands.queue(move |w: &mut World| {
                            let stars = {
                                let mut q = w.query::<(&Name, &TabStripState)>();
                                q.iter(w)
                                    .find(|(n, _)| n.as_str() == "review-stars")
                                    .map_or(4, |(_, s)| s.selected)
                            } as u8
                                + 1;
                            let text = field(w, "review-text");
                            if review(w, id, stars, text.trim()) {
                                let mut q = w.query_filtered::<Entity, With<ReviewDialog>>();
                                for e in q.iter(w).collect::<Vec<_>>() {
                                    w.trigger(DialogClose { entity: e });
                                }
                                open_requests(w);
                            }
                        });
                    }),
                ));
                f.spawn((
                    Button::new("review-cancel")
                        .label(tr("common-cancel"))
                        .build(&t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(|w: &mut World| {
                            let mut q = w.query_filtered::<Entity, With<ReviewDialog>>();
                            for e in q.iter(w).collect::<Vec<_>>() {
                                w.trigger(DialogClose { entity: e });
                            }
                        });
                    }),
                ));
            })
            .build(&t),
        ReviewDialog,
        DespawnOnExit(state),
    ));
}
