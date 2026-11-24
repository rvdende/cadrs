//! The organization screen's running-the-shop modules: Jobs (a schedule for each machine),
//! Clients, Parts (with what goes into them) and the members' Chat. Each change is an
//! operation on the organization's document ([`crate::organization::write`]).

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_org::{BomLine, Client, Job, JobState, Op, Org, Part, Stage, new_id};
use cadrs_ui::dialog::Dialog;
use cadrs_ui::i18n::tr;
use cadrs_ui::input::TextInput;
use cadrs_ui::tab_strip::TabStrip;
use cadrs_ui::theme::Theme;
use cadrs_ui::{Button, Column, TableRow};

use crate::AppState;
use crate::organization::{
    ModuleView, OpenOrg, OrgDialog, ask_remove, cell_button, choice, close_dialogs, dialog_buttons,
    field, muted, number, page_head, request_number, row, status_tag, strip_selected, t_text,
    table, text_row, write,
};

/// "2026-11-14 08:00" in local time ↔ milliseconds since the Unix epoch.
fn parse_when(text: &str) -> Option<u64> {
    let naive = chrono::NaiveDateTime::parse_from_str(text.trim(), "%Y-%m-%d %H:%M").ok()?;
    let local = naive.and_local_timezone(chrono::Local).single()?;
    u64::try_from(local.timestamp_millis()).ok()
}

pub(crate) fn show_when(millis: u64) -> String {
    chrono::DateTime::from_timestamp_millis(millis as i64)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// What an order is of: "6 × bracket, 2 × plate".
fn parts_of(r: &cadrs_org::Request) -> String {
    r.parts
        .iter()
        .map(|(p, n)| format!("{n} × {}", p.name))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The orders that can be scheduled: accepted and not yet delivered.
fn open_orders(org: &Org) -> Vec<(cadrs_org::Id, String, f64)> {
    org.requests
        .iter()
        .filter(|(_, r)| {
            matches!(
                r.stage,
                Stage::Accepted | Stage::InProduction | Stage::Ready
            )
        })
        .map(|(id, r)| {
            let minutes = r
                .quote
                .as_ref()
                .map(|q| q.lines.iter().map(|l| l.minutes).sum())
                .unwrap_or(60.0);
            (
                *id,
                format!("{} · {}", request_number(r.number), parts_of(r)),
                minutes,
            )
        })
        .collect()
}

fn job_state_key(state: JobState) -> &'static str {
    match state {
        JobState::Scheduled => "org-job-scheduled",
        JobState::Running => "org-job-running",
        JobState::Done => "org-job-done",
    }
}

pub(crate) fn jobs(b: &mut ChildSpawnerCommands, t: &Theme, v: &ModuleView) {
    let org = &v.org;
    page_head(b, t, "org-tab-jobs", "org-jobs-about");
    if org.machines.is_empty() {
        muted(b, t, tr("org-no-machines"));
        return;
    }
    let columns = vec![
        Column::new("when", tr("org-job-when-short")).width(150.0),
        Column::new("minutes", tr("org-job-minutes")).width(80.0),
        Column::new("order", tr("org-job-order")).width(300.0),
        Column::new("state", tr("org-job-state")).width(110.0),
        Column::new("actions", "").width(190.0),
    ];
    let mut i = 0;
    for (machine_id, machine) in &org.machines {
        subheading_text(b, t, machine.name.clone());
        let mut jobs: Vec<(&cadrs_org::Id, &Job)> = org
            .jobs
            .iter()
            .filter(|(_, j)| j.machine == *machine_id)
            .collect();
        jobs.sort_by_key(|(_, j)| j.start);
        let rows: Vec<TableRow> = jobs
            .into_iter()
            .map(|(id, job)| {
                let (id, job) = (*id, job.clone());
                let what = org
                    .requests
                    .get(&job.request)
                    .map_or_else(String::new, |r| {
                        format!("{} · {}", request_number(r.number), parts_of(r))
                    });
                let next = match job.state {
                    JobState::Scheduled => Some((JobState::Running, "org-job-start")),
                    JobState::Running => Some((JobState::Done, "org-job-finish")),
                    JobState::Done => None,
                };
                let n = i;
                i += 1;
                let state = job.state;
                let tc = t.clone();
                text_row(
                    t,
                    format!("org-job-{n}"),
                    &columns,
                    [show_when(job.start), format!("{:.0}", job.minutes), what],
                )
                .cell(move |c| {
                    c.spawn(status_tag(
                        &tc,
                        format!("org-job-state-{n}"),
                        tr(job_state_key(state)),
                        tc.foreground,
                    ));
                })
                .cell(move |c| {
                    if let Some((to, label)) = next {
                        let job = job.clone();
                        cell_button(c, format!("org-job-next-{n}"), tr(label), true, move |w| {
                            let mut job = job.clone();
                            job.state = to;
                            write(w, Op::SetJob { id, job: Some(job) });
                        });
                    }
                    cell_button(
                        c,
                        format!("org-job-remove-{n}"),
                        tr("common-remove"),
                        false,
                        move |w| ask_remove(w, Op::SetJob { id, job: None }),
                    );
                })
            })
            .collect();
        table(b, t, "org-jobs", columns.clone(), rows, "org-no-jobs");
    }
    let orders = open_orders(org);
    if orders.is_empty() {
        muted(b, t, tr("org-no-orders-to-schedule"));
        return;
    }
    let machines: Vec<(cadrs_org::Id, String)> = org
        .machines
        .iter()
        .map(|(id, m)| (*id, m.name.clone()))
        .collect();
    b.spawn((
        Button::new("org-add-job")
            .label(tr("org-add-job"))
            .icon("plus")
            .build(t),
        observe(
            move |_: On<Activate>, mut commands: Commands, theme: Res<Theme>| {
                let t = theme.clone();
                let tb = theme.clone();
                let orders = orders.clone();
                let machines = machines.clone();
                commands.spawn((
                    Dialog::new("org-job-dialog")
                        .title(tr("org-add-job"))
                        .width(600.0)
                        .body(move |b| {
                            let t = &tb;
                            b.spawn(t_text(t, "org-job-order"));
                            let whats = orders.iter().map(|(_, what, _)| what.clone());
                            b.spawn(choice(t, "org-job-order", whats.collect::<Vec<_>>(), 0));
                            b.spawn(t_text(t, "org-job-machine"));
                            let names = machines.iter().map(|(_, name)| name.clone());
                            b.spawn(choice(t, "org-job-machine", names.collect::<Vec<_>>(), 0));
                            b.spawn(t_text(t, "org-job-when"));
                            let start = show_when(cadrs_social::wall_millis() + 3_600_000);
                            let minutes = format!("{:.0}", orders[0].2.max(1.0));
                            b.spawn(Node {
                                column_gap: Val::Px(8.0),
                                ..default()
                            })
                            .with_children(|r| {
                                r.spawn(
                                    TextInput::new("org-job-start")
                                        .value(start)
                                        .width(Val::Px(180.0))
                                        .build(t),
                                );
                                r.spawn(
                                    TextInput::new("org-job-minutes")
                                        .value(minutes)
                                        .width(Val::Px(100.0))
                                        .build(t),
                                );
                            });
                        })
                        .footer(|f| dialog_buttons(f, "org-job", add_job))
                        .build(&t),
                    OrgDialog,
                    DespawnOnExit(AppState::Organization),
                ));
            },
        ),
    ));
}

/// A smaller heading that is a name (a machine's), not a catalog key.
fn subheading_text(b: &mut ChildSpawnerCommands, t: &Theme, text: String) {
    b.spawn((
        t.text(text, t.font_md, FontWeight::BOLD, t.foreground),
        Node {
            margin: UiRect::top(Val::Px(10.0)),
            ..default()
        },
    ));
}

fn add_job(world: &mut World) {
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let orders = open_orders(open.session.org());
    let machines: Vec<cadrs_org::Id> = open.session.org().machines.keys().copied().collect();
    let order = orders
        .get(strip_selected(world, "org-job-order"))
        .map(|o| o.0);
    let machine = machines
        .get(strip_selected(world, "org-job-machine"))
        .copied();
    let start = parse_when(&field(world, "org-job-start"));
    let minutes = number(world, "org-job-minutes").filter(|m| *m > 0.0);
    let (Some(request), Some(machine), Some(start), Some(minutes)) =
        (order, machine, start, minutes)
    else {
        crate::linked::toast(world, tr("org-job-bad"));
        return;
    };
    let job = Job {
        request,
        machine,
        start,
        minutes,
        state: JobState::Scheduled,
    };
    if write(
        world,
        Op::SetJob {
            id: new_id(),
            job: Some(job),
        },
    ) {
        close_dialogs(world);
    }
}

pub(crate) fn clients(b: &mut ChildSpawnerCommands, t: &Theme, v: &ModuleView) {
    let org = &v.org;
    page_head(b, t, "org-tab-clients", "org-clients-about");
    let columns = vec![
        Column::new("name", tr("org-client-name")).width(240.0),
        Column::new("requests", tr("org-client-requests")).width(100.0),
        Column::new("completed", tr("org-client-completed")).width(100.0),
        Column::new("contact", tr("org-client-contact")).width(300.0),
        Column::new("actions", "").width(100.0),
    ];
    // Everyone who sent a request, with their orders.
    // (how many requests, how many completed, what to call them: as the Requests page does).
    let mut by_account: std::collections::BTreeMap<_, (usize, usize, String)> = Default::default();
    for r in org.requests.values() {
        let e = by_account
            .entry(r.client)
            .or_insert_with(|| (0, 0, v.client_name(r)));
        e.0 += 1;
        if r.stage == Stage::Completed {
            e.1 += 1;
        }
    }
    let mut rows = Vec::new();
    for (account, (requests, completed, called)) in &by_account {
        let name = org
            .clients
            .values()
            .find(|c| c.account == Some(*account))
            .map_or_else(|| called.clone(), |c| c.name.clone());
        rows.push(
            text_row(
                t,
                format!("org-client-{}", rows.len()),
                &columns,
                [
                    name,
                    requests.to_string(),
                    completed.to_string(),
                    tr("org-client-on-cadrs"),
                ],
            )
            .cell(|_| {}),
        );
    }
    // Clients kept by hand (walk-ins, phone orders).
    for (id, client) in org.clients.iter().filter(|(_, c)| c.account.is_none()) {
        let id = *id;
        let n = rows.len();
        let contact = [
            client.email.as_str(),
            client.phone.as_str(),
            client.notes.as_str(),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
        rows.push(
            text_row(
                t,
                format!("org-client-{n}"),
                &columns,
                [client.name.clone(), "—".into(), "—".into(), contact],
            )
            .cell(move |c| {
                cell_button(
                    c,
                    format!("org-client-remove-{n}"),
                    tr("common-remove"),
                    false,
                    move |w| ask_remove(w, Op::SetClient { id, client: None }),
                );
            }),
        );
    }
    table(b, t, "org-clients", columns, rows, "org-no-clients");
    b.spawn((
        Button::new("org-add-client")
            .label(tr("org-add-client"))
            .icon("plus")
            .build(t),
        observe(
            |_: On<Activate>, mut commands: Commands, theme: Res<Theme>| {
                let t = theme.clone();
                let tb = theme.clone();
                commands.spawn((
                    Dialog::new("org-client-dialog")
                        .title(tr("org-add-client"))
                        .width(520.0)
                        .body(move |b| {
                            let t = &tb;
                            for name in [
                                "org-client-name",
                                "org-client-email",
                                "org-client-phone",
                                "org-client-notes",
                            ] {
                                b.spawn(t_text(t, name));
                                b.spawn(TextInput::new(name).build(t));
                            }
                        })
                        .footer(|f| dialog_buttons(f, "org-client", add_client))
                        .build(&t),
                    OrgDialog,
                    DespawnOnExit(AppState::Organization),
                ));
            },
        ),
    ));
}

fn add_client(world: &mut World) {
    let name = field(world, "org-client-name");
    if name.trim().is_empty() {
        crate::linked::toast(world, tr("org-client-needs-name"));
        return;
    }
    let client = Client {
        name: name.trim().to_owned(),
        account: None,
        email: field(world, "org-client-email").trim().to_owned(),
        phone: field(world, "org-client-phone").trim().to_owned(),
        notes: field(world, "org-client-notes").trim().to_owned(),
    };
    if write(
        world,
        Op::SetClient {
            id: new_id(),
            client: Some(client),
        },
    ) {
        close_dialogs(world);
    }
}

pub(crate) fn parts(b: &mut ChildSpawnerCommands, t: &Theme, v: &ModuleView) {
    let org = &v.org;
    page_head(b, t, "org-tab-parts", "org-parts-about");
    let columns = vec![
        Column::new("name", tr("org-part-name")).width(240.0),
        Column::new("revision", tr("org-part-revision")).width(90.0),
        Column::new("bom", tr("org-part-bom")).width(420.0),
        Column::new("actions", "").width(100.0),
    ];
    let rows = org
        .parts
        .iter()
        .enumerate()
        .map(|(i, (id, part))| {
            let id = *id;
            let bom: Vec<String> = part
                .bom
                .iter()
                .map(|l| match l {
                    BomLine::Material { material, sheets } => org
                        .materials
                        .get(material)
                        .map(|m| format!("{sheets} × {} {} mm", m.name, m.thickness))
                        .unwrap_or_default(),
                    BomLine::Part { part, quantity } => org
                        .parts
                        .get(part)
                        .map(|p| format!("{quantity} × {}", p.name))
                        .unwrap_or_default(),
                    BomLine::Bought { name, quantity } => format!("{quantity} × {name}"),
                })
                .collect();
            text_row(
                t,
                format!("org-part-{i}"),
                &columns,
                [part.name.clone(), part.revision.clone(), bom.join(", ")],
            )
            .cell(move |c| {
                cell_button(
                    c,
                    format!("org-part-remove-{i}"),
                    tr("common-remove"),
                    false,
                    move |w| ask_remove(w, Op::SetPart { id, part: None }),
                );
            })
        })
        .collect();
    table(b, t, "org-parts", columns, rows, "org-no-parts");
    b.spawn((
        Button::new("org-add-part")
            .label(tr("org-add-part"))
            .icon("plus")
            .build(t),
        observe(
            |_: On<Activate>, mut commands: Commands, theme: Res<Theme>| {
                let t = theme.clone();
                let tb = theme.clone();
                commands.spawn((
                    Dialog::new("org-part-dialog")
                        .title(tr("org-add-part"))
                        .width(560.0)
                        .body(move |b| {
                            let t = &tb;
                            b.spawn(t_text(t, "org-part-name"));
                            b.spawn(TextInput::new("org-part-name").autofocus().build(t));
                            b.spawn(t_text(t, "org-part-revision"));
                            b.spawn(TextInput::new("org-part-revision").value("A").build(t));
                            b.spawn(t_text(t, "org-part-bom"));
                            b.spawn(
                                TextInput::new("org-part-bom")
                                    .placeholder(tr("org-part-bom-placeholder"))
                                    .build(t),
                            );
                        })
                        .footer(|f| dialog_buttons(f, "org-part", add_part))
                        .build(&t),
                    OrgDialog,
                    DespawnOnExit(AppState::Organization),
                ));
            },
        ),
    ));
}

/// "0.5 mild steel 10, 4 M8 bolt" → lines: a stock material (by name and thickness) by the
/// sheet, or something bought.
fn parse_bom(text: &str, org: &cadrs_org::Org) -> Vec<BomLine> {
    text.split([',', ';'])
        .filter_map(|item| {
            let item = item.trim();
            let (amount, rest) = item.split_once(' ')?;
            let amount: f64 = amount.replace(',', ".").parse().ok()?;
            let rest = rest.trim();
            // A stock material: its name then its thickness.
            let material = org.materials.iter().find(|(_, m)| {
                rest.eq_ignore_ascii_case(&format!("{} {}", m.name, m.thickness))
                    || rest.eq_ignore_ascii_case(&format!("{} {}mm", m.name, m.thickness))
                    || rest.eq_ignore_ascii_case(&format!("{} {} mm", m.name, m.thickness))
            });
            Some(match material {
                Some((id, _)) => BomLine::Material {
                    material: *id,
                    sheets: amount,
                },
                None => BomLine::Bought {
                    name: rest.to_owned(),
                    quantity: amount.round().max(1.0) as u32,
                },
            })
        })
        .collect()
}

fn add_part(world: &mut World) {
    let name = field(world, "org-part-name");
    if name.trim().is_empty() {
        crate::linked::toast(world, tr("org-part-needs-name"));
        return;
    }
    let revision = field(world, "org-part-revision");
    let bom_text = field(world, "org-part-bom");
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let bom = parse_bom(&bom_text, open.session.org());
    let part = Part {
        name: name.trim().to_owned(),
        revision: revision.trim().to_owned(),
        bom,
    };
    if write(
        world,
        Op::SetPart {
            id: new_id(),
            part: Some(part),
        },
    ) {
        close_dialogs(world);
    }
}

/// The channels to show: the ones with messages, "general" always, and the one being shown.
fn channel_names(org: &Org, shown: &str) -> Vec<String> {
    let mut names: Vec<String> = org.channels.keys().cloned().collect();
    if !names.iter().any(|n| n == "general") {
        names.insert(0, "general".into());
    }
    if !names.iter().any(|n| n == shown) {
        names.push(shown.to_owned());
    }
    names
}

pub(crate) fn chat(b: &mut ChildSpawnerCommands, t: &Theme, v: &ModuleView) {
    let org = &v.org;
    page_head(b, t, "org-tab-chat", "org-chat-about");
    let names = channel_names(org, &v.channel);
    let selected = names.iter().position(|n| *n == v.channel).unwrap_or(0);
    let mut strip = TabStrip::new("org-channels").compact().selected(selected);
    for n in &names {
        strip = strip.tab(format!("#{n}"));
    }
    b.spawn((
        strip.build(t),
        observe(
            |ev: On<cadrs_ui::tab_strip::TabStripSelect>, mut open: ResMut<OpenOrg>| {
                let names = channel_names(open.session.org(), &open.channel);
                if let Some(name) = names.get(ev.index).cloned()
                    && open.channel != name
                {
                    open.channel = name;
                }
            },
        ),
    ));
    let messages = org.channels.get(&v.channel).cloned().unwrap_or_default();
    if messages.is_empty() {
        muted(b, t, tr("org-chat-empty"));
    }
    for (i, (who, text, when)) in messages.iter().enumerate() {
        let name = if Some(*who) == v.me {
            tr("shell-landing-me")
        } else {
            v.account_name(who)
        };
        row(b, |r| {
            r.spawn(t.text(name, t.font_sm, FontWeight::BOLD, t.foreground));
            r.spawn(t.text(
                show_when(*when),
                t.font_xs,
                FontWeight::NORMAL,
                t.muted_foreground,
            ));
        });
        b.spawn((
            Name::new(format!("org-chat-{i}")),
            t.paragraph_within(text.clone(), t.font_base, t.foreground, 720.0),
        ));
    }
    row(b, |r| {
        // What was being written when a message arrived is still here, with the keyboard.
        let (draft, focused) = v.draft.clone().unwrap_or_default();
        let mut input = TextInput::new("org-chat-input")
            .placeholder(tr("chat-placeholder"))
            .value(draft)
            .width(Val::Px(420.0));
        if focused {
            input = input.autofocus();
        }
        r.spawn(input.build(t));
        r.spawn((
            Button::new("org-chat-send")
                .label(tr("chat-send"))
                .primary()
                .build(t),
            observe(|_: On<Activate>, mut commands: Commands| commands.queue(say)),
        ));
        r.spawn(
            TextInput::new("org-channel-new")
                .placeholder(tr("org-channel-new-placeholder"))
                .width(Val::Px(180.0))
                .build(t),
        );
        r.spawn((
            Button::new("org-channel-open")
                .label(tr("org-channel-open"))
                .build(t),
            observe(|_: On<Activate>, mut commands: Commands| {
                commands.queue(|w: &mut World| {
                    let name = field(w, "org-channel-new")
                        .trim()
                        .trim_start_matches('#')
                        .to_lowercase();
                    if !name.is_empty()
                        && let Some(mut open) = w.get_resource_mut::<OpenOrg>()
                    {
                        open.channel = name;
                    }
                });
            }),
        ));
    });
}

fn say(world: &mut World) {
    let text = field(world, "org-chat-input");
    let Some(channel) = world.get_resource::<OpenOrg>().map(|o| o.channel.clone()) else {
        return;
    };
    if text.trim().is_empty() {
        return;
    }
    write(
        world,
        Op::Say {
            channel,
            text: text.trim().to_owned(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_round_trip() {
        let millis = parse_when("2026-11-14 08:30").unwrap();
        assert_eq!(show_when(millis), "2026-11-14 08:30");
        assert_eq!(parse_when("tomorrow"), None);
    }

    #[test]
    fn a_bom_finds_stock_materials() {
        let mut org = cadrs_org::Org::default();
        org.materials.insert(
            7,
            cadrs_org::Material {
                name: "mild steel".into(),
                thickness: 10.0,
                sheet: [2500.0, 1250.0],
            },
        );
        let bom = parse_bom("0.5 mild steel 10, 4 M8 bolt", &org);
        assert_eq!(
            bom,
            vec![
                BomLine::Material {
                    material: 7,
                    sheets: 0.5
                },
                BomLine::Bought {
                    name: "M8 bolt".into(),
                    quantity: 4
                }
            ]
        );
    }
}
