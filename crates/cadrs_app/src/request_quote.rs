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

/// The shop the next Request a quote starts on: the one whose page in the marketplace the user
/// chose **Request a quote** on.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct PreferredShop(pub Option<cadrs_social::keys::AccountId>);

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

/// Opens the document a request for a quote carried (as [`open`] packed it): a copy called
/// `name` in the user's documents, theirs to measure and plan from. It is the client's design
/// as it was when they asked; what they changed since isn't in it.
pub(crate) fn open_snapshot(world: &mut World, bytes: &[u8], name: &str) {
    let mut doc: cadrs_core::Document = match cadrs_core::collab::unpack(bytes) {
        Ok(doc) => doc,
        Err(e) => {
            crate::linked::toast(world, format!("{}: {e}", tr("org-request-no-document")));
            return;
        }
    };
    doc.id = cadrs_core::DocumentId::new();
    doc.name = name.trim().to_owned();
    let store = world.resource::<crate::DocumentStore>().0.clone();
    let user = world.resource::<crate::UserProfile>().id.clone();
    let meta = cadrs_core::DocumentMeta::new(&user, world.resource::<crate::AppClock>().now());
    if let Err(e) = store.create(&doc, &meta) {
        crate::linked::toast(world, format!("{}: {e}", tr("org-request-no-document")));
        return;
    }
    crate::move_document::open_document(world, doc.id);
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
    let doc_materials: Vec<String> = doc
        .doc
        .material_libraries
        .iter()
        .flat_map(|l| l.materials.iter().map(|m| m.name.clone()))
        .collect();
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
    let summaries_materials: Vec<String> = summaries
        .iter()
        .filter_map(|p| p.material.clone())
        .collect();
    let names: Vec<String> = shops.iter().map(|c| c.value.name.clone()).collect();
    // The shop asked for from its page in the marketplace, if one was.
    let chosen = world
        .get_resource::<PreferredShop>()
        .and_then(|preferred| preferred.0)
        .and_then(|account| shops.iter().position(|c| c.value.account == account))
        .unwrap_or(0);
    world.insert_resource(Draft {
        parts: summaries,
        snapshot,
        shops,
    });
    let theme = world.resource::<Theme>().clone();
    let t = theme.clone();
    let tb = theme.clone();
    // The materials to ask for: the Part Studio's (the standard ones and the document's own
    // libraries), and whatever the parts are of already.
    let mut materials: Vec<String> = cadrs_core::material::LIBRARY
        .iter()
        .map(|m| m.name.to_owned())
        .collect();
    let own = doc_materials
        .into_iter()
        .chain(summaries_materials)
        .filter(|name| !name.trim().is_empty());
    for name in own {
        if !materials.contains(&name) {
            materials.push(name);
        }
    }
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
                    b.spawn(crate::organization::choice(
                        t,
                        "rfq-shop",
                        names.clone(),
                        chosen,
                    ));
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
                    // The material, picked as in the Part Studio (a shop's machines name
                    // theirs from the same list): the parts' own unless another is chosen.
                    r.spawn(Node {
                        width: Val::Px(280.0),
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_child(
                        crate::organization::material_picker("rfq-material", materials)
                            .chosen(material)
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
        let selected = crate::organization::strip_selected(world, "rfq-shop");
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
    let material = crate::organization::picked(world, "rfq-material").unwrap_or_default();
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
    // What no shop would take is said here, before anything is sent: the dialog stays open
    // to take parts out or shorten the notes.
    if let Some(why) = cadrs_org::letters::request_problem(&message) {
        crate::linked::toast(world, tr_with("rfq-cannot-send", &[("why", why)]));
        return;
    }
    // Recorded once on its way: a failure to start leaves the dialog open to try again. (A
    // shop that is reached and refuses it marks it so: `social::refused`.)
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

/// The requests for quotes this user sent, and what became of them: Accept a quote here, and
/// pay for it. Opening it asks the bank how the user's wallet stands, for what was paid.
pub fn open_requests(world: &mut World) {
    crate::social::ensure_loaded(world);
    if let Some(me) = crate::credits::me(world)
        && world.contains_resource::<crate::credits::Credits>()
    {
        // The quoted requests' payments are asked after by name, the newest first.
        let mut quoted: Vec<(u64, cadrs_credits::MoveId)> = world
            .resource::<crate::social::Social>()
            .book
            .sent
            .iter()
            .filter_map(|(id, s)| {
                let quote = s.quote.as_ref()?;
                Some((s.sent, cadrs_credits::order_hold(&me, *id, quote.id)))
            })
            .collect();
        quoted.sort_by_key(|(sent, _)| std::cmp::Reverse(*sent));
        let holds = quoted.into_iter().map(|(_, hold)| hold).collect();
        crate::credits::name_holds(world, me, holds);
        crate::credits::refresh(world, me);
    }
    show_requests(world);
}

/// The open Quote requests dialog follows the wallet: a payment held or released shows as the
/// bank says it.
pub(crate) fn follow_wallet(world: &mut World, mut seen: Local<u32>) {
    let Some(version) = world
        .get_resource::<crate::credits::Credits>()
        .map(|c| c.version)
    else {
        return;
    };
    if std::mem::replace(&mut *seen, version) == version {
        return;
    }
    let mut q = world.query_filtered::<(), With<RequestsDialog>>();
    if q.iter(world).next().is_some() {
        show_requests(world);
    }
}

/// Releases what is held for order `id` to its shop. `quietly`: a try again in the background,
/// which says something only when it worked.
pub fn release_order(world: &mut World, id: cadrs_org::Id, quietly: bool) {
    let Some(me) = crate::credits::me(world) else {
        return;
    };
    let quote = world
        .resource::<crate::social::Social>()
        .book
        .sent
        .get(&id)
        .and_then(|s| s.quote.as_ref().map(|q| (q.id, q.total.credits())));
    let Some((quote, amount)) = quote else {
        return;
    };
    if amount == 0 {
        return;
    }
    // What the bank calls the order's hold: worked out, not looked up.
    let hold = cadrs_credits::order_hold(&me, id, quote);
    if quietly {
        crate::credits::release_quietly(world, me, hold);
    } else {
        crate::credits::release(world, me, hold, None);
    }
}

/// How long before a signed-for order whose payment is still held is released again, in
/// seconds: soon at first (the bank was away for a moment), then less and less often, up to a
/// minute. And how often such orders are looked for.
const RELEASE_AGAIN_AFTER: f64 = 5.0;
const RELEASE_AGAIN_AT_MOST: f64 = 60.0;
const LOOK_EVERY: f64 = 2.0;

/// The orders signed for whose payment the wallet still shows held (the bank wasn't reached
/// when they were signed): released again, now and then, until it is done.
pub(crate) fn release_signed(
    world: &mut World,
    mut looked: Local<Option<f64>>,
    mut asked: Local<std::collections::HashMap<cadrs_org::Id, (f64, f64)>>,
) {
    let now = world
        .resource::<Time<bevy::time::Real>>()
        .elapsed_secs_f64();
    if looked.is_some_and(|t| now - t < LOOK_EVERY) {
        return;
    }
    *looked = Some(now);
    let Some(me) = crate::credits::me(world) else {
        return;
    };
    let owed: Vec<cadrs_org::Id> = world
        .resource::<crate::social::Social>()
        .book
        .sent
        .iter()
        .filter(|(_, s)| matches!(s.delivery, Some(cadrs_org::letters::Delivery::Signed(_))))
        .map(|(id, _)| *id)
        .filter(|id| {
            matches!(
                crate::credits::payment_of(world, &me, *id),
                Some(crate::credits::OrderPayment::Held(_))
            )
        })
        .collect();
    asked.retain(|id, _| owed.contains(id));
    for id in owed {
        // (When it was last asked, and how long to wait before asking again.)
        let wait = match asked.get(&id) {
            Some((at, wait)) if now - at < *wait => continue,
            Some((_, wait)) => (wait * 2.0).min(RELEASE_AGAIN_AT_MOST),
            None => RELEASE_AGAIN_AFTER,
        };
        asked.insert(id, (now, wait));
        release_order(world, id, true);
    }
}

/// What an order is called where its payment is listed: its parts, cut to a line.
fn order_label(sent: &Sent) -> String {
    let parts: Vec<String> = sent
        .parts
        .iter()
        .map(|(p, n)| format!("{n} × {}", p.name))
        .collect();
    parts.join(", ").chars().take(80).collect()
}

/// A request as the dialog lists it.
#[derive(Clone)]
struct Row {
    id: cadrs_org::Id,
    /// The shop and the parts.
    what: String,
    stage: String,
    price: String,
    /// What accepting its quote costs in credits, when there is a quote to accept.
    accept_for: Option<u64>,
    messages: usize,
    action: Action,
    /// How its payment stands, in words.
    payment: String,
    /// Signed for, and its payment still held: Release payment is offered.
    release: bool,
}

fn show_requests(world: &mut World) {
    crate::social::ensure_loaded(world);
    let mut q = world.query_filtered::<Entity, With<RequestsDialog>>();
    for e in q.iter(world).collect::<Vec<_>>() {
        world.trigger(DialogClose { entity: e });
    }
    let me = crate::credits::me(world);
    let rows: Vec<Row> = world
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
                .map(|q| crate::credits::price_text(&q.currency, q.total))
                .unwrap_or_default();
            // How its payment stands, as the wallet last heard.
            let stands = me.and_then(|me| crate::credits::payment_of(world, &me, *id));
            let release = matches!(stands, Some(crate::credits::OrderPayment::Held(_)))
                && matches!(s.delivery, Some(cadrs_org::letters::Delivery::Signed(_)));
            let payment = stands
                .map(|p| crate::credits::payment_text(&p))
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
            Row {
                id: *id,
                what: format!("{} · {}", s.shop_name, parts.join(", ")),
                stage,
                price,
                // What accepting costs, when there is a quote to accept.
                accept_for: (s.stage == Stage::Quoted)
                    .then(|| s.quote.as_ref().map(|q| q.total.credits()))
                    .flatten(),
                messages: s.chat.len(),
                action: match (&s.delivery, &s.review) {
                    (Some(cadrs_org::letters::Delivery::ToSign(_)), _) => Action::Sign,
                    (Some(cadrs_org::letters::Delivery::Signed(_)), None) => Action::Review,
                    _ => Action::None,
                },
                payment,
                release,
            }
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
                for (i, row) in rows.iter().cloned().enumerate() {
                    let Row {
                        id,
                        what,
                        stage,
                        price,
                        accept_for,
                        messages,
                        action,
                        payment,
                        release,
                    } = row;
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
                        r.spawn((
                            Name::new(format!("rfq-request-price-{i}")),
                            t.text(price, t.font_base, FontWeight::NORMAL, t.foreground),
                        ));
                        if !payment.is_empty() {
                            r.spawn((
                                Name::new(format!("rfq-request-payment-{i}")),
                                t.text(payment, t.font_sm, FontWeight::BOLD, t.link),
                            ));
                        }
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
                        if release {
                            r.spawn((
                                Button::new(format!("rfq-release-{i}"))
                                    .label(tr("rfq-release-payment"))
                                    .small()
                                    .primary()
                                    .build(t),
                                observe(move |_: On<Activate>, mut commands: Commands| {
                                    commands
                                        .queue(move |w: &mut World| release_order(w, id, false));
                                }),
                            ));
                        }
                        if let Some(amount) = accept_for {
                            let label = if amount > 0 {
                                tr_with(
                                    "rfq-accept-and-pay",
                                    &[("amount", &cadrs_credits::show(amount))],
                                )
                            } else {
                                tr("rfq-accept")
                            };
                            r.spawn((
                                Button::new(format!("rfq-accept-{i}"))
                                    .label(label)
                                    .small()
                                    .primary()
                                    .build(t),
                                observe(move |_: On<Activate>, mut commands: Commands| {
                                    commands.queue(move |w: &mut World| accept_and_pay(w, id));
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

/// Accepts request `id`'s quote and pays for it: asked once, the credits are held for the shop
/// (out of the user's balance, not yet in the shop's), and then the shop is told. They are the
/// shop's when the user signs for the delivery, or come back if the shop returns them.
pub fn accept_and_pay(world: &mut World, id: cadrs_org::Id) {
    use crate::credits::{OrderPayment, Payment};
    let social = world.resource::<crate::social::Social>();
    let Some(sent) = social.book.sent.get(&id) else {
        return;
    };
    let amount = sent.quote.as_ref().map_or(0, |q| q.total.credits());
    let (shop, shop_name, label) = (sent.shop, sent.shop_name.clone(), order_label(sent));
    // The hold's id is the order's: pressed twice, or again after an answer was lost, the
    // bank holds once.
    let hold_id = cadrs_credits::order_hold_id(id, sent.quote.as_ref().map_or(0, |q| q.id));
    let Some(me) = crate::credits::me(world) else {
        crate::linked::toast(world, tr("social-no-keys"));
        return;
    };
    // Nothing to pay, or held already (the acceptance didn't get through the last time): only
    // the acceptance.
    let held = matches!(
        crate::credits::payment_of(world, &me, id),
        Some(OrderPayment::Held(_))
    );
    if amount == 0 || held {
        accept(world, id);
        return;
    }
    let shown = cadrs_credits::show(amount);
    let question = tr_with(
        "rfq-pay-question",
        &[("amount", &shown), ("name", &shop_name), ("order", &label)],
    );
    crate::wallet_ui::confirm(
        world,
        tr("rfq-pay-title"),
        question,
        tr_with("rfq-pay-held-until", &[("name", &shop_name)]),
        tr_with("rfq-pay-ok", &[("amount", &shown)]),
        move |world| {
            let payment = Payment {
                from: me,
                to: shop,
                to_name: shop_name,
                amount,
                note: String::new(),
                about: Some(cadrs_credits::About::order(id, label)),
                payout: false,
            };
            // Held first; the shop is told once it is.
            crate::credits::hold_as(
                world,
                payment,
                hold_id,
                Some(Box::new(move |world, answer| {
                    if answer.is_ok() {
                        accept(world, id);
                    }
                })),
            );
        },
    );
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
        // Signed for: what was held for the order is the shop's now. Asked by the order's
        // own hold, whatever this device last heard of its wallet; if the bank can't be
        // reached now, `release_signed` asks again until it is done.
        release_order(world, id, false);
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
