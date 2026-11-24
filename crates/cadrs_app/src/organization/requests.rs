//! The organization screen's Requests module: requests for quotes and the orders they
//! become, with what the shop does about each (quote, decline, reserve sheet, move it on,
//! deliver), and telling the client.

use super::*;

pub(crate) fn stage_key(stage: Stage) -> &'static str {
    match stage {
        Stage::Requested => "org-stage-requested",
        Stage::Quoted => "org-stage-quoted",
        Stage::Declined => "org-stage-declined",
        Stage::Accepted => "org-stage-accepted",
        Stage::InProduction => "org-stage-in-production",
        Stage::Ready => "org-stage-ready",
        Stage::Completed => "org-stage-completed",
    }
}

/// A status as a small coloured tag.
pub(crate) fn status_tag(t: &Theme, name: String, label: String, color: Color) -> impl Bundle {
    cadrs_ui::tag::Tag::new(name, label)
        .color(color)
        .outline()
        .build(t)
}

pub(super) fn stage_color(t: &Theme, stage: Stage) -> Color {
    match stage {
        Stage::Requested => t.link,
        Stage::Declined => t.muted_foreground,
        Stage::Completed => Color::srgb_u8(0x2f, 0x8f, 0x5b),
        _ => t.foreground,
    }
}

/// A request's number as people say it: "R-0007".
pub(crate) fn request_number(number: u32) -> String {
    format!("R-{number:04}")
}

/// The payment held for order `id` is given back to the client (asked once), or claimed when
/// it waited thirty days.
fn settle_payment(world: &mut World, id: cadrs_org::Id, claim: bool) {
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let Some(shop) = open.session.org().org else {
        return;
    };
    let Some(crate::credits::OrderPayment::Held(hold)) =
        crate::credits::payment_of(world, &shop, id)
    else {
        return;
    };
    if claim {
        crate::credits::claim(world, shop, hold.id);
        return;
    }
    let client = crate::credits::name_of(world, &hold.from);
    let shown = cadrs_credits::show(hold.amount);
    crate::wallet_ui::confirm(
        world,
        tr("org-request-return-payment"),
        tr_with(
            "org-request-return-question",
            &[("amount", &shown), ("name", &client)],
        ),
        tr("org-request-return-final"),
        tr_with("org-request-return-ok", &[("amount", &shown)]),
        move |world| {
            crate::credits::give_back(world, shop, hold.id, None);
        },
    );
}

/// The requests for quotes and the orders they became, newest first: each one a card with what
/// was asked for, where it stands, and what can be done next.
pub(super) fn requests(b: &mut ChildSpawnerCommands, t: &Theme, v: &ModuleView) {
    let org = &v.org;
    page_head(b, t, "org-tab-requests", "org-requests-about");
    if org.requests.is_empty() {
        muted(b, t, tr("org-no-requests"));
    }
    // More than a few waiting (a flood, perhaps): all of them declined in one go.
    let waiting = org
        .requests
        .values()
        .filter(|r| r.stage == Stage::Requested)
        .count();
    if waiting > 2 {
        b.spawn((
            Button::new("org-requests-decline-all")
                .label(tr_with(
                    "org-requests-decline-all",
                    &[("count", &waiting.to_string())],
                ))
                .small()
                .build(t),
            observe(|_: On<Activate>, mut commands: Commands| {
                commands.queue(|w: &mut World| ask_sweeping(w, Sweeping::DeclineAll));
            }),
        ));
    }
    let mut newest: Vec<(&cadrs_org::Id, &cadrs_org::Request)> = org.requests.iter().collect();
    newest.sort_by_key(|(_, r)| std::cmp::Reverse((r.number, r.received)));
    for (i, (id, r)) in newest.into_iter().enumerate() {
        let id = *id;
        let parts: Vec<String> = r
            .parts
            .iter()
            .map(|(p, n)| format!("{n} × {}", p.name))
            .collect();
        let price = r
            .quote
            .as_ref()
            .map(|q| crate::credits::price_text(&q.currency, q.total))
            .unwrap_or_default();
        let stage = r.stage;
        // How its payment stands in the organization's wallet, once the bank has said.
        let payment = v.wallet.as_ref().and_then(|w| w.payment(id));
        let unpaid = payment.is_none()
            && v.wallet.as_ref().is_some_and(|w| w.statement.is_some())
            && matches!(stage, Stage::Accepted | Stage::InProduction | Stage::Ready)
            && r.quote.as_ref().is_some_and(|q| q.total.credits() > 0);
        let may_spend = v.wallet.as_ref().is_some_and(|w| w.may_spend());
        // Less than the quote, or from someone who isn't the client: said, in red.
        let expected = r.quote.as_ref().map_or(0, |q| q.total.credits());
        let wrong = payment
            .as_ref()
            .and_then(|p| crate::credits::payment_problem(p, expected, &r.client));
        let client = v.client_name(r);
        let has_document = r
            .snapshot_blob()
            .is_some_and(|blob| cadrs_org::blob::has(&v.dir, &blob.hash));
        // The stock held for it.
        let held: Vec<String> = org
            .reservations
            .values()
            .filter(|res| res.request == id)
            .filter_map(|res| {
                let m = org.materials.get(&res.material)?;
                Some(format!("{} × {} {} mm", res.sheets, m.name, m.thickness))
            })
            .collect();
        b.spawn((
            Name::new(format!("org-request-{i}")),
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                padding: UiRect::all(Val::Px(12.0)),
                width: Val::Px(860.0),
                max_width: Val::Percent(100.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(t.radius)),
                ..default()
            },
            BorderColor::all(t.border),
        ))
        .with_children(|card| {
            row(card, |head| {
                head.spawn(t.text(
                    request_number(r.number),
                    t.font_base,
                    FontWeight::BOLD,
                    t.muted_foreground,
                ));
                head.spawn((
                    Name::new(format!("org-request-client-{i}")),
                    t.text(client, t.font_md, FontWeight::BOLD, t.foreground),
                ));
                head.spawn(status_tag(
                    t,
                    format!("org-request-stage-{i}"),
                    tr(stage_key(stage)),
                    stage_color(t, stage),
                ));
                if !price.is_empty() {
                    head.spawn((
                        Name::new(format!("org-request-price-{i}")),
                        t.text(price, t.font_base, FontWeight::MEDIUM, t.foreground),
                    ));
                }
                if let Some(payment) = &payment {
                    let mut text = crate::credits::payment_text(payment);
                    if let Some(wrong) = wrong {
                        text.push_str(" · ");
                        text.push_str(&tr_with(
                            wrong,
                            &[("amount", &cadrs_credits::show(expected))],
                        ));
                    }
                    head.spawn((
                        Name::new(format!("org-request-payment-{i}")),
                        t.text(
                            text,
                            t.font_sm,
                            FontWeight::BOLD,
                            if wrong.is_some() { t.danger } else { t.link },
                        ),
                    ));
                } else if unpaid {
                    head.spawn((
                        Name::new(format!("org-request-payment-{i}")),
                        t.text(
                            tr("credits-order-unpaid"),
                            t.font_sm,
                            FontWeight::BOLD,
                            t.danger,
                        ),
                    ));
                }
            });
            card.spawn((
                Name::new(format!("org-request-parts-{i}")),
                t.text(
                    parts.join(", "),
                    t.font_base,
                    FontWeight::NORMAL,
                    t.foreground,
                ),
            ));
            if !r.notes.trim().is_empty() {
                muted(card, t, r.notes.clone());
            }
            if !held.is_empty() {
                card.spawn((
                    Name::new(format!("org-request-held-{i}")),
                    t.text(
                        tr_with("org-request-held", &[("stock", &held.join(", "))]),
                        t.font_sm,
                        FontWeight::NORMAL,
                        t.muted_foreground,
                    ),
                ));
            }
            row(card, |actions| {
                let button = |actions: &mut ChildSpawnerCommands,
                              name: String,
                              label: String,
                              primary: bool,
                              act: fn(&mut World, cadrs_org::Id)| {
                    let mut b = Button::new(name).label(label).small();
                    b = if primary { b.primary() } else { b.ghost() };
                    actions.spawn((
                        b.build(t),
                        observe(move |_: On<Activate>, mut commands: Commands| {
                            commands.queue(move |w: &mut World| act(w, id));
                        }),
                    ));
                };
                // The order's next step first.
                match stage {
                    Stage::Requested => {
                        button(
                            actions,
                            format!("org-request-quote-{i}"),
                            tr("org-instant-quote"),
                            true,
                            instant_quote,
                        );
                        button(
                            actions,
                            format!("org-request-decline-{i}"),
                            tr("org-decline"),
                            false,
                            open_decline,
                        );
                    }
                    Stage::Quoted => {
                        button(
                            actions,
                            format!("org-request-quote-{i}"),
                            tr("org-quote-again"),
                            false,
                            instant_quote,
                        );
                        // A quote can still be taken back, until it is accepted.
                        button(
                            actions,
                            format!("org-request-decline-{i}"),
                            tr("org-decline"),
                            false,
                            open_decline,
                        );
                    }
                    Stage::Accepted => button(
                        actions,
                        format!("org-request-next-{i}"),
                        tr("org-start-production"),
                        true,
                        |w, id| advance(w, id, Stage::InProduction),
                    ),
                    Stage::InProduction => button(
                        actions,
                        format!("org-request-next-{i}"),
                        tr("org-mark-ready"),
                        true,
                        |w, id| advance(w, id, Stage::Ready),
                    ),
                    Stage::Ready => button(
                        actions,
                        format!("org-request-next-{i}"),
                        tr("org-delivered"),
                        true,
                        deliver,
                    ),
                    Stage::Declined | Stage::Completed => {}
                }
                // A payment held for it can be given back (it won't be made), or taken when
                // the client left it unsettled for thirty days.
                if let (Some(crate::credits::OrderPayment::Held(hold)), true) =
                    (&payment, may_spend)
                {
                    let waited = cadrs_social::wall_millis()
                        >= hold.time.saturating_add(cadrs_credits::CLAIM_AFTER);
                    if waited {
                        button(
                            actions,
                            format!("org-request-claim-{i}"),
                            tr("org-request-claim-payment"),
                            false,
                            |w, id| settle_payment(w, id, true),
                        );
                    }
                    button(
                        actions,
                        format!("org-request-return-{i}"),
                        tr("org-request-return-payment"),
                        false,
                        |w, id| settle_payment(w, id, false),
                    );
                }
                if has_document {
                    button(
                        actions,
                        format!("org-request-open-{i}"),
                        tr("org-request-open-document"),
                        false,
                        open_request_document,
                    );
                }
                if matches!(stage, Stage::Accepted | Stage::InProduction)
                    && !org.materials.is_empty()
                {
                    button(
                        actions,
                        format!("org-request-reserve-{i}"),
                        tr("org-reserve"),
                        false,
                        open_reserve,
                    );
                }
                button(
                    actions,
                    format!("org-request-chat-{i}"),
                    format!("{} ({})", tr("chat-messages"), r.chat.len()),
                    false,
                    |w, id| crate::chat_ui::open(w, crate::chat_ui::Thread::Shop(id)),
                );
                button(
                    actions,
                    format!("org-request-issue-{i}"),
                    tr("org-issue-report"),
                    false,
                    |w, id| {
                        crate::org_quality::open_new(
                            w,
                            Some(cadrs_org::quality::IssueAbout::Request(id)),
                        )
                    },
                );
                // Someone whose requests aren't wanted.
                if matches!(stage, Stage::Requested | Stage::Quoted | Stage::Declined)
                    && !org.blocked.contains(&r.client)
                {
                    button(
                        actions,
                        format!("org-request-block-{i}"),
                        tr("org-request-block"),
                        false,
                        |w, id| {
                            let client = w
                                .get_resource::<OpenOrg>()
                                .and_then(|o| o.session.org().requests.get(&id).map(|r| r.client));
                            if let Some(client) = client {
                                ask_sweeping(w, Sweeping::Block(client));
                            }
                        },
                    );
                }
            });
            if stage == Stage::Declined
                && let Some(reason) = r.decline_reason.as_ref().filter(|r| !r.trim().is_empty())
            {
                muted(
                    card,
                    t,
                    tr_with("org-declined-because", &[("reason", reason)]),
                );
            }
        });
    }
    // The accounts no requests are taken from, to let in again.
    if !org.blocked.is_empty() {
        subheading(b, t, "org-blocked");
        for (i, account) in org.blocked.iter().enumerate() {
            let account = *account;
            let name = org
                .requests
                .values()
                .find(|r| r.client == account)
                .map_or_else(|| v.account_name(&account), |r| v.client_name(r));
            row(b, |r| {
                r.spawn((
                    Name::new(format!("org-blocked-{i}")),
                    t.text(name, t.font_base, FontWeight::NORMAL, t.foreground),
                ));
                r.spawn((
                    Button::new(format!("org-unblock-{i}"))
                        .label(tr("org-unblock"))
                        .small()
                        .ghost()
                        .build(t),
                    observe(move |_: On<Activate>, mut commands: Commands| {
                        commands.queue(move |w: &mut World| {
                            write(
                                w,
                                Op::SetBlocked {
                                    account,
                                    blocked: false,
                                },
                            );
                        });
                    }),
                ));
            });
        }
    }
}

/// Something done to many requests at once, after asking.
#[derive(Resource, Clone, Copy)]
pub(super) enum Sweeping {
    /// Decline every request that waits for an answer.
    DeclineAll,
    /// Take no more requests from this account, and decline those of its that wait.
    Block(cadrs_social::keys::AccountId),
}

/// Asks before [`Sweeping`] is done.
pub(super) fn ask_sweeping(world: &mut World, what: Sweeping) {
    world.insert_resource(what);
    let (title, text) = match what {
        Sweeping::DeclineAll => ("org-decline-all-title", "org-decline-all-text"),
        Sweeping::Block(_) => ("org-block-title", "org-block-text"),
    };
    let theme = world.resource::<Theme>().clone();
    let tb = theme.clone();
    world.spawn((
        Dialog::new("org-sweeping-dialog")
            .title(tr(title))
            .width(500.0)
            .body(move |b| {
                b.spawn(tb.paragraph(tr(text), tb.font_base, tb.foreground));
            })
            .footer(|f| dialog_buttons(f, "org-sweeping", sweep_requests))
            .build(&theme),
        OrgDialog,
        DespawnOnExit(AppState::Organization),
    ));
    world.flush();
}

/// Does what was asked: the block first, then a decline for each request it covers that waits.
/// The clients are told, as for any decline.
fn sweep_requests(world: &mut World) {
    let Some(what) = world.remove_resource::<Sweeping>() else {
        return;
    };
    close_dialogs(world);
    if let Sweeping::Block(account) = what {
        let blocked = write(
            world,
            Op::SetBlocked {
                account,
                blocked: true,
            },
        );
        if !blocked {
            return;
        }
    }
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let waiting: Vec<cadrs_org::Id> = open
        .session
        .org()
        .requests
        .iter()
        .filter(|(_, r)| r.stage == Stage::Requested)
        .filter(|(_, r)| match what {
            Sweeping::DeclineAll => true,
            Sweeping::Block(account) => r.client == account,
        })
        .map(|(id, _)| *id)
        .collect();
    for request in waiting {
        let declined = write(
            world,
            Op::Decline {
                request,
                reason: String::new(),
            },
        );
        if declined {
            tell_client(
                world,
                request,
                cadrs_org::letters::Message::Decline {
                    request,
                    reason: String::new(),
                },
            );
        }
    }
}

/// Opens the document a client sent with request `request`: a copy of it in the user's
/// documents, named for the request.
pub(super) fn open_request_document(world: &mut World, request: cadrs_org::Id) {
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let Some(r) = open.session.org().requests.get(&request) else {
        return;
    };
    let Some(bytes) = r
        .snapshot_blob()
        .and_then(|blob| cadrs_org::blob::get(&open.dir(), &blob.hash))
    else {
        crate::linked::toast(world, tr("org-request-no-document"));
        return;
    };
    let name = tr_with(
        "org-request-document-name",
        &[
            ("number", &request_number(r.number)),
            ("shop", &open.session.org().profile.name),
        ],
    );
    crate::request_quote::open_snapshot(world, &bytes, &name);
}

/// The order the Reserve or Decline dialog is for.
#[derive(Resource)]
pub(super) struct Acting(cadrs_org::Id);

pub(super) fn open_decline(world: &mut World, request: cadrs_org::Id) {
    world.insert_resource(Acting(request));
    let theme = world.resource::<Theme>().clone();
    let tb = theme.clone();
    world.spawn((
        Dialog::new("org-decline-dialog")
            .title(tr("org-decline"))
            .width(520.0)
            .body(move |b| {
                b.spawn(t_text(&tb, "org-decline-reason"));
                b.spawn(
                    TextInput::new("org-decline-reason")
                        .lines(4)
                        .placeholder(tr("org-decline-reason-placeholder"))
                        .autofocus()
                        .build(&tb),
                );
            })
            .footer(|f| dialog_buttons(f, "org-decline", decline))
            .build(&theme),
        OrgDialog,
        DespawnOnExit(AppState::Organization),
    ));
    world.flush();
}

pub(super) fn decline(world: &mut World) {
    let Some(Acting(request)) = world.remove_resource::<Acting>() else {
        return;
    };
    let reason = field(world, "org-decline-reason").trim().to_owned();
    let op = Op::Decline {
        request,
        reason: reason.clone(),
    };
    if write(world, op) {
        close_dialogs(world);
        tell_client(
            world,
            request,
            cadrs_org::letters::Message::Decline { request, reason },
        );
    } else {
        world.insert_resource(Acting(request));
    }
}

pub(super) fn open_reserve(world: &mut World, request: cadrs_org::Id) {
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let org = open.session.org();
    let materials: Vec<String> = org
        .materials
        .iter()
        .map(|(id, m)| {
            format!(
                "{} {} mm ({} {})",
                m.name,
                m.thickness,
                org.available(*id),
                tr("org-free")
            )
        })
        .collect();
    world.insert_resource(Acting(request));
    let theme = world.resource::<Theme>().clone();
    let t = theme.clone();
    let tb = theme.clone();
    world.spawn((
        Dialog::new("org-reserve-dialog")
            .title(tr("org-reserve"))
            .width(520.0)
            .body(move |b| {
                let t = &tb;
                b.spawn(choice(t, "org-reserve-material", materials.clone(), 0));
                b.spawn(t_text(t, "org-reserve-sheets"));
                b.spawn(
                    TextInput::new("org-reserve-sheets")
                        .value("1")
                        .width(Val::Px(100.0))
                        .build(t),
                );
            })
            .footer(|f| dialog_buttons(f, "org-reserve", reserve))
            .build(&t),
        OrgDialog,
        DespawnOnExit(AppState::Organization),
    ));
    world.flush();
}

pub(super) fn reserve(world: &mut World) {
    let Some(Acting(request)) = world.remove_resource::<Acting>() else {
        return;
    };
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let materials: Vec<cadrs_org::Id> = open.session.org().materials.keys().copied().collect();
    let Some(material) = materials
        .get(strip_selected(world, "org-reserve-material"))
        .copied()
    else {
        return;
    };
    let sheets = number(world, "org-reserve-sheets").unwrap_or(1.0).max(1.0) as u32;
    let op = Op::Reserve {
        id: new_id(),
        reservation: cadrs_org::Reservation {
            material,
            sheets,
            request,
        },
    };
    if write(world, op) {
        close_dialogs(world);
    } else {
        // Refused (not enough free): keep the dialog for another try.
        world.insert_resource(Acting(request));
    }
}

/// Moves an order on, and tells the client.
pub fn advance(world: &mut World, request: cadrs_org::Id, stage: Stage) {
    if write(world, Op::Advance { request, stage }) {
        tell_client(
            world,
            request,
            cadrs_org::letters::Message::Stage { request, stage },
        );
    }
}

/// Delivered: the organization signs half the order's receipt and sends it to the client to
/// countersign; the order completes when it comes back.
pub fn deliver(world: &mut World, request: cadrs_org::Id) {
    crate::social::ensure_loaded(world);
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let org = open.session.org();
    let (Some(account), Some(client)) = (org.org, org.requests.get(&request).map(|r| r.client))
    else {
        return;
    };
    let Some(key) = crate::social::org_key(world, &account) else {
        crate::linked::toast(world, tr("org-needs-org-key"));
        return;
    };
    // What was held for it is used up.
    let held: Vec<cadrs_org::Id> = org
        .reservations
        .iter()
        .filter(|(_, r)| r.request == request)
        .map(|(id, _)| *id)
        .collect();
    for id in held {
        write(world, Op::Release { id, used: true });
    }
    let receipt = cadrs_social::receipts::Receipt::OrderCompleted {
        shop: account,
        client,
        order: cadrs_org::order_ref(request),
        at: cadrs_social::wall_millis(),
    }
    .sign_first(&key)
    .expect("the organization is the shop");
    tell_client(
        world,
        request,
        cadrs_org::letters::Message::Delivered { request, receipt },
    );
}

/// Sends `message` to the client of request `request`, from the open organization.
pub(crate) fn tell_client(
    world: &mut World,
    request: cadrs_org::Id,
    message: cadrs_org::letters::Message,
) {
    crate::social::ensure_loaded(world);
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let org = open.session.org();
    let (Some(client), Some(account)) = (org.requests.get(&request).map(|r| r.client), org.org)
    else {
        return;
    };
    // From the contacts, else the card the request came with.
    let card = world
        .resource::<crate::social::Social>()
        .contacts
        .get(&client)
        .cloned()
        .or_else(|| {
            world
                .get_resource::<OpenOrg>()?
                .session
                .org()
                .requests
                .get(&request)?
                .card
                .as_ref()
                .map(|c| c.value.clone())
        });
    match card {
        Some(card) => {
            crate::social::send(
                world,
                &card,
                message,
                Some(account),
                tr("social-what-reply"),
            );
        }
        None => crate::linked::toast(world, tr("social-no-client-card")),
    }
}

/// How long a quote stands, in milliseconds.
pub(crate) const QUOTE_VALID_FOR: u64 = 14 * 86_400_000;

/// Quotes a request at the price the rate card gives.
pub fn instant_quote(world: &mut World, request: cadrs_org::Id) {
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let org = open.session.org();
    let Some(rates) = org.rates.clone() else {
        crate::linked::toast(world, tr("org-no-rates"));
        return;
    };
    let Some(r) = org.requests.get(&request) else {
        return;
    };
    match cadrs_org::quote::estimate(&org.machines, &rates, &r.parts) {
        Ok(estimate) => {
            let quote = cadrs_org::Quote {
                id: new_id(),
                total: estimate.total,
                currency: estimate.currency,
                lines: estimate.lines,
                valid_until: cadrs_social::wall_millis() + QUOTE_VALID_FOR,
                note: String::new(),
            };
            let op = Op::Quote {
                request,
                quote: quote.clone(),
            };
            if write(world, op) {
                tell_client(
                    world,
                    request,
                    cadrs_org::letters::Message::Quote { request, quote },
                );
            }
        }
        Err(cadrs_org::quote::NoPrice::NoMachine { part, why }) => {
            crate::linked::toast(
                world,
                tr_with(
                    "org-no-machine-for",
                    &[("part", &part), ("why", &why.join("; "))],
                ),
            );
        }
        Err(cadrs_org::quote::NoPrice::NoRate(what)) => {
            crate::linked::toast(world, format!("{}: {what}", tr("org-no-rates")));
        }
    }
}
