//! The wallet screen: a balance, the faucet, and every movement, for the user's own wallet (the
//! account menu's **Wallet**, a dialog) and for an organization's (its **Wallet** module).
//!
//! - **Get test credits** takes from the faucet (a person's wallet only); while the hour runs
//!   it says how long is left.
//! - **Transfer…** sends credits to a contact, an organization, a shop, or a wallet address
//!   typed in; from an organization, **Pay out…** sends them to a person (its members first).
//!   Either asks once before anything moves: "Send 250 credits to Cape Waterjet?".
//! - The transactions list what came in and went out, who it was with and what for, newest
//!   first; **Show older** asks the bank for more. **Copy as CSV** puts them on the clipboard.
//!
//! Names, for the wallet `<p>` (`wallet` the user's, `org-wallet` an organization's):
//! `<p>-balance`, `<p>-held`, `<p>-faucet`, `<p>-faucet-wait`, `<p>-transfer`, `<p>-payout`,
//! `<p>-copy-address`, `<p>-copy-csv`, `<p>-movements` with rows `<p>-move-<i>`, `<p>-older`,
//! `<p>-error`. The dialogs: `wallet-dialog`; `wallet-transfer-dialog` with `wallet-to` (a
//! dropdown: `wallet-to-choose`, `-search`, `-item-<slug>`), `wallet-amount`, `wallet-note`,
//! `wallet-transfer-ok`; `wallet-confirm` with `wallet-confirm-ok`.

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_credits::{Credits as Amount, Kind, Movement, Statement};
use cadrs_social::keys::AccountId;
use cadrs_ui::dialog::{Dialog, DialogClose};
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::input::TextInput;
use cadrs_ui::theme::Theme;
use cadrs_ui::{Button, Column};

use crate::credits::{self, Credits, Payment};
use crate::organization::{
    ModuleView, cell_button, field, muted, page_head, picked, row, table, text_row,
};

pub struct WalletUiPlugin;

impl Plugin for WalletUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                sync_dialog,
                tick_faucet,
                crate::request_quote::follow_wallet,
                crate::request_quote::release_signed,
            ),
        );
    }
}

/// One movement as a line of the list.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Line {
    pub when: String,
    pub what: String,
    pub who: String,
    pub about: String,
    pub amount: String,
}

/// A wallet as its screen shows it: read from the world once, then drawn.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WalletView {
    pub account: AccountId,
    pub organization: bool,
    pub statement: Option<Statement>,
    pub lines: Vec<Line>,
    /// The movements the lines are of, newest first.
    pub movements: Vec<Movement>,
    pub more: bool,
    pub error: Option<String>,
    pub asking: bool,
    /// How long until the faucet gives again, in milliseconds (0: now); none for a wallet it
    /// doesn't serve.
    pub faucet_in: Option<u64>,
    /// There is a bank to ask.
    pub bank: bool,
    /// The payments held from or for the wallet that still wait.
    pub held: Vec<HeldLine>,
}

/// A held payment as the wallet lists it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HeldLine {
    pub id: cadrs_credits::MoveId,
    pub when: String,
    /// The other side's name.
    pub who: String,
    pub about: String,
    pub amount: Amount,
    /// This wallet is the payer: it can release. Otherwise it is the payee: it can return.
    pub payer: bool,
    /// Thirty days went by: the payee may claim it.
    pub due: bool,
    /// The order it is for, if it says one.
    pub order: Option<cadrs_org::Id>,
}

/// How long until the faucet gives to `wallet` again, by this device's clock since the bank
/// last said.
fn faucet_in(wallet: &credits::Wallet, now_ms: u64) -> Option<u64> {
    let statement = wallet.statement.as_ref()?;
    let due = statement.faucet_at?;
    let bank_now = statement
        .time
        .saturating_add(now_ms.saturating_sub(wallet.heard_ms));
    Some(due.saturating_sub(bank_now))
}

/// "42 min", "under a minute".
fn wait_text(ms: u64) -> String {
    let minutes = ms.div_ceil(60_000);
    if minutes <= 1 {
        tr("wallet-faucet-under-a-minute")
    } else {
        tr_with(
            "wallet-faucet-minutes",
            &[("minutes", &minutes.to_string())],
        )
    }
}

/// What a movement was, seen from `wallet`, and its amount with the sign it had there. A
/// payment held for the wallet moved nothing yet: its amount is in brackets.
pub(crate) fn describe(m: &Movement, wallet: &AccountId) -> (String, String) {
    let change = m.change_for(wallet);
    let incoming = &m.to == wallet;
    let what = tr(match (m.kind, incoming) {
        (Kind::Faucet, _) => "wallet-kind-faucet",
        (Kind::Transfer, true) => "wallet-kind-received",
        (Kind::Transfer, false) => "wallet-kind-sent",
        (Kind::Payout, true) => "wallet-kind-payout-in",
        (Kind::Payout, false) => "wallet-kind-payout-out",
        (Kind::Held, true) => "wallet-kind-held-in",
        (Kind::Held, false) => "wallet-kind-held-out",
        (Kind::Released | Kind::Claimed, true) => "wallet-kind-released-in",
        (Kind::Released | Kind::Claimed, false) => "wallet-kind-released-out",
        (Kind::Returned, true) => "wallet-kind-returned-in",
        (Kind::Returned, false) => "wallet-kind-returned-out",
        (Kind::Other, _) => "wallet-kind-other",
    });
    let shown = cadrs_credits::show(m.amount);
    let amount = match change {
        c if c > 0 => format!("+{shown}"),
        c if c < 0 => format!("\u{2212}{shown}"),
        _ => format!("({shown})"),
    };
    (what, amount)
}

impl WalletView {
    pub fn read(world: &World, account: AccountId, organization: bool) -> Self {
        let credits = world.resource::<Credits>();
        let wallet = credits.wallet(&account);
        let now = cadrs_social::wall_millis();
        let me = credits::me(world);
        let lines = wallet
            .map(|w| {
                w.movements
                    .iter()
                    .map(|m| {
                        let (what, amount) = describe(m, &account);
                        let other = m.other(&account).map_or_else(
                            || tr("credits-the-faucet"),
                            |a| credits::name_of(world, &a),
                        );
                        // An organization's wallet is spent from by its people: who did it.
                        let who = if organization && m.from == Some(account) && Some(m.by) != me {
                            format!("{other} ({})", credits::name_of(world, &m.by))
                        } else {
                            other
                        };
                        let label = m.about.as_ref().map(|a| a.label.as_str()).unwrap_or("");
                        let about = [label, m.note.as_str()]
                            .into_iter()
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                            .join(" · ");
                        Line {
                            when: crate::org_tabs::show_when(m.time),
                            what,
                            who,
                            about,
                            amount,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            account,
            organization,
            statement: wallet.and_then(|w| w.statement.clone()),
            lines,
            movements: wallet.map(|w| w.movements.clone()).unwrap_or_default(),
            more: wallet.is_some_and(|w| w.more),
            error: wallet.and_then(|w| w.error.clone()),
            asking: wallet.is_some_and(|w| w.asking),
            faucet_in: wallet.and_then(|w| faucet_in(w, now)),
            bank: world.contains_resource::<credits::BankAt>(),
            held: wallet
                .and_then(|w| w.statement.as_ref())
                .map(|s| {
                    s.holds
                        .iter()
                        .map(|h| {
                            let payer = h.from == account;
                            let other = if payer { h.to } else { h.from };
                            HeldLine {
                                id: h.id,
                                when: crate::org_tabs::show_when(h.time),
                                who: credits::name_of(world, &other),
                                about: h
                                    .about
                                    .as_ref()
                                    .map(|a| a.label.clone())
                                    .unwrap_or_default(),
                                amount: h.amount,
                                payer,
                                due: now >= h.time.saturating_add(cadrs_credits::CLAIM_AFTER),
                                order: h.about.as_ref().and_then(cadrs_credits::About::order_id),
                            }
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    pub fn may_spend(&self) -> bool {
        self.statement.as_ref().is_some_and(|s| s.may_spend)
    }

    /// How order `request`'s payment stands in this wallet.
    pub fn payment(&self, request: cadrs_org::Id) -> Option<credits::OrderPayment> {
        let named = self.statement.as_ref().map_or(&[][..], |s| &s.named[..]);
        let holds = self.statement.as_ref().map_or(&[][..], |s| &s.holds[..]);
        credits::order_payment(named, holds, &self.movements, request)
    }

    /// What came in this calendar month (by this device's clock), from the movements shown.
    pub fn received_this_month(&self) -> Amount {
        use chrono::Datelike;
        let month = |ms: u64| {
            chrono::DateTime::from_timestamp_millis(ms as i64)
                .map(|t| t.with_timezone(&chrono::Local))
                .map(|t| (t.year(), t.month()))
        };
        let now = month(cadrs_social::wall_millis());
        self.movements
            .iter()
            .filter(|m| month(m.time) == now)
            .map(|m| m.change_for(&self.account).max(0) as Amount)
            .sum()
    }
}

/// The text beside the faucet's button that counts down.
#[derive(Component)]
struct FaucetWait(AccountId);

/// A button that runs `act` on the world.
fn action(
    t: &Theme,
    button: Button,
    act: impl Fn(&mut World) + Send + Sync + Clone + 'static,
) -> impl Bundle {
    (
        button.build(t),
        observe(move |_: On<Activate>, mut commands: Commands| {
            let act = act.clone();
            commands.queue(move |world: &mut World| act(world));
        }),
    )
}

/// Draws the wallet `v` into `b`, its elements named `<prefix>-…`.
pub(crate) fn page(b: &mut ChildSpawnerCommands, t: &Theme, v: &WalletView, prefix: &'static str) {
    let account = v.account;
    let balance = v.statement.as_ref().map(|s| s.balance);
    b.spawn((
        Name::new(format!("{prefix}-balance")),
        t.text(
            match balance {
                Some(balance) => tr_with(
                    "wallet-balance",
                    &[("amount", &cadrs_credits::show(balance))],
                ),
                None if v.asking => tr("wallet-asking"),
                None => tr("wallet-unknown"),
            },
            t.font_xl * 1.4,
            FontWeight::SEMIBOLD,
            t.foreground,
        ),
    ));
    if let Some(s) = &v.statement
        && (s.held_out > 0 || s.held_in > 0)
    {
        let mut parts = Vec::new();
        if s.held_out > 0 {
            parts.push(tr_with(
                "wallet-held-out",
                &[("amount", &cadrs_credits::show(s.held_out))],
            ));
        }
        if s.held_in > 0 {
            parts.push(tr_with(
                "wallet-held-in",
                &[("amount", &cadrs_credits::show(s.held_in))],
            ));
        }
        b.spawn((
            Name::new(format!("{prefix}-held")),
            t.text(
                parts.join(" · "),
                t.font_base,
                FontWeight::NORMAL,
                t.muted_foreground,
            ),
        ));
    }
    if let Some(error) = &v.error {
        b.spawn((
            Name::new(format!("{prefix}-error")),
            t.paragraph_within(error.clone(), t.font_base, t.danger, 720.0),
        ));
    }
    if v.organization && v.statement.is_some() && !v.may_spend() {
        muted(b, t, tr("wallet-org-look-only"));
    }
    row(b, |r| {
        // The faucet is a person's: an organization earns its credits.
        if !v.organization {
            let ready = v.faucet_in.is_none_or(|ms| ms == 0);
            r.spawn(action(
                t,
                Button::new(format!("{prefix}-faucet"))
                    .label(tr_with(
                        "wallet-faucet",
                        &[("amount", &cadrs_credits::show(cadrs_credits::FAUCET))],
                    ))
                    .icon("faucet")
                    .primary()
                    .disabled(!ready),
                credits::faucet,
            ));
            r.spawn((
                Name::new(format!("{prefix}-faucet-wait")),
                FaucetWait(account),
                t.text(
                    match v.faucet_in {
                        Some(ms) if ms > 0 => {
                            tr_with("wallet-faucet-again-in", &[("time", &wait_text(ms))])
                        }
                        _ => String::new(),
                    },
                    t.font_sm,
                    FontWeight::NORMAL,
                    t.muted_foreground,
                ),
            ));
        }
        if !v.organization || v.may_spend() {
            r.spawn(action(
                t,
                Button::new(format!("{prefix}-transfer"))
                    .label(tr("wallet-transfer"))
                    .icon("send-credits"),
                move |world| open_transfer(world, account, false),
            ));
        }
        if v.organization && v.may_spend() {
            r.spawn(action(
                t,
                Button::new(format!("{prefix}-payout"))
                    .label(tr("wallet-payout"))
                    .icon("payout"),
                move |world| open_transfer(world, account, true),
            ));
        }
        r.spawn(action(
            t,
            Button::new(format!("{prefix}-copy-address"))
                .label(tr("wallet-copy-address"))
                .icon("copy")
                .ghost(),
            move |world| {
                copy(world, account.to_string());
                crate::linked::toast(world, tr("wallet-address-copied"));
            },
        ));
    });
    held(b, t, v, prefix);
    b.spawn((
        t.text(
            tr("wallet-transactions"),
            t.font_md,
            FontWeight::BOLD,
            t.foreground,
        ),
        Node {
            margin: UiRect::top(Val::Px(10.0)),
            ..default()
        },
    ));
    let columns = vec![
        Column::new("when", tr("wallet-col-when")).width(140.0),
        Column::new("what", tr("wallet-col-what")).width(150.0),
        Column::new("who", tr("wallet-col-who")).width(190.0),
        Column::new("about", tr("wallet-col-about")).width(200.0),
        Column::new("amount", tr("wallet-col-amount")).width(100.0),
    ];
    let rows = v
        .lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            text_row(
                t,
                format!("{prefix}-move-{i}"),
                &columns,
                [
                    line.when.clone(),
                    line.what.clone(),
                    line.who.clone(),
                    line.about.clone(),
                    line.amount.clone(),
                ],
            )
        })
        .collect();
    let (name, empty) = if prefix == "wallet" {
        ("wallet-movements", "wallet-none-yet")
    } else {
        ("org-wallet-movements", "wallet-none-yet-org")
    };
    table(b, t, name, columns, rows, empty);
    row(b, |r| {
        if v.more {
            r.spawn(action(
                t,
                Button::new(format!("{prefix}-older")).label(tr("wallet-older")),
                move |world| credits::older(world, account),
            ));
        }
        if !v.lines.is_empty() {
            let organization = v.organization;
            r.spawn(action(
                t,
                Button::new(format!("{prefix}-copy-csv"))
                    .label(tr("wallet-copy-csv"))
                    .ghost(),
                move |world| {
                    let csv = csv(&WalletView::read(world, account, organization));
                    copy(world, csv);
                    crate::linked::toast(world, tr("wallet-csv-copied"));
                },
            ));
        }
    });
    if !v.bank {
        muted(b, t, tr("credits-no-bank"));
    }
}

/// How many held payments the wallet lists (the largest; the bank sends no more than its own
/// ceiling either).
const HELD_SHOWN: usize = 50;

/// The payments held from or for the wallet that still wait, each with what can be done about
/// it here: the payer releases it, the payee returns it or, after thirty days, claims it.
fn held(b: &mut ChildSpawnerCommands, t: &Theme, v: &WalletView, prefix: &'static str) {
    if v.held.is_empty() {
        return;
    }
    let account = v.account;
    let may = !v.organization || v.may_spend();
    b.spawn((
        t.text(
            tr("wallet-held-payments"),
            t.font_md,
            FontWeight::BOLD,
            t.foreground,
        ),
        Node {
            margin: UiRect::top(Val::Px(10.0)),
            ..default()
        },
    ));
    let columns = vec![
        Column::new("when", tr("wallet-col-when")).width(140.0),
        Column::new("what", tr("wallet-col-what")).width(130.0),
        Column::new("who", tr("wallet-col-who")).width(180.0),
        Column::new("about", tr("wallet-col-about")).width(150.0),
        Column::new("amount", tr("wallet-col-amount")).width(80.0),
        Column::new("actions", "").width(150.0),
    ];
    let rows = v
        .held
        .iter()
        .take(HELD_SHOWN)
        .enumerate()
        .map(|(i, line)| {
            let (id, payer, due) = (line.id, line.payer, line.due);
            let what = tr(if payer {
                "wallet-kind-held-out"
            } else {
                "wallet-kind-held-in"
            });
            text_row(
                t,
                format!("{prefix}-hold-{i}"),
                &columns,
                [
                    line.when.clone(),
                    what,
                    line.who.clone(),
                    line.about.clone(),
                    cadrs_credits::show(line.amount),
                ],
            )
            .cell(move |c| {
                if !may {
                    return;
                }
                if payer {
                    cell_button(
                        c,
                        format!("{prefix}-hold-release-{i}"),
                        tr("wallet-hold-release"),
                        false,
                        move |world| settle(world, account, id, Settle::Release),
                    );
                } else {
                    if due {
                        cell_button(
                            c,
                            format!("{prefix}-hold-claim-{i}"),
                            tr("wallet-hold-claim"),
                            false,
                            move |world| settle(world, account, id, Settle::Claim),
                        );
                    }
                    cell_button(
                        c,
                        format!("{prefix}-hold-return-{i}"),
                        tr("wallet-hold-return"),
                        false,
                        move |world| settle(world, account, id, Settle::Return),
                    );
                }
            })
        })
        .collect();
    let name = if prefix == "wallet" {
        "wallet-holds"
    } else {
        "org-wallet-holds"
    };
    table(b, t, name, columns, rows, "wallet-none-yet");
    let open = v.statement.as_ref().map_or(0, |s| s.open_holds as usize);
    let shown = v.held.len().min(HELD_SHOWN);
    if open > shown {
        muted(
            b,
            t,
            tr_with(
                "wallet-held-more",
                &[("count", &(open - shown).to_string())],
            ),
        );
    }
}

#[derive(Clone, Copy)]
enum Settle {
    Release,
    Return,
    Claim,
}

/// Settles held payment `hold` of `wallet`, asked once first: none of it can be taken back.
fn settle(world: &mut World, wallet: AccountId, hold: cadrs_credits::MoveId, how: Settle) {
    let line = WalletView::read(world, wallet, false)
        .held
        .into_iter()
        .find(|line| line.id == hold);
    let Some(line) = line else {
        return;
    };
    let shown = cadrs_credits::show(line.amount);
    let args = [("amount", shown.as_str()), ("name", line.who.as_str())];
    let (title, question, last_word, ok) = match how {
        Settle::Release => (
            "wallet-hold-release",
            "wallet-hold-release-question",
            "wallet-hold-release-final",
            "wallet-hold-release-ok",
        ),
        Settle::Return => (
            "wallet-hold-return",
            "wallet-hold-return-question",
            "org-request-return-final",
            "org-request-return-ok",
        ),
        Settle::Claim => (
            "wallet-hold-claim",
            "wallet-hold-claim-question",
            "wallet-hold-claim-final",
            "wallet-hold-claim-ok",
        ),
    };
    confirm(
        world,
        tr(title),
        tr_with(question, &args),
        tr_with(last_word, &args),
        tr_with(ok, &args),
        move |world| {
            match how {
                Settle::Release => credits::release(world, wallet, hold, None),
                Settle::Return => credits::give_back(world, wallet, hold, None),
                Settle::Claim => credits::claim(world, wallet, hold),
            };
        },
    );
}

/// The held payments for `wallet` that are for none of `orders`: strangers' (anyone may hold
/// credits for a shop), which only clutter its list.
pub(crate) fn strays(held: &[HeldLine], orders: &[cadrs_org::Id]) -> Vec<cadrs_credits::MoveId> {
    held.iter()
        .filter(|line| !line.payer && line.order.is_none_or(|order| !orders.contains(&order)))
        .map(|line| line.id)
        .collect()
}

fn copy(world: &mut World, text: String) {
    if let Some(mut clip) = world.get_resource_mut::<bevy::clipboard::Clipboard>() {
        let _ = clip.set_text(text);
    }
}

/// The movements shown, as CSV: a heading, then a line each, fields quoted where they must be.
pub(crate) fn csv(v: &WalletView) -> String {
    let quote = |s: &str| {
        if s.contains([',', '"', '\n']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_owned()
        }
    };
    let mut out = String::from("when,what,who,for,amount\n");
    for line in &v.lines {
        // The amount as a plain number: no thousands' commas, a minus a spreadsheet reads.
        let amount = line.amount.replace(',', "").replace('\u{2212}', "-");
        let fields = [&line.when, &line.what, &line.who, &line.about, &amount];
        out.push_str(&fields.map(|f| quote(f)).join(","));
        out.push('\n');
    }
    out
}

/// The faucet's wait counts down; when it is over the wallet is drawn again, its button ready.
fn tick_faucet(
    mut waits: Query<(&FaucetWait, &mut Text)>,
    mut credits: ResMut<Credits>,
    time: Res<Time<bevy::time::Real>>,
    mut last: Local<f64>,
) {
    let now = time.elapsed_secs_f64();
    if waits.is_empty() || now - *last < 1.0 {
        return;
    }
    *last = now;
    let now_ms = cadrs_social::wall_millis();
    let mut over = false;
    for (wait, mut text) in &mut waits {
        let Some(ms) = credits.wallet(&wait.0).and_then(|w| faucet_in(w, now_ms)) else {
            continue;
        };
        let want = if ms > 0 {
            tr_with("wallet-faucet-again-in", &[("time", &wait_text(ms))])
        } else {
            String::new()
        };
        // It was waiting and no longer is: the button is drawn again, enabled.
        over |= ms == 0 && !text.0.is_empty();
        if text.0 != want {
            text.0 = want;
        }
    }
    if over {
        credits.version = credits.version.wrapping_add(1);
    }
}

/// An organization's Wallet module.
pub(crate) fn org_page(b: &mut ChildSpawnerCommands, t: &Theme, v: &ModuleView) {
    page_head(b, t, "org-tab-wallet", "org-wallet-about");
    let Some(wallet) = &v.wallet else {
        muted(b, t, tr("wallet-unknown"));
        return;
    };
    page(b, t, wallet, "org-wallet");
    // Anyone can hold credits for a shop. Those that are for none of its orders are returned
    // in one go.
    let orders: Vec<cadrs_org::Id> = v.org.requests.keys().copied().collect();
    let stray = strays(&wallet.held, &orders);
    if !stray.is_empty() && wallet.may_spend() {
        let account = wallet.account;
        let count = stray.len().to_string();
        muted(b, t, tr_with("org-wallet-strays", &[("count", &count)]));
        b.spawn(action(
            t,
            Button::new("org-wallet-return-strays")
                .label(tr_with("org-wallet-return-strays", &[("count", &count)]))
                .small(),
            move |world| {
                let stray = stray.clone();
                let count = stray.len().to_string();
                confirm(
                    world,
                    tr("wallet-hold-return"),
                    tr_with("org-wallet-return-strays-question", &[("count", &count)]),
                    tr("org-request-return-final"),
                    tr_with("org-wallet-return-strays", &[("count", &count)]),
                    move |world| {
                        for hold in stray {
                            credits::give_back(world, account, hold, None);
                        }
                    },
                );
            },
        ));
    }
}

/// The body of the user's wallet dialog.
#[derive(Component)]
struct WalletBody;

#[derive(Component)]
struct WalletDialog;

fn close(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<WalletDialog>>();
    for dialog in q.iter(world).collect::<Vec<_>>() {
        world.trigger(DialogClose { entity: dialog });
    }
}

/// A dialog's button that runs `act` on the world.
fn dialog_button(f: &mut ChildSpawner, button: Button, act: fn(&mut World)) {
    let t = f.world().resource::<Theme>().clone();
    f.spawn((
        button.build(&t),
        observe(move |_: On<Activate>, mut commands: Commands| {
            commands.queue(act);
        }),
    ));
}

/// Opens the user's wallet (the account menu's Wallet).
pub fn open(world: &mut World) {
    crate::social::ensure_loaded(world);
    let Some(me) = credits::me(world) else {
        crate::linked::toast(world, tr("social-no-keys"));
        return;
    };
    close(world);
    credits::unwatch(world, &me);
    credits::watch(world, me);
    let theme = world.resource::<Theme>().clone();
    world.spawn((
        Dialog::new("wallet-dialog")
            .title(tr("wallet-title"))
            .width(840.0)
            .body(|b| {
                b.spawn((
                    Name::new("wallet-body"),
                    WalletBody,
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(8.0),
                        min_height: Val::Px(320.0),
                        ..default()
                    },
                ));
            })
            .footer(|f| {
                dialog_button(
                    f,
                    Button::new("wallet-close").label(tr("common-close")),
                    close,
                );
            })
            .build(&theme),
        WalletDialog,
    ));
    world.flush();
}

/// The open wallet dialog shows the wallet as it is: drawn when it opens and whenever the
/// bank said something new.
fn sync_dialog(world: &mut World, mut shown: Local<Option<(Entity, WalletView)>>) {
    let mut q = world.query_filtered::<Entity, With<WalletBody>>();
    let Some(body) = q.iter(world).next() else {
        // Closed: the wallet is no longer on screen.
        if let Some((_, was)) = shown.take() {
            credits::unwatch(world, &was.account);
        }
        return;
    };
    let Some(me) = credits::me(world) else {
        return;
    };
    let view = WalletView::read(world, me, false);
    // The countdown runs in its own text: a minute passing isn't a reason to draw it all again.
    let same = |a: &WalletView, b: &WalletView| {
        let ready = |v: &WalletView| v.faucet_in.is_none_or(|ms| ms == 0);
        ready(a) == ready(b)
            && WalletView {
                faucet_in: None,
                ..a.clone()
            } == WalletView {
                faucet_in: None,
                ..b.clone()
            }
    };
    if shown
        .as_ref()
        .is_some_and(|(entity, was)| *entity == body && same(was, &view))
    {
        return;
    }
    let theme = world.resource::<Theme>().clone();
    let drawn = view.clone();
    let mut commands = world.commands();
    commands.entity(body).despawn_related::<Children>();
    commands
        .entity(body)
        .with_children(|b| page(b, &theme, &drawn, "wallet"));
    world.flush();
    *shown = Some((body, view));
}

/// The transfer being written: from which wallet, whether it is a payout, and who the
/// dropdown's names are.
#[derive(Resource)]
struct Transfer {
    from: AccountId,
    payout: bool,
    recipients: Vec<(String, AccountId)>,
}

/// What the confirm dialog does when it is answered yes.
#[derive(Resource)]
struct Confirming(Box<dyn FnOnce(&mut World) + Send + Sync>);

/// Asks once before credits move: `question` ("Send 250 credits to Cape Waterjet?"), a line
/// under it, and a button that says what it does. `act` runs on yes; nothing does on no.
pub(crate) fn confirm(
    world: &mut World,
    title: String,
    question: String,
    last_word: String,
    ok: String,
    act: impl FnOnce(&mut World) + Send + Sync + 'static,
) {
    close_all::<ConfirmDialog>(world);
    world.insert_resource(Confirming(Box::new(act)));
    let theme = world.resource::<Theme>().clone();
    let t = theme.clone();
    world.spawn((
        Dialog::new("wallet-confirm")
            .title(title)
            .width(440.0)
            .body(move |b| {
                b.spawn((
                    Name::new("wallet-confirm-text"),
                    t.paragraph_within(question, t.font_base, t.foreground, 400.0),
                ));
                b.spawn(t.paragraph_within(last_word, t.font_sm, t.muted_foreground, 400.0));
            })
            .footer(move |f| {
                dialog_button(
                    f,
                    Button::new("wallet-confirm-ok").label(ok).primary(),
                    confirmed,
                );
                dialog_button(
                    f,
                    Button::new("wallet-confirm-cancel").label(tr("common-cancel")),
                    close_all::<ConfirmDialog>,
                );
            })
            .build(&theme),
        ConfirmDialog,
    ));
    world.flush();
}

/// Answers the open confirm dialog yes (its button does; so does a script or a test).
pub fn confirmed(world: &mut World) {
    let Some(Confirming(act)) = world.remove_resource::<Confirming>() else {
        return;
    };
    close_all::<ConfirmDialog>(world);
    act(world);
}

#[derive(Component)]
struct TransferDialog;

#[derive(Component)]
struct ConfirmDialog;

fn close_all<M: Component>(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<M>>();
    for dialog in q.iter(world).collect::<Vec<_>>() {
        world.trigger(DialogClose { entity: dialog });
    }
}

/// Who credits of `from` can be sent to, by name: for a payout the organization's members
/// first, then the user; otherwise the user's own wallet and organizations, their contacts and
/// the shops heard of. Each name is there once; two accounts of one name are told apart by the
/// start of their address.
fn recipients(world: &World, from: AccountId, payout: bool) -> Vec<(String, AccountId)> {
    let social = world.resource::<crate::social::Social>();
    let me = credits::me(world);
    let mut accounts: Vec<AccountId> = Vec::new();
    let mut add = |account: AccountId| {
        if account != from && !accounts.contains(&account) {
            accounts.push(account);
        }
    };
    if let Some(open) = world.get_resource::<crate::organization::OpenOrg>()
        && open.session.org().org == Some(from)
    {
        for member in open.session.org().members.keys() {
            add(*member);
        }
    }
    me.into_iter().for_each(&mut add);
    if !payout {
        for org in &social.orgs {
            add(org.account);
        }
    }
    for card in &social.contacts.cards {
        let person = card.value.kind == cadrs_social::contact::Kind::Person;
        if !payout || person {
            add(card.value.account);
        }
    }
    if !payout {
        for shop in social.directory.listings().take(200) {
            add(shop.card.value.account);
        }
    }
    let mut named: Vec<(String, AccountId)> = accounts
        .into_iter()
        .map(|a| (credits::name_of(world, &a), a))
        .collect();
    let names: Vec<String> = named.iter().map(|(n, _)| n.clone()).collect();
    for (name, account) in &mut named {
        if names.iter().filter(|n| *n == name).count() > 1 {
            *name = format!("{name} · {}", credits::short(account));
        }
    }
    named
}

/// Opens Transfer… (or Pay out…) from wallet `from`.
pub fn open_transfer(world: &mut World, from: AccountId, payout: bool) {
    close_all::<TransferDialog>(world);
    let recipients = recipients(world, from, payout);
    let names: Vec<String> = recipients.iter().map(|(n, _)| n.clone()).collect();
    let balance = world.resource::<Credits>().balance(&from);
    let from_name = credits::name_of(world, &from);
    world.insert_resource(Transfer {
        from,
        payout,
        recipients,
    });
    let theme = world.resource::<Theme>().clone();
    let t = theme.clone();
    let label = move |b: &mut ChildSpawner, t: &Theme, key: &str| {
        b.spawn(t.text(tr(key), t.font_sm, FontWeight::BOLD, t.foreground));
    };
    world.spawn((
        Dialog::new("wallet-transfer-dialog")
            .title(tr(if payout {
                "wallet-payout-title"
            } else {
                "wallet-transfer-title"
            }))
            .width(520.0)
            .body(move |b| {
                let t = &t;
                b.spawn(t.text(
                    tr_with(
                        "wallet-transfer-from",
                        &[
                            ("name", &from_name),
                            (
                                "amount",
                                &balance.map_or_else(|| "?".to_owned(), cadrs_credits::show),
                            ),
                        ],
                    ),
                    t.font_sm,
                    FontWeight::NORMAL,
                    t.muted_foreground,
                ));
                label(
                    b,
                    t,
                    if payout {
                        "wallet-payout-to"
                    } else {
                        "wallet-to"
                    },
                );
                b.spawn(
                    cadrs_ui::SearchSelect::new("wallet-to")
                        .options(names)
                        .placeholder(tr("wallet-to-placeholder"))
                        .search_placeholder(tr("wallet-to-search"))
                        .empty_text(tr("wallet-to-none"), tr("wallet-to-none"))
                        .typed(tr("wallet-to-address"))
                        .build(t),
                );
                label(b, t, "wallet-amount");
                b.spawn(
                    TextInput::new("wallet-amount")
                        .placeholder(tr("wallet-amount-placeholder"))
                        .width(Val::Px(160.0))
                        .build(t),
                );
                label(b, t, "wallet-note");
                b.spawn(
                    TextInput::new("wallet-note")
                        .placeholder(tr("wallet-note-placeholder"))
                        .max_characters(cadrs_credits::MAX_NOTE)
                        .build(t),
                );
            })
            .footer(|f| {
                dialog_button(
                    f,
                    Button::new("wallet-transfer-ok")
                        .label(tr("wallet-transfer-review"))
                        .primary(),
                    review,
                );
                dialog_button(
                    f,
                    Button::new("wallet-transfer-cancel").label(tr("common-cancel")),
                    close_all::<TransferDialog>,
                );
            })
            .build(&theme),
        TransferDialog,
    ));
    world.flush();
}

/// Whose wallet a name from the dropdown is: one of its names, else a wallet address or a
/// contact's link typed in.
fn recipient(transfer: &Transfer, chosen: &str) -> Option<AccountId> {
    let chosen = chosen.trim();
    if let Some((_, account)) = transfer.recipients.iter().find(|(name, _)| name == chosen) {
        return Some(*account);
    }
    cadrs_credits::parse_address(chosen).or_else(|| {
        cadrs_social::contact::from_link(chosen)
            .ok()
            .map(|card| card.value.account)
    })
}

/// What stops a transfer of `amount` as typed, as the catalog key that says so.
pub(crate) fn problem(
    from: AccountId,
    to: Option<AccountId>,
    amount: &str,
    balance: Option<Amount>,
) -> Result<(AccountId, Amount), &'static str> {
    let to = to.ok_or("wallet-needs-recipient")?;
    if to == from {
        return Err("credits-refused-same-wallet");
    }
    let amount = cadrs_credits::parse(amount)
        .filter(|a| (1..=cadrs_credits::MAX_AMOUNT).contains(a))
        .ok_or("credits-refused-amount")?;
    if balance.is_some_and(|balance| amount > balance) {
        return Err("credits-refused-not-enough");
    }
    Ok((to, amount))
}

/// The transfer as written is checked and asked about once: nothing moves before the answer.
fn review(world: &mut World) {
    let Some(transfer) = world.get_resource::<Transfer>() else {
        return;
    };
    let (from, payout) = (transfer.from, transfer.payout);
    let chosen = picked(world, "wallet-to").unwrap_or_default();
    let to = recipient(world.resource::<Transfer>(), &chosen);
    let balance = world.resource::<Credits>().balance(&from);
    let (to, amount) = match problem(from, to, &field(world, "wallet-amount"), balance) {
        Ok(ok) => ok,
        Err(why) => {
            crate::linked::toast(world, tr(why));
            return;
        }
    };
    let to_name = world
        .resource::<Transfer>()
        .recipients
        .iter()
        .find(|(_, account)| *account == to)
        .map_or_else(|| credits::name_of(world, &to), |(name, _)| name.clone());
    let note: String = field(world, "wallet-note")
        .trim()
        .chars()
        .take(cadrs_credits::MAX_NOTE)
        .collect();
    let payment = Payment {
        from,
        to,
        to_name: to_name.clone(),
        amount,
        note,
        about: None,
        payout,
    };
    let shown = cadrs_credits::show(amount);
    let question = tr_with(
        if payout {
            "wallet-confirm-payout"
        } else {
            "wallet-confirm-send"
        },
        &[
            ("amount", &shown),
            ("name", &to_name),
            ("from", &credits::name_of(world, &from)),
        ],
    );
    let title = tr(if payout {
        "wallet-payout-title"
    } else {
        "wallet-transfer-title"
    });
    confirm(
        world,
        title,
        question,
        tr_with("wallet-confirm-final", &[("name", &to_name)]),
        tr_with("wallet-confirm-ok", &[("amount", &shown)]),
        move |world| {
            close_all::<TransferDialog>(world);
            world.remove_resource::<Transfer>();
            credits::pay(world, payment, None);
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_social::keys::SecretKey;

    fn someone() -> AccountId {
        SecretKey::generate().public()
    }

    #[test]
    fn a_transfer_is_checked_before_it_is_asked_about() {
        let (me, shop) = (someone(), someone());
        assert_eq!(problem(me, Some(shop), "250", Some(1000)), Ok((shop, 250)));
        assert_eq!(
            problem(me, Some(shop), " 1,000 ", Some(1000)),
            Ok((shop, 1000))
        );
        // The balance isn't known yet: the bank decides.
        assert_eq!(problem(me, Some(shop), "5000", None), Ok((shop, 5000)));
        assert_eq!(
            problem(me, None, "250", Some(1000)),
            Err("wallet-needs-recipient")
        );
        assert_eq!(
            problem(me, Some(me), "250", Some(1000)),
            Err("credits-refused-same-wallet")
        );
        for bad in ["", "0", "-5", "2.5", "lots", "1000000001"] {
            assert_eq!(
                problem(me, Some(shop), bad, Some(1000)),
                Err("credits-refused-amount"),
                "{bad}"
            );
        }
        assert_eq!(
            problem(me, Some(shop), "1001", Some(1000)),
            Err("credits-refused-not-enough")
        );
    }

    #[test]
    fn a_recipient_is_a_name_an_address_or_a_link() {
        let (me, shop, stranger) = (someone(), someone(), someone());
        let transfer = Transfer {
            from: me,
            payout: false,
            recipients: vec![("Cape Waterjet".into(), shop)],
        };
        assert_eq!(recipient(&transfer, " Cape Waterjet "), Some(shop));
        assert_eq!(recipient(&transfer, &stranger.to_string()), Some(stranger));
        assert_eq!(recipient(&transfer, "cape waterjet"), None);
        assert_eq!(recipient(&transfer, "not an address"), None);
        assert_eq!(recipient(&transfer, ""), None);
    }

    #[test]
    fn movements_read_from_either_side() {
        let (me, shop) = (someone(), someone());
        let movement = |kind, from: Option<AccountId>, to| Movement {
            seq: 1,
            id: [0; 16],
            time: 0,
            kind,
            from,
            to,
            amount: 1250,
            note: String::new(),
            about: None,
            by: me,
            hold: None,
        };
        let sent = movement(Kind::Transfer, Some(me), shop);
        assert_eq!(describe(&sent, &me).1, "\u{2212}1,250");
        assert_eq!(describe(&sent, &shop).1, "+1,250");
        // Held for the shop: out of my balance, not yet in the shop's.
        let held = movement(Kind::Held, Some(me), shop);
        assert_eq!(describe(&held, &me).1, "\u{2212}1,250");
        assert_eq!(describe(&held, &shop).1, "(1,250)");
        let released = movement(Kind::Released, Some(me), shop);
        assert_eq!(describe(&released, &me).1, "(1,250)");
        assert_eq!(describe(&released, &shop).1, "+1,250");
        let returned = movement(Kind::Returned, Some(me), shop);
        assert_eq!(describe(&returned, &me).1, "+1,250");
        assert_eq!(describe(&movement(Kind::Faucet, None, me), &me).1, "+1,250");
    }

    #[test]
    fn strays_are_what_is_held_for_no_order_of_ours() {
        let line = |n: u8, payer: bool, order: Option<cadrs_org::Id>| HeldLine {
            id: [n; 16],
            when: String::new(),
            who: String::new(),
            about: String::new(),
            amount: 1,
            payer,
            due: false,
            order,
        };
        let held = [
            line(1, false, Some(7)),
            line(2, false, Some(8)),
            line(3, false, None),
            // What the organization itself holds for someone isn't a stranger's.
            line(4, true, None),
        ];
        assert_eq!(strays(&held, &[7]), [[2; 16], [3; 16]]);
        assert!(strays(&held[..1], &[7]).is_empty());
    }

    #[test]
    fn the_history_copies_as_csv() {
        let view = WalletView {
            account: someone(),
            organization: false,
            statement: None,
            lines: vec![Line {
                when: "2026-11-23 14:05".into(),
                what: "Sent".into(),
                who: "Smith, Jones \"and\" Co".into(),
                about: "R-0007".into(),
                amount: "\u{2212}1,250".into(),
            }],
            movements: Vec::new(),
            more: false,
            error: None,
            asking: false,
            faucet_in: None,
            bank: true,
            held: Vec::new(),
        };
        assert_eq!(
            csv(&view),
            "when,what,who,for,amount\n\
             2026-11-23 14:05,Sent,\"Smith, Jones \"\"and\"\" Co\",R-0007,-1250\n"
        );
    }
}
